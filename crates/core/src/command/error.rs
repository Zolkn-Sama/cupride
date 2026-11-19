//! Erreurs des commandes.

use cupride_text::TextError;
use thiserror::Error;

use crate::document::DocumentError;

/// Erreur typée renvoyée par une commande ; son message est destiné à l'utilisateur.
#[derive(Debug, Error)]
pub enum CommandError {
    /// Aucune commande ne porte ce nom.
    #[error("commande inconnue : « {name} »{}", suggestion.as_ref().map(|s| format!(" (vouliez-vous dire « {s} » ?)")).unwrap_or_default())]
    Unknown {
        /// Nom demandé.
        name: String,
        /// Commande au nom le plus proche, s'il y en a une.
        suggestion: Option<String>,
    },

    /// La ligne de commande est mal formée (guillemet non fermé…).
    #[error("ligne de commande invalide : {0}")]
    Syntax(String),

    /// Une commande porte déjà ce nom.
    #[error("la commande « {name} » est déjà enregistrée")]
    AlreadyRegistered {
        /// Nom de la commande.
        name: String,
    },

    /// Les arguments ne correspondent pas à la définition de la commande.
    #[error("{command} : {message}")]
    InvalidArguments {
        /// Nom de la commande.
        command: String,
        /// Explication pour l'utilisateur.
        message: String,
    },

    /// La commande agit sur le document actif, et il n'y en a pas.
    #[error("aucun document ouvert")]
    NoActiveDocument,

    /// Erreur sur un document (ouverture, enregistrement…).
    #[error(transparent)]
    Document(#[from] DocumentError),

    /// Erreur sur le texte (position invalide…).
    #[error(transparent)]
    Text(#[from] TextError),

    /// Autre échec, avec un message pour l'utilisateur.
    #[error("{0}")]
    Failed(String),
}
