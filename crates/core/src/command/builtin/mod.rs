//! Commandes intégrées au noyau. Leur référence est tenue dans `docs/commands.md`.

mod edit;
mod file;

use super::{CommandError, CommandRegistry};
use crate::document::Document;
use crate::editor::{DocumentId, Editor};

/// Enregistre toutes les commandes intégrées.
pub(crate) fn register_all(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    file::register(registry)?;
    edit::register(registry)?;
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
