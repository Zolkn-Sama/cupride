//! Couches par-dessus l'éditeur : ligne de commande. Elles prennent le focus clavier en
//! s'ouvrant et le rendent en se fermant.

use std::time::Instant;

use cupride_core::UiRequest;
use cupride_core::clipboard::ClipboardKind;
use cupride_core::command::CommandOutput;
use cupride_core::keymap::{KeyCombo, Platform};
use cupride_ui::{CommandLine, InputEvent, MessageKind};

use super::App;

/// Éléments qui peuvent avoir le focus clavier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Element {
    Editor,
    CommandLine,
}

impl App {
    /// Exécute les demandes déposées par les commandes (`ui command-line`…).
    pub(super) fn process_ui_requests(&mut self) {
        for request in self.editor.take_ui_requests() {
            match request {
                UiRequest::CommandLine(text) => self.open_command_line(&text),
            }
        }
    }

    pub(super) fn open_command_line(&mut self, text: &str) {
        self.command_line = Some(CommandLine::new(text));
        self.focus.push(Element::CommandLine);
        self.blink.reset(Instant::now());
        self.request_redraw();
    }

    pub(super) fn close_command_line(&mut self) {
        self.command_line = None;
        self.focus.remove(Element::CommandLine);
        self.request_redraw();
    }

    /// Ferme toutes les couches (clic dans l'éditeur).
    pub(super) fn close_overlays(&mut self) {
        if self.command_line.is_some() {
            self.close_command_line();
        }
    }

    /// Touche pour la couche qui a le focus. Renvoie faux si elle ne la traite pas : elle passe
    /// alors au keymap (les raccourcis globaux restent utilisables).
    pub(super) fn overlay_key(&mut self, combo: Option<KeyCombo>, text: Option<&str>) -> bool {
        if self.focus.current() != Element::CommandLine {
            return false;
        }
        let Some(line) = &mut self.command_line else {
            return false;
        };
        if let Some(text) = text {
            line.input.insert(text);
            line.clear_message_if_error();
            self.after_overlay_edit();
            return true;
        }
        let Some(combo) = combo else {
            return true;
        };
        match line.input.key(combo, Platform::current()) {
            InputEvent::Handled => self.after_overlay_edit(),
            InputEvent::Submit => self.submit_command_line(),
            InputEvent::Cancel => self.close_command_line(),
            InputEvent::Copy(text) => {
                if let Err(error) = self
                    .editor
                    .clipboard_mut()
                    .set(ClipboardKind::Standard, text)
                {
                    tracing::warn!("presse-papiers : {error}");
                }
                self.after_overlay_edit();
            }
            InputEvent::Paste => {
                if let Some(text) = self.editor.clipboard_mut().get(ClipboardKind::Standard)
                    && let Some(line) = &mut self.command_line
                {
                    line.input.insert(&text);
                }
                self.after_overlay_edit();
            }
            InputEvent::Up | InputEvent::Down | InputEvent::Complete => {}
            InputEvent::Ignored => return false,
        }
        true
    }

    /// Texte validé par une méthode de saisie, pour la couche qui a le focus.
    pub(super) fn overlay_commit(&mut self, text: &str) -> bool {
        self.overlay_key(None, Some(text))
    }

    fn after_overlay_edit(&mut self) {
        self.blink.reset(Instant::now());
        self.request_redraw();
    }

    /// Entrée dans la ligne de commande : exécute la commande. Sans résultat, la ligne se
    /// ferme ; une erreur s'affiche au-dessus, texte gardé pour être corrigé ; une sortie
    /// s'affiche au-dessus d'une ligne vidée.
    fn submit_command_line(&mut self) {
        let Some(line) = &self.command_line else {
            return;
        };
        let text = line.input.text().trim().to_owned();
        if text.is_empty() {
            self.close_command_line();
            return;
        }
        tracing::debug!(commande = text, "ligne de commande");
        let result = self.editor.execute(&text);
        let start = Instant::now();
        match result {
            Ok(CommandOutput::None) => self.close_command_line(),
            Ok(CommandOutput::Message(message)) => {
                if let Some(line) = &mut self.command_line {
                    line.input.set_text("");
                    line.set_message(MessageKind::Output, &message);
                }
            }
            Err(error) => {
                if let Some(line) = &mut self.command_line {
                    line.set_message(MessageKind::Error, &error.to_string());
                }
            }
        }
        self.after_any_command(start);
    }
}
