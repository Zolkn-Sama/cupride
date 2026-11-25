//! Surveillance du disque (`notify`) : les dossiers affichés par l'explorateur, sans récursion.
//!
//! Les événements arrivent sur un thread de `notify`. Les chemins changés s'accumulent, et la
//! boucle d'événements n'est réveillée qu'une fois par rafale : le thread principal relit ensuite
//! les dossiers concernés, en une seule fois.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use notify::{RecursiveMode, Watcher};
use winit::event_loop::EventLoopProxy;

use crate::file_index::AppEvent;

/// Dossiers surveillés et chemins changés en attente.
pub(crate) struct DiskWatcher {
    watcher: Option<notify::RecommendedWatcher>,
    watched: BTreeSet<PathBuf>,
    changed: Arc<Mutex<Vec<PathBuf>>>,
    proxy: EventLoopProxy<AppEvent>,
    /// La surveillance est impossible (signalé une fois).
    failed: bool,
}

impl DiskWatcher {
    pub(crate) fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            watcher: None,
            watched: BTreeSet::new(),
            changed: Arc::new(Mutex::new(Vec::new())),
            proxy,
            failed: false,
        }
    }

    /// Surveille exactement les dossiers `wanted` : les nouveaux sont ajoutés, les autres
    /// retirés.
    pub(crate) fn sync(&mut self, wanted: BTreeSet<PathBuf>) {
        if wanted == self.watched || self.failed {
            return;
        }
        if self.watcher.is_none() {
            let changed = Arc::clone(&self.changed);
            let proxy = self.proxy.clone();
            let created =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    let Ok(event) = event else {
                        return;
                    };
                    if event.paths.is_empty() {
                        return;
                    }
                    let Ok(mut pending) = changed.lock() else {
                        return;
                    };
                    let wake = pending.is_empty();
                    pending.extend(event.paths);
                    if wake {
                        let _ = proxy.send_event(AppEvent::FilesChanged);
                    }
                });
            match created {
                Ok(watcher) => self.watcher = Some(watcher),
                Err(error) => {
                    tracing::warn!("surveillance du disque impossible : {error}");
                    self.failed = true;
                    return;
                }
            }
        }
        let Some(watcher) = &mut self.watcher else {
            return;
        };
        for dir in self.watched.difference(&wanted) {
            let _ = watcher.unwatch(dir);
        }
        for dir in wanted.difference(&self.watched) {
            if let Err(error) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                tracing::debug!(dossier = %dir.display(), "non surveillé : {error}");
            }
        }
        self.watched = wanted;
    }

    /// Chemins changés depuis le dernier appel.
    pub(crate) fn take_changed(&mut self) -> Vec<PathBuf> {
        self.changed
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default()
    }
}
