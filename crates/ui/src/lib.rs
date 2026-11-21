//! Interface de CuprIDE : transforme l'état du noyau en listes de dessin.
//!
//! - [`EditorView`] : vue d'un document ; seules les lignes visibles sont mises en forme.
//! - [`Theme`] : couleurs de l'interface ; Padparadscha par défaut.

mod editor_view;
mod theme;

pub use editor_view::{EditorView, ViewSettings};
pub use theme::Theme;
