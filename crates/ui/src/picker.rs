//! Sélecteur : un champ de recherche et une liste filtrée par recherche floue. Sert à la palette
//! de commandes et à l'ouverture rapide de fichiers.
//!
//! Il s'affiche en popup, centré en haut de la fenêtre, dans sa propre couche. Flèches haut et bas
//! pour choisir, Entrée pour valider, Échap pour fermer ; un clic sur une ligne la valide.

use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::fuzzy::Fuzzy;
use crate::label::Label;
use crate::layout;
use crate::style::Style;
use crate::text_input::TextInput;
use crate::theme::Theme;

/// Nombre maximal de lignes affichées.
pub const MAX_ROWS: usize = 12;

/// Un élément de la liste.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickerItem {
    /// Texte cherché et affiché : `file open`, `src/main.rs`.
    pub label: String,
    /// Description, à droite du libellé.
    pub detail: String,
    /// Indication à droite de la ligne : raccourci, dossier.
    pub hint: String,
}

#[derive(Debug, Default)]
struct Row {
    label: Label,
    detail: Label,
    hint: Label,
}

/// Sélecteur ouvert.
#[derive(Debug)]
pub struct Picker {
    /// Recherche saisie.
    pub input: TextInput,
    placeholder: String,
    items: Vec<PickerItem>,
    /// Priorité de chaque élément à score égal (plus petit : avant), par exemple la récence.
    priority: Vec<usize>,
    fuzzy: Fuzzy,
    /// Éléments retenus, du meilleur au moins bon.
    results: Vec<usize>,
    /// Recherche à laquelle correspondent `results` (`None` : à recalculer).
    searched: Option<String>,
    selected: usize,
    top: usize,
    input_label: Label,
    count_label: Label,
    rows: Vec<Row>,
    row_areas: Vec<Rect>,
    area: Rect,
}

impl Picker {
    /// Sélecteur sur `items`, avec `placeholder` affiché tant que la recherche est vide.
    /// `paths` adapte la recherche floue aux chemins.
    pub fn new(
        placeholder: &str,
        items: Vec<PickerItem>,
        priority: Vec<usize>,
        paths: bool,
    ) -> Self {
        let mut picker = Self {
            input: TextInput::default(),
            placeholder: placeholder.to_owned(),
            items: Vec::new(),
            priority: Vec::new(),
            fuzzy: Fuzzy::new(paths),
            results: Vec::new(),
            searched: None,
            selected: 0,
            top: 0,
            input_label: Label::default(),
            count_label: Label::default(),
            rows: Vec::new(),
            row_areas: Vec::new(),
            area: Rect::default(),
        };
        picker.set_items(items, priority);
        picker
    }

    /// Remplace les éléments (indexation en arrière-plan qui avance) ; la recherche est refaite.
    pub fn set_items(&mut self, items: Vec<PickerItem>, priority: Vec<usize>) {
        self.items = items;
        self.priority = priority;
        self.searched = None;
    }

    /// Nombre d'éléments.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Aucun élément ?
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Refait la recherche si la saisie ou les éléments ont changé.
    pub fn refresh(&mut self) {
        let query = self.input.text().trim().to_owned();
        if self.searched.as_ref() == Some(&query) {
            return;
        }
        let priority = &self.priority;
        let found = self.fuzzy.search(
            &query,
            self.items.iter().map(|item| item.label.as_str()),
            |index| priority.get(index).copied().unwrap_or(usize::MAX),
        );
        self.results = found.into_iter().map(|f| f.index).collect();
        self.selected = 0;
        self.top = 0;
        self.searched = Some(query);
    }

    /// Éléments retenus, du meilleur au moins bon (rangs dans la liste des éléments).
    pub fn results(&mut self) -> &[usize] {
        self.refresh();
        &self.results
    }

    /// Élément choisi (son rang dans la liste des éléments).
    pub fn selected(&mut self) -> Option<usize> {
        self.refresh();
        self.results.get(self.selected).copied()
    }

    /// Élément d'indice `index`.
    pub fn item(&self, index: usize) -> Option<&PickerItem> {
        self.items.get(index)
    }

    /// Déplace le choix de `delta` lignes, en boucle.
    pub fn move_selection(&mut self, delta: isize) {
        self.refresh();
        let count = self.results.len();
        if count == 0 {
            return;
        }
        let current = self.selected as isize;
        self.selected = (current + delta).rem_euclid(count as isize) as usize;
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + MAX_ROWS {
            self.top = self.selected + 1 - MAX_ROWS;
        }
    }

    /// Zone occupée, après [`Picker::prepare`].
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Élément sous le point (`x`, `y`), après [`Picker::prepare`].
    pub fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        let row = self.row_areas.iter().position(|r| r.contains(x, y))?;
        self.results.get(self.top + row).copied()
    }

    fn row_height(style: &Style) -> f32 {
        style.line_height + 2.0 * style.px(4.0)
    }

    /// Recherche, mise en forme des lignes visibles, et placement dans `window`.
    pub fn prepare(&mut self, text: &mut TextSystem, style: &Style, theme: &Theme, window: Rect) {
        self.refresh();
        let metrics = style.metrics();
        let query = self.input.text().trim().to_owned();
        let shown = self.results.len().saturating_sub(self.top).min(MAX_ROWS);
        let row_height = Self::row_height(style);
        let input_height = row_height + 2.0 * style.px(4.0);
        let width = (window.width * 0.6).clamp(style.px(420.0).min(window.width), style.px(900.0));
        let height = input_height + shown as f32 * row_height + style.px(6.0);
        self.area = layout::centered_top(window, width, height, (window.height * 0.12).round());

        let input_text = if self.input.text().is_empty() {
            self.placeholder.as_str()
        } else {
            self.input.text()
        };
        self.input_label
            .set(text, input_text, &style.family, metrics);
        let count = format!("{}/{}", self.results.len(), self.items.len());
        self.count_label.set(text, &count, &style.family, metrics);

        self.rows.resize_with(shown, Row::default);
        self.row_areas.clear();
        let mut y = self.area.y + input_height;
        for (slot, &index) in self.results[self.top..self.top + shown].iter().enumerate() {
            let item = &self.items[index];
            let positions = if query.is_empty() {
                Vec::new()
            } else {
                self.fuzzy.positions(&query, &item.label)
            };
            let row = &mut self.rows[slot];
            row.label.set_highlighted(
                text,
                &item.label,
                &positions,
                theme.accent,
                &style.family,
                metrics,
            );
            row.detail.set(text, &item.detail, &style.family, metrics);
            row.hint.set(text, &item.hint, &style.family, metrics);
            self.row_areas
                .push(Rect::new(self.area.x, y, self.area.width, row_height));
            y += row_height;
        }
    }

    /// Rectangle du curseur de recherche (pour la fenêtre de l'IME).
    pub fn cursor_rect(&self, style: &Style) -> Rect {
        let x = if self.input.text().is_empty() {
            0.0
        } else {
            self.input_label.x_at(self.input.cursor())
        };
        Rect::new(
            (self.area.x + style.px(12.0) + x).round(),
            self.area.y + style.px(4.0) + style.px(4.0),
            style.px(2.0).max(1.0),
            style.line_height,
        )
    }

    /// Dessine le sélecteur dans `layer`.
    pub fn draw<'a>(
        &'a self,
        layer: &mut Layer<'a>,
        style: &Style,
        theme: &Theme,
        cursor_visible: bool,
    ) {
        let area = self.area;
        let border = style.px(1.0).max(1.0);
        layer.quads.push(Quad::new(area, theme.border));
        let inner = layout::inset(area, border);
        layer.quads.push(Quad::new(inner, theme.popup));
        let pad = style.px(12.0);
        let row_height = Self::row_height(style);
        let input_top = area.y + style.px(8.0);
        let input_color = if self.input.text().is_empty() {
            theme.muted
        } else {
            theme.foreground
        };
        let selection = self.input.selection();
        if !selection.is_empty() && !self.input.text().is_empty() {
            let x0 = self.input_label.x_at(selection.start);
            let x1 = self.input_label.x_at(selection.end);
            layer.quads.push(Quad::new(
                Rect::new(area.x + pad + x0, input_top, x1 - x0, style.line_height),
                theme.selection,
            ));
        }
        layer.texts.extend(
            self.input_label
                .item(area.x + pad, input_top, inner, input_color),
        );
        layer.texts.extend(self.count_label.item(
            area.right() - pad - self.count_label.width(),
            input_top,
            inner,
            theme.muted,
        ));
        let separator_y = area.y + row_height + 2.0 * style.px(4.0) - border;
        layer.quads.push(Quad::new(
            Rect::new(inner.x, separator_y, inner.width, border),
            theme.border,
        ));
        if cursor_visible {
            layer
                .overlays
                .push(Quad::new(self.cursor_rect(style), theme.cursor));
        }
        for (slot, (row, rect)) in self.rows.iter().zip(&self.row_areas).enumerate() {
            if self.top + slot == self.selected {
                layer
                    .quads
                    .push(Quad::new(rect.intersect(&inner), theme.list_selection));
            }
            let top = rect.y + style.px(4.0);
            let clip = rect.intersect(&inner);
            let hint_left = rect.right() - pad - row.hint.width();
            layer
                .texts
                .extend(row.label.item(rect.x + pad, top, clip, theme.foreground));
            let detail_left = rect.x + pad + row.label.width() + style.px(16.0);
            let detail_clip = Rect::new(
                detail_left,
                rect.y,
                (hint_left - style.px(16.0) - detail_left).max(0.0),
                rect.height,
            );
            layer
                .texts
                .extend(row.detail.item(detail_left, top, detail_clip, theme.muted));
            layer
                .texts
                .extend(row.hint.item(hint_left, top, clip, theme.muted));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(labels: &[&str]) -> Vec<PickerItem> {
        labels
            .iter()
            .map(|l| PickerItem {
                label: (*l).to_owned(),
                ..PickerItem::default()
            })
            .collect()
    }

    #[test]
    fn filtrage_et_choix() {
        let labels = ["edit undo", "file open", "file save", "view zoom-in"];
        // « file save » a été utilisée récemment.
        let mut picker = Picker::new("Commande", items(&labels), vec![9, 9, 0, 9], false);
        assert_eq!(picker.results(), [2, 0, 1, 3], "récentes en tête");
        picker.input.insert("fi");
        assert_eq!(picker.results(), [2, 1]);
        picker.move_selection(1);
        assert_eq!(picker.selected(), Some(1));
        picker.move_selection(1);
        assert_eq!(picker.selected(), Some(2), "en boucle");
        picker.input.insert("x");
        assert_eq!(picker.selected(), None, "aucun résultat");
    }

    #[test]
    fn defilement_et_placement() {
        let labels: Vec<String> = (0..40).map(|i| format!("commande {i:02}")).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut picker = Picker::new("Commande", items(&refs), vec![], false);
        picker.move_selection(-1);
        assert_eq!(
            picker.selected(),
            Some(39),
            "remonter depuis le haut : le dernier"
        );
        picker.move_selection(1);
        assert_eq!(picker.selected(), Some(0));

        let mut text = TextSystem::new();
        let style = Style {
            family: "monospace".into(),
            font_size: 14.0,
            line_height: 20.0,
            scale: 1.0,
        };
        let window = Rect::new(0.0, 0.0, 1000.0, 700.0);
        picker.prepare(&mut text, &style, &Theme::default(), window);
        assert_eq!(picker.rows.len(), MAX_ROWS);
        let area = picker.area();
        assert!(area.x > 0.0 && area.right() < 1000.0, "centré");
        let first = picker.row_areas[0];
        assert_eq!(picker.item_at(first.x + 5.0, first.y + 5.0), Some(0));
        assert_eq!(picker.item_at(1.0, 1.0), None);
        let mut layer = Layer::default();
        picker.draw(&mut layer, &style, &Theme::default(), true);
        assert!(
            layer
                .quads
                .iter()
                .any(|q| q.color == Theme::default().list_selection)
        );
    }
}
