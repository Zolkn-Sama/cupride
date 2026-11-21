//! Vue d'un document : lignes visibles mises en forme et dessinées.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use cupride_core::Document;
use cupride_core::config::FontConfig;
use cupride_core::text::TextBuffer;
use cupride_render::text::{Attrs, Buffer, Family, Metrics, Shaping, Wrap};
use cupride_render::{Frame, Rect, TextItem, TextSystem};

use crate::theme::Theme;

/// Marge intérieure de la zone de texte, en pixels logiques.
const PADDING: f32 = 8.0;

/// Largeur d'une tabulation, en espaces.
const TAB_WIDTH: u16 = 4;

/// Réglages d'affichage, en pixels logiques.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewSettings {
    /// Famille de police (`monospace` ou un nom).
    pub font_family: String,
    /// Taille de police.
    pub font_size: f32,
    /// Hauteur de ligne, en multiple de la taille.
    pub line_height: f32,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self::from(&FontConfig::default())
    }
}

impl From<&FontConfig> for ViewSettings {
    fn from(font: &FontConfig) -> Self {
        Self {
            font_family: font.family.clone(),
            font_size: font.size.clamp(4.0, 200.0),
            line_height: font.line_height.clamp(0.8, 4.0),
        }
    }
}

/// Une ligne mise en forme, gardée tant que son texte ne change pas.
struct CachedLine {
    hash: u64,
    buffer: Buffer,
}

/// Vue d'un document dans un rectangle de la fenêtre.
///
/// Seules les lignes visibles sont mises en forme ; une ligne déjà mise en forme est réutilisée
/// tant que son texte, la police et l'échelle ne changent pas.
pub struct EditorView {
    settings: ViewSettings,
    /// Échelle : facteur de l'écran (DPI).
    scale: f32,
    /// Zone de la vue dans la fenêtre, en pixels physiques.
    bounds: Rect,
    /// Défilement vertical, en pixels physiques.
    scroll_y: f32,
    lines: HashMap<usize, CachedLine>,
    visible: Range<usize>,
}

impl std::fmt::Debug for EditorView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorView")
            .field("settings", &self.settings)
            .field("scale", &self.scale)
            .field("bounds", &self.bounds)
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

impl EditorView {
    /// Vue avec ces réglages, pour un écran d'échelle `scale`.
    pub fn new(settings: ViewSettings, scale: f32) -> Self {
        Self {
            settings,
            scale,
            bounds: Rect::default(),
            scroll_y: 0.0,
            lines: HashMap::new(),
            visible: 0..0,
        }
    }

    /// Change les réglages d'affichage ; toutes les lignes seront remises en forme.
    pub fn set_settings(&mut self, settings: ViewSettings) {
        if settings != self.settings {
            self.settings = settings;
            self.lines.clear();
        }
    }

    /// Change l'échelle de l'écran.
    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON {
            self.scale = scale;
            self.lines.clear();
        }
    }

    /// Place la vue dans la fenêtre, en pixels physiques.
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    /// Hauteur d'une ligne, en pixels physiques.
    pub fn line_height(&self) -> f32 {
        (self.settings.font_size * self.settings.line_height * self.scale).round()
    }

    fn metrics(&self) -> Metrics {
        Metrics::new(self.settings.font_size * self.scale, self.line_height())
    }

    fn padding(&self) -> f32 {
        PADDING * self.scale
    }

    /// Lignes actuellement visibles.
    pub fn visible_lines(&self) -> Range<usize> {
        self.visible.clone()
    }

    /// Nombre de lignes mises en forme gardées en cache.
    pub fn cached_lines(&self) -> usize {
        self.lines.len()
    }

    /// Lignes visibles pour un document de `line_count` lignes.
    fn compute_visible(&self, line_count: usize) -> Range<usize> {
        let line_height = self.line_height().max(1.0);
        let first = ((self.scroll_y - self.padding()) / line_height)
            .floor()
            .max(0.0) as usize;
        let count = (self.bounds.height / line_height).ceil() as usize + 1;
        first.min(line_count)..(first + count).min(line_count)
    }

    /// Met en forme les lignes visibles de `document`. À appeler avant [`EditorView::frame`].
    pub fn prepare(&mut self, text: &mut TextSystem, document: &Document) {
        let buffer = document.buffer();
        self.visible = self.compute_visible(buffer.len_lines());
        // Les lignes sorties de l'écran sont oubliées : la mémoire reste proportionnelle à l'écran.
        let visible = self.visible.clone();
        self.lines.retain(|line, _| visible.contains(line));

        let metrics = self.metrics();
        let family = self.settings.font_family.clone();
        for line in self.visible.clone() {
            let content = buffer.line(line).unwrap_or_default();
            let hash = hash_text(&content);
            if self
                .lines
                .get(&line)
                .is_some_and(|cached| cached.hash == hash)
            {
                continue;
            }
            let mut shaped = Buffer::new(text.font_system(), metrics);
            shaped.set_wrap(Wrap::None);
            shaped.set_tab_width(TAB_WIDTH);
            shaped.set_size(None, Some(metrics.line_height));
            shaped.set_text(&content, &attrs(&family), Shaping::Advanced, None);
            shaped.shape_until_scroll(text.font_system(), false);
            self.lines.insert(
                line,
                CachedLine {
                    hash,
                    buffer: shaped,
                },
            );
        }
    }

    /// Liste de dessin de la vue, à partir des lignes préparées.
    pub fn frame(&self, theme: &Theme) -> Frame<'_> {
        let mut frame = Frame::new(theme.background);
        let line_height = self.line_height();
        let left = self.bounds.x + self.padding();
        for line in self.visible.clone() {
            let Some(cached) = self.lines.get(&line) else {
                continue;
            };
            let top = self.bounds.y + self.padding() + line as f32 * line_height - self.scroll_y;
            frame.texts.push(TextItem {
                buffer: &cached.buffer,
                left,
                top,
                clip: self.bounds,
                color: theme.foreground,
            });
        }
        frame
    }
}

fn attrs(family: &str) -> Attrs<'_> {
    let family = if family.eq_ignore_ascii_case("monospace") {
        Family::Monospace
    } else {
        Family::Name(family)
    };
    Attrs::new().family(family)
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(height: f32) -> EditorView {
        let mut view = EditorView::new(ViewSettings::default(), 2.0);
        view.set_bounds(Rect::new(0.0, 0.0, 800.0, height));
        view
    }

    #[test]
    fn seules_les_lignes_visibles_sont_mises_en_forme() {
        let mut text = TextSystem::new();
        let document = Document::from_text(&"une ligne de code\n".repeat(100_000));
        let mut view = view(600.0);
        view.prepare(&mut text, &document);
        let visible = view.visible_lines();
        // 14 px × 1,4 × 2 = 39 px par ligne : environ 16 lignes dans 600 px.
        assert_eq!(view.line_height(), 39.0);
        assert_eq!(visible, 0..17);
        assert_eq!(view.cached_lines(), 17);
        assert_eq!(view.frame(&Theme::default()).texts.len(), 17);
    }

    #[test]
    fn lignes_reutilisees_tant_qu_elles_ne_changent_pas() {
        let mut text = TextSystem::new();
        let mut document = Document::from_text("a\nb\nc");
        let mut view = view(600.0);
        view.prepare(&mut text, &document);
        let before = view.lines[&1].buffer.lines[0].text().to_owned();
        assert_eq!(before, "b");
        document
            .transact(cupride_core::text::EditKind::Insert, |b, tx| {
                tx.apply(b, cupride_core::text::Edit::insert(2, "x"))
            })
            .unwrap();
        view.prepare(&mut text, &document);
        assert_eq!(view.lines[&1].buffer.lines[0].text(), "xb");
        assert_eq!(view.visible_lines(), 0..3);
    }

    #[test]
    fn changer_la_police_vide_le_cache() {
        let mut text = TextSystem::new();
        let document = Document::from_text("a\nb");
        let mut view = view(600.0);
        view.prepare(&mut text, &document);
        assert_eq!(view.cached_lines(), 2);
        view.set_settings(ViewSettings {
            font_size: 20.0,
            ..ViewSettings::default()
        });
        assert_eq!(view.cached_lines(), 0);
        view.set_scale(1.0);
        view.prepare(&mut text, &document);
        assert_eq!(view.line_height(), 28.0);
    }
}
