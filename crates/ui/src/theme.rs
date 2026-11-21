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
    /// Fond des barres (barre d'état, ligne de commande).
    pub panel: Color,
    /// Fond des popups (palette, complétions, notifications).
    pub popup: Color,
    /// Bordures et séparations.
    pub border: Color,
    /// Couleur d'accent : invite, élément choisi, correspondances de recherche.
    pub accent: Color,
    /// Texte secondaire : descriptions, raccourcis, aide.
    pub muted: Color,
    /// Fond de l'élément choisi d'une liste.
    pub list_selection: Color,
    /// Information.
    pub info: Color,
    /// Avertissement.
    pub warning: Color,
    /// Erreur.
    pub error: Color,
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
        panel: Color::rgb(0x17, 0x14, 0x16),
        popup: Color::rgb(0x2b, 0x24, 0x29),
        border: Color::rgb(0x42, 0x38, 0x3e),
        accent: Color::rgb(0xf7, 0x9e, 0x86),
        muted: Color::rgb(0x9a, 0x8a, 0x90),
        list_selection: Color::rgb(0x4a, 0x33, 0x33),
        info: Color::rgb(0x8c, 0xb8, 0xd8),
        warning: Color::rgb(0xe8, 0xc0, 0x6c),
        error: Color::rgb(0xf0, 0x6c, 0x6c),
    };
}

impl Default for Theme {
    fn default() -> Self {
        Self::PADPARADSCHA
    }
}
