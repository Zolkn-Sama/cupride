//! Sélecteurs : palette de commandes et ouverture rapide de fichiers.

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
    /// Fichiers du projet.
    Files,
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

    /// Ouvre l'ouverture rapide : les fichiers du projet (ou du dossier courant), indexés en
    /// arrière-plan ; la liste s'allonge au fil de l'indexation.
    pub(super) fn open_quick_open(&mut self) {
        self.refresh_file_index();
        let (items, priority) = self.file_items();
        self.picker = Some((
            PickerKind::Files,
            Picker::new("Fichier…", items, priority, true),
        ));
        self.focus.push(Element::Picker);
        self.blink.reset(Instant::now());
        self.request_redraw();
    }

    /// Lance (ou relance) l'indexation des fichiers du projet, ou du dossier courant sans projet.
    pub(super) fn refresh_file_index(&mut self) {
        let root = match self.editor.project() {
            Some(project) => project.to_owned(),
            None => match std::env::current_dir() {
                Ok(dir) => dir,
                Err(error) => {
                    tracing::warn!(
                        "dossier courant introuvable, pas d'index de fichiers : {error}"
                    );
                    return;
                }
            },
        };
        self.file_index.refresh(root, &self.proxy);
    }

    /// Éléments de l'ouverture rapide : les fichiers indexés, ceux déjà ouverts en tête.
    fn file_items(&self) -> (Vec<PickerItem>, Vec<usize>) {
        let root = self.file_index.root();
        let open: Vec<std::path::PathBuf> = self
            .editor
            .documents()
            .filter_map(|(_, d)| d.path().map(ToOwned::to_owned))
            .collect();
        let mut items = Vec::with_capacity(self.file_index.files().len());
        let mut priority = Vec::with_capacity(items.capacity());
        for file in self.file_index.files() {
            let is_open = root.is_some_and(|r| open.iter().any(|p| *p == r.join(file)));
            items.push(PickerItem {
                label: file.clone(),
                detail: String::new(),
                hint: if is_open {
                    "ouvert".to_owned()
                } else {
                    String::new()
                },
            });
            priority.push(usize::from(!is_open));
        }
        (items, priority)
    }

    /// Fichiers indexés arrivés : la liste de l'ouverture rapide est mise à jour.
    pub(super) fn files_indexed(&mut self) {
        let (changed, truncated) = self.file_index.drain();
        if truncated {
            self.editor.notify(
                cupride_core::notifications::Level::Warning,
                format!(
                    "projet très grand : seuls les {} premiers fichiers sont proposés",
                    crate::file_index::MAX_FILES
                ),
            );
        }
        if changed && matches!(self.picker, Some((PickerKind::Files, _))) {
            let (items, priority) = self.file_items();
            if let Some((_, picker)) = &mut self.picker {
                picker.set_items(items, priority);
            }
            self.request_redraw();
        }
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
            PickerKind::Files => {
                let Some(root) = self.file_index.root() else {
                    return;
                };
                let path = root.join(&item.label);
                let result = self
                    .editor
                    .run("file open", &[path.to_string_lossy().into_owned()]);
                self.after_command(result, Instant::now());
            }
        }
    }
}
