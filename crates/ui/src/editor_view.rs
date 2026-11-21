//! Vue d'un document : lignes visibles mises en forme et dessinées, sélections et curseurs.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use cupride_core::Document;
use cupride_core::config::Config;
use cupride_core::text::TextBuffer;
use cupride_render::text::{Attrs, Buffer, Family, Metrics, Shaping, Wrap};
use cupride_render::{Frame, Quad, Rect, TextItem, TextSystem};

use crate::theme::Theme;

/// Marge intérieure de la zone de texte, en pixels logiques.
const PADDING: f32 = 8.0;

/// Largeur d'une tabulation, en espaces.
const TAB_WIDTH: u16 = 4;

/// Épaisseur du curseur, en pixels logiques.
const CURSOR_WIDTH: f32 = 2.0;

/// Réglages d'affichage, en pixels logiques.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewSettings {
    /// Famille de police (`monospace` ou un nom).
    pub font_family: String,
    /// Taille de police.
    pub font_size: f32,
    /// Hauteur de ligne, en multiple de la taille.
    pub line_height: f32,
    /// Surlignage de la ligne du curseur principal.
    pub highlight_current_line: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self::from_config(&Config::default())
    }
}

impl ViewSettings {
    /// Réglages tirés de la configuration, bornés à des valeurs utilisables.
    pub fn from_config(config: &Config) -> Self {
        Self {
            font_family: config.font.family.clone(),
            font_size: config.font.size.clamp(4.0, 200.0),
            line_height: config.font.line_height.clamp(0.8, 4.0),
            highlight_current_line: config.view.highlight_current_line,
        }
    }

    /// Les deux réglages donnent-ils la même mise en forme du texte ?
    fn same_shaping(&self, other: &Self) -> bool {
        self.font_family == other.font_family
            && self.font_size == other.font_size
            && self.line_height == other.line_height
    }
}

/// Une ligne mise en forme, gardée tant que son texte ne change pas.
struct CachedLine {
    hash: u64,
    /// Position du début de la ligne dans le document, mise à jour à chaque préparation.
    start: usize,
    /// Longueur du contenu, sans la fin de ligne.
    len: usize,
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

    /// Change les réglages d'affichage ; si la police change, toutes les lignes seront remises
    /// en forme.
    pub fn set_settings(&mut self, settings: ViewSettings) {
        if !settings.same_shaping(&self.settings) {
            self.lines.clear();
        }
        self.settings = settings;
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
            let start = buffer.line_to_byte(line).unwrap_or_default();
            let content = buffer.line(line).unwrap_or_default();
            let hash = hash_text(&content);
            if let Some(cached) = self.lines.get_mut(&line)
                && cached.hash == hash
            {
                cached.start = start;
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
                    start,
                    len: content.len(),
                    buffer: shaped,
                },
            );
        }
    }

    /// Bord gauche du texte, en pixels physiques.
    fn text_left(&self) -> f32 {
        self.bounds.x + self.padding()
    }

    /// Haut de la ligne `line`, en pixels physiques.
    fn line_top(&self, line: usize) -> f32 {
        self.bounds.y + self.padding() + line as f32 * self.line_height() - self.scroll_y
    }

    /// Largeur réservée au saut de ligne d'une sélection qui continue sur la ligne suivante.
    fn newline_width(&self) -> f32 {
        (self.settings.font_size * self.scale * 0.5).round()
    }

    /// Liste de dessin de la vue, à partir des lignes préparées : ligne courante, sélections,
    /// texte, puis curseurs si `cursors_visible`.
    pub fn frame(&self, theme: &Theme, document: &Document, cursors_visible: bool) -> Frame<'_> {
        let mut frame = Frame::new(theme.background);
        let line_height = self.line_height();
        let selections = document.selections();
        let buffer = document.buffer();
        let line_of = |offset: usize| buffer.byte_to_line(offset).unwrap_or_default();

        if self.settings.highlight_current_line {
            let line = line_of(selections.primary().head);
            if self.visible.contains(&line) {
                let rect = Rect::new(
                    self.bounds.x,
                    self.line_top(line),
                    self.bounds.width,
                    line_height,
                );
                frame
                    .quads
                    .push(Quad::new(rect.intersect(&self.bounds), theme.current_line));
            }
        }

        for selection in selections.iter().filter(|s| !s.is_empty()) {
            let (first, last) = (line_of(selection.start()), line_of(selection.end()));
            let lines = first.max(self.visible.start)..(last + 1).min(self.visible.end);
            for line in lines {
                let Some(cached) = self.lines.get(&line) else {
                    continue;
                };
                let from = selection
                    .start()
                    .saturating_sub(cached.start)
                    .min(cached.len);
                let to = selection.end().saturating_sub(cached.start).min(cached.len);
                let mut right = x_at(&cached.buffer, to);
                // La sélection continue après la fin de la ligne : le saut de ligne est inclus.
                if selection.end() > cached.start + cached.len {
                    right += self.newline_width();
                }
                let left = x_at(&cached.buffer, from);
                let rect = Rect::new(
                    self.text_left() + left,
                    self.line_top(line),
                    right - left,
                    line_height,
                );
                frame
                    .quads
                    .push(Quad::new(rect.intersect(&self.bounds), theme.selection));
            }
        }

        for line in self.visible.clone() {
            let Some(cached) = self.lines.get(&line) else {
                continue;
            };
            frame.texts.push(TextItem {
                buffer: &cached.buffer,
                left: self.text_left(),
                top: self.line_top(line),
                clip: self.bounds,
                color: theme.foreground,
            });
        }

        if cursors_visible {
            let width = (CURSOR_WIDTH * self.scale).round().max(1.0);
            for selection in selections.iter() {
                if let Some(rect) = self.cursor_rect(buffer, selection.head, width) {
                    frame
                        .overlays
                        .push(Quad::new(rect.intersect(&self.bounds), theme.cursor));
                }
            }
        }
        frame
    }

    /// Rectangle du curseur à la position `offset`, s'il est sur une ligne préparée.
    fn cursor_rect(&self, buffer: &impl TextBuffer, offset: usize, width: f32) -> Option<Rect> {
        let line = buffer.byte_to_line(offset).ok()?;
        let cached = self.lines.get(&line)?;
        let x = x_at(&cached.buffer, offset.saturating_sub(cached.start));
        Some(Rect::new(
            (self.text_left() + x - width / 2.0).round(),
            self.line_top(line),
            width,
            self.line_height(),
        ))
    }

    /// Position du curseur principal dans la fenêtre (pour placer la fenêtre de l'IME), si sa
    /// ligne est préparée.
    pub fn primary_cursor_rect(&self, document: &Document) -> Option<Rect> {
        let head = document.selections().primary().head;
        self.cursor_rect(document.buffer(), head, 1.0)
    }
}

/// Abscisse, depuis le bord gauche du texte, de la position `index` (en octets) d'une ligne mise
/// en forme. Au milieu d'une ligature, la position est interpolée.
fn x_at(buffer: &Buffer, index: usize) -> f32 {
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
    use cupride_core::text::{Selection, Selections};

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
        let frame = view.frame(&Theme::default(), &document, true);
        assert_eq!(frame.texts.len(), 17);
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
        assert_eq!(
            view.lines[&2].start, 5,
            "début de ligne décalé par l'insertion"
        );
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
            highlight_current_line: false,
            ..ViewSettings::default()
        });
        assert_eq!(
            view.cached_lines(),
            2,
            "pas de remise en forme sans changement de police"
        );
        view.set_settings(ViewSettings {
            font_size: 20.0,
            ..ViewSettings::default()
        });
        assert_eq!(view.cached_lines(), 0);
        view.set_scale(1.0);
        view.prepare(&mut text, &document);
        assert_eq!(view.line_height(), 28.0);
    }

    #[test]
    fn curseurs_et_selections_multiples() {
        let mut text = TextSystem::new();
        let mut document = Document::from_text("abcd\nefgh\nijkl");
        // Une sélection sur deux lignes, et un curseur seul sur la troisième.
        let mut selections = Selections::single(Selection::new(2, 7));
        selections.push(Selection::cursor(12));
        document.set_selections(selections);
        let mut view = view(600.0);
        view.prepare(&mut text, &document);
        let theme = Theme::default();

        let frame = view.frame(&theme, &document, true);
        let selection: Vec<&Quad> = frame
            .quads
            .iter()
            .filter(|q| q.color == theme.selection)
            .collect();
        assert_eq!(selection.len(), 2, "un rectangle par ligne sélectionnée");
        let (first, second) = (selection[0].rect, selection[1].rect);
        assert!(first.x > view.text_left(), "commence après « ab »");
        assert!(first.width > 0.0 && second.width > 0.0);
        assert_eq!(
            second.x,
            view.text_left(),
            "deuxième ligne depuis son début"
        );
        assert_eq!(second.y - first.y, view.line_height());

        assert_eq!(frame.overlays.len(), 2, "un curseur par sélection");
        assert!(frame.overlays.iter().all(|q| q.color == theme.cursor));
        let current: Vec<&Quad> = frame
            .quads
            .iter()
            .filter(|q| q.color == theme.current_line)
            .collect();
        assert_eq!(current.len(), 1);
        // Le curseur ajouté est devenu la sélection principale : sa ligne est surlignée.
        assert_eq!(current[0].rect.y, first.y + 2.0 * view.line_height());

        // Curseurs éteints (clignotement) ; ligne courante désactivée.
        view.set_settings(ViewSettings {
            highlight_current_line: false,
            ..ViewSettings::default()
        });
        let frame = view.frame(&theme, &document, false);
        assert!(frame.overlays.is_empty());
        assert!(frame.quads.iter().all(|q| q.color != theme.current_line));
    }

    #[test]
    fn abscisse_des_positions() {
        let mut text = TextSystem::new();
        let document = Document::from_text("abc");
        let mut view = view(600.0);
        view.prepare(&mut text, &document);
        let buffer = &view.lines[&0].buffer;
        let (x0, x1, x3) = (x_at(buffer, 0), x_at(buffer, 1), x_at(buffer, 3));
        assert_eq!(x0, 0.0);
        assert!(x1 > 0.0);
        // Police à chasse fixe : trois caractères font trois fois la largeur d'un.
        assert!((x3 - 3.0 * x1).abs() < 0.5, "{x1} {x3}");
        assert_eq!(x_at(buffer, 99), x3, "au-delà de la fin : fin de ligne");
    }
}
