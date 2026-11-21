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
}

impl Theme {
    /// Padparadscha, le thème par défaut : un saphir rose-orangé sur fond sombre.
    pub const PADPARADSCHA: Self = Self {
        name: "Padparadscha",
        background: Color::rgb(0x1e, 0x1a, 0x1d),
        foreground: Color::rgb(0xf2, 0xe6, 0xe3),
        cursor: Color::rgb(0xf7, 0x9e, 0x86),
        selection: Color::rgba(0xe8, 0x7e, 0x6c, 0x48),
        current_line: Color::rgba(0xff, 0xff, 0xff, 0x0b),
    };
}

impl Default for Theme {
    fn default() -> Self {
        Self::PADPARADSCHA
    }
}
