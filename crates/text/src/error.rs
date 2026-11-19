//! Erreurs de la crate `text`.

use thiserror::Error;

/// Erreur renvoyée par les opérations sur le texte.
///
/// Toutes les positions sont exprimées en octets UTF-8, sauf mention contraire.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TextError {
    /// La position dépasse la fin du texte.
    #[error("position {offset} hors du texte (longueur {len})")]
    OffsetOutOfBounds {
        /// Position demandée.
        offset: usize,
        /// Longueur du texte.
        len: usize,
    },

    /// La plage est inversée ou dépasse la fin du texte.
    #[error("plage {start}..{end} invalide (longueur {len})")]
    InvalidRange {
        /// Début de la plage.
        start: usize,
        /// Fin de la plage.
        end: usize,
        /// Longueur du texte.
        len: usize,
    },

    /// La position tombe au milieu d'un caractère UTF-8.
    #[error("position {offset} au milieu d'un caractère UTF-8")]
    NotCharBoundary {
        /// Position demandée.
        offset: usize,
    },

    /// La ligne n'existe pas.
    #[error("ligne {line} inexistante (le texte a {count} lignes)")]
    LineOutOfBounds {
        /// Ligne demandée (à partir de 0).
        line: usize,
        /// Nombre de lignes du texte.
        count: usize,
    },

    /// La colonne n'existe pas dans la ligne, ou tombe au milieu d'un caractère.
    #[error("colonne {column} invalide dans la ligne {line}")]
    InvalidColumn {
        /// Ligne (à partir de 0).
        line: usize,
        /// Colonne demandée, dans l'unité de la conversion utilisée.
        column: usize,
    },
}

/// Résultat des opérations sur le texte.
pub type Result<T> = std::result::Result<T, TextError>;
