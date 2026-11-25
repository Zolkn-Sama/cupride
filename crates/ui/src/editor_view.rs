//! Vue d'un document : lignes visibles mises en forme et dessinées, sélections et curseurs.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::time::Instant;

use cupride_core::Document;
use cupride_core::config::{Config, LineNumbers};
use cupride_core::layout::PanelId;
use cupride_core::text::Selections;
use cupride_core::text::TextBuffer;
use cupride_render::text::{Buffer, Metrics, Shaping, Wrap};
use cupride_render::{Frame, Layer, Quad, Rect, TextItem, TextSystem};

use crate::gutter::{self, Gutter, GutterMarker, MARKER_LANE, NUMBER_GAP};
use crate::label::attrs;
use crate::scroll::Scroll;
use crate::theme::Theme;

/// Marge intérieure de la zone de texte, en pixels logiques.
const PADDING: f32 = 8.0;

/// Largeur d'une tabulation, en espaces.
const TAB_WIDTH: u16 = 4;

/// Épaisseur du curseur, en pixels logiques.
const CURSOR_WIDTH: f32 = 2.0;

/// Lignes parcourues par cran de molette.
pub const WHEEL_LINES: f32 = 3.0;

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
    /// Numéros de ligne dans la gouttière.
    pub line_numbers: LineNumbers,
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
            line_numbers: config.view.line_numbers,
        }
    }

    /// Police et dimensions de l'interface à l'échelle `scale`, alignées sur celles de l'éditeur.
    pub fn style(&self, scale: f32) -> crate::style::Style {
        crate::style::Style {
            family: self.font_family.clone(),
            font_size: self.font_size * scale,
            line_height: (self.font_size * self.line_height * scale).round(),
            scale,
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
    /// Texte en cours de composition (IME) affiché dans cette ligne : position dans la ligne et
    /// longueur, en octets.
    preedit: Option<(usize, usize)>,
    buffer: Buffer,
}

impl CachedLine {
    /// Abscisse de la position `index` du contenu de la ligne (en octets, sans le texte en
    /// composition), depuis le bord gauche du texte.
    fn x(&self, index: usize) -> f32 {
        let index = match self.preedit {
            Some((at, len)) if index >= at => index + len,
            _ => index,
        };
        x_at(&self.buffer, index)
    }
}

/// Forme du pointeur de la souris.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// Flèche.
    Default,
    /// Barre de texte : au-dessus d'une zone de texte.
    Text,
    /// Redimensionnement horizontal : au-dessus d'une séparation verticale.
    ResizeHorizontal,
    /// Redimensionnement vertical : au-dessus d'une séparation horizontale.
    ResizeVertical,
}

/// Texte en cours de composition par une méthode de saisie (IME), pas encore inséré.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preedit {
    /// Texte composé.
    pub text: String,
    /// Curseur dans ce texte (en octets), ou `None` pour le masquer.
    pub cursor: Option<usize>,
}

/// Vue d'un document dans un rectangle de la fenêtre.
///
/// Seules les lignes visibles sont mises en forme ; une ligne déjà mise en forme est réutilisée
/// tant que son texte, la police et l'échelle ne changent pas. Une vue rattachée à un panneau
/// ([`EditorView::for_panel`]) affiche les sélections de ce panneau : un même document peut être
/// affiché dans plusieurs vues, chacune avec son défilement et ses curseurs.
pub struct EditorView {
    /// Panneau affiché, dont la vue montre les sélections.
    panel: Option<PanelId>,
    settings: ViewSettings,
    /// Échelle : facteur de l'écran (DPI).
    scale: f32,
    /// Zone de la vue dans la fenêtre, en pixels physiques.
    bounds: Rect,
    /// Défilement horizontal et vertical, en pixels physiques.
    scroll: Scroll,
    /// Nombre de lignes du document, à la dernière préparation.
    line_count: usize,
    /// Plus grande largeur de ligne rencontrée, pour borner le défilement horizontal.
    content_width: f32,
    /// Le curseur principal doit être rendu visible à la prochaine préparation.
    reveal: bool,
    /// Texte en cours de composition, affiché au curseur principal.
    preedit: Option<Preedit>,
    /// Numéros de ligne mis en forme.
    gutter: Gutter,
    /// Marqueurs de la gouttière.
    markers: Vec<GutterMarker>,
    /// Ligne du curseur principal, à la dernière préparation.
    cursor_line: usize,
    lines: HashMap<usize, CachedLine>,
    visible: Range<usize>,
}

impl std::fmt::Debug for EditorView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorView")
            .field("panel", &self.panel)
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
            panel: None,
            settings,
            scale,
            bounds: Rect::default(),
            scroll: Scroll::default(),
            line_count: 1,
            content_width: 0.0,
            reveal: false,
            preedit: None,
            gutter: Gutter::default(),
            markers: Vec::new(),
            cursor_line: 0,
            lines: HashMap::new(),
            visible: 0..0,
        }
    }

    /// Vue du panneau `panel` : elle affiche les sélections de ce panneau.
    pub fn for_panel(panel: PanelId, settings: ViewSettings, scale: f32) -> Self {
        Self {
            panel: Some(panel),
            ..Self::new(settings, scale)
        }
    }

    /// Panneau affiché.
    pub fn panel(&self) -> Option<PanelId> {
        self.panel
    }

    /// Sélections affichées : celles du panneau de la vue, ou celles du document.
    pub fn selections<'d>(&self, document: &'d Document) -> &'d Selections {
        match self.panel {
            Some(panel) => document.selections_in(panel),
            None => document.selections(),
        }
    }

    /// Réglages d'affichage.
    pub fn settings(&self) -> &ViewSettings {
        &self.settings
    }

    /// Change les réglages d'affichage ; si la police change, toutes les lignes seront remises
    /// en forme.
    pub fn set_settings(&mut self, settings: ViewSettings) {
        if !settings.same_shaping(&self.settings) {
            self.lines.clear();
            self.gutter.clear();
            self.content_width = 0.0;
        }
        self.settings = settings;
    }

    /// Change l'échelle : facteur de l'écran (DPI) multiplié par le zoom de l'interface. Le
    /// défilement suit, pour garder le même texte en haut de la vue.
    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON && scale > 0.0 {
            // Verticalement, en lignes : la hauteur de ligne est arrondie au pixel.
            let [x, y] = self.scroll.position();
            let lines = y / self.line_height().max(1.0);
            let x = x * scale / self.scale;
            self.scale = scale;
            self.scroll.jump_to([x, lines * self.line_height()]);
            self.lines.clear();
            self.gutter.clear();
            self.content_width = 0.0;
        }
    }

    /// Place la vue dans la fenêtre, en pixels physiques.
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    /// Place de la vue dans la fenêtre.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Police et dimensions de l'interface, alignées sur celles de l'éditeur.
    pub fn style(&self) -> crate::style::Style {
        self.settings.style(self.scale)
    }

    /// Échelle : facteur de l'écran multiplié par le zoom.
    pub fn scale(&self) -> f32 {
        self.scale
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

    /// Nombre de lignes entièrement visibles : la taille d'une page.
    pub fn page_lines(&self) -> usize {
        ((self.bounds.height - 2.0 * self.padding()) / self.line_height().max(1.0)).floor() as usize
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
        let first = ((self.scroll_y() - self.padding()) / line_height)
            .floor()
            .max(0.0) as usize;
        let count = (self.bounds.height / line_height).ceil() as usize + 1;
        first.min(line_count)..(first + count).min(line_count)
    }

    /// Met en forme les lignes visibles de `document`. À appeler avant [`EditorView::frame`].
    pub fn prepare(&mut self, text: &mut TextSystem, document: &Document) {
        let buffer = document.buffer();
        self.line_count = buffer.len_lines();
        self.scroll.clamp(self.max_scroll());
        let head = self.selections(document).primary().head;
        let reveal = std::mem::take(&mut self.reveal);
        if reveal {
            self.reveal_line(buffer.byte_to_line(head).unwrap_or_default());
        }
        self.visible = self.compute_visible(self.line_count);
        // Les lignes sorties de l'écran sont oubliées : la mémoire reste proportionnelle à l'écran.
        let visible = self.visible.clone();
        self.lines.retain(|line, _| visible.contains(line));

        let metrics = self.metrics();
        let family = self.settings.font_family.clone();
        let head_line = buffer.byte_to_line(head).unwrap_or_default();
        for line in self.visible.clone() {
            let start = buffer.line_to_byte(line).unwrap_or_default();
            let mut content = buffer.line(line).unwrap_or_default();
            let len = content.len();
            // Le texte en composition est affiché dans la ligne du curseur, sans être inséré.
            let mut preedit = None;
            if line == head_line
                && let Some(composing) = self.preedit.as_ref().filter(|p| !p.text.is_empty())
            {
                let at = head.saturating_sub(start).min(len);
                if content.is_char_boundary(at) {
                    content.insert_str(at, &composing.text);
                    preedit = Some((at, composing.text.len()));
                }
            }
            let hash = hash_text(&content);
            if let Some(cached) = self.lines.get_mut(&line)
                && cached.hash == hash
            {
                cached.start = start;
                cached.len = len;
                cached.preedit = preedit;
                continue;
            }
            let mut shaped = Buffer::new(text.font_system(), metrics);
            shaped.set_wrap(Wrap::None);
            shaped.set_tab_width(TAB_WIDTH);
            shaped.set_size(None, Some(metrics.line_height));
            shaped.set_text(&content, &attrs(&family), Shaping::Advanced, None);
            shaped.shape_until_scroll(text.font_system(), false);
            self.content_width = self.content_width.max(x_at(&shaped, content.len()));
            self.lines.insert(
                line,
                CachedLine {
                    hash,
                    start,
                    len,
                    preedit,
                    buffer: shaped,
                },
            );
        }
        self.cursor_line = head_line;
        self.gutter.prepare(
            text,
            metrics,
            &attrs(&family),
            self.visible.clone(),
            head_line,
            self.settings.line_numbers,
        );
        if reveal {
            self.reveal_column(buffer, head);
        }
    }

    /// Remplace les marqueurs de la gouttière.
    pub fn set_markers(&mut self, markers: Vec<GutterMarker>) {
        self.markers = markers;
    }

    /// Largeur de la colonne des marqueurs.
    fn marker_lane(&self) -> f32 {
        (MARKER_LANE * self.scale).round()
    }

    /// Largeur réservée aux numéros (chiffres et espace avant le texte), nulle sans numéros.
    fn numbers_width(&self) -> f32 {
        if self.settings.line_numbers == LineNumbers::Off {
            return 0.0;
        }
        let digits = gutter::digits(self.line_count) as f32;
        (digits * self.gutter.digit_width + NUMBER_GAP * self.scale).round()
    }

    /// Largeur de la gouttière.
    pub fn gutter_width(&self) -> f32 {
        self.marker_lane() + self.numbers_width()
    }

    /// Zone du texte : la vue sans la gouttière.
    fn text_area(&self) -> Rect {
        let gutter = self.gutter_width().min(self.bounds.width);
        Rect::new(
            self.bounds.x + gutter,
            self.bounds.y,
            self.bounds.width - gutter,
            self.bounds.height,
        )
    }

    /// Défilement vertical actuel.
    fn scroll_y(&self) -> f32 {
        self.scroll.position()[1]
    }

    /// Défilement horizontal actuel.
    fn scroll_x(&self) -> f32 {
        self.scroll.position()[0]
    }

    /// Défilement maximal : la dernière ligne peut monter jusqu'en haut de la vue ; à droite,
    /// jusqu'à la fin de la plus longue ligne vue, plus une marge.
    fn max_scroll(&self) -> [f32; 2] {
        let max_y = self.line_count.saturating_sub(1) as f32 * self.line_height();
        let max_x = self.content_width + 2.0 * self.padding() + self.reveal_margin()
            - self.text_area().width;
        [max_x.max(0.0), max_y]
    }

    /// Marge horizontale gardée autour du curseur rendu visible.
    fn reveal_margin(&self) -> f32 {
        (self.settings.font_size * self.scale * 2.0).round()
    }

    /// Fait défiler de (`dx`, `dy`) pixels physiques ; avec `smooth`, le mouvement est animé
    /// (molette), sinon immédiat (pavé tactile).
    pub fn scroll_by(&mut self, dx: f32, dy: f32, smooth: bool, now: Instant) {
        let [x, y] = if smooth {
            self.scroll.target()
        } else {
            self.scroll.position()
        };
        let [max_x, max_y] = self.max_scroll();
        let target = [(x + dx).clamp(0.0, max_x), (y + dy).clamp(0.0, max_y)];
        if smooth {
            self.scroll.animate_to(target, now);
        } else {
            self.scroll.jump_to(target);
        }
    }

    /// Avance l'animation de défilement ; renvoie vrai s'il faut encore redessiner.
    pub fn tick(&mut self, now: Instant) -> bool {
        self.scroll.tick(now)
    }

    /// Une animation de défilement est-elle en cours ?
    pub fn is_scrolling(&self) -> bool {
        self.scroll.is_animating()
    }

    /// Texte en cours de composition par l'IME, affiché au curseur principal ; `None` quand la
    /// composition est finie ou abandonnée.
    pub fn set_preedit(&mut self, preedit: Option<Preedit>) {
        if preedit != self.preedit {
            self.preedit = preedit;
            self.reveal = true;
        }
    }

    /// Demande que le curseur principal soit rendu visible à la prochaine préparation : la vue
    /// suit le curseur.
    pub fn follow_cursor(&mut self) {
        self.reveal = true;
    }

    /// Fait défiler verticalement, au minimum, pour que la ligne `line` soit entièrement visible.
    fn reveal_line(&mut self, line: usize) {
        let line_height = self.line_height();
        let top = self.padding() + line as f32 * line_height;
        let [x, y] = self.scroll.position();
        let y = if top - self.padding() < y {
            top - self.padding()
        } else if top + line_height + self.padding() > y + self.bounds.height {
            top + line_height + self.padding() - self.bounds.height
        } else {
            y
        };
        self.scroll.jump_to([x, y]);
        self.scroll.clamp(self.max_scroll());
    }

    /// Fait défiler horizontalement, au minimum, pour que la position `offset` soit visible.
    fn reveal_column(&mut self, buffer: &impl TextBuffer, offset: usize) {
        let Some(line) = buffer.byte_to_line(offset).ok() else {
            return;
        };
        let Some(cached) = self.lines.get(&line) else {
            return;
        };
        let cursor = self.padding() + cached.x(offset.saturating_sub(cached.start));
        let margin = self.reveal_margin();
        let [x, y] = self.scroll.position();
        let x = if cursor - margin < x {
            cursor - margin
        } else if cursor + margin > x + self.text_area().width {
            cursor + margin - self.text_area().width
        } else {
            x
        };
        self.scroll.jump_to([x, y]);
        self.scroll.clamp(self.max_scroll());
    }

    /// Bord gauche du texte, en pixels physiques.
    fn text_left(&self) -> f32 {
        self.text_area().x + self.padding() - self.scroll_x()
    }

    /// Haut de la ligne `line`, en pixels physiques.
    fn line_top(&self, line: usize) -> f32 {
        self.bounds.y + self.padding() + line as f32 * self.line_height() - self.scroll_y()
    }

    /// Largeur réservée au saut de ligne d'une sélection qui continue sur la ligne suivante.
    fn newline_width(&self) -> f32 {
        (self.settings.font_size * self.scale * 0.5).round()
    }

    /// Liste de dessin de la vue, à partir des lignes préparées : ligne courante, sélections,
    /// texte, puis curseurs si `cursors_visible`.
    pub fn frame(&self, theme: &Theme, document: &Document, cursors_visible: bool) -> Frame<'_> {
        let mut frame = Frame::new(theme.background);
        self.draw(frame.base(), theme, document, cursors_visible);
        frame
    }

    /// Dessine la vue dans la couche `frame` : fond, ligne courante, sélections, texte,
    /// gouttière, puis curseurs si `cursors_visible`.
    pub fn draw<'a>(
        &'a self,
        frame: &mut Layer<'a>,
        theme: &Theme,
        document: &Document,
        cursors_visible: bool,
    ) {
        frame.quads.push(Quad::new(self.bounds, theme.background));
        let line_height = self.line_height();
        let selections = self.selections(document);
        let buffer = document.buffer();
        let line_of = |offset: usize| buffer.byte_to_line(offset).unwrap_or_default();
        let text_area = self.text_area();

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
                let mut right = cached.x(to);
                // La sélection continue après la fin de la ligne : le saut de ligne est inclus.
                if selection.end() > cached.start + cached.len {
                    right += self.newline_width();
                }
                let left = cached.x(from);
                let rect = Rect::new(
                    self.text_left() + left,
                    self.line_top(line),
                    right - left,
                    line_height,
                );
                frame
                    .quads
                    .push(Quad::new(rect.intersect(&text_area), theme.selection));
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
                clip: text_area,
                color: theme.foreground,
            });
        }
        self.draw_gutter(frame, theme);

        // Texte en composition : souligné.
        let head_line = line_of(selections.primary().head);
        if let Some((cached, (at, len))) = self
            .lines
            .get(&head_line)
            .and_then(|c| c.preedit.map(|p| (c, p)))
        {
            let thickness = self.scale.round().max(1.0);
            let left = x_at(&cached.buffer, at);
            let right = x_at(&cached.buffer, at + len);
            let rect = Rect::new(
                self.text_left() + left,
                self.line_top(head_line) + line_height - 2.0 * thickness,
                right - left,
                thickness,
            );
            frame
                .overlays
                .push(Quad::new(rect.intersect(&text_area), theme.foreground));
        }

        if cursors_visible {
            let width = (CURSOR_WIDTH * self.scale).round().max(1.0);
            for selection in selections.iter() {
                if let Some(rect) = self.cursor_rect(buffer, selection.head, width) {
                    frame
                        .overlays
                        .push(Quad::new(rect.intersect(&text_area), theme.cursor));
                }
            }
        }
    }

    /// Numéros de ligne et marqueurs.
    fn draw_gutter<'a>(&'a self, frame: &mut Layer<'a>, theme: &Theme) {
        let gutter = Rect::new(
            self.bounds.x,
            self.bounds.y,
            self.gutter_width().min(self.bounds.width),
            self.bounds.height,
        );
        let right = gutter.right() - (NUMBER_GAP * self.scale).round();
        for line in self.visible.clone() {
            let Some(number) = self.gutter.numbers.get(&line) else {
                continue;
            };
            frame.texts.push(TextItem {
                buffer: &number.buffer,
                left: (right - number.width).round(),
                top: self.line_top(line),
                clip: gutter,
                color: if line == self.cursor_line {
                    theme.line_number_active
                } else {
                    theme.line_number
                },
            });
        }
        let width = (3.0 * self.scale).round().max(1.0);
        let lane = self.marker_lane();
        for marker in self
            .markers
            .iter()
            .filter(|m| self.visible.contains(&m.line))
        {
            let rect = Rect::new(
                (gutter.x + (lane - width) / 2.0).round(),
                self.line_top(marker.line),
                width,
                self.line_height(),
            );
            frame
                .quads
                .push(Quad::new(rect.intersect(&gutter), marker.color));
        }
    }

    /// Rectangle du curseur à la position `offset`, s'il est sur une ligne préparée.
    fn cursor_rect(&self, buffer: &impl TextBuffer, offset: usize, width: f32) -> Option<Rect> {
        let line = buffer.byte_to_line(offset).ok()?;
        let cached = self.lines.get(&line)?;
        let index = offset.saturating_sub(cached.start);
        let x = match (cached.preedit, &self.preedit) {
            // Curseur principal pendant une composition : à la position indiquée par l'IME.
            (Some((at, len)), Some(composing)) if index == at => x_at(
                &cached.buffer,
                at + composing.cursor.unwrap_or(len).min(len),
            ),
            _ => cached.x(index),
        };
        Some(Rect::new(
            (self.text_left() + x - width / 2.0).round(),
            self.line_top(line),
            width,
            self.line_height(),
        ))
    }

    /// Position dans le document sous le point (`x`, `y`) de la fenêtre, en pixels physiques :
    /// la limite de caractère la plus proche. Au-dessus du texte : début du document ; en
    /// dessous : fin. Sur une ligne non préparée (hors de la vue, pendant un glisser), le début
    /// ou la fin de la ligne selon qu'elle est au-dessus ou en dessous.
    pub fn offset_at(&self, x: f32, y: f32, buffer: &impl TextBuffer) -> usize {
        let line_height = self.line_height().max(1.0);
        let row = ((y - self.bounds.y - self.padding() + self.scroll_y()) / line_height).floor();
        if row < 0.0 {
            return 0;
        }
        let line = row as usize;
        if line >= buffer.len_lines() {
            return buffer.len_bytes();
        }
        let Some(cached) = self.lines.get(&line) else {
            let range = buffer.line_content_range(line).unwrap_or(0..0);
            return if line < self.visible.start {
                range.start
            } else {
                range.end
            };
        };
        let index = index_at(&cached.buffer, x - self.text_left()).min(cached.len);
        cached.start + index
    }

    /// Forme du pointeur au point (`x`, `y`) de la fenêtre.
    pub fn pointer_at(&self, x: f32, y: f32) -> Pointer {
        if self.text_area().contains(x, y) {
            Pointer::Text
        } else {
            Pointer::Default
        }
    }

    /// Position du curseur principal dans la fenêtre (pour placer la fenêtre de l'IME), si sa
    /// ligne est préparée.
    pub fn primary_cursor_rect(&self, document: &Document) -> Option<Rect> {
        let head = self.selections(document).primary().head;
        self.cursor_rect(document.buffer(), head, 1.0)
    }
}

/// Position (en octets) de la limite de glyphe la plus proche de l'abscisse `x`, mesurée depuis
/// le bord gauche du texte.
fn index_at(buffer: &Buffer, x: f32) -> usize {
    let mut end = 0;
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            if x < glyph.x + glyph.w / 2.0 {
                return glyph.start;
            }
            end = end.max(glyph.end);
        }
    }
    end
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
        assert_eq!(
            frame.layers[0].texts.len(),
            2 * 17,
            "17 lignes et leurs 17 numéros"
        );
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
        let selection: Vec<&Quad> = frame.layers[0]
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

        assert_eq!(
            frame.layers[0].overlays.len(),
            2,
            "un curseur par sélection"
        );
        assert!(
            frame.layers[0]
                .overlays
                .iter()
                .all(|q| q.color == theme.cursor)
        );
        let current: Vec<&Quad> = frame.layers[0]
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
        assert!(frame.layers[0].overlays.is_empty());
        assert!(
            frame.layers[0]
                .quads
                .iter()
                .all(|q| q.color != theme.current_line)
        );
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

    #[test]
    fn defilement_change_les_lignes_visibles() {
        let mut text = TextSystem::new();
        let document = Document::from_text(&"ligne\n".repeat(1_000));
        let mut view = view(600.0);
        view.prepare(&mut text, &document);
        let now = Instant::now();
        // Pavé tactile : immédiat.
        view.scroll_by(0.0, 39.0 * 100.0, false, now);
        view.prepare(&mut text, &document);
        assert_eq!(view.visible_lines().start, 99);
        assert_eq!(view.cached_lines(), view.visible_lines().len());
        // Molette : animée, la position finale est atteinte après la durée de l'animation.
        view.scroll_by(0.0, -39.0 * 50.0, true, now);
        assert!(view.is_scrolling());
        assert!(view.tick(now + crate::scroll::SCROLL_DURATION / 2));
        assert!(!view.tick(now + crate::scroll::SCROLL_DURATION));
        view.prepare(&mut text, &document);
        assert_eq!(view.visible_lines().start, 49);
        // Bornes : ni avant le début, ni après la dernière ligne en haut de la vue.
        view.scroll_by(0.0, -1e9, false, now);
        view.prepare(&mut text, &document);
        assert_eq!(view.visible_lines().start, 0);
        view.scroll_by(0.0, 1e9, false, now);
        view.prepare(&mut text, &document);
        assert_eq!(view.visible_lines().start, 999);
    }

    #[test]
    fn la_vue_suit_le_curseur() {
        let mut text = TextSystem::new();
        let long = "x".repeat(400);
        let mut document = Document::from_text(&format!("{}{long}\nfin", "ligne\n".repeat(500)));
        let mut view = view(600.0);
        view.prepare(&mut text, &document);

        // Curseur en fin de la longue ligne 500 : la vue descend et défile vers la droite.
        let head = 500 * 6 + 400;
        document.set_selections(Selections::cursor(head));
        view.follow_cursor();
        view.prepare(&mut text, &document);
        assert!(view.visible_lines().contains(&500));
        let cursor = view.primary_cursor_rect(&document).unwrap();
        assert!(cursor.y >= 0.0 && cursor.bottom() <= 600.0, "{cursor:?}");
        assert!(cursor.x > 0.0 && cursor.right() <= 800.0, "{cursor:?}");
        assert!(view.scroll_x() > 0.0);

        // Retour au début du document : la vue remonte et revient à gauche.
        document.set_selections(Selections::cursor(0));
        view.follow_cursor();
        view.prepare(&mut text, &document);
        assert_eq!(view.visible_lines().start, 0);
        assert_eq!(view.scroll_x(), 0.0);

        // Sans demande de suivi, défiler ne ramène pas au curseur.
        view.scroll_by(0.0, 39.0 * 200.0, false, Instant::now());
        view.prepare(&mut text, &document);
        assert_eq!(view.visible_lines().start, 199);
    }

    #[test]
    fn texte_en_composition_affiche_au_curseur() {
        let mut text = TextSystem::new();
        let mut document = Document::from_text("ab");
        document.set_selections(Selections::cursor(1));
        let mut view = view(600.0);
        let theme = Theme::default();
        view.prepare(&mut text, &document);
        let before = view.primary_cursor_rect(&document).unwrap().x;

        view.set_preedit(Some(Preedit {
            text: "日本".to_owned(),
            cursor: Some(3),
        }));
        view.prepare(&mut text, &document);
        assert_eq!(
            view.lines[&0].buffer.lines[0].text(),
            "a日本b",
            "affiché, pas inséré"
        );
        assert_eq!(document.buffer().to_string(), "ab");
        let during = view.primary_cursor_rect(&document).unwrap().x;
        assert!(during > before, "curseur après « 日 »");
        let frame = view.frame(&theme, &document, false);
        assert_eq!(
            frame.layers[0].overlays.len(),
            1,
            "soulignement de la composition"
        );
        let underline = frame.layers[0].overlays[0].rect;
        assert!(underline.x <= before + 1.0 && underline.right() > during);

        view.set_preedit(None);
        view.prepare(&mut text, &document);
        assert_eq!(view.lines[&0].buffer.lines[0].text(), "ab");
        assert_eq!(view.primary_cursor_rect(&document).unwrap().x, before);
    }

    #[test]
    fn position_sous_la_souris() {
        let mut text = TextSystem::new();
        let document = Document::from_text("abcd\nef\n");
        let mut view = view(600.0);
        view.prepare(&mut text, &document);
        let buffer = document.buffer();
        let left = view.text_left();
        let top = view.line_top(0);
        let char_width = view.lines[&0].x(1);
        let lh = view.line_height();
        let at = |x: f32, y: f32| view.offset_at(x, y, buffer);
        assert_eq!(
            at(left - 5.0, top + 1.0),
            0,
            "à gauche du texte : début de ligne"
        );
        assert_eq!(at(left + 0.4 * char_width, top + 1.0), 0);
        assert_eq!(
            at(left + 0.6 * char_width, top + 1.0),
            1,
            "limite la plus proche"
        );
        assert_eq!(at(left + 2.6 * char_width, top + lh - 1.0), 3);
        assert_eq!(
            at(left + 50.0 * char_width, top + 1.0),
            4,
            "après la fin : fin de ligne"
        );
        assert_eq!(
            at(left + 1.2 * char_width, top + lh + 1.0),
            6,
            "deuxième ligne"
        );
        assert_eq!(at(left, -100.0), 0, "au-dessus : début");
        assert_eq!(at(left, 10_000.0), 8, "en dessous : fin");
        assert_eq!(view.pointer_at(left + 10.0, 10.0), Pointer::Text);
        assert_eq!(view.pointer_at(10.0, 10.0), Pointer::Default, "gouttière");
        assert_eq!(view.pointer_at(left + 10.0, 900.0), Pointer::Default);
        assert_eq!(
            at(5.0, top + lh + 1.0),
            5,
            "clic dans la gouttière : début de ligne"
        );
    }

    #[test]
    fn gouttiere_numeros_et_marqueurs() {
        let mut text = TextSystem::new();
        let mut document = Document::from_text(&"x\n".repeat(1_500));
        document.set_selections(Selections::cursor(2 * 2));
        let theme = Theme::default();
        let mut view = view(600.0);
        view.set_markers(vec![GutterMarker {
            line: 2,
            color: cupride_render::Color::rgb(0xff, 0, 0),
        }]);
        view.prepare(&mut text, &document);
        let label = |view: &EditorView, line: usize| view.gutter.numbers[&line].label.clone();
        assert_eq!(label(&view, 0), "1");
        assert_eq!(label(&view, 4), "5");
        // 1 501 lignes : quatre chiffres réservés, le texte commence après la gouttière.
        let digit = view.gutter.digit_width;
        assert!(digit > 0.0);
        assert!(view.gutter_width() >= 4.0 * digit);
        assert!(view.text_left() > view.gutter_width());

        let frame = view.frame(&theme, &document, false);
        let numbers: Vec<_> = frame.layers[0]
            .texts
            .iter()
            .filter(|t| t.color == theme.line_number || t.color == theme.line_number_active)
            .collect();
        assert_eq!(numbers.len(), view.visible_lines().len());
        let active: Vec<_> = numbers
            .iter()
            .filter(|t| t.color == theme.line_number_active)
            .collect();
        assert_eq!(
            active.len(),
            1,
            "numéro de la ligne du curseur mis en valeur"
        );
        // Alignés à droite : « 10 » commence plus à gauche que « 9 ».
        let left_of = |line: usize| {
            numbers
                .iter()
                .find(|t| t.top == view.line_top(line))
                .unwrap()
                .left
        };
        assert!(left_of(9) < left_of(8));
        let marker = frame.layers[0]
            .quads
            .iter()
            .find(|q| q.color.r == 0xff && q.color.g == 0)
            .unwrap();
        assert!(marker.rect.right() <= view.marker_lane());
        assert_eq!(marker.rect.y, view.line_top(2));

        // Relatifs : distance au curseur (ligne 2), numéro réel sur sa ligne.
        view.set_settings(ViewSettings {
            line_numbers: LineNumbers::Relative,
            ..ViewSettings::default()
        });
        view.prepare(&mut text, &document);
        assert_eq!(label(&view, 0), "2");
        assert_eq!(label(&view, 2), "3");
        assert_eq!(label(&view, 7), "5");

        // Sans numéros : la colonne des marqueurs reste réservée.
        view.set_settings(ViewSettings {
            line_numbers: LineNumbers::Off,
            ..ViewSettings::default()
        });
        view.prepare(&mut text, &document);
        assert!(view.gutter.numbers.is_empty());
        assert_eq!(view.gutter_width(), view.marker_lane());
    }

    #[test]
    fn zoom_a_chaud() {
        let mut text = TextSystem::new();
        let document = Document::from_text(&"ligne\n".repeat(1_000));
        let mut view = view(600.0);
        view.prepare(&mut text, &document);
        view.scroll_by(0.0, 39.0 * 100.0, false, Instant::now());
        view.prepare(&mut text, &document);
        assert_eq!(view.visible_lines().start, 99);
        let width = view.lines[&100].x(5);

        // Zoom ×1,5 sur l'écran ×2 : tout grandit, le haut de la vue reste sur le même texte.
        view.set_scale(3.0);
        view.prepare(&mut text, &document);
        assert_eq!(view.line_height(), (14.0_f32 * 1.4 * 3.0).round());
        assert_eq!(view.visible_lines().start, 99);
        let zoomed = view.lines[&100].x(5);
        assert!((zoomed / width - 1.5).abs() < 0.05, "{width} → {zoomed}");
    }
}
