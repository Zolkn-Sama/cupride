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

use cupride_core::Editor;
use cupride_core::layout::{Bounds, Divider, PanelId, PanelKind};
use cupride_render::{Layer, Quad, Rect, TextSystem};

use crate::editor_view::{EditorView, Pointer, ViewSettings};
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

/// Zone d'onglets, placée dans la fenêtre.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    /// Onglets de la zone.
    pub panels: Vec<PanelId>,
    /// Onglet affiché.
    pub active: Option<PanelId>,
    /// Place de la zone.
    pub bounds: Rect,
    /// Place du contenu du panneau affiché.
    pub content: Rect,
}

/// Vues de tous les panneaux, et disposition des zones dans la fenêtre.
pub struct PanelViews {
    registry: ViewRegistry,
    views: HashMap<PanelId, Box<dyn PanelView>>,
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

    /// Épaisseur des séparations entre les zones.
    pub fn gap(&self) -> f32 {
        self.style().px(1.0).max(1.0)
    }

    /// Place de l'ensemble des zones, à la dernière disposition.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Crée les vues des nouveaux panneaux, oublie celles des panneaux fermés, et place chaque
    /// zone dans `area`.
    pub fn layout(&mut self, editor: &Editor, area: Rect) {
        let workspace = editor.workspace();
        self.views
            .retain(|panel, _| workspace.panel(*panel).is_some());
        for panel in workspace.panels() {
            if self.views.contains_key(&panel.id()) {
                continue;
            }
            match self
                .registry
                .create(panel.kind(), panel.id(), &self.settings, self.scale)
            {
                Some(view) => {
                    self.views.insert(panel.id(), view);
                }
                None => tracing::warn!(
                    panneau = %panel.id(),
                    type_ = %panel.kind(),
                    "type de panneau sans vue"
                ),
            }
        }
        self.area = area;
        let gap = self.gap();
        let tree = workspace.tree();
        self.zones = tree
            .areas(bounds(area), gap)
            .into_iter()
            .map(|a| Zone {
                panels: a.group.panels().to_vec(),
                active: a.group.active(),
                bounds: rect(a.bounds),
                content: rect(a.bounds),
            })
            .collect();
        self.dividers = tree.dividers(bounds(area), gap);
        for zone in &self.zones {
            if let Some(view) = zone.active.and_then(|p| self.views.get_mut(&p)) {
                view.set_bounds(zone.content);
            }
        }
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
        for divider in &self.dividers {
            layer
                .quads
                .push(Quad::new(rect(divider.bounds), theme.border));
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
        let mut views = PanelViews::new(ViewRegistry::default(), ViewSettings::default(), 1.0);
        views.layout(&editor, Rect::new(0.0, 0.0, 801.0, 600.0));
        assert_eq!(views.zones().len(), 2);
        assert_eq!(views.editor_view(left).unwrap().bounds().width, 400.0);
        assert_eq!(views.editor_view(right).unwrap().bounds().x, 401.0);
        assert_eq!(views.dividers().len(), 1);
        assert_eq!(views.panel_at(600.0, 10.0), Some(right));
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
        views.layout(&editor, Rect::new(0.0, 0.0, 800.0, 600.0));
        assert!(views.view(right).is_none());
        assert_eq!(views.editor_view(left).unwrap().bounds().width, 800.0);
    }
}
