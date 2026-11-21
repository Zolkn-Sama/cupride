//! Police et dimensions communes aux éléments d'interface, en pixels physiques.

use cupride_render::text::Metrics;

/// Police et dimensions de l'interface, déduites des réglages de l'éditeur et de l'échelle.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// Famille de police.
    pub family: String,
    /// Taille du texte, en pixels physiques.
    pub font_size: f32,
    /// Hauteur d'une ligne, en pixels physiques.
    pub line_height: f32,
    /// Échelle (écran × zoom).
    pub scale: f32,
}

impl Style {
    /// Métriques de texte.
    pub fn metrics(&self) -> Metrics {
        Metrics::new(self.font_size, self.line_height)
    }

    /// `logical` pixels logiques, en pixels physiques arrondis.
    pub fn px(&self, logical: f32) -> f32 {
        (logical * self.scale).round()
    }
}
