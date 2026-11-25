//! Récupération après plantage.
//!
//! Les documents qui ont des modifications non enregistrées sont copiés régulièrement dans le
//! dossier `recovery/` des données de l'utilisateur — **jamais** dans les fichiers du projet. Un
//! fichier verrou y est posé au lancement et retiré à la fermeture normale : s'il est encore là au
//! lancement suivant, CuprIDE s'est arrêté brutalement, et les copies trouvées sont proposées à la
//! restauration (`recovery restore`), ou oubliées (`recovery discard`).

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::CommandError;
use crate::editor::{DocumentId, Editor};
use crate::text::{Edit, EditKind, Selections, TextBuffer};

/// Nom du fichier verrou.
const LOCK: &str = "session.lock";
/// Nom de l'index des copies.
const INDEX: &str = "index.json";
/// Sous-dossier où sont mises à l'abri les copies d'une session interrompue, le temps de
/// décider : les copies de la nouvelle session ne les écrasent jamais.
const PREVIOUS: &str = "previous";

/// Une copie de document non enregistré.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backup {
    /// Fichier de la copie, dans le dossier de récupération.
    pub file: String,
    /// Fichier du document, s'il en a un.
    pub path: Option<PathBuf>,
    /// Nom affiché.
    pub name: String,
}

/// État de la récupération : dossier, et révision de chaque document déjà copiée.
#[derive(Debug, Clone, Default)]
pub struct Recovery {
    dir: Option<PathBuf>,
    copied: BTreeMap<DocumentId, u64>,
}

/// Copies d'une session précédente, trouvées au lancement.
pub fn backups(dir: &Path) -> Vec<Backup> {
    fs::read_to_string(dir.join(INDEX))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Écrit `contents` dans `path`, de façon atomique.
fn write_atomic(dir: &Path, path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(contents)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

impl Editor {
    /// Commence la session de récupération dans `dir` : renvoie les copies laissées par une
    /// session qui s'est arrêtée brutalement (verrou encore présent), puis pose le verrou. Les
    /// copies restent sur le disque jusqu'à `recovery restore` ou `recovery discard`.
    pub fn start_recovery(&mut self, dir: PathBuf) -> io::Result<Vec<Backup>> {
        fs::create_dir_all(&dir)?;
        let crashed = dir.join(LOCK).exists();
        let previous = dir.join(PREVIOUS);
        let mut found = backups(&previous);
        if crashed && !backups(&dir).is_empty() {
            // Les copies de la session interrompue sont mises à l'abri.
            let _ = fs::remove_dir_all(&previous);
            fs::create_dir_all(&previous)?;
            for entry in fs::read_dir(&dir)?.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".txt") || name == INDEX {
                    fs::rename(entry.path(), previous.join(&name))?;
                }
            }
            found = backups(&previous);
        }
        // Les copies de la session précédente, elle, sont retirées.
        self.clear_backups_in(&dir)?;
        write_atomic(
            &dir,
            &dir.join(LOCK),
            std::process::id().to_string().as_bytes(),
        )?;
        self.recovery.dir = Some(dir);
        Ok(found)
    }

    /// Fermeture normale : copies et verrou retirés.
    pub fn finish_recovery(&mut self) -> io::Result<()> {
        let Some(dir) = self.recovery.dir.take() else {
            return Ok(());
        };
        self.clear_backups_in(&dir)?;
        match fs::remove_file(dir.join(LOCK)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Y a-t-il des modifications non encore copiées ?
    pub fn needs_backup(&self) -> bool {
        self.recovery.dir.is_some()
            && self.documents().any(|(id, d)| {
                d.is_modified() && self.recovery.copied.get(&id) != Some(&d.revision())
            })
    }

    /// Copie les documents qui ont des modifications non enregistrées (seulement ceux qui ont
    /// changé depuis la dernière copie), et retire les copies devenues inutiles. Renvoie le
    /// nombre de documents copiés.
    pub fn backup_documents(&mut self) -> io::Result<usize> {
        let Some(dir) = self.recovery.dir.clone() else {
            return Ok(0);
        };
        let mut index = Vec::new();
        let mut written = 0;
        let mut copied = BTreeMap::new();
        for (id, document) in self.documents() {
            if !document.is_modified() {
                continue;
            }
            let file = format!("{}.txt", document_key(id));
            if self.recovery.copied.get(&id) != Some(&document.revision()) {
                write_atomic(
                    &dir,
                    &dir.join(&file),
                    document.buffer().to_text().as_bytes(),
                )?;
                written += 1;
            }
            copied.insert(id, document.revision());
            index.push(Backup {
                file,
                path: document.path().map(Path::to_path_buf),
                name: document.display_name(),
            });
        }
        // Copies des documents enregistrés ou fermés depuis : retirées.
        for id in self.recovery.copied.keys() {
            if !copied.contains_key(id) {
                let _ = fs::remove_file(dir.join(format!("{}.txt", document_key(*id))));
            }
        }
        let json = serde_json::to_vec_pretty(&index).map_err(io::Error::other)?;
        write_atomic(&dir, &dir.join(INDEX), &json)?;
        self.recovery.copied = copied;
        Ok(written)
    }

    /// Restaure les copies d'une session interrompue : le texte de chaque copie remplace celui du
    /// document de son fichier (ouvert au besoin), en une modification annulable, non
    /// enregistrée ; une copie sans fichier devient un nouveau document. Renvoie le nombre de
    /// documents restaurés.
    pub fn restore_backups(&mut self, from: &[Backup]) -> Result<usize, CommandError> {
        let dir = self
            .recovery
            .dir
            .clone()
            .ok_or_else(|| CommandError::Failed("récupération indisponible".into()))?;
        let mut restored = 0;
        for backup in from {
            let Ok(text) = fs::read_to_string(dir.join(PREVIOUS).join(&backup.file)) else {
                continue;
            };
            let id = match &backup.path {
                Some(path) => match self.open(path) {
                    Ok(id) => id,
                    Err(_) => self.new_document(),
                },
                None => self.new_document(),
            };
            let Some(document) = self.document_mut(id) else {
                continue;
            };
            document.transact(EditKind::Other, |buffer, tx| {
                let edit = Edit::replace(buffer, 0..buffer.len_bytes(), text.clone())?;
                tx.apply(buffer, edit)?;
                tx.set_selections_after(Selections::cursor(0));
                Ok(())
            })?;
            restored += 1;
        }
        self.discard_backups(from);
        self.flush_events();
        Ok(restored)
    }

    /// Oublie les copies d'une session interrompue.
    pub fn discard_backups(&mut self, from: &[Backup]) {
        if let Some(dir) = &self.recovery.dir {
            let previous = dir.join(PREVIOUS);
            for backup in from {
                let _ = fs::remove_file(previous.join(&backup.file));
            }
            if fs::read_dir(&previous)
                .is_ok_and(|mut d| d.all(|e| e.is_ok_and(|e| e.file_name() == INDEX)))
            {
                let _ = fs::remove_dir_all(&previous);
            }
        }
        self.recovery_found.clear();
    }

    /// Copies trouvées au lancement, pas encore restaurées ni oubliées.
    pub fn found_backups(&self) -> &[Backup] {
        &self.recovery_found
    }

    /// Garde les copies trouvées au lancement, pour `recovery restore`.
    pub fn set_found_backups(&mut self, found: Vec<Backup>) {
        self.recovery_found = found;
    }

    fn clear_backups_in(&self, dir: &Path) -> io::Result<()> {
        for entry in fs::read_dir(dir)?.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".txt") || name == INDEX {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

/// Nom de fichier propre à un document de la session.
fn document_key(id: DocumentId) -> String {
    use slotmap::Key;
    format!("{:016x}", id.data().as_ffi())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copie_puis_restauration_apres_plantage() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        let project = dir.path().join("a.txt");
        fs::write(&project, "original").unwrap();

        // Première session : deux documents modifiés, puis « plantage » (pas de fermeture).
        let mut first = Editor::new();
        assert!(first.start_recovery(recovery.clone()).unwrap().is_empty());
        first.open(&project).unwrap();
        first.execute("edit insert 'modifié : '").unwrap();
        first.new_document();
        first.execute("edit insert 'brouillon'").unwrap();
        assert!(first.needs_backup());
        assert_eq!(first.backup_documents().unwrap(), 2);
        assert!(!first.needs_backup());
        assert_eq!(first.backup_documents().unwrap(), 0, "rien n'a changé");
        assert_eq!(
            fs::read_to_string(&project).unwrap(),
            "original",
            "projet intact"
        );
        drop(first);

        // Seconde session : les copies sont trouvées, et à l'abri des copies de cette session.
        let mut second = Editor::new();
        let found = second.start_recovery(recovery.clone()).unwrap();
        assert_eq!(found.len(), 2);
        second.new_document();
        second.execute("edit insert 'autre chose'").unwrap();
        second.backup_documents().unwrap();
        second.close(second.active_id().unwrap());
        assert_eq!(second.restore_backups(&found).unwrap(), 2);
        let texts: Vec<String> = second
            .documents()
            .map(|(_, d)| d.buffer().to_text())
            .collect();
        assert!(
            texts.contains(&"modifié : original".to_owned()),
            "{texts:?}"
        );
        assert!(texts.contains(&"brouillon".to_owned()), "{texts:?}");
        assert!(
            second.documents().all(|(_, d)| d.is_modified()),
            "à enregistrer"
        );
        second.execute("edit undo").unwrap();
        // Fermeture normale : plus rien à récupérer.
        second.finish_recovery().unwrap();
        let mut third = Editor::new();
        assert!(third.start_recovery(recovery).unwrap().is_empty());
    }

    #[test]
    fn copies_inutiles_retirees() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.txt");
        fs::write(&path, "x").unwrap();
        let mut editor = Editor::new();
        editor.start_recovery(dir.path().join("r")).unwrap();
        editor.open(&path).unwrap();
        editor.execute("edit insert 'y'").unwrap();
        editor.backup_documents().unwrap();
        assert_eq!(backups(&dir.path().join("r")).len(), 1);
        editor.execute("file save").unwrap();
        editor.backup_documents().unwrap();
        assert!(
            backups(&dir.path().join("r")).is_empty(),
            "enregistré : plus de copie"
        );
        let files = fs::read_dir(dir.path().join("r")).unwrap().count();
        assert_eq!(files, 2, "verrou et index seulement");
        // Une session interrompue sans copie : rien à proposer.
        let mut again = Editor::new();
        assert!(
            again
                .start_recovery(dir.path().join("r"))
                .unwrap()
                .is_empty()
        );
    }
}
