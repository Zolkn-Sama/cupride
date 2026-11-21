//! Sélecteurs : palette de commandes.

use std::time::Instant;

use cupride_core::command::CommandDef;
use cupride_core::keymap::{KeySequence, Platform};
use cupride_ui::{Picker, PickerItem};

use super::App;
use super::overlays::Element;

/// Ce que liste le sélecteur ouvert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PickerKind {
    /// Palette de commandes.
    Commands,
}

/// Raccourci tel qu'affiché : `cmd` sous macOS plutôt que `meta`.
fn display_keys(keys: &KeySequence) -> String {
    let text = keys.to_string();
    match Platform::current() {
        Platform::MacOs => text.replace("meta", "cmd"),
        Platform::Other => text,
    }
}

/// La commande a-t-elle des arguments obligatoires ? Elle passe alors par la ligne de commande.
fn needs_arguments(command: &CommandDef) -> bool {
    command
        .args()
        .get_arguments()
        .any(|arg| arg.is_required_set())
}

impl App {
    /// Ouvre la palette : toutes les commandes, les plus récemment utilisées en tête.
    pub(super) fn open_palette(&mut self) {
        let commands: Vec<_> = self.editor.commands().list().collect();
        let entries = self.history.entries();
        let items: Vec<PickerItem> = commands
            .iter()
            .map(|command| {
                let name = command.full_name();
                let hint = self
                    .editor
                    .keymap()
                    .keys_for(&name)
                    .next()
                    .map(display_keys)
                    .unwrap_or_default();
                PickerItem {
                    hint,
                    detail: command.description().to_owned(),
                    label: name,
                }
            })
            .collect();
        // Récence : rang depuis la fin de l'historique de la commande la plus récente.
        let priority: Vec<usize> = items
            .iter()
            .map(|item| {
                entries
                    .iter()
                    .rev()
                    .position(|entry| {
                        entry == &item.label || entry.starts_with(&format!("{} ", item.label))
                    })
                    .unwrap_or(usize::MAX)
            })
            .collect();
        self.picker = Some((
            PickerKind::Commands,
            Picker::new("Commande…", items, priority, false),
        ));
        self.focus.push(Element::Picker);
        self.blink.reset(Instant::now());
        self.request_redraw();
    }

    pub(super) fn close_picker(&mut self) {
        self.picker = None;
        self.focus.remove(Element::Picker);
        self.request_redraw();
    }

    /// Entrée ou clic : la commande choisie est exécutée ; si elle attend des arguments, la ligne
    /// de commande s'ouvre, préremplie.
    pub(super) fn choose(&mut self, index: Option<usize>) {
        let Some((kind, picker)) = &mut self.picker else {
            return;
        };
        let Some(index) = index.or_else(|| picker.selected()) else {
            return;
        };
        let Some(item) = picker.item(index).cloned() else {
            return;
        };
        let kind = *kind;
        self.close_picker();
        match kind {
            PickerKind::Commands => {
                let Some(command) = self.editor.commands().get(&item.label) else {
                    return;
                };
                if needs_arguments(&command) {
                    self.open_command_line(&format!("{} ", item.label));
                } else {
                    self.remember_command(&item.label);
                    self.execute(&item.label, Instant::now());
                }
            }
        }
    }
}
