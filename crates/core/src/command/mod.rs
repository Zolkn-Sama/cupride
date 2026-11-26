//! Système de commandes : toute action de l'IDE est une commande.
//!
//! Raccourcis, palette, ligne de commande interne, CLI externe et plugins passent tous par le même
//! [`CommandRegistry`]. Une commande est identifiée par son groupe et son nom (`file open`), et ses
//! arguments sont décrits avec `clap` : la même définition sert partout.

pub(crate) mod builtin;
pub mod complete;
mod error;
pub mod parser;
mod registry;

pub use error::CommandError;
pub use registry::{CommandDef, CommandRegistry, Handler};

/// Argument entre guillemets si besoin, pour composer une ligne de commande (`file open 'mon
/// fichier.rs'`).
pub fn quote(argument: &str) -> String {
    shlex::try_quote(argument).map_or_else(|_| argument.to_owned(), |q| q.into_owned())
}

/// Document actif et son identifiant, ou erreur s'il n'y en a pas.
pub(crate) fn builtin_active(
    editor: &mut crate::Editor,
) -> Result<(crate::DocumentId, &mut crate::Document), CommandError> {
    builtin::active(editor)
}

/// Symboles du document actif (plan) : ligne et colonne de leur nom (à partir de 1), symbole.
pub fn outline(
    editor: &crate::Editor,
) -> Result<Vec<(usize, usize, cupride_syntax::Symbol)>, CommandError> {
    builtin::outline(editor)
}

/// Registre des seules commandes intégrées : pour reconnaître une commande sans éditeur (CLI).
pub fn builtin_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();
    builtin::register_all(&mut registry).expect("les commandes intégrées ont des noms uniques");
    registry
}

/// Résultat d'une commande.
pub type CommandResult = Result<CommandOutput, CommandError>;

/// Ce qu'une commande réussie renvoie à l'appelant (affiché dans la barre d'état ou la console).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CommandOutput {
    /// Rien à afficher.
    #[default]
    None,
    /// Message court pour l'utilisateur.
    Message(String),
}

impl CommandOutput {
    /// Message pour l'utilisateur.
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}
