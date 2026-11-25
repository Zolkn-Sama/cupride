//! Recherche dans le projet, dans un thread à part : les résultats arrivent fichier par fichier,
//! et la boucle d'événements n'est réveillée qu'une fois par rafale.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cupride_core::project_search::{FileMatches, Summary, search_project};
use cupride_core::search::SearchQuery;

/// Résultats arrivés, pas encore lus.
#[derive(Debug, Default)]
struct Pending {
    generation: u64,
    files: Vec<FileMatches>,
    summary: Option<Summary>,
}

/// Ce qu'il y a à lire : numéro de la recherche, fichiers, et bilan si elle est finie.
pub(crate) type Batch = (u64, Vec<FileMatches>, Option<Summary>);

/// Recherche dans le projet en arrière-plan.
#[derive(Debug, Default)]
pub(crate) struct ProjectWorker {
    cancel: Option<Arc<AtomicBool>>,
    pending: Arc<Mutex<Pending>>,
}

impl ProjectWorker {
    /// Lance la recherche `generation` de `query` dans `root`, en annulant la précédente.
    /// `wake` est appelé depuis le thread quand des résultats attendent.
    pub(crate) fn start(
        &mut self,
        wake: impl Fn() + Send + 'static,
        generation: u64,
        root: PathBuf,
        query: SearchQuery,
    ) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(Arc::clone(&cancel));
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Pending {
                generation,
                ..Pending::default()
            };
        }
        let pending = Arc::clone(&self.pending);
        let spawned = std::thread::Builder::new()
            .name("cupride-recherche-projet".to_owned())
            .spawn(move || {
                // Ajoute aux résultats en attente ; réveille la boucle s'ils étaient vides.
                let push = |files: Vec<FileMatches>, summary: Option<Summary>| {
                    let Ok(mut slot) = pending.lock() else {
                        return;
                    };
                    if slot.generation != generation {
                        return;
                    }
                    let wake_up = slot.files.is_empty() && slot.summary.is_none();
                    slot.files.extend(files);
                    if summary.is_some() {
                        slot.summary = summary;
                    }
                    drop(slot);
                    if wake_up {
                        wake();
                    }
                };
                let result = search_project(&root, &query, &cancel, |file| push(vec![file], None));
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let summary = result.unwrap_or_else(|error| {
                    tracing::warn!("recherche dans le projet : {error}");
                    Summary::default()
                });
                push(Vec::new(), Some(summary));
            });
        if let Err(error) = spawned {
            tracing::warn!("recherche dans le projet impossible : {error}");
        }
    }

    /// Résultats arrivés depuis la dernière lecture.
    pub(crate) fn take(&mut self) -> Option<Batch> {
        let mut pending = self.pending.lock().ok()?;
        if pending.files.is_empty() && pending.summary.is_none() {
            return None;
        }
        Some((
            pending.generation,
            std::mem::take(&mut pending.files),
            pending.summary.take(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn resultats_au_fil_de_l_eau() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..20 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "un motif ici\n").unwrap();
        }
        let mut worker = ProjectWorker::default();
        let (sender, woken) = mpsc::channel();
        worker.start(
            move || {
                let _ = sender.send(());
            },
            7,
            dir.path().to_path_buf(),
            SearchQuery::new("motif"),
        );
        let mut files = 0;
        let mut finished = None;
        while finished.is_none() {
            woken.recv_timeout(Duration::from_secs(30)).unwrap();
            while let Some((generation, batch, summary)) = worker.take() {
                assert_eq!(generation, 7);
                files += batch.len();
                finished = finished.or(summary);
            }
        }
        assert_eq!(files, 20);
        assert_eq!(finished.unwrap().searched, 20);
    }
}
