//! Panneau des résultats de la recherche dans le projet.
//!
//! En tête, le bilan (« 12 résultats dans 3 fichiers ») ; puis chaque fichier et ses lignes, les
//! occurrences en couleur. Les résultats arrivent au fil de l'eau : le panneau se redessine à
//! chaque fichier lu. En aperçu de remplacement, chaque ligne s'affiche telle qu'elle deviendrait.
//! Flèches, pages, Début et Fin pour se déplacer ; Entrée (ou un double clic) ouvre le fichier à
//! la ligne, Espace (ou un clic) en aperçu.

use std::any::Any;
use std::collections::HashMap;
use std::time::Instant;

use cupride_core::Editor;
use cupride_core::command::quote;
use cupride_core::keymap::{Key, KeyCombo, Modifiers, NamedKey};
use cupride_core::layout::PanelId;
use cupride_core::project_search::ProjectSearch;
use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::editor_view::ViewSettings;
use crate::label::Label;
use crate::panels::{PanelContext, PanelView, ViewAction};
use crate::style::Style;

/// Une ligne du panneau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultRow {
    /// Un fichier (rang dans les résultats).
    File(usize),
    /// Une ligne d'un fichier.
    Line(usize, usize),
}

/// Lignes du panneau pour ces résultats.
pub fn result_rows(search: &ProjectSearch) -> Vec<ResultRow> {
    let mut rows = Vec::new();
    for (file, matches) in search.files.iter().enumerate() {
        rows.push(ResultRow::File(file));
        rows.extend((0..matches.lines.len()).map(|line| ResultRow::Line(file, line)));
    }
    rows
}

/// Vue du panneau `search`.
pub struct SearchResultsView {
    panel: PanelId,
    settings: ViewSettings,
    scale: f32,
    bounds: Rect,
    /// Ligne sélectionnée, et recherche dont elle vient.
    selected: Option<ResultRow>,
    generation: u64,
    rows: Vec<ResultRow>,
    scroll: f32,
    follow: bool,
    summary: Label,
    labels: HashMap<(ResultRow, bool), Label>,
    /// Ce que la vue montre, pour savoir quoi ouvrir (fichier, ligne, colonne).
    targets: HashMap<ResultRow, (String, usize, usize)>,
}

impl std::fmt::Debug for SearchResultsView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchResultsView")
            .field("panel", &self.panel)
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}

impl SearchResultsView {
    /// Vue du panneau `panel`.
    pub fn new(panel: PanelId, settings: ViewSettings, scale: f32) -> Self {
        Self {
            panel,
            settings,
            scale,
            bounds: Rect::default(),
            selected: None,
            generation: 0,
            rows: Vec::new(),
            scroll: 0.0,
            follow: false,
            summary: Label::default(),
            labels: HashMap::new(),
            targets: HashMap::new(),
        }
    }

    fn style(&self) -> Style {
        self.settings.style(self.scale)
    }

    fn row_height(&self) -> f32 {
        let style = self.style();
        style.line_height + style.px(4.0)
    }

    fn padding(&self) -> f32 {
        self.style().px(6.0)
    }

    /// Haut de la liste, sous le bilan.
    fn list_top(&self) -> f32 {
        self.bounds.y + self.padding() + self.row_height()
    }

    fn selected_index(&self) -> Option<usize> {
        self.rows.iter().position(|r| Some(*r) == self.selected)
    }

    /// Lignes sélectionnables, et sélection, à jour avec les résultats.
    pub fn sync(&mut self, editor: &Editor) {
        let Some(search) = editor.project_search() else {
            self.rows.clear();
            self.selected = None;
            return;
        };
        if search.generation != self.generation {
            self.generation = search.generation;
            self.selected = None;
            self.scroll = 0.0;
            self.labels.clear();
        }
        self.rows = result_rows(search);
        if self.selected.is_none_or(|s| !self.rows.contains(&s)) {
            self.selected = self
                .rows
                .iter()
                .copied()
                .find(|r| matches!(r, ResultRow::Line(..)));
        }
        self.targets.clear();
        for row in &self.rows {
            if let ResultRow::Line(file, line) = *row {
                let found = &search.files[file];
                let line_match = &found.lines[line];
                let column = line_match
                    .ranges
                    .first()
                    .map_or(0, |r| line_match.text[..r.start].chars().count());
                self.targets.insert(
                    *row,
                    (
                        found.path.to_string_lossy().into_owned(),
                        line_match.line + 1,
                        column + 1,
                    ),
                );
            }
        }
    }

    /// Ligne sélectionnée.
    pub fn selected(&self) -> Option<ResultRow> {
        self.selected
    }

    /// Déplace la sélection de `delta` lignes (fichiers compris).
    pub fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let index = self.selected_index().unwrap_or(0) as isize + delta;
        let index = index.clamp(0, self.rows.len() as isize - 1) as usize;
        self.selected = Some(self.rows[index]);
        self.follow = true;
    }

    /// Ouvre la ligne sélectionnée (sur un fichier : sa première ligne).
    fn open(&self, preview: bool) -> ViewAction {
        let row = match self.selected {
            Some(ResultRow::File(file)) => ResultRow::Line(file, 0),
            Some(row) => row,
            None => return ViewAction::Handled,
        };
        let Some((path, line, column)) = self.targets.get(&row) else {
            return ViewAction::Handled;
        };
        let preview = if preview { " --preview" } else { "" };
        ViewAction::Run(format!(
            "file open{preview} {} --line {line} --column {column}",
            quote(path)
        ))
    }

    /// Ligne sous l'ordonnée `y`.
    fn row_at(&self, y: f32) -> Option<usize> {
        let offset = y - self.list_top() + self.scroll;
        if offset < 0.0 {
            return None;
        }
        let index = (offset / self.row_height().max(1.0)) as usize;
        (index < self.rows.len()).then_some(index)
    }

    fn visible_height(&self) -> f32 {
        (self.bounds.bottom() - self.list_top()).max(0.0)
    }

    fn max_scroll(&self) -> f32 {
        (self.rows.len() as f32 * self.row_height() + self.padding() - self.visible_height())
            .max(0.0)
    }

    fn reveal_selection(&mut self) {
        let Some(index) = self.selected_index() else {
            return;
        };
        let height = self.row_height();
        let top = index as f32 * height;
        if top < self.scroll {
            self.scroll = top;
        } else if top + height > self.scroll + self.visible_height() {
            self.scroll = top + height - self.visible_height();
        }
    }
}

/// Positions (en octets) des caractères des plages `ranges` d'un texte, décalées de `shift`.
fn highlighted(text: &str, ranges: &[std::ops::Range<usize>], shift: usize) -> Vec<usize> {
    text.char_indices()
        .filter(|(i, _)| ranges.iter().any(|r| r.contains(i)))
        .map(|(i, _)| i + shift)
        .collect()
}

impl PanelView for SearchResultsView {
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_settings(&mut self, settings: &ViewSettings) {
        if settings != &self.settings {
            self.settings = settings.clone();
            self.labels.clear();
        }
    }

    fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON && scale > 0.0 {
            self.scroll *= scale / self.scale;
            self.scale = scale;
            self.labels.clear();
        }
    }

    fn prepare(&mut self, text: &mut TextSystem, cx: &PanelContext<'_>) {
        self.sync(cx.editor);
        if std::mem::take(&mut self.follow) {
            self.reveal_selection();
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        let style = self.style();
        let metrics = style.metrics();
        let summary = cx.editor.project_search().map_or_else(
            || "Aucune recherche : search project <motif> (mod+shift+f)".to_owned(),
            ProjectSearch::describe,
        );
        self.summary.set(text, &summary, &style.family, metrics);
        let Some(search) = cx.editor.project_search() else {
            return;
        };
        let preview = search.replacement.is_some();
        let height = self.row_height().max(1.0);
        let first = (self.scroll / height) as usize;
        let count = (self.bounds.height / height).ceil() as usize + 1;
        let visible: Vec<ResultRow> = self.rows.iter().skip(first).take(count).copied().collect();
        self.labels
            .retain(|(row, p), _| *p == preview && visible.contains(row));
        for row in visible {
            let label = self.labels.entry((row, preview)).or_default();
            match row {
                ResultRow::File(file) => {
                    let found = &search.files[file];
                    let path = found
                        .path
                        .strip_prefix(&search.root)
                        .unwrap_or(&found.path)
                        .to_string_lossy()
                        .into_owned();
                    label.set(
                        text,
                        &format!("{path} ({})", found.count()),
                        &style.family,
                        metrics,
                    );
                }
                ResultRow::Line(file, line) => {
                    let line_match = &search.files[file].lines[line];
                    let prefix = format!("{:>5}  ", line_match.line + 1);
                    match search.preview(line_match).filter(|_| preview) {
                        Some(new_text) => {
                            label.set(text, &format!("{prefix}{new_text}"), &style.family, metrics);
                        }
                        None => label.set_highlighted(
                            text,
                            &format!("{prefix}{}", line_match.text),
                            &highlighted(&line_match.text, &line_match.ranges, prefix.len()),
                            cx.theme.accent,
                            &style.family,
                            metrics,
                        ),
                    }
                }
            }
        }
    }

    fn draw<'a>(&'a self, layer: &mut Layer<'a>, cx: &PanelContext<'_>) {
        let theme = cx.theme;
        layer.quads.push(Quad::new(self.bounds, theme.panel));
        let style = self.style();
        let padding = self.padding();
        let height = self.row_height();
        let text_offset = ((height - style.line_height) / 2.0).round();
        layer.texts.extend(self.summary.item(
            self.bounds.x + padding,
            self.bounds.y + padding + text_offset,
            self.bounds,
            theme.muted,
        ));
        let list = Rect::new(
            self.bounds.x,
            self.list_top(),
            self.bounds.width,
            self.visible_height(),
        );
        let preview = cx
            .editor
            .project_search()
            .is_some_and(|s| s.replacement.is_some());
        let selected = self.selected_index();
        for (index, row) in self.rows.iter().enumerate() {
            let top = list.y + index as f32 * height - self.scroll;
            if top + height < list.y {
                continue;
            }
            if top > list.bottom() {
                break;
            }
            if Some(index) == selected {
                let color = if cx.focused {
                    theme.list_selection
                } else {
                    theme.current_line
                };
                let rect = Rect::new(list.x, top, list.width, height);
                layer.quads.push(Quad::new(rect.intersect(&list), color));
            }
            let Some(label) = self.labels.get(&(*row, preview)) else {
                continue;
            };
            let (indent, color) = match row {
                ResultRow::File(_) => (0.0, theme.foreground),
                ResultRow::Line(..) if preview => (style.px(8.0), theme.accent),
                ResultRow::Line(..) => (style.px(8.0), theme.muted),
            };
            layer.texts.extend(label.item(
                list.x + padding + indent,
                top + text_offset,
                list,
                color,
            ));
        }
    }

    fn scroll_by(&mut self, _dx: f32, dy: f32, _smooth: bool, _now: Instant) {
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    fn key(&mut self, combo: &KeyCombo) -> ViewAction {
        if combo.modifiers != Modifiers::NONE {
            return ViewAction::Ignored;
        }
        let page = (self.visible_height() / self.row_height().max(1.0)).max(1.0) as isize;
        match combo.key {
            Key::Named(NamedKey::Up) => self.move_selection(-1),
            Key::Named(NamedKey::Down) => self.move_selection(1),
            Key::Named(NamedKey::PageUp) => self.move_selection(-page),
            Key::Named(NamedKey::PageDown) => self.move_selection(page),
            Key::Named(NamedKey::Home) => self.move_selection(isize::MIN / 2),
            Key::Named(NamedKey::End) => self.move_selection(isize::MAX / 2),
            Key::Named(NamedKey::Enter) => return self.open(false),
            Key::Named(NamedKey::Space) => return self.open(true),
            _ => return ViewAction::Ignored,
        }
        ViewAction::Handled
    }

    fn click(&mut self, _x: f32, y: f32, clicks: u8) -> ViewAction {
        let Some(index) = self.row_at(y) else {
            return ViewAction::Handled;
        };
        self.selected = Some(self.rows[index]);
        self.open(clicks < 2)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cupride_core::search::SearchQuery;

    #[test]
    fn resultats_et_ouverture() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "rien\n  été vert\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "vert\n").unwrap();
        let mut editor = Editor::new();
        editor.set_project(dir.path()).unwrap();
        editor.run_project_search(SearchQuery::new("vert")).unwrap();
        let mut view = SearchResultsView::new(PanelId::new(1), ViewSettings::default(), 1.0);
        view.set_bounds(Rect::new(0.0, 0.0, 300.0, 400.0));
        view.sync(&editor);
        assert_eq!(view.rows.len(), 4, "deux fichiers, une ligne chacun");
        let first = view.selected().unwrap();
        assert!(
            matches!(first, ResultRow::Line(..)),
            "première ligne sélectionnée"
        );
        let ViewAction::Run(command) = view.open(false) else {
            panic!("ouverture attendue");
        };
        assert!(command.starts_with("file open "), "{command}");
        assert!(
            command.ends_with("--line 2 --column 7") || command.ends_with("--line 1 --column 1"),
            "{command}"
        );
        let combo = |t| KeyCombo::parse(t, cupride_core::keymap::Platform::Other).unwrap();
        assert_eq!(view.key(&combo("down")), ViewAction::Handled);
        assert!(matches!(view.key(&combo("space")), ViewAction::Run(c) if c.contains("--preview")));
        assert_eq!(
            view.key(&combo("ctrl+p")),
            ViewAction::Ignored,
            "raccourcis globaux"
        );
        // Colonne en caractères : « été » compte 3 caractères avant « vert ».
        let vert = std::iter::once(6..10).collect::<Vec<_>>();
        assert_eq!(highlighted("été vert", &vert, 2), vec![8, 9, 10, 11]);
    }
}
