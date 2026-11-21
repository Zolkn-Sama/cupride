//! Interface de CuprIDE : transforme l'état du noyau en listes de dessin.
//!
//! Boîte à outils commune à tous les panneaux :
//!
//! - [`layout`] : découpage de l'espace en lignes et colonnes (tailles fixes, proportions) ;
//! - [`Focus`] : focus clavier (pile : une couche ouverte prend le focus, le rend en se fermant)
//!   et survol de la souris ;
//! - couches superposées : chaque popup se dessine dans sa propre [`cupride_render::Layer`],
//!   au-dessus des panneaux ;
//! - [`Label`] : texte d'interface, mis en forme seulement quand il change ;
//! - [`Theme`] : toutes les couleurs ; aucun élément n'a de couleur en dur.
//!
//! Éléments :
//!
//! - [`EditorView`] : vue d'un document ; seules les lignes visibles sont mises en forme ;
//!   sélections, curseurs et ligne courante ; défilement animé et vue qui suit le curseur.
//! - [`GutterMarker`] : marqueur de la gouttière (diagnostic, Git, point d'arrêt), à côté des
//!   numéros de ligne.
//! - [`MouseSelection`] : clics simples, doubles et triples, glisser, Maj+clic et Alt+clic.
//! - [`CursorBlink`] : clignotement du curseur, arrêté au repos.

mod blink;
mod editor_view;
mod focus;
mod gutter;
mod label;
pub mod layout;
mod mouse;
mod scroll;
mod theme;

pub use blink::{BLINK_IDLE_STOP, CursorBlink};
pub use editor_view::{EditorView, Pointer, Preedit, ViewSettings, WHEEL_LINES};
pub use focus::Focus;
pub use gutter::GutterMarker;
pub use label::Label;
pub use mouse::{ClickCounter, ClickModifiers, MULTI_CLICK_DELAY, MouseSelection};
pub use theme::Theme;
