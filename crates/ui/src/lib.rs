//! Interface de CuprIDE : transforme l'état du noyau en listes de dessin.
//!
//! - [`EditorView`] : vue d'un document ; seules les lignes visibles sont mises en forme ;
//!   sélections, curseurs et ligne courante.
//! - [`CursorBlink`] : clignotement du curseur, arrêté au repos.
//! - [`Theme`] : couleurs de l'interface ; Padparadscha par défaut.

mod blink;
mod editor_view;
mod theme;

pub use blink::{BLINK_IDLE_STOP, CursorBlink};
pub use editor_view::{EditorView, ViewSettings};
pub use theme::Theme;
