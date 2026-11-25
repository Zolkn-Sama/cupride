//! Fichiers modifiés hors de l'IDE.
//!
//! L'interface signale les fichiers changés sur le disque (surveillance avec `notify`, et au
//! retour du focus) ; le noyau compare chaque document à son fichier :
//!
//! - document sans modification : rechargé ;
//! - document modifié : rien n'est perdu ni écrasé ; une notification propose de recharger la
//!   version du disque (`file reload --force`), de garder la sienne (`file keep`) ou de comparer
//!   (`file compare`) ; `file save` refuse d'écraser le fichier sans `--force` ;
//! - fichier supprimé ou déplacé : une notification ; le document reste ouvert.

use std::path::{Path, PathBuf};

use crate::document::DiskState;
use crate::editor::{DocumentId, Editor};
use crate::notifications::Level;

impl Editor {
    /// Compare les documents à leur fichier : tous, ou ceux dont le fichier (ou le dossier) est
    /// dans `changed`. Renvoie le nombre de documents rechargés.
    pub fn check_disk(&mut self, changed: Option<&[PathBuf]>) -> usize {
        let concerned = |path: &Path| {
            changed.is_none_or(|paths| {
                paths
                    .iter()
                    .any(|p| p == path || path.parent() == Some(p.as_path()))
            })
        };
        let ids: Vec<DocumentId> = self
            .documents()
            .filter(|(_, d)| d.path().is_some_and(&concerned))
            .map(|(id, _)| id)
            .collect();
        let mut reloaded = 0;
        for id in ids {
            let Some(document) = self.document_mut(id) else {
                continue;
            };
            let name = document.display_name();
            let state = document.disk_state();
            let message = match state {
                DiskState::Same => {
                    document.disk_notice = None;
                    None
                }
                _ if document.disk_notice == Some(state) => None,
                DiskState::Missing => {
                    document.disk_notice = Some(state);
                    Some((
                        Level::Warning,
                        format!(
                            "« {name} » a été supprimé ou déplacé sur le disque : le document reste \\
                             ouvert (file save le recrée)"
                        ),
                    ))
                }
                DiskState::Changed if !document.is_modified() => match document.reload() {
                    Ok(()) => {
                        reloaded += 1;
                        Some((
                            Level::Info,
                            format!("« {name} » a changé sur le disque : rechargé"),
                        ))
                    }
                    Err(error) => Some((Level::Error, error.to_string())),
                },
                DiskState::Changed => {
                    document.disk_notice = Some(state);
                    Some((
                        Level::Warning,
                        format!(
                            "« {name} » a changé sur le disque et dans l'éditeur : file compare \\
                             (comparer), file keep (garder la vôtre) ou file reload --force \\
                             (prendre celle du disque)"
                        ),
                    ))
                }
            };
            if let Some((level, message)) = message {
                tracing::info!("{message}");
                self.notify(level, message);
            }
        }
        self.flush_events();
        reloaded
    }
}

/// Différences entre le fichier sur le disque et le document : format unifié, lignes `-` du
/// disque, `+` de l'éditeur.
pub fn compare(name: &str, disk: &str, editor: &str) -> String {
    similar::TextDiff::from_lines(disk, editor)
        .unified_diff()
        .context_radius(3)
        .header(&format!("{name} (disque)"), &format!("{name} (éditeur)"))
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::command::CommandError;
    use crate::editor::Editor;
    use crate::text::TextBuffer;

    #[test]
    fn rechargement_ou_conflit() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        fs::write(&a, "un").unwrap();
        fs::write(&b, "deux").unwrap();
        let mut e = Editor::new();
        let doc_a = e.open(&a).unwrap();
        let doc_b = e.open(&b).unwrap();
        e.execute("edit insert 'ici '").unwrap();
        fs::write(&a, "un, modifié ailleurs").unwrap();
        fs::write(&b, "deux, modifié ailleurs").unwrap();
        // Seuls les fichiers signalés sont examinés.
        assert_eq!(e.check_disk(Some(&[dir.path().join("autre.txt")])), 0);
        let before = e.notifications().history().count();
        let canonical = dir.path().canonicalize().unwrap();
        assert_eq!(e.check_disk(Some(&[canonical])), 1);
        assert_eq!(
            text(&e, doc_a),
            "un, modifié ailleurs",
            "non modifié : rechargé"
        );
        assert_eq!(text(&e, doc_b), "ici deux", "modifié : rien n'est perdu");
        assert_eq!(e.notifications().history().count(), before + 2);
        e.check_disk(None);
        assert_eq!(
            e.notifications().history().count(),
            before + 2,
            "signalé une fois"
        );
        // Enregistrer écraserait l'autre version : refusé sans --force.
        assert!(
            matches!(e.execute("file save"), Err(CommandError::Failed(m)) if m.contains("disque"))
        );
        let diff = match e.execute("file compare").unwrap() {
            crate::command::CommandOutput::Message(m) => m,
            other => panic!("{other:?}"),
        };
        assert!(diff.contains("différences"), "{diff}");
        let compared = e.active_document().unwrap().buffer().to_text();
        assert!(compared.contains("-deux, modifié ailleurs"), "{compared}");
        assert!(compared.contains("+ici deux"), "{compared}");
        e.set_active(doc_b);
        e.execute("file keep").unwrap();
        e.execute("file save").unwrap();
        assert_eq!(fs::read_to_string(&b).unwrap(), "ici deux");
        // Supprimé : signalé, le document reste.
        fs::remove_file(&a).unwrap();
        e.check_disk(None);
        assert!(e.document(doc_a).is_some());
        let last = e.notifications().history().last().unwrap().message.clone();
        assert!(last.contains("supprimé ou déplacé"), "{last}");
    }

    fn text(e: &Editor, id: crate::DocumentId) -> String {
        e.document(id).unwrap().buffer().to_text()
    }
}
