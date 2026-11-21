//! Texte d'interface : une ligne mise en forme, refaite seulement quand elle change.

use cupride_render::text::{Attrs, Buffer, Family, Metrics, Shaping, Wrap};
use cupride_render::{Color, Rect, TextItem, TextSystem};

/// Une ligne de texte mise en forme.
pub struct Label {
    text: String,
    family: String,
    metrics: Metrics,
    buffer: Option<Buffer>,
    width: f32,
}

impl std::fmt::Debug for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Label")
            .field("text", &self.text)
            .finish_non_exhaustive()
    }
}

impl Default for Label {
    fn default() -> Self {
        Self {
            text: String::new(),
            family: String::new(),
            metrics: Metrics::new(1.0, 1.0),
            buffer: None,
            width: 0.0,
        }
    }
}

/// Attributs de police pour la famille `family` (`monospace` ou un nom).
pub(crate) fn attrs(family: &str) -> Attrs<'_> {
    let family = if family.eq_ignore_ascii_case("monospace") {
        Family::Monospace
    } else {
        Family::Name(family)
    };
    Attrs::new().family(family)
}

impl Label {
    /// Met `text` en forme, si lui, la police ou la taille ont changé.
    pub fn set(
        &mut self,
        text_system: &mut TextSystem,
        text: &str,
        family: &str,
        metrics: Metrics,
    ) {
        if self.buffer.is_some()
            && self.text == text
            && self.family == family
            && self.metrics == metrics
        {
            return;
        }
        let mut buffer = Buffer::new(text_system.font_system(), metrics);
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, Some(metrics.line_height));
        buffer.set_text(text, &attrs(family), Shaping::Advanced, None);
        buffer.shape_until_scroll(text_system.font_system(), false);
        self.width = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max);
        self.buffer = Some(buffer);
        self.text = text.to_owned();
        self.family = family.to_owned();
        self.metrics = metrics;
    }

    /// Texte affiché.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Largeur du texte, en pixels physiques.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Abscisse de la position `index` (en octets) depuis le début du texte.
    pub fn x_at(&self, index: usize) -> f32 {
        let Some(buffer) = &self.buffer else {
            return 0.0;
        };
        let mut end = 0.0_f32;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                if (glyph.start..glyph.end).contains(&index) {
                    let span = (glyph.end - glyph.start).max(1) as f32;
                    return glyph.x + glyph.w * (index - glyph.start) as f32 / span;
                }
                end = end.max(glyph.x + glyph.w);
            }
        }
        end
    }

    /// Élément de dessin, à la position (`left`, `top`), découpé par `clip` ; `None` tant que
    /// le texte n'a pas été mis en forme.
    pub fn item(&self, left: f32, top: f32, clip: Rect, color: Color) -> Option<TextItem<'_>> {
        Some(TextItem {
            buffer: self.buffer.as_ref()?,
            left,
            top,
            clip,
            color,
        })
    }
}
