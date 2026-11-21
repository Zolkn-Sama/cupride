//! Interface de CuprIDE : transforme l'état du noyau en listes de dessin.
//!
//! - [`EditorView`] : vue d'un document ; seules les lignes visibles sont mises en forme ;
//!   sélections, curseurs et ligne courante ; défilement animé et vue qui suit le curseur.
//! - [`GutterMarker`] : marqueur de la gouttière (diagnostic, Git, point d'arrêt), à côté des
//!   numéros de ligne.
//! - [`MouseSelection`] : clics simples, doubles et triples, glisser, Maj+clic et Alt+clic.
//! - [`CursorBlink`] : clignotement du curseur, arrêté au repos.
//! - [`Theme`] : couleurs de l'interface ; Padparadscha par défaut.

mod blink;
mod editor_view;
mod gutter;
mod mouse;
mod scroll;
mod theme;

pub use blink::{BLINK_IDLE_STOP, CursorBlink};
pub use editor_view::{EditorView, Pointer, Preedit, ViewSettings, WHEEL_LINES};
pub use gutter::GutterMarker;
pub use mouse::{ClickCounter, ClickModifiers, MULTI_CLICK_DELAY, MouseSelection};
pub use theme::Theme;
