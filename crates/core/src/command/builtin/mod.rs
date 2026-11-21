//! Commandes intégrées au noyau. Leur référence est tenue dans `docs/commands.md`.

mod config;
mod edit;
mod file;
mod notifications;
mod ui;
mod view;

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
