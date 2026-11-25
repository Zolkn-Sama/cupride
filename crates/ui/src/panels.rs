//! Vues des panneaux : interface commune, registre des types et disposition dans la fenêtre.
//!
//! Le modèle d'un panneau (identifiant, type, état) vit dans `layout` ; sa vue vit ici. Le
//! [`ViewRegistry`] associe chaque type de panneau à la fabrique de sa vue : les plugins pourront
//! y ajouter leurs types. [`PanelViews`] garde une vue par panneau, crée celles des nouveaux
//! panneaux, oublie celles des panneaux fermés, et place chaque zone du layout dans la fenêtre.

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::time::Instant;

use cupride_core::layout::{Bounds, Divider, PanelId, PanelKind};
use cupride_core::{DocumentId, Editor};
use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::editor_view::{EditorView, Pointer, ViewSettings};
use crate::label::Label;
use crate::style::Style;
use crate::theme::Theme;

/// Ce qu'une vue reçoit pour se préparer et se dessiner.
#[derive(Debug, Clone, Copy)]
pub struct PanelContext<'a> {
    /// État de l'éditeur.
    pub editor: &'a Editor,
    /// Panneau affiché par la vue.
    pub panel: PanelId,
    /// Couleurs.
    pub theme: &'a Theme,
    /// Le panneau a-t-il le focus clavier ?
    pub focused: bool,
    /// Les curseurs sont-ils allumés (clignotement) ? Ils ne sont dessinés que dans le panneau
    /// qui a le focus.
    pub cursors_visible: bool,
}

/// Vue d'un panneau : transforme l'état du noyau en dessin.
pub trait PanelView: Any {
    /// Place la vue dans la fenêtre, en pixels physiques.
    fn set_bounds(&mut self, bounds: Rect);

    /// Place de la vue dans la fenêtre.
    fn bounds(&self) -> Rect;

    /// Applique de nouveaux réglages d'affichage.
    fn set_settings(&mut self, _settings: &ViewSettings) {}

    /// Change l'échelle (écran × zoom).
    fn set_scale(&mut self, _scale: f32) {}

    /// Prépare le dessin (mise en forme du texte visible).
    fn prepare(&mut self, text: &mut TextSystem, cx: &PanelContext<'_>);

    /// Dessine la vue dans `layer`.
    fn draw<'a>(&'a self, layer: &mut Layer<'a>, cx: &PanelContext<'_>);

    /// Fait défiler de (`dx`, `dy`) pixels physiques, animé si `smooth`.
    fn scroll_by(&mut self, _dx: f32, _dy: f32, _smooth: bool, _now: Instant) {}

    /// Avance les animations ; renvoie vrai s'il faut encore redessiner.
    fn tick(&mut self, _now: Instant) -> bool {
        false
    }

    /// Forme du pointeur au point (`x`, `y`).
    fn pointer_at(&self, _x: f32, _y: f32) -> Pointer {
        Pointer::Default
    }

    /// Pour retrouver le type concret de la vue.
    fn as_any(&self) -> &dyn Any;

    /// Pour retrouver le type concret de la vue, modifiable.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl PanelView for EditorView {
    fn set_bounds(&mut self, bounds: Rect) {
        EditorView::set_bounds(self, bounds);
    }

    fn bounds(&self) -> Rect {
        EditorView::bounds(self)
    }

    fn set_settings(&mut self, settings: &ViewSettings) {
        EditorView::set_settings(self, settings.clone());
    }

    fn set_scale(&mut self, scale: f32) {
        EditorView::set_scale(self, scale);
    }

    fn prepare(&mut self, text: &mut TextSystem, cx: &PanelContext<'_>) {
        if let Some(document) = panel_document(cx) {
            EditorView::prepare(self, text, document);
        }
    }

    fn draw<'a>(&'a self, layer: &mut Layer<'a>, cx: &PanelContext<'_>) {
        match panel_document(cx) {
            Some(document) => EditorView::draw(
                self,
                layer,
                cx.theme,
                document,
                cx.focused && cx.cursors_visible,
            ),
            None => layer
                .quads
                .push(Quad::new(EditorView::bounds(self), cx.theme.background)),
        }
    }

    fn scroll_by(&mut self, dx: f32, dy: f32, smooth: bool, now: Instant) {
        EditorView::scroll_by(self, dx, dy, smooth, now);
    }

    fn tick(&mut self, now: Instant) -> bool {
        EditorView::tick(self, now)
    }

    fn pointer_at(&self, x: f32, y: f32) -> Pointer {
        EditorView::pointer_at(self, x, y)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Document affiché par le panneau du contexte.
fn panel_document<'a>(cx: &PanelContext<'a>) -> Option<&'a cupride_core::Document> {
    let id = cx.editor.panel_document(cx.panel)?;
    cx.editor.document(id)
}

/// Fabrique la vue d'un panneau, à partir de son identifiant, des réglages et de l'échelle.
pub type ViewFactory = Box<dyn Fn(PanelId, &ViewSettings, f32) -> Box<dyn PanelView>>;

/// Registre des types de panneaux : chaque type est associé à la fabrique de sa vue.
pub struct ViewRegistry {
    factories: BTreeMap<String, ViewFactory>,
}

impl fmt::Debug for ViewRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewRegistry")
            .field("kinds", &self.factories.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Default for ViewRegistry {
    /// Registre des types intégrés : l'éditeur de texte.
    fn default() -> Self {
        let mut registry = Self::empty();
        registry.register(
            PanelKind::EDITOR,
            Box::new(|panel, settings, scale| {
                Box::new(EditorView::for_panel(panel, settings.clone(), scale))
            }),
        );
        registry
    }
}

impl ViewRegistry {
    /// Registre sans aucun type.
    pub fn empty() -> Self {
        Self {
            factories: BTreeMap::new(),
        }
    }

    /// Associe le type `kind` à la fabrique `factory`, à la place de l'ancienne s'il y en avait
    /// une.
    pub fn register(&mut self, kind: impl Into<String>, factory: ViewFactory) {
        self.factories.insert(kind.into(), factory);
    }

    /// Types connus.
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.factories.keys().map(String::as_str)
    }

    /// Le type `kind` est-il connu ?
    pub fn contains(&self, kind: &str) -> bool {
        self.factories.contains_key(kind)
    }

    /// Vue d'un panneau de type `kind`, ou `None` pour un type inconnu.
    pub fn create(
        &self,
        kind: &PanelKind,
        panel: PanelId,
        settings: &ViewSettings,
        scale: f32,
    ) -> Option<Box<dyn PanelView>> {
        let factory = self.factories.get(kind.as_str())?;
        Some(factory(panel, settings, scale))
    }
}

/// Onglet d'une zone, placé dans sa barre.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    /// Panneau de l'onglet.
    pub panel: PanelId,
    /// Place de l'onglet (il peut dépasser de la barre : il est alors découpé).
    pub bounds: Rect,
    /// Onglet d'aperçu.
    pub preview: bool,
}

/// Zone d'onglets, placée dans la fenêtre.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    /// Onglets de la zone.
    pub panels: Vec<PanelId>,
    /// Onglet affiché.
    pub active: Option<PanelId>,
    /// Place de la zone.
    pub bounds: Rect,
    /// Barre des onglets, en haut de la zone.
    pub tab_bar: Rect,
    /// Onglets, de gauche à droite.
    pub tabs: Vec<Tab>,
    /// Place du contenu du panneau affiché.
    pub content: Rect,
}

/// Titre de l'onglet du panneau `panel` : nom du document (● s'il est modifié), ou nom du type
/// de panneau.
pub fn panel_title(editor: &Editor, panel: PanelId) -> String {
    if let Some(document) = editor
        .panel_document(panel)
        .and_then(|id| editor.document(id))
    {
        let modified = if document.is_modified() { " ●" } else { "" };
        return format!("{}{modified}", document.display_name());
    }
    match editor.workspace().panel(panel).map(|p| p.kind().as_str()) {
        Some(PanelKind::EXPLORER) => "Explorateur".to_owned(),
        Some(kind) => kind.to_owned(),
        None => String::new(),
    }
}

/// Vues de tous les panneaux, et disposition des zones dans la fenêtre.
pub struct PanelViews {
    registry: ViewRegistry,
    views: HashMap<PanelId, Box<dyn PanelView>>,
    /// Document affiché par chaque vue à sa création : un panneau qui change de document (onglet
    /// d'aperçu) reçoit une vue neuve.
    documents: HashMap<PanelId, Option<DocumentId>>,
    /// Titres des onglets.
    labels: HashMap<PanelId, Label>,
    settings: ViewSettings,
    scale: f32,
    area: Rect,
    zones: Vec<Zone>,
    dividers: Vec<Divider>,
}

impl fmt::Debug for PanelViews {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PanelViews")
            .field("registry", &self.registry)
            .field("views", &self.views.len())
            .field("zones", &self.zones)
            .finish_non_exhaustive()
    }
}

/// Rectangle de l'interface à partir d'un rectangle du layout.
pub(crate) fn rect(bounds: Bounds) -> Rect {
    Rect::new(bounds.x, bounds.y, bounds.width, bounds.height)
}

/// Rectangle du layout à partir d'un rectangle de l'interface.
pub(crate) fn bounds(rect: Rect) -> Bounds {
    Bounds::new(rect.x, rect.y, rect.width, rect.height)
}

impl PanelViews {
    /// Vues fabriquées par `registry`, avec ces réglages et cette échelle.
    pub fn new(registry: ViewRegistry, settings: ViewSettings, scale: f32) -> Self {
        Self {
            registry,
            views: HashMap::new(),
            documents: HashMap::new(),
            labels: HashMap::new(),
            settings,
            scale,
            area: Rect::default(),
            zones: Vec::new(),
            dividers: Vec::new(),
        }
    }

    /// Registre des types de panneaux.
    pub fn registry_mut(&mut self) -> &mut ViewRegistry {
        &mut self.registry
    }

    /// Applique de nouveaux réglages à toutes les vues.
    pub fn set_settings(&mut self, settings: ViewSettings) {
        if settings != self.settings {
            for view in self.views.values_mut() {
                view.set_settings(&settings);
            }
            self.settings = settings;
        }
    }

    /// Change l'échelle de toutes les vues.
    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON && scale > 0.0 {
            for view in self.views.values_mut() {
                view.set_scale(scale);
            }
            self.scale = scale;
        }
    }

    /// Police et dimensions de l'interface.
    pub fn style(&self) -> Style {
        self.settings.style(self.scale)
    }

    /// Hauteur de la barre des onglets.
    pub fn tab_bar_height(style: &Style) -> f32 {
        style.line_height + 2.0 * style.px(5.0)
    }

    /// Épaisseur des séparations entre les zones.
    pub fn gap(&self) -> f32 {
        self.style().px(1.0).max(1.0)
    }

    /// Place de l'ensemble des zones, à la dernière disposition.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Crée les vues des nouveaux panneaux, oublie celles des panneaux fermés, met en forme les
    /// titres des onglets, et place chaque zone dans `area`.
    pub fn layout(&mut self, text: &mut TextSystem, editor: &Editor, area: Rect) {
        let workspace = editor.workspace();
        let exists = |panel: &PanelId| workspace.panel(*panel).is_some();
        self.views.retain(|panel, _| exists(panel));
        self.labels.retain(|panel, _| exists(panel));
        self.documents.retain(|panel, _| exists(panel));
        for panel in workspace.panels() {
            let id = panel.id();
            let document = editor.panel_document(id);
            if self.views.contains_key(&id) && self.documents.get(&id) == Some(&document) {
                continue;
            }
            self.documents.insert(id, document);
            match self
                .registry
                .create(panel.kind(), id, &self.settings, self.scale)
            {
                Some(view) => {
                    self.views.insert(id, view);
                }
                None => {
                    self.views.remove(&id);
                    tracing::warn!(panneau = %id, type_ = %panel.kind(), "type de panneau sans vue");
                }
            }
        }
        self.area = area;
        let style = self.style();
        let gap = self.gap();
        let bar_height = Self::tab_bar_height(&style);
        let padding = style.px(12.0);
        let tree = workspace.tree();
        let mut zones = Vec::new();
        for group_area in tree.areas(bounds(area), gap) {
            let group = group_area.group;
            let zone = rect(group_area.bounds);
            let bar_height = bar_height.min(zone.height);
            let tab_bar = Rect::new(zone.x, zone.y, zone.width, bar_height);
            let content = Rect::new(
                zone.x,
                zone.y + bar_height,
                zone.width,
                zone.height - bar_height,
            );
            let mut tabs = Vec::with_capacity(group.len());
            let mut x = zone.x;
            for &panel in group.panels() {
                let preview = group.preview() == Some(panel);
                let label = self.labels.entry(panel).or_default();
                label.set_italic(
                    text,
                    &panel_title(editor, panel),
                    preview,
                    &style.family,
                    style.metrics(),
                );
                let width = (label.width() + 2.0 * padding).round();
                tabs.push(Tab {
                    panel,
                    bounds: Rect::new(x, tab_bar.y, width, bar_height),
                    preview,
                });
                x += width;
            }
            // Trop d'onglets : la barre défile pour montrer l'onglet affiché.
            if let Some(active) = tabs.iter().find(|t| Some(t.panel) == group.active()) {
                let overflow = active.bounds.right() - tab_bar.right();
                if overflow > 0.0 {
                    for tab in &mut tabs {
                        tab.bounds.x -= overflow;
                    }
                }
            }
            zones.push(Zone {
                panels: group.panels().to_vec(),
                active: group.active(),
                bounds: zone,
                tab_bar,
                tabs,
                content,
            });
        }
        self.zones = zones;
        self.dividers = tree.dividers(bounds(area), gap);
        for zone in &self.zones {
            if let Some(view) = zone.active.and_then(|p| self.views.get_mut(&p)) {
                view.set_bounds(zone.content);
            }
        }
    }

    /// Onglet sous le point (`x`, `y`).
    pub fn tab_at(&self, x: f32, y: f32) -> Option<PanelId> {
        let zone = self.zones.iter().find(|z| z.tab_bar.contains(x, y))?;
        zone.tabs
            .iter()
            .find(|t| t.bounds.contains(x, y))
            .map(|t| t.panel)
    }

    /// Zone dont la barre d'onglets est sous le point (`x`, `y`).
    pub fn tab_bar_at(&self, x: f32, y: f32) -> Option<&Zone> {
        self.zones.iter().find(|z| z.tab_bar.contains(x, y))
    }

    /// Zones, à la dernière disposition.
    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    /// Séparations entre les zones, à la dernière disposition.
    pub fn dividers(&self) -> &[Divider] {
        &self.dividers
    }

    /// Panneau affiché sous le point (`x`, `y`).
    pub fn panel_at(&self, x: f32, y: f32) -> Option<PanelId> {
        self.zones
            .iter()
            .find(|z| z.bounds.contains(x, y))
            .and_then(|z| z.active)
    }

    /// Vue du panneau `panel`.
    pub fn view(&self, panel: PanelId) -> Option<&dyn PanelView> {
        self.views.get(&panel).map(|v| &**v)
    }

    /// Vue du panneau `panel`, modifiable.
    pub fn view_mut(&mut self, panel: PanelId) -> Option<&mut dyn PanelView> {
        match self.views.get_mut(&panel) {
            Some(view) => Some(&mut **view),
            None => None,
        }
    }

    /// Vue d'éditeur du panneau `panel`.
    pub fn editor_view(&self, panel: PanelId) -> Option<&EditorView> {
        self.view(panel)?.as_any().downcast_ref()
    }

    /// Vue d'éditeur du panneau `panel`, modifiable.
    pub fn editor_view_mut(&mut self, panel: PanelId) -> Option<&mut EditorView> {
        self.view_mut(panel)?.as_any_mut().downcast_mut()
    }

    /// Avance les animations des vues affichées ; renvoie vrai s'il faut encore redessiner.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut animating = false;
        for zone in &self.zones {
            if let Some(view) = zone.active.and_then(|p| self.views.get_mut(&p)) {
                animating |= view.tick(now);
            }
        }
        animating
    }

    /// Contexte du panneau `panel`.
    fn context<'a>(
        editor: &'a Editor,
        panel: PanelId,
        theme: &'a Theme,
        focused: bool,
        cursors_visible: bool,
    ) -> PanelContext<'a> {
        PanelContext {
            editor,
            panel,
            theme,
            focused: focused && editor.active_panel() == Some(panel),
            cursors_visible,
        }
    }

    /// Prépare les vues affichées. `focused` : la zone des panneaux a le focus clavier (aucune
    /// couche ouverte par-dessus, fenêtre active).
    pub fn prepare(
        &mut self,
        text: &mut TextSystem,
        editor: &Editor,
        theme: &Theme,
        focused: bool,
        cursors_visible: bool,
    ) {
        for zone in &self.zones {
            let Some(panel) = zone.active else {
                continue;
            };
            if let Some(view) = self.views.get_mut(&panel) {
                let cx = Self::context(editor, panel, theme, focused, cursors_visible);
                view.prepare(text, &cx);
            }
        }
    }

    /// Dessine les zones et leurs séparations.
    pub fn draw<'a>(
        &'a self,
        layer: &mut Layer<'a>,
        editor: &Editor,
        theme: &Theme,
        focused: bool,
        cursors_visible: bool,
    ) {
        layer.quads.push(Quad::new(self.area, theme.background));
        for zone in &self.zones {
            let Some(panel) = zone.active else {
                continue;
            };
            match self.views.get(&panel) {
                Some(view) => {
                    let cx = Self::context(editor, panel, theme, focused, cursors_visible);
                    view.draw(layer, &cx);
                }
                None => layer.quads.push(Quad::new(zone.content, theme.panel)),
            }
        }
        let active = editor.active_panel();
        for zone in &self.zones {
            let has_active = active.is_some() && zone.active == active;
            self.draw_tabs(layer, zone, theme, has_active, has_active && focused);
        }
        for divider in &self.dividers {
            layer
                .quads
                .push(Quad::new(rect(divider.bounds), theme.border));
        }
    }

    /// Barre d'onglets d'une zone : onglet affiché sur le fond du contenu, les autres sur celui
    /// des panneaux, séparés par un trait ; aperçu en gris et en italique. La zone du panneau
    /// actif se reconnaît à son onglet affiché, souligné en haut de la couleur d'accent (en
    /// gris si le clavier est ailleurs) ; dans les autres zones, les titres sont en gris.
    fn draw_tabs<'a>(
        &'a self,
        layer: &mut Layer<'a>,
        zone: &Zone,
        theme: &Theme,
        active_zone: bool,
        focused: bool,
    ) {
        let style = self.style();
        let line = style.px(1.0).max(1.0);
        let bar = zone.tab_bar;
        if bar.height <= 0.0 {
            return;
        }
        layer.quads.push(Quad::new(bar, theme.panel));
        layer.quads.push(Quad::new(
            Rect::new(bar.x, bar.bottom() - line, bar.width, line),
            theme.border,
        ));
        let padding = style.px(12.0);
        let top = bar.y + ((bar.height - style.line_height) / 2.0).round();
        for tab in &zone.tabs {
            let clip = tab.bounds.intersect(&bar);
            if clip.width <= 0.0 {
                continue;
            }
            let shown = zone.active == Some(tab.panel);
            if shown {
                layer.quads.push(Quad::new(clip, theme.background));
                if active_zone {
                    let marker = Rect::new(clip.x, clip.y, clip.width, style.px(2.0).max(1.0));
                    let color = if focused { theme.accent } else { theme.muted };
                    layer.quads.push(Quad::new(marker, color));
                }
            }
            let separator = Rect::new(tab.bounds.right() - line, bar.y, line, bar.height);
            layer
                .quads
                .push(Quad::new(separator.intersect(&bar), theme.border));
            let color = if tab.preview || !active_zone {
                theme.muted
            } else {
                theme.foreground
            };
            if let Some(label) = self.labels.get(&tab.panel) {
                layer
                    .texts
                    .extend(label.item(tab.bounds.x + padding, top, clip, color));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cupride_core::NewPanel;
    use cupride_core::layout::Direction;
    use cupride_core::text::Selections;

    #[test]
    fn registre_des_types() {
        let registry = ViewRegistry::default();
        assert!(registry.contains("editor"));
        assert_eq!(registry.kinds().collect::<Vec<_>>(), vec!["editor"]);
        let settings = ViewSettings::default();
        let view = registry
            .create(&PanelKind::new("editor"), PanelId::new(1), &settings, 1.0)
            .unwrap();
        assert!(view.as_any().downcast_ref::<EditorView>().is_some());
        assert!(
            registry
                .create(&PanelKind::new("inconnu"), PanelId::new(2), &settings, 1.0)
                .is_none()
        );
    }

    #[test]
    fn une_vue_par_panneau() {
        let mut editor = Editor::new();
        let doc = editor.new_document();
        let left = editor.active_panel().unwrap();
        let right = editor
            .open_editor_panel(doc, NewPanel::Split(Direction::Right))
            .unwrap();
        let mut text = TextSystem::new();
        let mut views = PanelViews::new(ViewRegistry::default(), ViewSettings::default(), 1.0);
        views.layout(&mut text, &editor, Rect::new(0.0, 0.0, 801.0, 600.0));
        assert_eq!(views.zones().len(), 2);
        let bar = PanelViews::tab_bar_height(&views.style());
        let left_view = views.editor_view(left).unwrap().bounds();
        assert_eq!(
            left_view,
            Rect::new(0.0, bar, 400.0, 600.0 - bar),
            "sous les onglets"
        );
        assert_eq!(views.editor_view(right).unwrap().bounds().x, 401.0);
        assert_eq!(views.dividers().len(), 1);
        assert_eq!(views.panel_at(600.0, 100.0), Some(right));
        // Chaque vue affiche les sélections de son panneau.
        editor
            .active_document_mut()
            .unwrap()
            .set_selections(Selections::cursor(0));
        let document = editor.document(doc).unwrap();
        assert_eq!(
            views.editor_view(right).unwrap().selections(document),
            document.selections_in(right)
        );
        // Panneau fermé : sa vue est oubliée.
        editor.close_panel(right, false).unwrap();
        views.layout(&mut text, &editor, Rect::new(0.0, 0.0, 800.0, 600.0));
        assert!(views.view(right).is_none());
        assert_eq!(views.editor_view(left).unwrap().bounds().width, 800.0);
    }

    #[test]
    fn onglets() {
        let mut editor = Editor::new();
        let mut config = editor.config().clone();
        config.tabs.preview = true;
        editor.apply_config(config);
        let a = editor.new_document();
        let pa = editor.active_panel().unwrap();
        editor.execute("edit insert 'x'").unwrap();
        let b = editor.insert_document(cupride_core::Document::from_text(""));
        let pb = editor.active_panel().unwrap();
        editor
            .workspace_mut()
            .tree_mut()
            .set_preview(pb, true)
            .unwrap();
        assert_eq!(panel_title(&editor, pa), "sans titre ●", "modifié");
        let mut text = TextSystem::new();
        let mut views = PanelViews::new(ViewRegistry::default(), ViewSettings::default(), 1.0);
        views.layout(&mut text, &editor, Rect::new(0.0, 0.0, 800.0, 600.0));
        let zone = &views.zones()[0];
        assert_eq!(zone.tabs.len(), 2);
        assert!(zone.tabs[1].preview);
        assert_eq!(zone.tabs[0].bounds.right(), zone.tabs[1].bounds.x);
        let tab = zone.tabs[0].bounds;
        let (x, y) = (tab.x + tab.width / 2.0, tab.y + tab.height / 2.0);
        assert_eq!(views.tab_at(x, y), Some(pa));
        assert_eq!(views.tab_at(x, 300.0), None, "hors de la barre");

        // L'aperçu suivant remplace l'onglet d'aperçu (et sa vue).
        let c = editor.create_document();
        editor.show_document_preview(c);
        assert_eq!(editor.panel_document(pb), Some(c), "aperçu remplacé");
        assert!(editor.document(b).is_none());
        views.layout(&mut text, &editor, Rect::new(0.0, 0.0, 800.0, 600.0));
        assert!(editor.document(a).is_some());

        // Beaucoup d'onglets : l'onglet affiché reste visible.
        for _ in 0..30 {
            editor.new_document();
        }
        views.layout(&mut text, &editor, Rect::new(0.0, 0.0, 800.0, 600.0));
        let zone = &views.zones()[0];
        let shown = zone
            .tabs
            .iter()
            .find(|t| Some(t.panel) == zone.active)
            .unwrap();
        assert!(shown.bounds.right() <= zone.tab_bar.right());
        assert!(shown.bounds.x >= zone.tab_bar.x);
    }
}
