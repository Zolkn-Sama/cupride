//! Événements du noyau.
//!
//! Le noyau publie ce qui arrive aux documents : l'interface, le client LSP et les plugins s'y
//! abonnent pour réagir, sans que le noyau dépende d'eux. Deux façons de s'abonner :
//!
//! - une fonction appelée à chaque événement ([`EventBus::subscribe`]), sur le thread du noyau ;
//! - un canal ([`EventBus::subscribe_channel`]), pour recevoir les événements sur un autre thread.
//!
//! Les événements sont publiés à la fin de chaque commande, et après chaque appel de l'[`Editor`]
//! qui ouvre, ferme ou active un document. Une modification faite directement sur un document
//! (sans passer par une commande) est publiée au prochain [`Editor::flush_events`].
//!
//! [`Editor`]: crate::Editor
//! [`Editor::flush_events`]: crate::Editor::flush_events

use std::fmt;
use std::sync::mpsc;

use crate::editor::DocumentId;

/// Ce qui est arrivé dans le noyau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Un document a été ouvert ou créé.
    DocumentOpened(DocumentId),
    /// Le texte d'un document a changé.
    DocumentModified(DocumentId),
    /// Un document a été enregistré.
    DocumentSaved(DocumentId),
    /// Un document a été fermé.
    DocumentClosed(DocumentId),
    /// Le document actif a changé.
    FocusChanged {
        /// Document actif avant.
        from: Option<DocumentId>,
        /// Document actif après.
        to: Option<DocumentId>,
    },
}

/// Identifiant d'un abonnement, pour se désabonner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

type Callback = Box<dyn FnMut(&Event) + Send>;

/// Distribue les événements aux abonnés.
#[derive(Default)]
pub struct EventBus {
    next_id: u64,
    callbacks: Vec<(SubscriptionId, Callback)>,
    channels: Vec<mpsc::Sender<Event>>,
}

impl fmt::Debug for EventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventBus")
            .field("callbacks", &self.callbacks.len())
            .field("channels", &self.channels.len())
            .finish()
    }
}

impl EventBus {
    /// Aucun abonné.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appelle `callback` à chaque événement.
    pub fn subscribe(&mut self, callback: impl FnMut(&Event) + Send + 'static) -> SubscriptionId {
        let id = SubscriptionId(self.next_id);
        self.next_id += 1;
        self.callbacks.push((id, Box::new(callback)));
        id
    }

    /// Retire un abonnement. Renvoie `false` s'il n'existait pas.
    pub fn unsubscribe(&mut self, id: SubscriptionId) -> bool {
        let before = self.callbacks.len();
        self.callbacks.retain(|(sub, _)| *sub != id);
        self.callbacks.len() != before
    }

    /// Renvoie un canal qui reçoit tous les événements. L'abonnement prend fin quand le
    /// récepteur est détruit.
    pub fn subscribe_channel(&mut self) -> mpsc::Receiver<Event> {
        let (sender, receiver) = mpsc::channel();
        self.channels.push(sender);
        receiver
    }

    /// Nombre d'abonnés (fonctions et canaux encore ouverts).
    pub fn subscriber_count(&self) -> usize {
        self.callbacks.len() + self.channels.len()
    }

    /// Publie un événement à tous les abonnés.
    pub fn publish(&mut self, event: Event) {
        for (_, callback) in &mut self.callbacks {
            callback(&event);
        }
        self.channels.retain(|sender| sender.send(event).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::Editor;

    /// Abonne un journal à l'éditeur et le renvoie.
    fn record(editor: &mut Editor) -> Arc<Mutex<Vec<Event>>> {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&log);
        editor
            .events_mut()
            .subscribe(move |event| sink.lock().unwrap().push(*event));
        log
    }

    fn take(log: &Arc<Mutex<Vec<Event>>>) -> Vec<Event> {
        std::mem::take(&mut *log.lock().unwrap())
    }

    #[test]
    fn cycle_de_vie_d_un_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "texte").unwrap();
        let mut editor = Editor::new();
        let log = record(&mut editor);

        editor
            .execute(&format!("file open '{}'", path.display()))
            .unwrap();
        let id = editor.active_id().unwrap();
        assert_eq!(
            take(&log),
            [
                Event::DocumentOpened(id),
                Event::FocusChanged {
                    from: None,
                    to: Some(id)
                }
            ]
        );

        editor.execute("edit insert x").unwrap();
        assert_eq!(take(&log), [Event::DocumentModified(id)]);

        editor.execute("edit move right").unwrap();
        assert_eq!(take(&log), [], "un déplacement ne modifie pas le texte");

        editor.execute("file save").unwrap();
        assert_eq!(take(&log), [Event::DocumentSaved(id)]);

        editor.execute("edit undo").unwrap();
        assert_eq!(take(&log), [Event::DocumentModified(id)]);

        editor.execute("file close --force").unwrap();
        assert_eq!(
            take(&log),
            [
                Event::DocumentClosed(id),
                Event::FocusChanged {
                    from: Some(id),
                    to: None
                }
            ]
        );
    }

    #[test]
    fn rouvrir_un_document_ouvert_ne_publie_que_le_focus() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let mut editor = Editor::new();
        let id_a = editor.open(&a).unwrap();
        let id_b = editor.open(&b).unwrap();
        let log = record(&mut editor);
        editor.open(&a).unwrap();
        assert_eq!(
            take(&log),
            [Event::FocusChanged {
                from: Some(id_b),
                to: Some(id_a)
            }]
        );
        editor.open(&a).unwrap();
        assert_eq!(take(&log), []);
    }

    #[test]
    fn modification_directe_publiee_au_prochain_flush() {
        let mut editor = Editor::new();
        let id = editor.new_document();
        let log = record(&mut editor);
        editor
            .document_mut(id)
            .unwrap()
            .transact(cupride_text::EditKind::Insert, |b, tx| {
                tx.apply(b, cupride_text::Edit::insert(0, "x"))
            })
            .unwrap();
        assert_eq!(take(&log), []);
        editor.flush_events();
        assert_eq!(take(&log), [Event::DocumentModified(id)]);
    }

    #[test]
    fn canal_sur_un_autre_thread() {
        let mut editor = Editor::new();
        let receiver = editor.events_mut().subscribe_channel();
        let listener = std::thread::spawn(move || receiver.iter().take(2).collect::<Vec<_>>());
        let id = editor.new_document();
        let received = listener.join().unwrap();
        assert_eq!(
            received,
            [
                Event::DocumentOpened(id),
                Event::FocusChanged {
                    from: None,
                    to: Some(id)
                }
            ]
        );
        // Le récepteur détruit, l'abonnement disparaît au prochain événement.
        editor.new_document();
        assert_eq!(editor.events().subscriber_count(), 0);
    }

    #[test]
    fn desabonnement() {
        let mut bus = EventBus::new();
        let count = Arc::new(Mutex::new(0));
        let sink = Arc::clone(&count);
        let id = bus.subscribe(move |_| *sink.lock().unwrap() += 1);
        let doc = Editor::new().new_document();
        bus.publish(Event::DocumentSaved(doc));
        assert!(bus.unsubscribe(id));
        assert!(!bus.unsubscribe(id));
        bus.publish(Event::DocumentSaved(doc));
        assert_eq!(*count.lock().unwrap(), 1);
    }
}
