//! Workspace : les fenêtres, leurs panneaux et leurs arbres de layout.
//!
//! Chaque fenêtre a son arbre et son panneau actif ; les identifiants de panneaux sont communs à
//! toutes les fenêtres, et un panneau n'est que dans une seule. Une fenêtre est **active** : celle
//! qui a le focus. [`Workspace::tree`] et [`Workspace::active`] désignent son arbre et son panneau
//! actif.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::panel::{Panel, PanelId, PanelKind};
use crate::tree::{LayoutError, LayoutTree};

/// Identifiant stable d'une fenêtre : son numéro, à partir de 1, jamais réutilisé dans une
/// session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowId(u64);

impl WindowId {
    /// Identifiant de valeur `value`.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Valeur de l'identifiant.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Une fenêtre : son arbre de layout et son panneau actif.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowLayout {
    id: WindowId,
    tree: LayoutTree,
    active: Option<PanelId>,
}

impl WindowLayout {
    fn new(id: WindowId) -> Self {
        Self {
            id,
            tree: LayoutTree::new(),
            active: None,
        }
    }

    /// Identifiant.
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// Arbre de layout.
    pub fn tree(&self) -> &LayoutTree {
        &self.tree
    }

    /// Panneau actif de la fenêtre (celui qui aura le focus quand elle l'aura).
    pub fn active(&self) -> Option<PanelId> {
        self.active
    }
}

/// Fenêtres, panneaux et fenêtre active.
///
/// Chaque panneau d'un arbre a un modèle, et inversement : un workspace lu depuis JSON est réparé
/// pour le garantir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawWorkspace", into = "RawWorkspace")]
pub struct Workspace {
    windows: Vec<WindowLayout>,
    active_window: Option<WindowId>,
    panels: BTreeMap<PanelId, Panel>,
    next_id: u64,
    next_window: u64,
    /// Panneaux du plus anciennement au plus récemment actif : à la fermeture du panneau actif,
    /// le focus revient au précédent de sa fenêtre.
    recent: Vec<PanelId>,
    /// Arbre vide, montré quand il n'y a aucune fenêtre.
    empty: LayoutTree,
}

/// Forme sérialisée d'une fenêtre.
#[derive(Serialize, Deserialize)]
struct RawWindow {
    id: WindowId,
    tree: LayoutTree,
    #[serde(default)]
    active: Option<PanelId>,
}

/// Forme sérialisée du workspace. Les sessions d'avant le multi-fenêtres (`tree` et `active` à
/// la racine) sont lues comme une seule fenêtre.
#[derive(Serialize, Deserialize)]
struct RawWorkspace {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    windows: Vec<RawWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_window: Option<WindowId>,
    #[serde(default, skip_serializing)]
    tree: Option<LayoutTree>,
    panels: Vec<Panel>,
    #[serde(default, skip_serializing)]
    active: Option<PanelId>,
}

impl From<RawWorkspace> for Workspace {
    fn from(raw: RawWorkspace) -> Self {
        let mut raw_windows = raw.windows;
        if raw_windows.is_empty()
            && let Some(tree) = raw.tree
        {
            raw_windows.push(RawWindow {
                id: WindowId::new(1),
                tree,
                active: raw.active,
            });
        }
        let mut panels: BTreeMap<PanelId, Panel> =
            raw.panels.into_iter().map(|p| (p.id(), p)).collect();
        let mut windows: Vec<WindowLayout> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for raw_window in raw_windows {
            if windows.iter().any(|w| w.id == raw_window.id) {
                continue;
            }
            let mut tree = raw_window.tree;
            // Un panneau sans modèle, ou déjà dans une autre fenêtre, est retiré de l'arbre.
            for id in tree.panels() {
                if !panels.contains_key(&id) || !seen.insert(id) {
                    let _ = tree.remove(id);
                }
            }
            let active = raw_window
                .active
                .filter(|id| tree.contains(*id))
                .or_else(|| tree.groups().first().and_then(|g| g.active()));
            windows.push(WindowLayout {
                id: raw_window.id,
                tree,
                active,
            });
        }
        // Un modèle hors des arbres est oublié.
        panels.retain(|id, _| seen.contains(id));
        let next_id = panels.keys().last().map_or(1, |id| id.get() + 1);
        let next_window = windows.iter().map(|w| w.id.get() + 1).max().unwrap_or(1);
        let active_window = raw
            .active_window
            .filter(|id| windows.iter().any(|w| w.id == *id))
            .or_else(|| windows.first().map(|w| w.id));
        let recent = windows.iter().filter_map(|w| w.active).collect();
        Self {
            windows,
            active_window,
            panels,
            next_id,
            next_window,
            recent,
            empty: LayoutTree::new(),
        }
    }
}

impl From<Workspace> for RawWorkspace {
    fn from(workspace: Workspace) -> Self {
        Self {
            windows: workspace
                .windows
                .into_iter()
                .map(|w| RawWindow {
                    id: w.id,
                    tree: w.tree,
                    active: w.active,
                })
                .collect(),
            active_window: workspace.active_window,
            tree: None,
            panels: workspace.panels.into_values().collect(),
            active: None,
        }
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    /// Workspace d'une fenêtre, sans panneau.
    pub fn new() -> Self {
        Self {
            windows: vec![WindowLayout::new(WindowId::new(1))],
            active_window: Some(WindowId::new(1)),
            panels: BTreeMap::new(),
            next_id: 1,
            next_window: 2,
            recent: Vec::new(),
            empty: LayoutTree::new(),
        }
    }

    /// Fenêtres, dans l'ordre de leur ouverture.
    pub fn windows(&self) -> &[WindowLayout] {
        &self.windows
    }

    /// Fenêtre `id`.
    pub fn window(&self, id: WindowId) -> Option<&WindowLayout> {
        self.windows.iter().find(|w| w.id == id)
    }

    fn window_mut(&mut self, id: WindowId) -> Option<&mut WindowLayout> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// Fenêtre active (celle qui a le focus).
    pub fn active_window(&self) -> Option<WindowId> {
        self.active_window
    }

    /// Fenêtre qui contient le panneau `panel`.
    pub fn window_of(&self, panel: PanelId) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|w| w.tree.contains(panel))
            .map(|w| w.id)
    }

    /// Ouvre une fenêtre vide, sans lui donner le focus.
    pub fn create_window(&mut self) -> WindowId {
        let id = WindowId::new(self.next_window);
        self.next_window += 1;
        self.windows.push(WindowLayout::new(id));
        id
    }

    /// Donne le focus à la fenêtre `id`. Renvoie faux si elle n'existe pas.
    pub fn focus_window(&mut self, id: WindowId) -> bool {
        if self.window(id).is_none() {
            return false;
        }
        self.active_window = Some(id);
        if let Some(panel) = self.window(id).and_then(|w| w.active) {
            self.recent.retain(|p| *p != panel);
            self.recent.push(panel);
        }
        true
    }

    /// Ferme la fenêtre `id` : elle et ses panneaux sont retirés, et leurs modèles renvoyés. Le
    /// focus passe à la fenêtre la plus récemment active.
    pub fn remove_window(&mut self, id: WindowId) -> Option<Vec<Panel>> {
        let index = self.windows.iter().position(|w| w.id == id)?;
        let window = self.windows.remove(index);
        let panels: Vec<Panel> = window
            .tree
            .panels()
            .into_iter()
            .filter_map(|p| self.panels.remove(&p))
            .collect();
        self.recent.retain(|p| self.panels.contains_key(p));
        if self.active_window == Some(id) {
            let recent = self.recent.iter().rev().find_map(|p| self.window_of(*p));
            self.active_window = recent.or_else(|| self.windows.last().map(|w| w.id));
        }
        Some(panels)
    }

    /// Arbre de la fenêtre active (un arbre vide s'il n'y a aucune fenêtre).
    pub fn tree(&self) -> &LayoutTree {
        self.active_window
            .and_then(|id| self.window(id))
            .map_or(&self.empty, |w| &w.tree)
    }

    /// Arbre de la fenêtre active, modifiable : diviser, déplacer, redimensionner. S'il n'y a
    /// aucune fenêtre, une fenêtre est ouverte. Pour ajouter ou retirer un panneau, passer par
    /// [`Workspace::create`] et [`Workspace::remove`], qui tiennent les modèles à jour.
    pub fn tree_mut(&mut self) -> &mut LayoutTree {
        let id = match self.active_window.filter(|id| self.window(*id).is_some()) {
            Some(id) => id,
            None => {
                let id = self.create_window();
                self.active_window = Some(id);
                id
            }
        };
        &mut self.window_mut(id).expect("la fenêtre active existe").tree
    }

    /// Modèle du panneau `id`.
    pub fn panel(&self, id: PanelId) -> Option<&Panel> {
        self.panels.get(&id)
    }

    /// Modèle du panneau `id`, modifiable.
    pub fn panel_mut(&mut self, id: PanelId) -> Option<&mut Panel> {
        self.panels.get_mut(&id)
    }

    /// Tous les panneaux, de toutes les fenêtres.
    pub fn panels(&self) -> impl Iterator<Item = &Panel> {
        self.panels.values()
    }

    /// Nombre de panneaux.
    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    /// Panneau qui a le focus : le panneau actif de la fenêtre active.
    pub fn active(&self) -> Option<PanelId> {
        self.active_window
            .and_then(|id| self.window(id))
            .and_then(|w| w.active)
    }

    /// Crée un panneau de type `kind`, sans le placer dans un arbre : le placer ensuite avec
    /// [`LayoutTree::add`] ou [`LayoutTree::split`]. L'identifiant n'est jamais réutilisé.
    pub fn create(&mut self, kind: PanelKind) -> PanelId {
        let id = PanelId::new(self.next_id);
        self.next_id += 1;
        self.panels.insert(id, Panel::new(id, kind));
        id
    }

    /// Donne le focus au panneau `id` : il est affiché dans sa zone, et sa fenêtre devient la
    /// fenêtre active.
    pub fn focus(&mut self, id: PanelId) -> Result<(), LayoutError> {
        let window = self.window_of(id).ok_or(LayoutError::UnknownPanel(id))?;
        let layout = self.window_mut(window).expect("fenêtre trouvée");
        layout.tree.activate(id)?;
        layout.active = Some(id);
        self.active_window = Some(window);
        self.recent.retain(|p| *p != id);
        self.recent.push(id);
        Ok(())
    }

    /// Retire le panneau `id` de son arbre et renvoie son modèle. S'il était le panneau actif de
    /// sa fenêtre, l'onglet qui le remplace dans sa zone le devient, ou, si la zone disparaît, le
    /// dernier panneau actif de la fenêtre.
    pub fn remove(&mut self, id: PanelId) -> Option<Panel> {
        let panel = self.panels.remove(&id)?;
        self.recent.retain(|p| *p != id);
        let Some(window) = self.window_of(id) else {
            return Some(panel);
        };
        let recent = self.recent.clone();
        let layout = self.window_mut(window).expect("fenêtre trouvée");
        let sibling = layout
            .tree
            .group_of(id)
            .and_then(|g| g.panels().iter().copied().find(|p| *p != id));
        let _ = layout.tree.remove(id);
        if layout.active == Some(id) {
            let tree = &layout.tree;
            let next = sibling
                .and_then(|s| tree.group_of(s))
                .and_then(|g| g.active())
                .or_else(|| {
                    let last = recent.iter().rev().find(|p| tree.contains(**p))?;
                    tree.group_of(*last)?.active()
                })
                .or_else(|| tree.panels().first().copied());
            layout.active = None;
            if let Some(next) = next {
                let _ = layout.tree.activate(next);
                layout.active = Some(next);
                if self.active_window == Some(window) {
                    self.recent.retain(|p| *p != next);
                    self.recent.push(next);
                }
            }
        }
        Some(panel)
    }

    /// Remplace les fenêtres et les panneaux par ceux de `other` (layout chargé). Ses panneaux
    /// et ses fenêtres reçoivent des identifiants neufs, à la suite de ceux déjà attribués : un
    /// identifiant ne sert jamais deux fois. Un workspace qui n'a encore aucun panneau (au
    /// lancement) repart de 1. Renvoie la correspondance entre anciens et nouveaux identifiants
    /// des panneaux, puis des fenêtres.
    pub fn replace(
        &mut self,
        other: Workspace,
    ) -> (BTreeMap<PanelId, PanelId>, BTreeMap<WindowId, WindowId>) {
        let fresh = self.panels.is_empty();
        let mut renamed = BTreeMap::new();
        let mut next_id = if fresh {
            1
        } else {
            self.next_id.max(other.next_id)
        };
        for id in other.panels.keys() {
            renamed.insert(*id, PanelId::new(next_id));
            next_id += 1;
        }
        let mut windows = BTreeMap::new();
        let mut next_window = if fresh {
            1
        } else {
            self.next_window.max(other.next_window)
        };
        self.windows = other
            .windows
            .into_iter()
            .map(|mut window| {
                let id = WindowId::new(next_window);
                next_window += 1;
                windows.insert(window.id, id);
                window.id = id;
                window
                    .tree
                    .rename(&mut |p| renamed.get(&p).copied().unwrap_or(p));
                window.active = window.active.and_then(|p| renamed.get(&p).copied());
                window
            })
            .collect();
        self.panels = other
            .panels
            .into_values()
            .filter_map(|mut panel| {
                let id = *renamed.get(&panel.id())?;
                panel.set_id(id);
                Some((id, panel))
            })
            .collect();
        self.active_window = other
            .active_window
            .and_then(|id| windows.get(&id).copied())
            .or_else(|| self.windows.first().map(|w| w.id));
        self.recent = self.active().into_iter().collect();
        self.next_id = next_id;
        self.next_window = next_window;
        (renamed, windows)
    }

    /// Prochain identifiant de panneau attribué.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Les invariants sont-ils respectés : arbres valides, chaque panneau dans un seul arbre,
    /// un modèle par panneau des arbres et inversement, panneaux actifs dans leur arbre ?
    pub fn is_valid(&self) -> bool {
        let mut shown = Vec::new();
        for window in &self.windows {
            if !window.tree.is_valid() || window.active.is_some_and(|p| !window.tree.contains(p)) {
                return false;
            }
            shown.extend(window.tree.panels());
        }
        let count = shown.len();
        shown.sort();
        shown.dedup();
        count == shown.len()
            && shown.len() == self.panels.len()
            && shown.iter().all(|id| self.panels.contains_key(id))
            && self
                .active_window
                .is_none_or(|id| self.window(id).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Direction;

    fn editor() -> PanelKind {
        PanelKind::new(PanelKind::EDITOR)
    }

    #[test]
    fn creer_placer_retirer() {
        let mut ws = Workspace::new();
        let a = ws.create(editor());
        ws.tree_mut().add(a, None).unwrap();
        ws.focus(a).unwrap();
        let b = ws.create(editor());
        ws.tree_mut().split(a, Direction::Right, b).unwrap();
        ws.focus(b).unwrap();
        let c = ws.create(PanelKind::new(PanelKind::EXPLORER));
        ws.tree_mut().add(c, Some(a)).unwrap();
        assert!(ws.is_valid());
        assert_eq!(ws.active(), Some(b));
        assert_eq!(ws.panel(c).unwrap().kind().as_str(), "explorer");

        // b ferme sa zone : le focus revient à la zone du dernier panneau actif, a, qui
        // affiche c.
        assert_eq!(ws.remove(b).unwrap().id(), b);
        assert_eq!(ws.active(), Some(c));
        // c ferme : a le remplace dans sa zone.
        ws.focus(a).unwrap();
        ws.focus(c).unwrap();
        ws.remove(c);
        assert_eq!(ws.active(), Some(a));
        ws.remove(a);
        assert_eq!(ws.active(), None);
        assert!(ws.tree().is_empty());
        assert!(ws.remove(c).is_none());
        // Les identifiants ne sont jamais réutilisés.
        assert_eq!(ws.create(editor()).get(), 4);
    }

    #[test]
    fn etat_des_panneaux() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Scroll {
            line: usize,
        }
        let mut ws = Workspace::new();
        let a = ws.create(editor());
        assert_eq!(ws.panel(a).unwrap().state_as::<Scroll>(), None);
        ws.panel_mut(a).unwrap().store_state(&Scroll { line: 42 });
        assert_eq!(
            ws.panel(a).unwrap().state_as::<Scroll>(),
            Some(Scroll { line: 42 })
        );
    }

    #[test]
    fn serialisation_et_reparation() {
        let mut ws = Workspace::new();
        let a = ws.create(editor());
        ws.tree_mut().add(a, None).unwrap();
        let b = ws.create(editor());
        ws.tree_mut().split(a, Direction::Down, b).unwrap();
        ws.focus(b).unwrap();
        ws.panel_mut(b)
            .unwrap()
            .set_state(serde_json::json!({ "path": "src/main.rs" }));
        let json = serde_json::to_string(&ws).unwrap();
        let back: Workspace = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tree(), ws.tree());
        assert_eq!(back.active(), Some(b));
        assert_eq!(back.panel(b), ws.panel(b));
        assert_eq!(back.next_id(), 3);

        // Panneau de l'arbre sans modèle, modèle hors de l'arbre, actif inconnu.
        let json = r#"{
            "tree": {"split": {"axis": "horizontal", "children": [
                {"weight": 1, "node": {"tabs": {"panels": [1]}}},
                {"weight": 1, "node": {"tabs": {"panels": [2]}}}
            ]}},
            "panels": [{"id": 1, "kind": "editor"}, {"id": 7, "kind": "editor"}],
            "active": 2
        }"#;
        let ws: Workspace = serde_json::from_str(json).unwrap();
        assert!(ws.is_valid());
        assert_eq!(ws.tree().panels(), vec![PanelId::new(1)]);
        assert_eq!(ws.active(), Some(PanelId::new(1)));
        assert_eq!(ws.next_id(), 2);
    }

    #[test]
    fn remplacer_renumerote_les_panneaux() {
        let mut ws = Workspace::new();
        for _ in 0..5 {
            ws.create(editor());
        }
        let mut loaded = Workspace::new();
        let a = loaded.create(editor());
        loaded.tree_mut().add(a, None).unwrap();
        let b = loaded.create(editor());
        loaded.tree_mut().split(a, Direction::Right, b).unwrap();
        loaded.tree_mut().set_preview(b, true).unwrap();
        loaded.focus(b).unwrap();
        let (renamed, _) = ws.replace(loaded);
        assert_eq!(renamed[&a], PanelId::new(6));
        assert_eq!(renamed[&b], PanelId::new(7));
        assert_eq!(ws.tree().panels(), vec![PanelId::new(6), PanelId::new(7)]);
        assert_eq!(ws.active(), Some(PanelId::new(7)));
        assert_eq!(ws.panel(PanelId::new(7)).unwrap().id(), PanelId::new(7));
        assert_eq!(
            ws.tree().group_of(PanelId::new(7)).unwrap().preview(),
            Some(PanelId::new(7))
        );
        assert!(ws.is_valid());
        assert_eq!(ws.create(editor()).get(), 8);
    }

    #[test]
    fn plusieurs_fenetres() {
        let mut ws = Workspace::new();
        let first = ws.active_window().unwrap();
        let a = ws.create(editor());
        ws.tree_mut().add(a, None).unwrap();
        ws.focus(a).unwrap();
        let second = ws.create_window();
        assert_eq!(
            ws.active_window(),
            Some(first),
            "pas de focus à l'ouverture"
        );
        assert!(ws.focus_window(second));
        assert!(ws.tree().is_empty(), "chaque fenêtre a son arbre");
        assert_eq!(ws.active(), None);
        let b = ws.create(editor());
        ws.tree_mut().add(b, None).unwrap();
        ws.focus(b).unwrap();
        assert_eq!(ws.window_of(b), Some(second));
        // Donner le focus à un panneau d'une autre fenêtre change de fenêtre.
        ws.focus(a).unwrap();
        assert_eq!(ws.active_window(), Some(first));
        assert_eq!(
            ws.window(second).unwrap().active(),
            Some(b),
            "chacune garde le sien"
        );
        assert!(ws.is_valid());

        // Sérialisation de plusieurs fenêtres.
        let json = serde_json::to_string(&ws).unwrap();
        let back: Workspace = serde_json::from_str(&json).unwrap();
        assert_eq!(back.windows().len(), 2);
        assert_eq!(back.active_window(), Some(first));
        assert_eq!(back.window(second).unwrap().tree().panels(), vec![b]);

        // Fermer une fenêtre retire ses panneaux ; le focus va à la plus récente.
        ws.focus(b).unwrap();
        let removed = ws.remove_window(second).unwrap();
        assert_eq!(removed.iter().map(Panel::id).collect::<Vec<_>>(), vec![b]);
        assert_eq!(ws.active_window(), Some(first));
        assert_eq!(ws.active(), Some(a));
        assert!(ws.is_valid());
        ws.remove_window(first).unwrap();
        assert_eq!(ws.active_window(), None);
        assert!(ws.tree().is_empty());
        // Sans fenêtre, placer un panneau en ouvre une ; les numéros ne reviennent jamais.
        let c = ws.create(editor());
        ws.tree_mut().add(c, None).unwrap();
        assert_eq!(ws.active_window(), Some(WindowId::new(3)));
        assert!(ws.is_valid());
    }

    #[test]
    fn panneau_dans_deux_fenetres_repare() {
        let json = r#"{
            "windows": [
                {"id": 1, "tree": {"tabs": {"panels": [1, 2]}}, "active": 2},
                {"id": 4, "tree": {"tabs": {"panels": [2, 3]}}}
            ],
            "active_window": 9,
            "panels": [{"id": 1, "kind": "editor"}, {"id": 2, "kind": "editor"},
                       {"id": 3, "kind": "editor"}]
        }"#;
        let ws: Workspace = serde_json::from_str(json).unwrap();
        assert!(ws.is_valid());
        assert_eq!(
            ws.window(WindowId::new(4)).unwrap().tree().panels(),
            vec![PanelId::new(3)]
        );
        assert_eq!(
            ws.active_window(),
            Some(WindowId::new(1)),
            "fenêtre inconnue"
        );
        let mut ws = ws;
        assert_eq!(ws.create_window(), WindowId::new(5));
    }

    #[test]
    fn restaurer_au_lancement_numerote_depuis_un() {
        let mut saved = Workspace::new();
        saved.create_window();
        let a = saved.create(editor());
        saved.tree_mut().add(a, None).unwrap();
        let mut fresh = Workspace::new();
        let (panels, windows) = fresh.replace(saved);
        assert_eq!(panels[&a], PanelId::new(1));
        assert_eq!(
            fresh
                .windows()
                .iter()
                .map(|w| w.id().get())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(windows.len(), 2);
        assert_eq!(fresh.create_window(), WindowId::new(3));
    }
}
