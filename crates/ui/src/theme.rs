//! Thème de couleurs.

use cupride_render::Color;

/// Couleurs de l'interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Nom du thème.
    pub name: &'static str,
    /// Fond de l'éditeur.
    pub background: Color,
    /// Texte.
    pub foreground: Color,
    /// Curseurs.
    pub cursor: Color,
    /// Fond des sélections.
    pub selection: Color,
    /// Fond de la ligne du curseur principal.
    pub current_line: Color,
    /// Numéros de ligne.
    pub line_number: Color,
    /// Numéro de la ligne du curseur principal.
    pub line_number_active: Color,
}

impl Theme {
    /// Padparadscha, le thème par défaut : un saphir rose-orangé sur fond sombre.
    pub const PADPARADSCHA: Self = Self {
        name: "Padparadscha",
        background: Color::rgb(0x1e, 0x1a, 0x1d),
        foreground: Color::rgb(0xf2, 0xe6, 0xe3),
        cursor: Color::rgb(0xf7, 0x9e, 0x86),
        selection: Color::rgba(0xe8, 0x7e, 0x6c, 0x48),
        // Opaque : un blanc très transparent paraît bien plus clair, le mélange se faisant en
        // couleurs linéaires.
        current_line: Color::rgb(0x2a, 0x24, 0x28),
        line_number: Color::rgb(0x6e, 0x60, 0x66),
        line_number_active: Color::rgb(0xf7, 0x9e, 0x86),
    };
}

impl Default for Theme {
    fn default() -> Self {
        Self::PADPARADSCHA
    }
}
