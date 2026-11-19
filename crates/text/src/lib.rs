//! Buffer de texte de CuprIDE.
//!
//! - [`TextBuffer`] : API commune des buffers (positions en octets UTF-8).
//! - [`StringBuffer`] : implémentation de référence, simple et évidente, utilisée pour tester
//!   les autres.
//!
//! Cette crate n'a aucune dépendance interne.

#[cfg(test)]
pub(crate) mod suite;

mod buffer;
mod error;
mod string_buffer;

pub use buffer::{Lines, TextBuffer};
pub use error::{Result, TextError};
pub use string_buffer::StringBuffer;
