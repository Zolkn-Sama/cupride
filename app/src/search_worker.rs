//! Recherche dans un thread à part : l'interface ne bloque jamais, même sur un fichier de 100 Mo.
//!
//! Le texte du document est copié (quelques millisecondes pour 100 Mo), puis cherché dans un
//! thread. Une nouvelle recherche annule la précédente ; ses résultats, arrivés trop tard, sont
//! ignorés.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cupride_core::DocumentId;
use cupride_core::search::find_all;
use regex::Regex;

/// Occurrences trouvées dans un document, à une révision.
#[derive(Debug)]
pub(crate) struct Found {
    pub(crate) generation: u64,
    pub(crate) document: DocumentId,
    pub(crate) revision: u64,
    pub(crate) matches: Vec<Range<usize>>,
}

/// Recherches en arrière-plan.
#[derive(Debug, Default)]
pub(crate) struct SearchWorker {
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    found: Arc<Mutex<Option<Found>>>,
    /// Ce qui est cherché en ce moment, ou a été trouvé en dernier : (document, révision).
    target: Option<(DocumentId, u64)>,
}

impl SearchWorker {
    /// Cherche `regex` dans `text` (le document `document` à la révision `revision`), en annulant
    /// la recherche précédente. `done` est appelé à la fin, depuis le thread (l'application y
    /// réveille sa boucle d'événements).
    pub(crate) fn start(
        &mut self,
        done: impl FnOnce() + Send + 'static,
        document: DocumentId,
        revision: u64,
        text: Arc<str>,
        regex: Regex,
    ) {
        self.stop();
        self.generation += 1;
        self.target = Some((document, revision));
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(Arc::clone(&cancel));
        let found = Arc::clone(&self.found);
        let generation = self.generation;
        let spawned = std::thread::Builder::new()
            .name("cupride-recherche".to_owned())
            .spawn(move || {
                let Some(matches) = find_all(&text, &regex, &cancel) else {
                    return;
                };
                if let Ok(mut slot) = found.lock() {
                    *slot = Some(Found {
                        generation,
                        document,
                        revision,
                        matches,
                    });
                }
                done();
            });
        if let Err(error) = spawned {
            tracing::warn!("recherche impossible : {error}");
        }
    }

    /// Annule la recherche en cours.
    pub(crate) fn stop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.target = None;
    }

    /// Le document à cette révision est-il déjà cherché (ou en train de l'être) ?
    pub(crate) fn covers(&self, document: DocumentId, revision: u64) -> bool {
        self.target == Some((document, revision))
    }

    /// Résultats de la dernière recherche lancée, s'ils sont arrivés.
    pub(crate) fn take(&mut self) -> Option<Found> {
        let found = self.found.lock().ok()?.take()?;
        (found.generation == self.generation).then_some(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn ne_bloque_pas_et_annule_l_ancienne() {
        let mut editor = cupride_core::Editor::new();
        let document = editor.new_document();
        let text: Arc<str> = "une ligne avec un mot rare\n".repeat(500_000).into();
        let regex = cupride_core::search::SearchQuery::new("rare")
            .compile()
            .unwrap();
        let mut worker = SearchWorker::default();
        let (sender, done) = mpsc::channel();
        let first = sender.clone();
        let start = Instant::now();
        worker.start(
            move || first.send(1).unwrap(),
            document,
            1,
            Arc::clone(&text),
            regex.clone(),
        );
        // Une seconde recherche remplace la première.
        worker.start(move || sender.send(2).unwrap(), document, 2, text, regex);
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "l'appelant n'attend pas"
        );
        assert!(worker.covers(document, 2));
        let deadline = Duration::from_secs(30);
        // La seconde finit toujours ; la première a pu être annulée ou finir avant.
        while done.recv_timeout(deadline).unwrap() != 2 {}
        let found = worker.take().expect("résultats de la dernière recherche");
        assert_eq!(found.revision, 2);
        assert_eq!(
            found.matches.len(),
            cupride_core::search::MAX_MATCHES,
            "plafonnées"
        );
        assert!(worker.take().is_none(), "lus une seule fois");
    }
}
