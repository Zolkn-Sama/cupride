//! Liste de dessin d'une frame, par couches.
//!
//! Une frame est une pile de [`Layer`] : la première contient les panneaux, les suivantes ce qui
//! s'affiche par-dessus (popups, palette, notifications). Chaque couche est dessinée entièrement,
//! texte compris, avant la suivante : une popup cache bien le texte des panneaux en dessous.

use crate::color::Color;
use crate::quad::Quad;
use crate::text::TextItem;

/// Une couche : rectangles sous le texte, texte, puis rectangles par-dessus le texte.
#[derive(Debug, Clone, Default)]
pub struct Layer<'a> {
    /// Rectangles dessinés sous le texte : fonds, sélections, ligne courante.
    pub quads: Vec<Quad>,
    /// Textes.
    pub texts: Vec<TextItem<'a>>,
    /// Rectangles dessinés par-dessus le texte : curseurs, soulignements.
    pub overlays: Vec<Quad>,
}

/// Ce qu'il faut dessiner dans une image, construit par l'interface à chaque frame.
#[derive(Debug, Clone, Default)]
pub struct Frame<'a> {
    /// Couleur de fond.
    pub background: Color,
    /// Couches, de la plus basse à la plus haute ; il y en a toujours au moins une.
    pub layers: Vec<Layer<'a>>,
}

impl<'a> Frame<'a> {
    /// Frame d'une couche vide, de la couleur de fond donnée.
    pub fn new(background: Color) -> Self {
        Self {
            background,
            layers: vec![Layer::default()],
        }
    }

    /// Couche du bas : les panneaux.
    pub fn base(&mut self) -> &mut Layer<'a> {
        if self.layers.is_empty() {
            self.layers.push(Layer::default());
        }
        &mut self.layers[0]
    }

    /// Ajoute une couche au-dessus des autres et la renvoie.
    pub fn push_layer(&mut self) -> &mut Layer<'a> {
        self.layers.push(Layer::default());
        let last = self.layers.len() - 1;
        &mut self.layers[last]
    }
}
