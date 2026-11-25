//! Barre de recherche : un champ de saisie, les options (casse, mot entier, expression régulière)
//! et la position dans les occurrences (« 3/12 »).
//!
//! Elle s'affiche en haut à droite de la zone active, par-dessus le texte. La recherche est
//! incrémentale : chaque frappe relance la recherche (l'application la fait dans un thread à part).
//! Une seconde ligne, facultative, donne le texte de remplacement ; Tab passe d'un champ à
//! l'autre.

use cupride_core::search::SearchQuery;
use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::label::Label;
use crate::style::Style;
use crate::text_input::TextInput;
use crate::theme::Theme;

/// Largeur maximale de la barre, en pixels logiques.
const WIDTH: f32 = 480.0;

/// Option de recherche, cliquable dans la barre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindOption {
    /// Respecter la casse.
    Case,
    /// Mot entier.
    Word,
    /// Expression régulière.
    Regex,
}

impl FindOption {
    /// Toutes les options, dans l'ordre de la barre.
    pub const ALL: [Self; 3] = [Self::Case, Self::Word, Self::Regex];

    fn label(self) -> &'static str {
        match self {
            Self::Case => "Aa",
            Self::Word => "mot",
            Self::Regex => ".*",
        }
    }
}

/// Barre de recherche ouverte.
#[derive(Debug, Default)]
pub struct FindBar {
    /// Motif saisi.
    pub input: TextInput,
    /// Texte de remplacement, quand la ligne de remplacement est affichée.
    pub replace: Option<TextInput>,
    /// Le clavier est dans le champ de remplacement.
    replace_focused: bool,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
    /// Position dans les occurrences, ou erreur.
    status: Option<(String, bool)>,
    area: Rect,
    input_area: Rect,
    options: Vec<(FindOption, Rect)>,
    text: Label,
    replace_area: Rect,
    replace_prompt: Label,
    replace_text: Label,
    option_labels: Vec<Label>,
    status_label: Label,
}

impl FindBar {
    /// Barre préremplie avec `query`, motif sélectionné : la frappe le remplace.
    pub fn new(query: &SearchQuery) -> Self {
        let mut input = TextInput::with_text(&query.pattern);
        input.select_all();
        Self {
            input,
            case_sensitive: query.case_sensitive,
            whole_word: query.whole_word,
            regex: query.regex,
            ..Self::default()
        }
    }

    /// Motif et options.
    pub fn query(&self) -> SearchQuery {
        SearchQuery {
            pattern: self.input.text().to_owned(),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            regex: self.regex,
        }
    }

    /// L'option est-elle active ?
    pub fn is_on(&self, option: FindOption) -> bool {
        match option {
            FindOption::Case => self.case_sensitive,
            FindOption::Word => self.whole_word,
            FindOption::Regex => self.regex,
        }
    }

    /// Active ou désactive une option.
    pub fn toggle(&mut self, option: FindOption) {
        let flag = match option {
            FindOption::Case => &mut self.case_sensitive,
            FindOption::Word => &mut self.whole_word,
            FindOption::Regex => &mut self.regex,
        };
        *flag = !*flag;
    }

    /// Affiche la ligne de remplacement, avec `text`.
    pub fn show_replace(&mut self, text: &str) {
        if self.replace.is_none() {
            self.replace = Some(TextInput::with_text(text));
        }
    }

    /// Le clavier est-il dans le champ de remplacement ?
    pub fn replace_focused(&self) -> bool {
        self.replace_focused && self.replace.is_some()
    }

    /// Passe le clavier d'un champ à l'autre (s'il y a une ligne de remplacement).
    pub fn switch_field(&mut self) {
        self.replace_focused = self.replace.is_some() && !self.replace_focused;
    }

    /// Donne le clavier au champ de remplacement (`true`) ou de recherche.
    pub fn focus_replace(&mut self, replace: bool) {
        self.replace_focused = replace && self.replace.is_some();
    }

    /// Champ qui a le clavier.
    pub fn focused_input(&mut self) -> &mut TextInput {
        match &mut self.replace {
            Some(replace) if self.replace_focused => replace,
            _ => &mut self.input,
        }
    }

    /// Texte de remplacement.
    pub fn replacement(&self) -> Option<&str> {
        self.replace.as_ref().map(TextInput::text)
    }

    /// Texte à droite : « 3/12 », « aucun résultat », ou une erreur (`error`).
    pub fn set_status(&mut self, status: Option<String>, error: bool) {
        self.status = status.map(|s| (s, error));
    }

    /// Statut affiché.
    pub fn status(&self) -> Option<&str> {
        self.status.as_ref().map(|(s, _)| s.as_str())
    }

    /// Hauteur de la barre.
    pub fn height(style: &Style) -> f32 {
        style.line_height + 2.0 * style.px(6.0)
    }

    /// Met en forme les textes, et place la barre en haut à droite de `zone`.
    pub fn prepare(&mut self, text: &mut TextSystem, style: &Style, zone: Rect) {
        let metrics = style.metrics();
        let pad = style.px(8.0);
        let width = style.px(WIDTH).min(zone.width - 2.0 * pad).max(0.0);
        let height = Self::height(style).min(zone.height);
        let rows = if self.replace.is_some() { 2.0 } else { 1.0 };
        self.area = Rect::new(
            zone.right() - width - pad,
            zone.y + pad,
            width,
            (rows * height).min(zone.height),
        );
        self.text
            .set(text, self.input.text(), &style.family, metrics);
        let status = self.status.as_ref().map_or("", |(s, _)| s.as_str());
        self.status_label.set(text, status, &style.family, metrics);
        self.option_labels
            .resize_with(FindOption::ALL.len(), Label::default);
        // Options et statut à droite, la saisie prend le reste.
        let mut right = self.area.right() - pad;
        if !status.is_empty() {
            right -= self.status_label.width() + pad;
        }
        self.options.clear();
        for (option, label) in FindOption::ALL.iter().zip(&mut self.option_labels).rev() {
            label.set(text, option.label(), &style.family, metrics);
            let width = label.width() + style.px(8.0);
            right -= width;
            self.options.push((
                *option,
                Rect::new(
                    right,
                    self.area.y + style.px(4.0),
                    width,
                    height - style.px(8.0),
                ),
            ));
            right -= style.px(2.0);
        }
        self.options.reverse();
        self.input_area = Rect::new(
            self.area.x + pad,
            self.area.y,
            (right - pad - self.area.x - pad).max(0.0),
            height,
        );
        if let Some(replace) = &self.replace {
            self.replace_prompt.set(text, "par", &style.family, metrics);
            self.replace_text
                .set(text, replace.text(), &style.family, metrics);
            let left = self.area.x + pad + self.replace_prompt.width() + style.px(8.0);
            self.replace_area = Rect::new(
                left,
                self.area.y + height,
                (self.area.right() - pad - left).max(0.0),
                height,
            );
        }
    }

    /// Zone occupée, après [`FindBar::prepare`].
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Option sous le point (`x`, `y`).
    pub fn option_at(&self, x: f32, y: f32) -> Option<FindOption> {
        self.options
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(o, _)| *o)
    }

    fn text_top(&self, style: &Style) -> f32 {
        self.area.y + style.px(6.0)
    }

    /// Rectangle du curseur de saisie (pour placer la fenêtre de l'IME).
    pub fn cursor_rect(&self, style: &Style) -> Rect {
        let (area, label, cursor) = match &self.replace {
            Some(replace) if self.replace_focused => {
                (self.replace_area, &self.replace_text, replace.cursor())
            }
            _ => (self.input_area, &self.text, self.input.cursor()),
        };
        Rect::new(
            (area.x + label.x_at(cursor)).round(),
            area.y + style.px(6.0),
            style.px(2.0).max(1.0),
            style.line_height,
        )
    }

    /// Le point (`x`, `y`) est-il sur la ligne de remplacement ?
    pub fn in_replace(&self, x: f32, y: f32) -> bool {
        self.replace.is_some()
            && Rect::new(
                self.area.x,
                self.replace_area.y,
                self.area.width,
                self.replace_area.height,
            )
            .contains(x, y)
    }

    /// Dessine la barre.
    pub fn draw<'a>(
        &'a self,
        layer: &mut Layer<'a>,
        style: &Style,
        theme: &Theme,
        cursor_visible: bool,
    ) {
        let area = self.area;
        let line = style.px(1.0).max(1.0);
        layer.quads.push(Quad::new(area, theme.popup));
        for edge in [
            Rect::new(area.x, area.y, area.width, line),
            Rect::new(area.x, area.bottom() - line, area.width, line),
            Rect::new(area.x, area.y, line, area.height),
            Rect::new(area.right() - line, area.y, line, area.height),
        ] {
            layer.quads.push(Quad::new(edge, theme.border));
        }
        let top = self.text_top(style);
        let selection = self.input.selection();
        if !selection.is_empty() {
            let x0 = self.text.x_at(selection.start);
            let x1 = self.text.x_at(selection.end);
            layer.quads.push(Quad::new(
                Rect::new(self.input_area.x + x0, top, x1 - x0, style.line_height)
                    .intersect(&self.input_area),
                theme.selection,
            ));
        }
        layer.texts.extend(self.text.item(
            self.input_area.x,
            top,
            self.input_area,
            theme.foreground,
        ));
        if let Some(replace) = &self.replace {
            let top = self.replace_area.y + style.px(6.0);
            layer.texts.extend(self.replace_prompt.item(
                area.x + style.px(8.0),
                top,
                area,
                theme.muted,
            ));
            let selection = replace.selection();
            if !selection.is_empty() {
                let x0 = self.replace_text.x_at(selection.start);
                let x1 = self.replace_text.x_at(selection.end);
                layer.quads.push(Quad::new(
                    Rect::new(self.replace_area.x + x0, top, x1 - x0, style.line_height)
                        .intersect(&self.replace_area),
                    theme.selection,
                ));
            }
            layer.texts.extend(self.replace_text.item(
                self.replace_area.x,
                top,
                self.replace_area,
                theme.foreground,
            ));
        }
        if cursor_visible {
            layer.overlays.push(Quad::new(
                self.cursor_rect(style).intersect(&area),
                theme.cursor,
            ));
        }
        for ((option, rect), label) in self.options.iter().zip(&self.option_labels) {
            let on = self.is_on(*option);
            if on {
                layer.quads.push(Quad::new(*rect, theme.list_selection));
            }
            let color = if on { theme.foreground } else { theme.muted };
            layer
                .texts
                .extend(label.item(rect.x + style.px(4.0), top, *rect, color));
        }
        if let Some((_, error)) = &self.status {
            let color = if *error { theme.error } else { theme.muted };
            let x = area.right() - style.px(8.0) - self.status_label.width();
            layer
                .texts
                .extend(self.status_label.item(x, top, area, color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> Style {
        Style {
            family: "monospace".into(),
            font_size: 14.0,
            line_height: 20.0,
            scale: 1.0,
        }
    }

    #[test]
    fn options_et_placement() {
        let query = SearchQuery {
            whole_word: true,
            ..SearchQuery::new("chat")
        };
        let mut bar = FindBar::new(&query);
        assert_eq!(bar.query(), query);
        assert_eq!(bar.input.selection(), 0..4, "motif sélectionné");
        bar.input.insert("chien");
        bar.toggle(FindOption::Regex);
        let query = bar.query();
        assert_eq!(query.pattern, "chien");
        assert!(query.regex && query.whole_word && !query.case_sensitive);

        let mut text = TextSystem::new();
        bar.set_status(Some("3/12".into()), false);
        bar.prepare(&mut text, &style(), Rect::new(100.0, 50.0, 900.0, 600.0));
        let area = bar.area();
        assert_eq!(area.right(), 992.0, "en haut à droite de la zone");
        assert_eq!(area.y, 58.0);
        assert_eq!(area.width, 480.0);
        let (_, case) = bar.options[0];
        let (x, y) = (case.x + 2.0, case.y + 2.0);
        assert_eq!(bar.option_at(x, y), Some(FindOption::Case));
        assert_eq!(bar.option_at(area.x + 5.0, area.y + 5.0), None);
        let mut layer = Layer::default();
        bar.draw(&mut layer, &style(), &Theme::default(), true);
        assert!(!layer.texts.is_empty());
        // Zone étroite : la barre s'y réduit.
        bar.prepare(&mut text, &style(), Rect::new(0.0, 0.0, 200.0, 300.0));
        assert!(bar.area().width <= 184.0);
    }

    #[test]
    fn ligne_de_remplacement() {
        let mut bar = FindBar::new(&SearchQuery::new("a"));
        bar.switch_field();
        assert!(!bar.replace_focused(), "pas de champ de remplacement");
        bar.show_replace("b");
        bar.switch_field();
        assert!(bar.replace_focused());
        bar.focused_input().insert("c");
        assert_eq!(bar.replacement(), Some("bc"));
        assert_eq!(bar.input.text(), "a");
        let mut text = TextSystem::new();
        bar.prepare(&mut text, &style(), Rect::new(0.0, 0.0, 800.0, 600.0));
        assert_eq!(
            bar.area().height,
            2.0 * FindBar::height(&style()),
            "deux lignes"
        );
        let area = bar.area();
        assert!(bar.in_replace(area.x + 5.0, area.bottom() - 5.0));
        assert!(!bar.in_replace(area.x + 5.0, area.y + 5.0));
        assert!(bar.cursor_rect(&style()).y > area.y + FindBar::height(&style()) - 1.0);
    }
}
