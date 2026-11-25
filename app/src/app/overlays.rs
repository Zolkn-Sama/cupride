//! Couches par-dessus l'éditeur : ligne de commande. Elles prennent le focus clavier en
//! s'ouvrant et le rendent en se fermant.

use std::time::Instant;

use cupride_core::UiRequest;
use cupride_core::clipboard::ClipboardKind;
use cupride_core::command::CommandOutput;
use cupride_core::command::complete::{self, Completion};
use cupride_core::keymap::{KeyCombo, Platform};
use cupride_ui::{CommandLine, InputEvent, MessageKind};

use super::App;

/// Complétion en cours : propositions et rang de celle affichée (Tab répété les parcourt).
#[derive(Debug, Clone)]
pub(super) struct CompletionCycle {
    completion: Completion,
    index: Option<usize>,
}

/// Nombre maximal de propositions listées sous la ligne.
const SHOWN_CANDIDATES: usize = 12;

/// Éléments qui peuvent avoir le focus clavier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Element {
    Editor,
    CommandLine,
    /// Palette de commandes ou ouverture rapide.
    Picker,
}

impl App {
    /// Exécute les demandes déposées par les commandes (`ui command-line`…).
    pub(super) fn process_ui_requests(&mut self) {
        for request in self.editor.take_ui_requests() {
            match request {
                UiRequest::CommandLine(text) => self.open_command_line(&text),
                UiRequest::Palette => {
                    self.close_overlays();
                    self.open_palette();
                }
                UiRequest::QuickOpen => {
                    self.close_overlays();
                    self.open_quick_open();
                }
                UiRequest::Reveal(path) => {
                    if let Some(state) = &mut self.window {
                        for view in state.views.views_mut() {
                            view.reveal(&path);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn open_command_line(&mut self, text: &str) {
        if self.picker.is_some() {
            self.close_picker();
        }
        self.command_line = Some(CommandLine::new(text));
        self.history.reset();
        self.completion = None;
        self.refresh_hint();
        self.focus.push(Element::CommandLine);
        self.blink.reset(Instant::now());
        self.request_redraw();
    }

    pub(super) fn close_command_line(&mut self) {
        self.command_line = None;
        self.history.reset();
        self.completion = None;
        self.focus.remove(Element::CommandLine);
        self.request_redraw();
    }

    /// Ferme toutes les couches (clic dans l'éditeur).
    pub(super) fn close_overlays(&mut self) {
        if self.command_line.is_some() {
            self.close_command_line();
        }
        if self.picker.is_some() {
            self.close_picker();
        }
    }

    /// Touche pour le sélecteur ouvert.
    fn picker_key(&mut self, combo: Option<KeyCombo>, text: Option<&str>) -> bool {
        let Some((_, picker)) = &mut self.picker else {
            return false;
        };
        if let Some(text) = text {
            picker.input.insert(text);
            self.after_overlay_edit();
            return true;
        }
        let Some(combo) = combo else {
            return true;
        };
        match picker.input.key(combo, Platform::current()) {
            InputEvent::Handled | InputEvent::Complete => {}
            InputEvent::Up => picker.move_selection(-1),
            InputEvent::Down => picker.move_selection(1),
            InputEvent::Submit => {
                self.choose(None);
                return true;
            }
            InputEvent::Cancel => {
                self.close_picker();
                return true;
            }
            InputEvent::Copy(text) => {
                if let Err(error) = self
                    .editor
                    .clipboard_mut()
                    .set(ClipboardKind::Standard, text)
                {
                    tracing::warn!("presse-papiers : {error}");
                }
            }
            InputEvent::Paste => {
                if let Some(text) = self.editor.clipboard_mut().get(ClipboardKind::Standard)
                    && let Some((_, picker)) = &mut self.picker
                {
                    picker.input.insert(&text);
                }
            }
            InputEvent::Ignored => return false,
        }
        self.after_overlay_edit();
        true
    }

    /// Touche pour la couche qui a le focus. Renvoie faux si elle ne la traite pas : elle passe
    /// alors au keymap (les raccourcis globaux restent utilisables).
    pub(super) fn overlay_key(&mut self, combo: Option<KeyCombo>, text: Option<&str>) -> bool {
        match self.focus.current() {
            Element::Picker => return self.picker_key(combo, text),
            Element::CommandLine => {}
            Element::Editor => return false,
        }
        let Some(line) = &mut self.command_line else {
            return false;
        };
        if let Some(text) = text {
            line.input.insert(text);
            line.clear_message_if_error();
            self.completion = None;
            self.refresh_hint();
            self.after_overlay_edit();
            return true;
        }
        let Some(combo) = combo else {
            return true;
        };
        let event = line.input.key(combo, Platform::current());
        if event != InputEvent::Complete {
            self.completion = None;
        }
        match event {
            InputEvent::Handled => {
                self.refresh_hint();
                self.after_overlay_edit();
            }
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
            InputEvent::Up | InputEvent::Down => {
                let current = line.input.text().to_owned();
                let entry = if event == InputEvent::Up {
                    self.history.previous(&current)
                } else {
                    self.history.following()
                };
                if let Some(entry) = entry.map(ToOwned::to_owned)
                    && let Some(line) = &mut self.command_line
                {
                    line.input.set_text(&entry);
                }
                self.after_overlay_edit();
            }
            InputEvent::Complete => {
                self.complete();
                self.after_overlay_edit();
                return true;
            }
            InputEvent::Ignored => return false,
        }
        true
    }

    /// Texte validé par une méthode de saisie, pour la couche qui a le focus.
    pub(super) fn overlay_commit(&mut self, text: &str) -> bool {
        self.overlay_key(None, Some(text))
    }

    /// Tab : une proposition est complétée ; plusieurs : leur début commun, et elles sont
    /// listées sous la ligne ; Tab de nouveau les parcourt une à une.
    fn complete(&mut self) {
        let Some(line) = &mut self.command_line else {
            return;
        };
        if let Some(cycle) = &mut self.completion {
            let count = cycle.completion.candidates.len();
            let index = cycle.index.map_or(0, |i| (i + 1) % count);
            cycle.index = Some(index);
            let candidate = &cycle.completion.candidates[index];
            line.input
                .replace(cycle.completion.start..line.input.cursor(), candidate);
            return;
        }
        let completion = complete::complete(
            self.editor.commands(),
            line.input.text(),
            line.input.cursor(),
        );
        match completion.candidates.len() {
            0 => line.set_hint(Some("aucune complétion".to_owned())),
            1 => {
                let candidate = &completion.candidates[0];
                let space = if candidate.ends_with('/') { "" } else { " " };
                line.input.replace(
                    completion.start..line.input.cursor(),
                    &format!("{candidate}{space}"),
                );
                self.refresh_hint();
            }
            count => {
                line.input.replace(
                    completion.start..line.input.cursor(),
                    &completion.common_prefix(),
                );
                let mut shown: Vec<&str> = completion
                    .candidates
                    .iter()
                    .take(SHOWN_CANDIDATES)
                    .map(String::as_str)
                    .collect();
                if count > SHOWN_CANDIDATES {
                    shown.push("…");
                }
                line.set_hint(Some(shown.join("  ")));
                self.completion = Some(CompletionCycle {
                    completion,
                    index: None,
                });
            }
        }
    }

    /// Aide de la commande en cours de saisie, sous la ligne.
    fn refresh_hint(&mut self) {
        if let Some(line) = &mut self.command_line {
            let hint = complete::help_hint(self.editor.commands(), line.input.text());
            line.set_hint(hint);
        }
    }

    pub(super) fn after_overlay_edit(&mut self) {
        self.blink.reset(Instant::now());
        self.request_redraw();
    }

    /// Ajoute une commande à l'historique, gardé entre les sessions.
    pub(super) fn remember_command(&mut self, command: &str) {
        self.history.push(command);
        if let Some(path) = &self.history_path
            && let Err(error) = self.history.save(path)
        {
            tracing::warn!("historique des commandes non enregistré : {error}");
        }
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
        self.remember_command(&text);
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
