//! Interface de CuprIDE : transforme l'état du noyau en listes de dessin.
//!
//! Boîte à outils commune à tous les panneaux :
//!
//! - [`layout`] : découpage de l'espace en lignes et colonnes (tailles fixes, proportions) ;
//! - [`Focus`] : focus clavier (pile : une couche ouverte prend le focus, le rend en se fermant)
//!   et survol de la souris ;
//! - couches superposées : chaque popup se dessine dans sa propre [`cupride_render::Layer`],
//!   au-dessus des panneaux ;
//! - [`Label`] : texte d'interface, mis en forme seulement quand il change ; [`Style`] : police
//!   et dimensions communes ;
//! - [`Theme`] : toutes les couleurs ; aucun élément n'a de couleur en dur. [`Themes`] : thèmes
//!   intégrés (Padparadscha, Clair, Sombre) et de l'utilisateur, au format TOML.
//!
//! Éléments :
//!
//! - [`EditorView`] : vue d'un document ; seules les lignes visibles sont mises en forme ;
//!   sélections, curseurs et ligne courante ; défilement animé et vue qui suit le curseur.
//! - [`GutterMarker`] : marqueur de la gouttière (diagnostic, Git, point d'arrêt), à côté des
//!   numéros de ligne.
//! - [`MouseSelection`] : clics simples, doubles et triples, glisser, Maj+clic et Alt+clic.
//! - [`TextInput`] : champ de saisie d'une ligne ; [`CommandLine`] : ligne de commande interne.
//! - [`StatusBar`] : barre d'état, faite d'éléments nommés ([`StatusElements`]) choisis par la
//!   configuration.
//! - [`Toasts`] : notifications affichées quelques secondes en bas à droite.
//! - [`CursorBlink`] : clignotement du curseur, arrêté au repos.

mod blink;
mod command_line;
mod editor_view;
mod focus;
mod gutter;
mod history;
mod label;
pub mod layout;
mod mouse;
mod scroll;
mod status_bar;
mod style;
mod text_input;
mod theme;
mod toasts;

pub use blink::{BLINK_IDLE_STOP, CursorBlink};
pub use command_line::{CommandLine, MessageKind};
pub use editor_view::{EditorView, Pointer, Preedit, ViewSettings, WHEEL_LINES};
pub use focus::Focus;
pub use gutter::GutterMarker;
pub use history::{CommandHistory, HISTORY_LIMIT};
pub use label::Label;
pub use mouse::{ClickCounter, ClickModifiers, MULTI_CLICK_DELAY, MouseSelection};
pub use status_bar::{StatusBar, StatusContext, StatusElement, StatusElements};
pub use style::Style;
pub use text_input::{InputEvent, TextInput};
pub use theme::{DEFAULT_THEME, ParsedTheme, SyntaxColors, Theme, Themes};
pub use toasts::{MAX_TOASTS, Toasts};
