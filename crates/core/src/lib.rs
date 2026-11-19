//! Noyau de CuprIDE, sans interface graphique.
//!
//! - [`Editor`] : état global de l'éditeur (documents ouverts, document actif, workspace).
//! - [`Document`] : un texte, son fichier, son historique et ses sélections.
//!
//! Ne dépend jamais de `render`, `ui` ou `app` : tout le noyau se teste avec `cargo test` et se
//! pilote depuis le REPL console.

mod document;
mod editor;

pub use cupride_layout as layout;
pub use cupride_text as text;
pub use document::{Document, DocumentError, LineEnding};
pub use editor::{DocumentId, Editor};
