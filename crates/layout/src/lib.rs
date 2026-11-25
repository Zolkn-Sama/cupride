//! Arbre de layout de CuprIDE.
//!
//! - [`LayoutTree`] : arbre d'une fenêtre, fait de splits ([`Split`], axe et proportions) et de
//!   groupes d'onglets ([`TabGroup`]). Diviser, fermer, déplacer et redimensionner le laissent
//!   toujours valide ; il se sérialise en JSON.
//! - [`PanelId`] : identifiant stable d'un panneau.
//! - [`geometry`] : rectangles, axes et directions ; le layout calcule la place de chaque zone,
//!   pour que la navigation et le redimensionnement se testent sans interface.
//!
//! Cette crate n'a aucune dépendance interne.

pub mod geometry;
mod panel;
mod tree;

pub use geometry::{Axis, Bounds, Direction};
pub use panel::PanelId;
pub use tree::{
    Child, Divider, GroupArea, LayoutError, LayoutTree, Node, Placement, Split, TabGroup,
};

/// Ensemble des fenêtres et de leur layout.
///
/// Encore vide : son contenu (fenêtres, écrans, arbres de panneaux) arrive avec l'épic Layout.
/// Il existe dès maintenant pour que l'état de l'éditeur ait sa forme définitive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspace {}

impl Workspace {
    /// Workspace vide.
    pub fn new() -> Self {
        Self::default()
    }
}
