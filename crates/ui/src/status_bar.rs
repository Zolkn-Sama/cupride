//! Barre d'état : une ligne en bas de la fenêtre, avec des éléments à gauche et à droite.
//!
//! Chaque élément a un nom (`file`, `position`…) et une fonction qui calcule son texte à partir de
//! l'état ; la configuration (`[status_bar]`) choisit lesquels afficher, et dans quel ordre. Les
//! plugins pourront en ajouter avec [`StatusElements::register`].

use std::collections::BTreeMap;

use cupride_core::language::language_of;
use cupride_core::layout::PanelId;
use cupride_core::text::Selections;
use cupride_core::text::TextBuffer;
use cupride_core::text::position::offset_to_grapheme_point;
use cupride_core::{Document, Editor};
use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::label::Label;
use crate::style::Style;
use crate::theme::Theme;

/// Ce que les éléments peuvent lire.
#[derive(Debug, Clone, Copy)]
pub struct StatusContext<'a> {
    /// État de l'éditeur.
    pub editor: &'a Editor,
    /// Séquence de touches en cours (`ctrl+k …`), vide sinon.
    pub pending_keys: &'a str,
    /// Panneau actif de la fenêtre dont c'est la barre d'état ; `None` : le document actif.
    pub panel: Option<PanelId>,
}

impl<'a> StatusContext<'a> {
    /// Document décrit par la barre : celui du panneau de la fenêtre. Un panneau sans document
    /// (explorateur) qui a le focus décrit le document actif ; dans une autre fenêtre, rien.
    pub fn document(&self) -> Option<&'a Document> {
        let Some(panel) = self.panel else {
            return self.editor.active_document();
        };
        match self.editor.panel_document(panel) {
            Some(id) => self.editor.document(id),
            None if self.editor.active_panel() == Some(panel) => self.editor.active_document(),
            None => None,
        }
    }

    /// Sélections décrites par la barre : celles du panneau de la fenêtre.
    pub fn selections(&self) -> Option<&'a Selections> {
        let document = self.document()?;
        Some(
            match self
                .panel
                .filter(|p| self.editor.panel_document(*p).is_some())
            {
                Some(panel) => document.selections_in(panel),
                None => document.selections(),
            },
        )
    }
}

/// Calcule le texte d'un élément, ou `None` pour ne rien afficher.
pub type StatusElement = Box<dyn Fn(&StatusContext<'_>) -> Option<String> + Send + Sync>;

/// Éléments disponibles, par nom.
pub struct StatusElements {
    elements: BTreeMap<String, StatusElement>,
}

impl std::fmt::Debug for StatusElements {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.elements.keys()).finish()
    }
}

impl Default for StatusElements {
    fn default() -> Self {
        let mut elements = Self {
            elements: BTreeMap::new(),
        };
        elements.register("file", |c| Some(c.document()?.display_name()));
        elements.register("modified", |c| {
            c.document()?.is_modified().then(|| "●".to_owned())
        });
        elements.register("keys", |c| {
            (!c.pending_keys.is_empty()).then(|| format!("{} …", c.pending_keys))
        });
        elements.register("selections", |c| {
            let count = c.selections()?.len();
            (count > 1).then(|| format!("{count} curseurs"))
        });
        elements.register("position", |c| {
            let document = c.document()?;
            let selection = c.selections()?.primary();
            let point = offset_to_grapheme_point(document.buffer(), selection.head).ok()?;
            let mut text = format!("{}:{}", point.line + 1, point.column + 1);
            if !selection.is_empty() {
                let selected = document.buffer().slice(selection.range()).ok()?;
                text.push_str(&format!(" ({} sélectionnés)", selected.chars().count()));
            }
            Some(text)
        });
        elements.register("indent", |c| Some(c.document()?.indent().describe()));
        elements.register("line_ending", |c| {
            Some(match c.document()?.line_ending() {
                cupride_core::LineEnding::Lf => "LF".to_owned(),
                cupride_core::LineEnding::Crlf => "CRLF".to_owned(),
            })
        });
        elements.register("encoding", |c| {
            Some(if c.document()?.has_bom() {
                "UTF-8 BOM".to_owned()
            } else {
                "UTF-8".to_owned()
            })
        });
        elements.register("language", |c| {
            let path = c.document()?.path()?;
            Some(language_of(path).unwrap_or("Texte brut").to_owned())
        });
        elements
    }
}

impl StatusElements {
    /// Ajoute (ou remplace) l'élément `name`.
    pub fn register(
        &mut self,
        name: &str,
        element: impl Fn(&StatusContext<'_>) -> Option<String> + Send + Sync + 'static,
    ) {
        self.elements.insert(name.to_owned(), Box::new(element));
    }

    /// Noms des éléments disponibles.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.elements.keys().map(String::as_str)
    }

    /// Texte de l'élément `name`, ou `None` s'il n'existe pas ou n'a rien à afficher.
    pub fn text(&self, name: &str, context: &StatusContext<'_>) -> Option<String> {
        (self.elements.get(name)?)(context)
    }

    /// Noms de `names` qui ne désignent aucun élément (fautes de frappe à signaler).
    pub fn unknown<'a>(&self, names: &'a [String]) -> Vec<&'a str> {
        names
            .iter()
            .map(String::as_str)
            .filter(|name| !self.elements.contains_key(*name))
            .collect()
    }
}

/// Barre d'état mise en forme.
#[derive(Debug, Default)]
pub struct StatusBar {
    left: Vec<Label>,
    right: Vec<Label>,
    area: Rect,
}

impl StatusBar {
    /// Hauteur de la barre, en pixels physiques.
    pub fn height(style: &Style) -> f32 {
        style.line_height + 2.0 * style.px(3.0)
    }

    /// Calcule les éléments `left` et `right`, les met en forme, et place la barre en bas de
    /// `window`.
    pub fn prepare(
        &mut self,
        text: &mut TextSystem,
        style: &Style,
        window: Rect,
        elements: &StatusElements,
        context: &StatusContext<'_>,
        (left, right): (&[String], &[String]),
    ) {
        let height = Self::height(style).min(window.height);
        self.area = Rect::new(window.x, window.bottom() - height, window.width, height);
        let mut shape = |labels: &mut Vec<Label>, names: &[String]| {
            let texts: Vec<String> = names
                .iter()
                .filter_map(|name| elements.text(name, context))
                .collect();
            labels.resize_with(texts.len(), Label::default);
            for (label, value) in labels.iter_mut().zip(&texts) {
                label.set(text, value, &style.family, style.metrics());
            }
        };
        shape(&mut self.left, left);
        shape(&mut self.right, right);
    }

    /// Zone occupée, après [`StatusBar::prepare`].
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Textes affichés, à gauche puis à droite.
    pub fn texts(&self) -> (Vec<&str>, Vec<&str>) {
        (
            self.left.iter().map(Label::text).collect(),
            self.right.iter().map(Label::text).collect(),
        )
    }

    /// Dessine la barre dans `layer`.
    pub fn draw<'a>(&'a self, layer: &mut Layer<'a>, style: &Style, theme: &Theme) {
        let area = self.area;
        layer.quads.push(Quad::new(area, theme.panel));
        layer.quads.push(Quad::new(
            Rect::new(area.x, area.y, area.width, style.px(1.0).max(1.0)),
            theme.border,
        ));
        let gap = style.px(16.0);
        let top = area.y + style.px(3.0);
        let mut x = area.x + style.px(10.0);
        for label in &self.left {
            layer
                .texts
                .extend(label.item(x, top, area, theme.foreground));
            x += label.width() + gap;
        }
        let mut x = area.right() - style.px(10.0);
        for label in self.right.iter().rev() {
            x -= label.width();
            layer.texts.extend(label.item(x, top, area, theme.muted));
            x -= gap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cupride_core::Document;
    use cupride_core::config::StatusBarConfig;
    use cupride_core::text::{Selection, Selections};

    #[test]
    fn elements_par_defaut() {
        let mut editor = Editor::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        std::fs::write(&path, "fn main() {\r\n    é😀\r\n}\r\n").unwrap();
        editor.open(&path).unwrap();
        let document = editor.active_document_mut().unwrap();
        // Un curseur sur la deuxième ligne, et la sélection principale « fn ».
        let mut selections = Selections::cursor(13 + 4 + 2 + 4);
        selections.push(Selection::new(0, 2));
        document.set_selections(selections);

        let elements = StatusElements::default();
        let context = StatusContext {
            editor: &editor,
            pending_keys: "ctrl+k",
            panel: None,
        };
        let text = |name| elements.text(name, &context);
        assert_eq!(text("file").as_deref(), Some("main.rs"));
        assert_eq!(text("modified"), None);
        assert_eq!(text("keys").as_deref(), Some("ctrl+k …"));
        assert_eq!(text("selections").as_deref(), Some("2 curseurs"));
        assert_eq!(text("position").as_deref(), Some("1:3 (2 sélectionnés)"));
        assert_eq!(text("line_ending").as_deref(), Some("CRLF"));
        assert_eq!(text("encoding").as_deref(), Some("UTF-8"));
        assert_eq!(text("language").as_deref(), Some("Rust"));
        assert_eq!(text("inconnu"), None);

        let config = StatusBarConfig::default();
        assert!(elements.unknown(&config.left).is_empty());
        assert!(elements.unknown(&config.right).is_empty());
        assert_eq!(elements.unknown(&["fichier".to_owned()]), ["fichier"]);
    }

    #[test]
    fn position_en_graphemes_et_extension() {
        let mut editor = Editor::new();
        let mut document = Document::from_text("é😀x\nabc");
        document.set_selections(Selections::cursor(6));
        editor.insert_document(document);
        let mut elements = StatusElements::default();
        elements.register("plugin", |_| Some("extension".into()));
        let context = StatusContext {
            editor: &editor,
            pending_keys: "",
            panel: None,
        };
        assert_eq!(elements.text("position", &context).as_deref(), Some("1:3"));
        assert_eq!(elements.text("language", &context), None, "sans fichier");
        assert_eq!(
            elements.text("plugin", &context).as_deref(),
            Some("extension")
        );
        assert!(elements.names().any(|n| n == "plugin"));

        let mut text = TextSystem::new();
        let style = Style {
            family: "monospace".into(),
            font_size: 14.0,
            line_height: 20.0,
            scale: 1.0,
        };
        let mut bar = StatusBar::default();
        let left = vec!["file".to_owned(), "keys".to_owned()];
        let right = vec!["position".to_owned(), "plugin".to_owned()];
        bar.prepare(
            &mut text,
            &style,
            Rect::new(0.0, 0.0, 800.0, 600.0),
            &elements,
            &context,
            (&left, &right),
        );
        assert_eq!(bar.area().bottom(), 600.0);
        let (l, r) = bar.texts();
        assert_eq!(
            l,
            ["sans titre"],
            "sans séquence de touches : seulement le nom"
        );
        assert_eq!(r, ["1:3", "extension"]);
    }
}
