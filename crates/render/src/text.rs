//! Texte : polices, mise en forme et éléments de texte d'une frame.
//!
//! La mise en forme (*shaping*) est faite par `cosmic-text` : ligatures, emojis, écritures
//! complexes, polices système. Les glyphes sont rasterisés une seule fois puis gardés dans un
//! atlas de texture GPU, réutilisé d'une frame à l'autre.

use glyphon::SwashCache;
pub use glyphon::cosmic_text;
pub use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap};

use crate::color::Color;
use crate::geometry::Rect;

/// Polices et caches de mise en forme, partagés par toutes les fenêtres.
pub struct TextSystem {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl std::fmt::Debug for TextSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextSystem")
            .field("faces", &self.font_system.db().len())
            .finish()
    }
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TextSystem {
    /// Charge les polices installées sur le système.
    pub fn new() -> Self {
        let start = std::time::Instant::now();
        let font_system = FontSystem::new();
        tracing::info!(
            faces = font_system.db().len(),
            duree_ms = start.elapsed().as_millis() as u64,
            "polices système chargées"
        );
        Self {
            font_system,
            swash_cache: SwashCache::new(),
        }
    }

    /// Système sans aucune police : il ne peut rasteriser aucun glyphe. Sert à vérifier
    /// qu'un rendu n'utilise que des glyphes déjà présents dans l'atlas.
    #[cfg(test)]
    pub(crate) fn without_fonts() -> Self {
        Self {
            font_system: FontSystem::new_with_locale_and_db(
                "fr-FR".to_owned(),
                glyphon::cosmic_text::fontdb::Database::new(),
            ),
            swash_cache: SwashCache::new(),
        }
    }

    /// Polices et cache de rasterisation, pour le rendu.
    pub(crate) fn parts(&mut self) -> (&mut FontSystem, &mut SwashCache) {
        (&mut self.font_system, &mut self.swash_cache)
    }

    /// Accès aux polices, pour mettre en forme des [`Buffer`].
    pub fn font_system(&mut self) -> &mut FontSystem {
        &mut self.font_system
    }

    /// Indique si une famille de polices porte ce nom (sans tenir compte de la casse).
    pub fn has_family(&self, name: &str) -> bool {
        self.font_system.db().faces().any(|face| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(name))
        })
    }
}

/// Texte déjà mis en forme, à dessiner à une position, découpé par un rectangle.
#[derive(Debug, Clone, Copy)]
pub struct TextItem<'a> {
    /// Texte mis en forme, en pixels physiques.
    pub buffer: &'a Buffer,
    /// Position du coin haut gauche du texte.
    pub left: f32,
    /// Position du haut du texte.
    pub top: f32,
    /// Zone hors de laquelle rien n'est dessiné.
    pub clip: Rect,
    /// Couleur par défaut du texte.
    pub color: Color,
}

impl<'a> TextItem<'a> {
    pub(crate) fn to_area(self) -> glyphon::TextArea<'a> {
        // Bornes entières : arrondies vers l'extérieur pour ne rien couper.
        let bounds = glyphon::TextBounds {
            left: self.clip.x.floor() as i32,
            top: self.clip.y.floor() as i32,
            right: self.clip.right().ceil() as i32,
            bottom: self.clip.bottom().ceil() as i32,
        };
        glyphon::TextArea {
            buffer: self.buffer,
            left: self.left,
            top: self.top,
            scale: 1.0,
            bounds,
            default_color: glyphon::Color::rgba(
                self.color.r,
                self.color.g,
                self.color.b,
                self.color.a,
            ),
            custom_glyphs: &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Identifiants des glyphes de `text` mis en forme avec `family`.
    fn glyph_ids(system: &mut TextSystem, family: Family<'_>, text: &str) -> Vec<u16> {
        let mut buffer = Buffer::new(system.font_system(), Metrics::new(16.0, 20.0));
        buffer.set_text(text, &Attrs::new().family(family), Shaping::Advanced, None);
        buffer.shape_until_scroll(system.font_system(), false);
        buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.glyph_id))
            .collect()
    }

    #[test]
    fn ligatures_appliquees() {
        let mut system = TextSystem::new();
        // Polices de code à ligatures si elles sont installées, sinon la ligature « fi » d'une
        // police courante qui la possède : seul compte que la mise en forme substitue les
        // glyphes. La police générique à empattements n'en a pas toujours (Times New Roman).
        let candidates = [
            ("Fira Code", "->"),
            ("JetBrains Mono", "->"),
            ("Cascadia Code", "->"),
            ("Times", "fi"),
            ("Palatino", "fi"),
            ("Helvetica Neue", "fi"),
            ("DejaVu Sans", "fi"),
            ("Noto Serif", "fi"),
        ];
        let mut tested = false;
        for (name, ligature) in candidates {
            if !system.has_family(name) {
                continue;
            }
            let family = Family::Name(name);
            let separate: Vec<u16> = ligature
                .chars()
                .flat_map(|c| glyph_ids(&mut system, family, &c.to_string()))
                .collect();
            let joined = glyph_ids(&mut system, family, ligature);
            if joined != separate {
                return;
            }
            tested = true;
        }
        assert!(!tested, "aucune ligature appliquée par la mise en forme");
        eprintln!("test ignoré : aucune police à ligatures installée");
    }
}
