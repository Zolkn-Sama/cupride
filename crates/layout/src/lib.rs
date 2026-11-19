//! Arbre de layout de CuprIDE.
//!
//! Contiendra les splits, groupes d'onglets, panneaux, fenêtres et écrans, ainsi que leur
//! sérialisation. Cette crate n'a aucune dépendance interne.

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
