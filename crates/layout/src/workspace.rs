//! Workspace : les panneaux, leur arbre de layout et le panneau actif.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::panel::{Panel, PanelId, PanelKind};
use crate::tree::{LayoutError, LayoutTree};

/// Panneaux de la fenêtre, leur disposition et le panneau qui a le focus.
///
/// Chaque panneau de l'arbre a un modèle, et inversement : un workspace lu depuis JSON est réparé
/// pour le garantir. Le contenu de plusieurs fenêtres et de leurs écrans arrive avec l'épic
/// Multi-fenêtres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawWorkspace", into = "RawWorkspace")]
pub struct Workspace {
    tree: LayoutTree,
    panels: BTreeMap<PanelId, Panel>,
    active: Option<PanelId>,
    next_id: u64,
    /// Panneaux du plus anciennement au plus récemment actif : à la fermeture du panneau actif,
    /// le focus revient au précédent.
    recent: Vec<PanelId>,
}

/// Forme sérialisée du workspace.
#[derive(Serialize, Deserialize)]
struct RawWorkspace {
    tree: LayoutTree,
    panels: Vec<Panel>,
    #[serde(default)]
    active: Option<PanelId>,
}

impl From<RawWorkspace> for Workspace {
    fn from(raw: RawWorkspace) -> Self {
        let mut tree = raw.tree;
        let mut panels: BTreeMap<PanelId, Panel> =
            raw.panels.into_iter().map(|p| (p.id(), p)).collect();
        // Un panneau sans modèle est retiré de l'arbre ; un modèle hors de l'arbre est oublié.
        for id in tree.panels() {
            if !panels.contains_key(&id) {
                let _ = tree.remove(id);
            }
        }
        let shown = tree.panels();
        panels.retain(|id, _| shown.contains(id));
        let next_id = panels.keys().last().map_or(1, |id| id.get() + 1);
        let active = raw
            .active
            .filter(|id| panels.contains_key(id))
            .or_else(|| tree.groups().first().and_then(|g| g.active()));
        Self {
            tree,
            panels,
            active,
            next_id,
            recent: active.into_iter().collect(),
        }
    }
}

impl From<Workspace> for RawWorkspace {
    fn from(workspace: Workspace) -> Self {
        Self {
            tree: workspace.tree,
            panels: workspace.panels.into_values().collect(),
            active: workspace.active,
        }
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    /// Workspace sans panneau.
    pub fn new() -> Self {
        Self {
            tree: LayoutTree::new(),
            panels: BTreeMap::new(),
            active: None,
            next_id: 1,
            recent: Vec::new(),
        }
    }

    /// Arbre de layout.
    pub fn tree(&self) -> &LayoutTree {
        &self.tree
    }

    /// Arbre de layout, modifiable : diviser, déplacer, redimensionner. Pour ajouter ou retirer
    /// un panneau, passer par [`Workspace::create`] et [`Workspace::remove`], qui tiennent les
    /// modèles à jour.
    pub fn tree_mut(&mut self) -> &mut LayoutTree {
        &mut self.tree
    }

    /// Modèle du panneau `id`.
    pub fn panel(&self, id: PanelId) -> Option<&Panel> {
        self.panels.get(&id)
    }

    /// Modèle du panneau `id`, modifiable.
    pub fn panel_mut(&mut self, id: PanelId) -> Option<&mut Panel> {
        self.panels.get_mut(&id)
    }

    /// Tous les panneaux.
    pub fn panels(&self) -> impl Iterator<Item = &Panel> {
        self.panels.values()
    }

    /// Nombre de panneaux.
    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    /// Panneau qui a le focus.
    pub fn active(&self) -> Option<PanelId> {
        self.active
    }

    /// Crée un panneau de type `kind`, sans le placer dans l'arbre : le placer ensuite avec
    /// [`LayoutTree::add`] ou [`LayoutTree::split`]. L'identifiant n'est jamais réutilisé.
    pub fn create(&mut self, kind: PanelKind) -> PanelId {
        let id = PanelId::new(self.next_id);
        self.next_id += 1;
        self.panels.insert(id, Panel::new(id, kind));
        id
    }

    /// Donne le focus au panneau `id` et l'affiche dans sa zone.
    pub fn focus(&mut self, id: PanelId) -> Result<(), LayoutError> {
        self.tree.activate(id)?;
        self.active = Some(id);
        self.recent.retain(|p| *p != id);
        self.recent.push(id);
        Ok(())
    }

    /// Retire le panneau `id` de l'arbre et renvoie son modèle. S'il avait le focus, il passe à
    /// l'onglet qui le remplace dans sa zone, ou, si la zone disparaît, au dernier panneau actif.
    pub fn remove(&mut self, id: PanelId) -> Option<Panel> {
        let panel = self.panels.remove(&id)?;
        let sibling = self
            .tree
            .group_of(id)
            .and_then(|g| g.panels().iter().copied().find(|p| *p != id));
        let _ = self.tree.remove(id);
        self.recent.retain(|p| *p != id);
        if self.active == Some(id) {
            self.active = None;
            let next = sibling
                .and_then(|s| self.tree.group_of(s))
                .and_then(|g| g.active())
                .or_else(|| {
                    let last = self.recent.iter().rev().find(|p| self.tree.contains(**p))?;
                    self.tree.group_of(*last)?.active()
                })
                .or_else(|| self.tree.panels().first().copied());
            if let Some(next) = next {
                let _ = self.focus(next);
            }
        }
        Some(panel)
    }

    /// Remplace l'arbre et les panneaux par ceux de `other` (layout chargé), en gardant la suite
    /// des identifiants : un identifiant ne sert jamais deux fois.
    pub fn replace(&mut self, other: Workspace) {
        let next_id = self.next_id.max(other.next_id);
        *self = other;
        self.next_id = next_id;
    }

    /// Prochain identifiant attribué.
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Les invariants sont-ils respectés : arbre valide, un modèle par panneau de l'arbre et
    /// inversement, panneau actif dans l'arbre ?
    pub fn is_valid(&self) -> bool {
        let shown = self.tree.panels();
        self.tree.is_valid()
            && shown.len() == self.panels.len()
            && shown.iter().all(|id| self.panels.contains_key(id))
            && self.active.is_none_or(|id| self.tree.contains(id))
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
    fn remplacer_garde_la_suite_des_identifiants() {
        let mut ws = Workspace::new();
        for _ in 0..5 {
            ws.create(editor());
        }
        ws.replace(Workspace::new());
        assert_eq!(ws.panel_count(), 0);
        assert_eq!(ws.create(editor()).get(), 6);
    }
}
