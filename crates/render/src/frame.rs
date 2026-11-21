//! Liste de dessin d'une frame.

use crate::color::Color;
use crate::quad::Quad;
use crate::text::TextItem;

/// Ce qu'il faut dessiner dans une image, construit par l'interface à chaque frame.
#[derive(Debug, Clone, Default)]
pub struct Frame<'a> {
    /// Couleur de fond.
    pub background: Color,
    /// Rectangles dessinés sous le texte : sélections, ligne courante, fonds.
    pub quads: Vec<Quad>,
    /// Textes.
    pub texts: Vec<TextItem<'a>>,
    /// Rectangles dessinés par-dessus le texte : curseurs, soulignements.
    pub overlays: Vec<Quad>,
}

impl Frame<'_> {
    /// Frame vide, de la couleur de fond donnée.
    pub fn new(background: Color) -> Self {
        Self {
            background,
            quads: Vec::new(),
            texts: Vec::new(),
            overlays: Vec::new(),
        }
    }
}
