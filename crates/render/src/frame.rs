//! Liste de dessin d'une frame.

use crate::color::Color;
use crate::text::TextItem;

/// Ce qu'il faut dessiner dans une image, construit par l'interface à chaque frame.
#[derive(Debug, Clone, Default)]
pub struct Frame<'a> {
    /// Couleur de fond.
    pub background: Color,
    /// Textes.
    pub texts: Vec<TextItem<'a>>,
}

impl Frame<'_> {
    /// Frame vide, de la couleur de fond donnée.
    pub fn new(background: Color) -> Self {
        Self {
            background,
            texts: Vec::new(),
        }
    }
}
