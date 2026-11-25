//! Arbre de layout de CuprIDE.
//!
//! - [`LayoutTree`] : arbre d'une fenêtre, fait de splits ([`Split`], axe et proportions) et de
//!   groupes d'onglets ([`TabGroup`]). Diviser, fermer, déplacer et redimensionner le laissent
//!   toujours valide ; il se sérialise en JSON.
//! - [`Panel`] : modèle d'un panneau, sérialisable — identifiant stable ([`PanelId`]), type
//!   ([`PanelKind`]) et état. Son affichage (la vue) vit dans l'interface.
//! - [`Workspace`] : les fenêtres ([`WindowLayout`], chacune avec son arbre et son panneau
//!   actif), les panneaux, et la fenêtre qui a le focus.
//! - [`geometry`] : rectangles, axes et directions ; le layout calcule la place de chaque zone,
//!   pour que la navigation et le redimensionnement se testent sans interface.
//!
//! Cette crate n'a aucune dépendance interne.

pub mod geometry;
mod panel;
mod tree;
mod workspace;

pub use geometry::{Axis, Bounds, Direction};
pub use panel::{Panel, PanelId, PanelKind};
pub use tree::{
    Child, DROP_EDGE, Divider, DropTarget, GroupArea, LayoutError, LayoutTree, Node, Placement,
    Split, TabGroup,
};
pub use workspace::{WindowGeometry, WindowId, WindowLayout, Workspace};
