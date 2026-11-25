//! Commandes intégrées au noyau. Leur référence est tenue dans `docs/commands.md`.

mod config;
mod edit;
mod explorer;
mod file;
mod keys;
mod layout;
mod notifications;
mod panel;
mod recovery;
mod search;
mod tab;
mod theme;
mod ui;
mod view;
mod window;

use super::{CommandError, CommandRegistry};
use crate::document::Document;
use crate::editor::{DocumentId, Editor};

/// Enregistre toutes les commandes intégrées.
pub(crate) fn register_all(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    file::register(registry)?;
    edit::register(registry)?;
    config::register(registry)?;
    view::register(registry)?;
    ui::register(registry)?;
    notifications::register(registry)?;
    keys::register(registry)?;
    theme::register(registry)?;
    panel::register(registry)?;
    tab::register(registry)?;
    layout::register(registry)?;
    explorer::register(registry)?;
    window::register(registry)?;
    search::register(registry)?;
    recovery::register(registry)?;
    Ok(())
}

/// Document actif et son identifiant, ou erreur s'il n'y en a pas.
pub(crate) fn active(editor: &mut Editor) -> Result<(DocumentId, &mut Document), CommandError> {
    let id = editor.active_id().ok_or(CommandError::NoActiveDocument)?;
    let document = editor
        .document_mut(id)
        .ok_or(CommandError::NoActiveDocument)?;
    Ok((id, document))
}
