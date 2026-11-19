//! Buffer de texte de CuprIDE.
//!
//! - [`TextBuffer`] : API commune des buffers (positions en octets UTF-8).
//! - [`Rope`] : implémentation utilisée par l'éditeur, en O(log n) quelle que soit la taille.
//! - [`StringBuffer`] : implémentation de référence, simple et évidente, utilisée pour tester
//!   la rope.
//! - [`movement`] : déplacements par graphème, mot, ligne, et plages de suppression.
//! - [`position`] : conversions octets ↔ ligne et colonne (octets, graphèmes, UTF-16).
//!
//! Cette crate n'a aucune dépendance interne.

#[cfg(test)]
pub(crate) mod suite;

mod buffer;
mod error;
pub mod movement;
pub mod position;
mod rope;
mod string_buffer;

pub use buffer::{Lines, TextBuffer};
pub use error::{Result, TextError};
pub use position::Point;
pub use rope::Rope;
pub use string_buffer::StringBuffer;
