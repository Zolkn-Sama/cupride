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
