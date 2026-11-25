//! Index des fichiers du projet, pour l'ouverture rapide.
//!
//! Le dossier est parcouru dans un thread à part, en respectant `.gitignore`, `.ignore` et les
//! fichiers cachés (crate `ignore`, celle de ripgrep) : l'interface n'attend jamais. Les fichiers
//! arrivent par lots, et chaque lot réveille la boucle d'événements.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use winit::event_loop::EventLoopProxy;

/// Nombre maximal de fichiers indexés : au-delà, le reste est ignoré (et signalé).
pub(crate) const MAX_FILES: usize = 200_000;

/// Taille d'un lot envoyé à l'interface.
const BATCH: usize = 2_000;

/// Un index terminé depuis moins longtemps n'est pas refait à l'ouverture.
const FRESH_FOR: Duration = Duration::from_secs(10);

/// Événements envoyés à la boucle d'événements par les autres threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppEvent {
    /// Des fichiers ont été indexés.
    FilesIndexed,
    /// Des fichiers ou dossiers surveillés ont changé sur le disque.
    FilesChanged,
}

/// Ce que le thread d'indexation envoie.
#[derive(Debug)]
enum Message {
    /// Chemins relatifs à la racine, avec `/` comme séparateur.
    Batch(Vec<String>),
    /// Parcours terminé ; vrai si la limite de fichiers a été atteinte.
    Done(bool),
}

/// Parcourt `root` et appelle `batch` avec les fichiers trouvés, par lots. Renvoie vrai si la
/// limite `max` a été atteinte.
pub(crate) fn scan(root: &Path, max: usize, mut batch: impl FnMut(Vec<String>)) -> bool {
    let walker = ignore::WalkBuilder::new(root)
        // .gitignore respecté même hors d'un dépôt Git.
        .require_git(false)
        .build();
    let mut current = Vec::with_capacity(BATCH);
    let mut count = 0;
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let text = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        current.push(text);
        count += 1;
        if current.len() == BATCH {
            batch(std::mem::replace(&mut current, Vec::with_capacity(BATCH)));
        }
        if count >= max {
            batch(current);
            return true;
        }
    }
    batch(current);
    false
}

/// Fichiers du projet, indexés en arrière-plan.
#[derive(Debug, Default)]
pub(crate) struct FileIndex {
    root: Option<PathBuf>,
    files: Vec<String>,
    receiver: Option<Receiver<Message>>,
    finished: Option<Instant>,
    truncated: bool,
}

impl FileIndex {
    /// Dossier indexé.
    pub(crate) fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Fichiers indexés jusqu'ici.
    pub(crate) fn files(&self) -> &[String] {
        &self.files
    }

    /// L'indexation est-elle en cours ?
    pub(crate) fn is_scanning(&self) -> bool {
        self.receiver.is_some()
    }

    /// Lance l'indexation de `root`, sauf si elle est en cours, ou terminée il y a peu sur le
    /// même dossier.
    pub(crate) fn refresh(&mut self, root: PathBuf, proxy: &EventLoopProxy<AppEvent>) {
        let same_root = self.root.as_ref() == Some(&root);
        if same_root
            && (self.is_scanning() || self.finished.is_some_and(|t| t.elapsed() < FRESH_FOR))
        {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let proxy = proxy.clone();
        let scanned = root.clone();
        let spawned = std::thread::Builder::new()
            .name("cupride-index".to_owned())
            .spawn(move || {
                let start = Instant::now();
                let send = |message| {
                    let ok = sender.send(message).is_ok();
                    // La boucle d'événements peut être déjà fermée : sans importance.
                    let _ = proxy.send_event(AppEvent::FilesIndexed);
                    ok
                };
                let truncated = scan(&scanned, MAX_FILES, |batch| {
                    send(Message::Batch(batch));
                });
                send(Message::Done(truncated));
                tracing::debug!(
                    dossier = %scanned.display(),
                    duree_ms = start.elapsed().as_millis() as u64,
                    "fichiers indexés"
                );
            });
        match spawned {
            Ok(_) => {
                // Un nouveau dossier repart de zéro ; le même garde sa liste jusqu'au premier lot.
                if !same_root {
                    self.files.clear();
                }
                self.root = Some(root);
                self.receiver = Some(receiver);
                self.finished = None;
                self.truncated = false;
            }
            Err(error) => tracing::warn!("indexation des fichiers impossible : {error}"),
        }
    }

    /// Prend les lots arrivés. Renvoie (changé, terminé avec la limite atteinte).
    pub(crate) fn drain(&mut self) -> (bool, bool) {
        let Some(receiver) = &self.receiver else {
            return (false, false);
        };
        let mut changed = false;
        let mut first_batch = self.finished.is_none() && !self.files.is_empty();
        let mut truncated_now = false;
        while let Ok(message) = receiver.try_recv() {
            match message {
                Message::Batch(batch) => {
                    // Réindexation du même dossier : l'ancienne liste est remplacée au premier lot.
                    if first_batch {
                        self.files.clear();
                        first_batch = false;
                    }
                    self.files.extend(batch);
                    changed = true;
                }
                Message::Done(truncated) => {
                    self.receiver = None;
                    self.finished = Some(Instant::now());
                    self.truncated = truncated;
                    truncated_now = truncated;
                    changed = true;
                    break;
                }
            }
        }
        (changed, truncated_now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parcours_respectant_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/ui")).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::create_dir_all(root.join(".cache")).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        for file in [
            "src/main.rs",
            "src/ui/view.rs",
            "README.md",
            "debug.log",
            "target/debug/app",
            ".cache/x",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let mut files = Vec::new();
        let truncated = scan(root, MAX_FILES, |batch| files.extend(batch));
        files.sort();
        assert!(!truncated);
        assert_eq!(files, ["README.md", "src/main.rs", "src/ui/view.rs"]);

        let mut limited = Vec::new();
        assert!(
            scan(root, 2, |batch| limited.extend(batch)),
            "limite atteinte"
        );
        assert_eq!(limited.len(), 2);
    }
}
