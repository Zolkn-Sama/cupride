//! Arbre de layout d'une fenêtre : splits et groupes d'onglets.
//!
//! # Invariants
//!
//! Après chaque opération, l'arbre est **toujours valide** ([`LayoutTree::is_valid`]) :
//!
//! - un panneau apparaît au plus une fois ;
//! - un groupe d'onglets n'est vide que s'il est la racine (fenêtre sans panneau) ;
//! - un split a au moins deux enfants : un split qui n'en a plus qu'un est remplacé par cet enfant ;
//! - un split n'a jamais pour enfant un split de même axe : ses enfants sont remontés ;
//! - les poids des enfants d'un split sont positifs et leur somme vaut 1.
//!
//! Un arbre lu depuis JSON est réparé pour respecter ces invariants : une sauvegarde abîmée ne
//! produit jamais un layout inutilisable.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::geometry::{Axis, Bounds, Direction, spans};
use crate::panel::PanelId;

/// Erreur d'une opération sur le layout.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LayoutError {
    /// Le panneau n'est pas dans le layout.
    #[error("panneau {0} introuvable dans le layout")]
    UnknownPanel(PanelId),
    /// Le panneau est déjà dans le layout.
    #[error("le panneau {0} est déjà dans le layout")]
    AlreadyPresent(PanelId),
    /// La fenêtre n'existe pas.
    #[error("fenêtre {0} introuvable")]
    UnknownWindow(u64),
    /// Le panneau est seul dans sa zone : il ne peut pas être placé à côté de lui-même.
    #[error("le panneau {0} est seul dans sa zone : il ne peut pas être placé à côté de lui-même")]
    AloneInGroup(PanelId),
}

/// Groupe d'onglets : une zone de la fenêtre, qui affiche un panneau parmi plusieurs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabGroup {
    panels: Vec<PanelId>,
    #[serde(default)]
    active: usize,
    /// Onglet d'aperçu : remplacé par le prochain fichier ouvert dans ce groupe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview: Option<PanelId>,
}

impl TabGroup {
    fn single(panel: PanelId) -> Self {
        Self {
            panels: vec![panel],
            active: 0,
            preview: None,
        }
    }

    /// Panneaux du groupe, dans l'ordre des onglets.
    pub fn panels(&self) -> &[PanelId] {
        &self.panels
    }

    /// Panneau affiché, s'il y en a un.
    pub fn active(&self) -> Option<PanelId> {
        self.panels.get(self.active).copied()
    }

    /// Rang de l'onglet affiché.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Onglet d'aperçu, s'il y en a un.
    pub fn preview(&self) -> Option<PanelId> {
        self.preview
    }

    /// Le groupe contient-il `panel` ?
    pub fn contains(&self, panel: PanelId) -> bool {
        self.panels.contains(&panel)
    }

    /// Nombre d'onglets.
    pub fn len(&self) -> usize {
        self.panels.len()
    }

    /// Le groupe est-il vide ?
    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }

    fn index_of(&self, panel: PanelId) -> Option<usize> {
        self.panels.iter().position(|p| *p == panel)
    }

    /// Insère `panel` au rang `index` et l'affiche.
    fn insert(&mut self, index: usize, panel: PanelId) {
        let index = index.min(self.panels.len());
        self.panels.insert(index, panel);
        self.active = index;
    }

    /// Retire `panel`. S'il était affiché, l'onglet suivant (ou le dernier) le remplace.
    fn remove(&mut self, panel: PanelId) -> bool {
        let Some(index) = self.index_of(panel) else {
            return false;
        };
        self.panels.remove(index);
        if self.preview == Some(panel) {
            self.preview = None;
        }
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.panels.len().saturating_sub(1));
        true
    }
}

/// Enfant d'un split, avec sa part de la place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Child {
    weight: f32,
    node: Node,
}

impl Child {
    /// Part de la place du split, entre 0 et 1.
    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// Contenu.
    pub fn node(&self) -> &Node {
        &self.node
    }
}

/// Split : la place est partagée entre les enfants, le long d'un axe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Split {
    axis: Axis,
    children: Vec<Child>,
}

impl Split {
    /// Axe du partage.
    pub fn axis(&self) -> Axis {
        self.axis
    }

    /// Enfants, de gauche à droite ou de haut en bas.
    pub fn children(&self) -> &[Child] {
        &self.children
    }
}

/// Nœud de l'arbre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    /// Partage de la place entre plusieurs nœuds.
    Split(Split),
    /// Zone d'onglets.
    Tabs(TabGroup),
}

/// Où placer un panneau déplacé, par rapport à un panneau cible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Dans la zone de la cible, comme nouvel onglet.
    Tab,
    /// Dans une nouvelle zone, du côté indiqué de la zone de la cible.
    Side(Direction),
}

/// Part de la largeur (ou de la hauteur) d'une zone, depuis chaque bord, où lâcher un panneau
/// l'installe dans une nouvelle zone de ce côté plutôt qu'en onglet.
pub const DROP_EDGE: f32 = 0.25;

/// Où un panneau lâché à la souris serait placé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropTarget {
    /// Panneau affiché dans la zone visée : la cible de [`LayoutTree::move_panel`].
    pub anchor: PanelId,
    /// Placement par rapport à cette zone.
    pub placement: Placement,
    /// Place que prendrait le panneau : toute la zone en onglet, sa moitié de ce côté sinon.
    pub preview: Bounds,
}

/// Zone d'onglets et sa place dans la fenêtre.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupArea<'a> {
    /// Groupe d'onglets.
    pub group: &'a TabGroup,
    /// Place occupée.
    pub bounds: Bounds,
}

/// Séparation entre deux enfants d'un split, que l'on peut faire glisser.
#[derive(Debug, Clone, PartialEq)]
pub struct Divider {
    /// Chemin du split dans l'arbre (rang de l'enfant à chaque niveau).
    pub split: Vec<usize>,
    /// La séparation est entre les enfants `index` et `index + 1`.
    pub index: usize,
    /// Axe du split : une séparation d'un split horizontal est verticale, et glisse de gauche à
    /// droite.
    pub axis: Axis,
    /// Place de la séparation (l'espace entre les deux enfants ; largeur nulle sans espace).
    pub bounds: Bounds,
}

/// Arbre de layout d'une fenêtre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "Node", into = "Node")]
pub struct LayoutTree {
    root: Node,
}

impl Default for LayoutTree {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Node> for LayoutTree {
    /// Arbre à partir d'un nœud quelconque, réparé pour respecter les invariants.
    fn from(root: Node) -> Self {
        let mut tree = Self { root };
        tree.normalize();
        tree
    }
}

impl From<LayoutTree> for Node {
    fn from(tree: LayoutTree) -> Self {
        tree.root
    }
}

impl LayoutTree {
    /// Arbre vide : une seule zone, sans onglet.
    pub fn new() -> Self {
        Self {
            root: Node::Tabs(TabGroup::default()),
        }
    }

    /// Racine de l'arbre.
    pub fn root(&self) -> &Node {
        &self.root
    }

    /// L'arbre ne contient-il aucun panneau ?
    pub fn is_empty(&self) -> bool {
        matches!(&self.root, Node::Tabs(group) if group.is_empty())
    }

    /// Tous les panneaux, zone par zone (de gauche à droite, de haut en bas), dans l'ordre des
    /// onglets.
    pub fn panels(&self) -> Vec<PanelId> {
        self.groups()
            .into_iter()
            .flat_map(|g| g.panels.iter().copied())
            .collect()
    }

    /// Toutes les zones d'onglets, de gauche à droite et de haut en bas.
    pub fn groups(&self) -> Vec<&TabGroup> {
        fn walk<'a>(node: &'a Node, out: &mut Vec<&'a TabGroup>) {
            match node {
                Node::Tabs(group) => out.push(group),
                Node::Split(split) => split.children.iter().for_each(|c| walk(&c.node, out)),
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    /// Le layout contient-il `panel` ?
    pub fn contains(&self, panel: PanelId) -> bool {
        self.path_of(panel).is_some()
    }

    /// Zone d'onglets qui contient `panel`.
    pub fn group_of(&self, panel: PanelId) -> Option<&TabGroup> {
        let path = self.path_of(panel)?;
        match self.node(&path)? {
            Node::Tabs(group) => Some(group),
            Node::Split(_) => None,
        }
    }

    /// Chemin de la zone qui contient `panel`.
    fn path_of(&self, panel: PanelId) -> Option<Vec<usize>> {
        fn walk(node: &Node, panel: PanelId, path: &mut Vec<usize>) -> bool {
            match node {
                Node::Tabs(group) => group.contains(panel),
                Node::Split(split) => split.children.iter().enumerate().any(|(i, c)| {
                    path.push(i);
                    let found = walk(&c.node, panel, path);
                    if !found {
                        path.pop();
                    }
                    found
                }),
            }
        }
        let mut path = Vec::new();
        walk(&self.root, panel, &mut path).then_some(path)
    }

    fn node(&self, path: &[usize]) -> Option<&Node> {
        let mut node = &self.root;
        for &index in path {
            match node {
                Node::Split(split) => node = &split.children.get(index)?.node,
                Node::Tabs(_) => return None,
            }
        }
        Some(node)
    }

    fn node_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut node = &mut self.root;
        for &index in path {
            match node {
                Node::Split(split) => node = &mut split.children.get_mut(index)?.node,
                Node::Tabs(_) => return None,
            }
        }
        Some(node)
    }

    fn group_mut(&mut self, panel: PanelId) -> Result<&mut TabGroup, LayoutError> {
        let path = self
            .path_of(panel)
            .ok_or(LayoutError::UnknownPanel(panel))?;
        match self.node_mut(&path) {
            Some(Node::Tabs(group)) => Ok(group),
            _ => Err(LayoutError::UnknownPanel(panel)),
        }
    }

    /// Première zone (en haut à gauche).
    fn first_group_mut(&mut self) -> &mut TabGroup {
        let mut node = &mut self.root;
        loop {
            match node {
                Node::Tabs(group) => return group,
                Node::Split(split) => node = &mut split.children[0].node,
            }
        }
    }

    /// Ajoute `panel` comme onglet, juste après `beside` dans sa zone, ou à la fin de la première
    /// zone sans `beside`. Le nouvel onglet est affiché.
    pub fn add(&mut self, panel: PanelId, beside: Option<PanelId>) -> Result<(), LayoutError> {
        if self.contains(panel) {
            return Err(LayoutError::AlreadyPresent(panel));
        }
        let (group, index) = match beside {
            Some(beside) => {
                let group = self.group_mut(beside)?;
                let index = group.index_of(beside).map_or(group.len(), |i| i + 1);
                (group, index)
            }
            None => {
                let group = self.first_group_mut();
                let index = group.len();
                (group, index)
            }
        };
        group.insert(index, panel);
        Ok(())
    }

    /// Divise la zone de `target` : `panel` occupe une nouvelle zone du côté `direction`, et la
    /// place est partagée en deux. Si la zone est déjà dans un split de même axe, la nouvelle
    /// zone s'y ajoute à côté d'elle, sans imbriquer de split.
    pub fn split(
        &mut self,
        target: PanelId,
        direction: Direction,
        panel: PanelId,
    ) -> Result<(), LayoutError> {
        if self.contains(panel) {
            return Err(LayoutError::AlreadyPresent(panel));
        }
        let path = self
            .path_of(target)
            .ok_or(LayoutError::UnknownPanel(target))?;
        let axis = direction.axis();
        let new_group = Node::Tabs(TabGroup::single(panel));
        if let Some((&index, parent)) = path.split_last()
            && let Some(Node::Split(split)) = self.node_mut(parent)
            && split.axis == axis
        {
            let weight = split.children[index].weight / 2.0;
            split.children[index].weight = weight;
            let at = if direction.is_forward() {
                index + 1
            } else {
                index
            };
            split.children.insert(
                at,
                Child {
                    weight,
                    node: new_group,
                },
            );
        } else {
            let node = self
                .node_mut(&path)
                .ok_or(LayoutError::UnknownPanel(target))?;
            let old = std::mem::replace(node, Node::Tabs(TabGroup::default()));
            let (first, second) = if direction.is_forward() {
                (old, new_group)
            } else {
                (new_group, old)
            };
            *node = Node::Split(Split {
                axis,
                children: vec![
                    Child {
                        weight: 0.5,
                        node: first,
                    },
                    Child {
                        weight: 0.5,
                        node: second,
                    },
                ],
            });
        }
        self.normalize();
        Ok(())
    }

    /// Retire `panel`. Une zone vidée disparaît et sa place revient à ses voisines ; un split qui
    /// n'a plus qu'un enfant est remplacé par cet enfant.
    pub fn remove(&mut self, panel: PanelId) -> Result<(), LayoutError> {
        self.group_mut(panel)?.remove(panel);
        self.normalize();
        Ok(())
    }

    /// Déplace `panel` par rapport à `target` : dans sa zone comme onglet, ou dans une nouvelle
    /// zone à côté de la sienne.
    pub fn move_panel(
        &mut self,
        panel: PanelId,
        target: PanelId,
        placement: Placement,
    ) -> Result<(), LayoutError> {
        let source = self
            .group_of(panel)
            .ok_or(LayoutError::UnknownPanel(panel))?;
        let target_group = self
            .group_of(target)
            .ok_or(LayoutError::UnknownPanel(target))?;
        let same_group = source.contains(target);
        // Un panneau de la zone cible qui y restera : il sert de repère une fois `panel` retiré.
        let anchor = if same_group {
            target_group.panels.iter().copied().find(|p| *p != panel)
        } else {
            Some(target)
        };
        match placement {
            Placement::Tab if same_group => self.activate(panel),
            Placement::Tab => {
                self.remove(panel)?;
                self.add(panel, Some(target))
            }
            Placement::Side(direction) => {
                let anchor = anchor.ok_or(LayoutError::AloneInGroup(panel))?;
                self.remove(panel)?;
                self.split(anchor, direction, panel)
            }
        }
    }

    /// Affiche l'onglet `panel` dans sa zone.
    pub fn activate(&mut self, panel: PanelId) -> Result<(), LayoutError> {
        let group = self.group_mut(panel)?;
        group.active = group.index_of(panel).unwrap_or(group.active);
        Ok(())
    }

    /// Affiche l'onglet suivant (ou précédent) de la zone de `panel`, en revenant au début après
    /// le dernier. Renvoie le panneau affiché.
    pub fn cycle(&mut self, panel: PanelId, forward: bool) -> Result<PanelId, LayoutError> {
        let group = self.group_mut(panel)?;
        let len = group.len();
        let index = group.index_of(panel).unwrap_or(group.active);
        group.active = if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        };
        Ok(group.panels[group.active])
    }

    /// Place l'onglet `panel` au rang `index` de sa zone (borné au dernier rang).
    pub fn reorder(&mut self, panel: PanelId, index: usize) -> Result<(), LayoutError> {
        let group = self.group_mut(panel)?;
        let from = group
            .index_of(panel)
            .ok_or(LayoutError::UnknownPanel(panel))?;
        let active = group.active();
        group.panels.remove(from);
        let index = index.min(group.panels.len());
        group.panels.insert(index, panel);
        group.active = active
            .and_then(|a| group.index_of(a))
            .unwrap_or(group.active);
        Ok(())
    }

    /// Fait de `panel` l'onglet d'aperçu de sa zone (ou lui retire ce statut).
    pub fn set_preview(&mut self, panel: PanelId, preview: bool) -> Result<(), LayoutError> {
        let group = self.group_mut(panel)?;
        if preview {
            group.preview = Some(panel);
        } else if group.preview == Some(panel) {
            group.preview = None;
        }
        Ok(())
    }

    /// Place de chaque zone dans `area`, les enfants des splits étant séparés de `gap`.
    pub fn areas(&self, area: Bounds, gap: f32) -> Vec<GroupArea<'_>> {
        let mut areas = Vec::new();
        self.walk_geometry(area, gap, &mut |_, node, bounds| {
            if let Node::Tabs(group) = node {
                areas.push(GroupArea { group, bounds });
            }
        });
        areas
    }

    /// Séparations entre les zones, dans `area`.
    pub fn dividers(&self, area: Bounds, gap: f32) -> Vec<Divider> {
        let mut dividers = Vec::new();
        self.walk_geometry(area, gap, &mut |path, node, bounds| {
            let Node::Split(split) = node else {
                return;
            };
            let weights: Vec<f32> = split.children.iter().map(|c| c.weight).collect();
            let parts = spans(bounds.length(split.axis), gap, &weights);
            for (index, pair) in parts.windows(2).enumerate() {
                let start = bounds.start(split.axis) + pair[0].0 + pair[0].1;
                let end = bounds.start(split.axis) + pair[1].0;
                dividers.push(Divider {
                    split: path.to_vec(),
                    index,
                    axis: split.axis,
                    bounds: bounds.with_span(split.axis, start, (end - start).max(0.0)),
                });
            }
        });
        dividers
    }

    /// Parcourt l'arbre en calculant la place de chaque nœud.
    fn walk_geometry<'a>(
        &'a self,
        area: Bounds,
        gap: f32,
        visit: &mut dyn FnMut(&[usize], &'a Node, Bounds),
    ) {
        fn walk<'a>(
            node: &'a Node,
            bounds: Bounds,
            gap: f32,
            path: &mut Vec<usize>,
            visit: &mut dyn FnMut(&[usize], &'a Node, Bounds),
        ) {
            visit(path, node, bounds);
            if let Node::Split(split) = node {
                let weights: Vec<f32> = split.children.iter().map(|c| c.weight).collect();
                let parts = spans(bounds.length(split.axis), gap, &weights);
                for (index, (child, (start, length))) in
                    split.children.iter().zip(parts).enumerate()
                {
                    path.push(index);
                    let child_bounds =
                        bounds.with_span(split.axis, bounds.start(split.axis) + start, length);
                    walk(&child.node, child_bounds, gap, path, visit);
                    path.pop();
                }
            }
        }
        walk(&self.root, area, gap, &mut Vec::new(), visit);
    }

    /// Place du nœud au chemin `path`.
    fn bounds_of(&self, path: &[usize], area: Bounds, gap: f32) -> Option<Bounds> {
        let mut found = None;
        self.walk_geometry(area, gap, &mut |p, _, bounds| {
            if p == path {
                found = Some(bounds);
            }
        });
        found
    }

    /// Où `panel`, lâché au point (`x`, `y`), serait placé : en onglet au centre d'une zone, dans
    /// une nouvelle zone près de ses bords. `None` hors des zones, ou si le déplacement ne
    /// changerait rien (le panneau seul dans sa zone, lâché sur elle).
    pub fn drop_target(
        &self,
        panel: PanelId,
        x: f32,
        y: f32,
        area: Bounds,
        gap: f32,
    ) -> Option<DropTarget> {
        let areas = self.areas(area, gap);
        let zone = areas.iter().find(|a| a.bounds.contains(x, y))?;
        let anchor = zone.group.active()?;
        let b = zone.bounds;
        // Distance relative à chaque bord ; le plus proche l'emporte s'il est assez près.
        let edges = [
            (Direction::Left, (x - b.x) / b.width.max(1.0)),
            (Direction::Right, (b.right() - x) / b.width.max(1.0)),
            (Direction::Up, (y - b.y) / b.height.max(1.0)),
            (Direction::Down, (b.bottom() - y) / b.height.max(1.0)),
        ];
        let side = edges
            .into_iter()
            .filter(|(_, distance)| *distance < DROP_EDGE)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(direction, _)| direction);
        let own_zone = zone.group.contains(panel);
        match side {
            None if own_zone => None,
            None => Some(DropTarget {
                anchor,
                placement: Placement::Tab,
                preview: b,
            }),
            Some(_) if own_zone && zone.group.len() == 1 => None,
            Some(direction) => {
                let axis = direction.axis();
                let half = b.length(axis) / 2.0;
                let start = if direction.is_forward() {
                    b.start(axis) + half
                } else {
                    b.start(axis)
                };
                Some(DropTarget {
                    anchor,
                    placement: Placement::Side(direction),
                    preview: b.with_span(axis, start, half),
                })
            }
        }
    }

    /// Zone sous le point (`x`, `y`).
    pub fn group_at(&self, x: f32, y: f32, area: Bounds, gap: f32) -> Option<&TabGroup> {
        self.areas(area, gap)
            .into_iter()
            .find(|a| a.bounds.contains(x, y))
            .map(|a| a.group)
    }

    /// Fait glisser la séparation `index` du split `split` à la position `position` (le long de
    /// l'axe du split). Chacune des deux zones voisines garde au moins `min_size`. Renvoie vrai
    /// si les tailles ont changé.
    pub fn drag_divider(
        &mut self,
        split: &[usize],
        index: usize,
        position: f32,
        area: Bounds,
        gap: f32,
        min_size: f32,
    ) -> bool {
        let Some(bounds) = self.bounds_of(split, area, gap) else {
            return false;
        };
        let Some(Node::Split(node)) = self.node_mut(split) else {
            return false;
        };
        if index + 1 >= node.children.len() {
            return false;
        }
        let axis = node.axis;
        let weights: Vec<f32> = node.children.iter().map(|c| c.weight).collect();
        let parts = spans(bounds.length(axis), gap, &weights);
        let first_start = bounds.start(axis) + parts[index].0;
        let pair = parts[index].1 + parts[index + 1].1;
        if pair <= 0.0 {
            return false;
        }
        let min = min_size.max(0.0).min(pair / 2.0);
        // La séparation est au milieu de l'espace entre les deux zones.
        let first = (position - gap / 2.0 - first_start).clamp(min, pair - min);
        set_pair(&mut node.children, index, index + 1, first / pair)
    }

    /// Agrandit (ou réduit, si `delta` est négatif) la zone de `panel` de `delta` le long de
    /// `axis`, en prenant la place à sa voisine (la suivante, ou la précédente pour la dernière).
    /// Chaque zone garde au moins `min_size`. Renvoie vrai si les tailles ont changé.
    pub fn resize(
        &mut self,
        panel: PanelId,
        axis: Axis,
        delta: f32,
        area: Bounds,
        gap: f32,
        min_size: f32,
    ) -> bool {
        let Some(path) = self.path_of(panel) else {
            return false;
        };
        // Le split d'axe `axis` le plus proche de la zone.
        let found = (0..path.len()).rev().find(
            |&depth| matches!(self.node(&path[..depth]), Some(Node::Split(s)) if s.axis == axis),
        );
        let Some(depth) = found else {
            return false;
        };
        let (parent, child) = (&path[..depth], path[depth]);
        let Some(bounds) = self.bounds_of(parent, area, gap) else {
            return false;
        };
        let Some(Node::Split(split)) = self.node_mut(parent) else {
            return false;
        };
        let neighbor = if child + 1 < split.children.len() {
            child + 1
        } else {
            child - 1
        };
        let weights: Vec<f32> = split.children.iter().map(|c| c.weight).collect();
        let parts = spans(bounds.length(axis), gap, &weights);
        let pair = parts[child].1 + parts[neighbor].1;
        if pair <= 0.0 {
            return false;
        }
        let min = min_size.max(0.0).min(pair / 2.0);
        let size = (parts[child].1 + delta).clamp(min, pair - min);
        set_pair(&mut split.children, child, neighbor, size / pair)
    }

    /// Zone voisine de celle de `panel` dans la direction `direction` : celle qui la touche de ce
    /// côté, la plus proche et qui la recouvre le plus. Renvoie son onglet affiché.
    pub fn neighbor(
        &self,
        panel: PanelId,
        direction: Direction,
        area: Bounds,
        gap: f32,
    ) -> Option<PanelId> {
        let areas = self.areas(area, gap);
        let current = areas.iter().find(|a| a.group.contains(panel))?.bounds;
        let axis = direction.axis();
        let cross = axis.cross();
        let tolerance = gap.max(0.0) + 0.5;
        areas
            .iter()
            .filter(|a| !a.group.contains(panel))
            .filter_map(|a| {
                let b = a.bounds;
                let distance = if direction.is_forward() {
                    b.start(axis) - current.end(axis)
                } else {
                    current.start(axis) - b.end(axis)
                };
                let overlap =
                    b.end(cross).min(current.end(cross)) - b.start(cross).max(current.start(cross));
                (distance >= -tolerance && overlap > 0.0).then_some((distance, overlap, a))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0).then_with(|| y.1.total_cmp(&x.1)))
            .and_then(|(_, _, a)| a.group.active())
    }

    /// Renomme chaque panneau `p` en `rename(p)` (identifiants neufs d'un layout chargé).
    pub fn rename(&mut self, rename: &mut dyn FnMut(PanelId) -> PanelId) {
        fn walk(node: &mut Node, rename: &mut dyn FnMut(PanelId) -> PanelId) {
            match node {
                Node::Tabs(group) => {
                    let renamed: Vec<(PanelId, PanelId)> =
                        group.panels.iter().map(|p| (*p, rename(*p))).collect();
                    group.preview = group
                        .preview
                        .and_then(|p| renamed.iter().find(|(old, _)| *old == p))
                        .map(|(_, new)| *new);
                    group.panels = renamed.into_iter().map(|(_, new)| new).collect();
                }
                Node::Split(split) => split
                    .children
                    .iter_mut()
                    .for_each(|c| walk(&mut c.node, rename)),
            }
        }
        walk(&mut self.root, rename);
        self.normalize();
    }

    /// Vérifie les invariants (voir la [documentation du module](self)).
    pub fn is_valid(&self) -> bool {
        fn check(
            node: &Node,
            root: bool,
            parent: Option<Axis>,
            seen: &mut HashSet<PanelId>,
        ) -> bool {
            match node {
                Node::Tabs(group) => {
                    (root || !group.is_empty())
                        && (group.is_empty() || group.active < group.len())
                        && group.preview.is_none_or(|p| group.contains(p))
                        && group.panels.iter().all(|p| seen.insert(*p))
                }
                Node::Split(split) => {
                    let sum: f32 = split.children.iter().map(|c| c.weight).sum();
                    split.children.len() >= 2
                        && parent != Some(split.axis)
                        && (sum - 1.0).abs() < 1e-3
                        && split
                            .children
                            .iter()
                            .all(|c| c.weight.is_finite() && c.weight > 0.0)
                        && split
                            .children
                            .iter()
                            .all(|c| check(&c.node, false, Some(split.axis), seen))
                }
            }
        }
        check(&self.root, true, None, &mut HashSet::new())
    }

    /// Rétablit les invariants : zones vides retirées, splits simplifiés et aplatis, poids
    /// corrigés, doublons retirés.
    fn normalize(&mut self) {
        let root = std::mem::replace(&mut self.root, Node::Tabs(TabGroup::default()));
        self.root = normalize(root, &mut HashSet::new()).unwrap_or(Node::Tabs(TabGroup::default()));
    }
}

/// Donne la part `first` (entre 0 et 1) de la place des enfants `a` et `b` à `a`, le reste à
/// `b`. Renvoie vrai si les poids ont changé.
fn set_pair(children: &mut [Child], a: usize, b: usize, first: f32) -> bool {
    let total = children[a].weight + children[b].weight;
    let weight_a = total * first;
    let weight_b = total - weight_a;
    if weight_a <= 0.0 || weight_b <= 0.0 {
        return false;
    }
    let changed = (children[a].weight - weight_a).abs() > 1e-6;
    children[a].weight = weight_a;
    children[b].weight = weight_b;
    changed
}

/// Nœud respectant les invariants, ou `None` s'il ne contient aucun panneau.
fn normalize(node: Node, seen: &mut HashSet<PanelId>) -> Option<Node> {
    match node {
        Node::Tabs(mut group) => {
            let active = group.active();
            group.panels.retain(|p| seen.insert(*p));
            if group.panels.is_empty() {
                return None;
            }
            group.active = active
                .and_then(|a| group.index_of(a))
                .unwrap_or(0)
                .min(group.panels.len() - 1);
            if group.preview.is_some_and(|p| !group.contains(p)) {
                group.preview = None;
            }
            Some(Node::Tabs(group))
        }
        Node::Split(split) => {
            let axis = split.axis;
            let valid = split
                .children
                .iter()
                .all(|c| c.weight.is_finite() && c.weight > 0.0);
            let mut children: Vec<Child> = Vec::with_capacity(split.children.len());
            for child in split.children {
                let weight = if valid { child.weight } else { 1.0 };
                match normalize(child.node, seen) {
                    None => {}
                    // Un split de même axe est aplati : ses enfants prennent sa place.
                    Some(Node::Split(inner)) if inner.axis == axis => {
                        let inner_sum: f32 = inner.children.iter().map(|c| c.weight).sum();
                        children.extend(inner.children.into_iter().map(|c| Child {
                            weight: weight * c.weight / inner_sum,
                            node: c.node,
                        }));
                    }
                    Some(node) => children.push(Child { weight, node }),
                }
            }
            match children.len() {
                0 => None,
                1 => children.pop().map(|c| c.node),
                _ => {
                    let sum: f32 = children.iter().map(|c| c.weight).sum();
                    for child in &mut children {
                        child.weight /= sum;
                    }
                    Some(Node::Split(Split { axis, children }))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: u64) -> PanelId {
        PanelId::new(id)
    }

    const AREA: Bounds = Bounds::new(0.0, 0.0, 1000.0, 600.0);

    /// Forme de l'arbre, lisible : `H[1 | V[2 | 3]]`, onglets entre parenthèses `(1 *2)` (`*`
    /// marque l'onglet affiché quand il y en a plusieurs).
    fn shape(tree: &LayoutTree) -> String {
        fn render(node: &Node) -> String {
            match node {
                Node::Tabs(group) if group.len() <= 1 => group
                    .panels
                    .first()
                    .map_or_else(|| "()".to_owned(), ToString::to_string),
                Node::Tabs(group) => {
                    let tabs: Vec<String> = group
                        .panels
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            if i == group.active {
                                format!("*{p}")
                            } else {
                                p.to_string()
                            }
                        })
                        .collect();
                    format!("({})", tabs.join(" "))
                }
                Node::Split(split) => {
                    let axis = match split.axis {
                        Axis::Horizontal => "H",
                        Axis::Vertical => "V",
                    };
                    let children: Vec<String> =
                        split.children.iter().map(|c| render(&c.node)).collect();
                    format!("{axis}[{}]", children.join(" | "))
                }
            }
        }
        assert!(tree.is_valid(), "arbre invalide : {tree:?}");
        render(&tree.root)
    }

    fn weights(tree: &LayoutTree) -> Vec<f32> {
        match &tree.root {
            Node::Split(split) => split
                .children
                .iter()
                .map(|c| (c.weight * 10_000.0).round() / 10_000.0)
                .collect(),
            Node::Tabs(_) => vec![1.0],
        }
    }

    /// 1 | 2 côte à côte.
    fn two_columns() -> LayoutTree {
        let mut tree = LayoutTree::new();
        tree.add(p(1), None).unwrap();
        tree.split(p(1), Direction::Right, p(2)).unwrap();
        tree
    }

    #[test]
    fn arbre_vide_puis_onglets() {
        let mut tree = LayoutTree::new();
        assert!(tree.is_empty());
        assert_eq!(shape(&tree), "()");
        tree.add(p(1), None).unwrap();
        tree.add(p(2), Some(p(1))).unwrap();
        tree.add(p(3), Some(p(1))).unwrap();
        assert_eq!(shape(&tree), "(1 *3 2)", "après l'onglet de référence");
        assert_eq!(tree.add(p(2), None), Err(LayoutError::AlreadyPresent(p(2))));
        assert_eq!(
            tree.add(p(4), Some(p(9))),
            Err(LayoutError::UnknownPanel(p(9)))
        );
        assert_eq!(tree.panels(), vec![p(1), p(3), p(2)]);
    }

    #[test]
    fn diviser() {
        let mut tree = two_columns();
        assert_eq!(shape(&tree), "H[1 | 2]");
        assert_eq!(weights(&tree), vec![0.5, 0.5]);
        tree.split(p(1), Direction::Left, p(3)).unwrap();
        assert_eq!(shape(&tree), "H[3 | 1 | 2]", "même axe : pas d'imbrication");
        assert_eq!(weights(&tree), vec![0.25, 0.25, 0.5]);
        tree.split(p(2), Direction::Down, p(4)).unwrap();
        assert_eq!(shape(&tree), "H[3 | 1 | V[2 | 4]]");
        tree.split(p(4), Direction::Up, p(5)).unwrap();
        assert_eq!(shape(&tree), "H[3 | 1 | V[2 | 5 | 4]]");
        assert_eq!(
            tree.split(p(9), Direction::Up, p(6)),
            Err(LayoutError::UnknownPanel(p(9)))
        );
        assert_eq!(
            tree.split(p(1), Direction::Up, p(2)),
            Err(LayoutError::AlreadyPresent(p(2)))
        );
    }

    #[test]
    fn fermer_simplifie_l_arbre() {
        let mut tree = two_columns();
        tree.split(p(2), Direction::Down, p(3)).unwrap();
        assert_eq!(shape(&tree), "H[1 | V[2 | 3]]");
        tree.remove(p(3)).unwrap();
        assert_eq!(shape(&tree), "H[1 | 2]", "split à un enfant simplifié");
        assert_eq!(weights(&tree), vec![0.5, 0.5]);
        tree.remove(p(1)).unwrap();
        assert_eq!(shape(&tree), "2");
        tree.remove(p(2)).unwrap();
        assert!(tree.is_empty());
        assert_eq!(tree.remove(p(2)), Err(LayoutError::UnknownPanel(p(2))));
    }

    #[test]
    fn fermer_aplatit_les_splits_de_meme_axe() {
        // H[1 | V[2 | H[3 | 4]]] : fermer 2 laisse H[3 | 4] seul dans le V, qui disparaît ; ses
        // enfants rejoignent le H de la racine.
        let mut tree = two_columns();
        tree.split(p(2), Direction::Down, p(3)).unwrap();
        tree.split(p(3), Direction::Right, p(4)).unwrap();
        assert_eq!(shape(&tree), "H[1 | V[2 | H[3 | 4]]]");
        tree.remove(p(2)).unwrap();
        assert_eq!(shape(&tree), "H[1 | 3 | 4]");
        assert_eq!(weights(&tree), vec![0.5, 0.25, 0.25]);
    }

    #[test]
    fn fermer_un_onglet_affiche_le_suivant() {
        let mut tree = LayoutTree::new();
        for id in 1..=4 {
            tree.add(p(id), None).unwrap();
        }
        tree.activate(p(2)).unwrap();
        tree.remove(p(2)).unwrap();
        assert_eq!(shape(&tree), "(1 *3 4)");
        tree.activate(p(4)).unwrap();
        tree.remove(p(4)).unwrap();
        assert_eq!(shape(&tree), "(1 *3)", "le dernier : le précédent");
        tree.remove(p(1)).unwrap();
        assert_eq!(shape(&tree), "3");
    }

    #[test]
    fn onglets_suivant_precedent_reordonner() {
        let mut tree = LayoutTree::new();
        for id in 1..=3 {
            tree.add(p(id), None).unwrap();
        }
        assert_eq!(tree.cycle(p(3), true).unwrap(), p(1), "revient au début");
        assert_eq!(tree.cycle(p(1), false).unwrap(), p(3));
        tree.reorder(p(3), 0).unwrap();
        assert_eq!(shape(&tree), "(*3 1 2)", "l'onglet affiché suit");
        tree.reorder(p(1), 99).unwrap();
        assert_eq!(shape(&tree), "(*3 2 1)");
        tree.set_preview(p(2), true).unwrap();
        assert_eq!(tree.group_of(p(2)).unwrap().preview(), Some(p(2)));
        tree.remove(p(2)).unwrap();
        assert_eq!(tree.group_of(p(1)).unwrap().preview(), None);
    }

    #[test]
    fn deplacer() {
        let mut tree = two_columns();
        tree.add(p(3), Some(p(2))).unwrap();
        assert_eq!(shape(&tree), "H[1 | (2 *3)]");
        // Onglet vers une autre zone.
        tree.move_panel(p(3), p(1), Placement::Tab).unwrap();
        assert_eq!(shape(&tree), "H[(1 *3) | 2]");
        // Vers le bas de sa propre zone : il en sort.
        tree.move_panel(p(3), p(1), Placement::Side(Direction::Down))
            .unwrap();
        assert_eq!(shape(&tree), "H[V[1 | 3] | 2]");
        // La zone qu'il quitte disparaît si elle est vide.
        tree.move_panel(p(1), p(2), Placement::Tab).unwrap();
        assert_eq!(shape(&tree), "H[3 | (2 *1)]");
        // Seul dans sa zone : impossible de le mettre à côté de lui-même.
        assert_eq!(
            tree.move_panel(p(3), p(3), Placement::Side(Direction::Left)),
            Err(LayoutError::AloneInGroup(p(3)))
        );
        assert_eq!(shape(&tree), "H[3 | (2 *1)]", "inchangé");
        tree.move_panel(p(3), p(3), Placement::Tab).unwrap();
        tree.move_panel(p(3), p(2), Placement::Side(Direction::Up))
            .unwrap();
        assert_eq!(shape(&tree), "V[3 | (2 *1)]");
    }

    #[test]
    fn geometrie_des_zones() {
        let mut tree = two_columns();
        tree.split(p(2), Direction::Down, p(3)).unwrap();
        let areas = tree.areas(AREA, 2.0);
        assert_eq!(areas.len(), 3);
        assert_eq!(areas[0].bounds, Bounds::new(0.0, 0.0, 499.0, 600.0));
        assert_eq!(areas[1].bounds, Bounds::new(501.0, 0.0, 499.0, 299.0));
        assert_eq!(areas[2].bounds, Bounds::new(501.0, 301.0, 499.0, 299.0));
        let dividers = tree.dividers(AREA, 2.0);
        assert_eq!(dividers.len(), 2);
        assert_eq!(dividers[0].bounds, Bounds::new(499.0, 0.0, 2.0, 600.0));
        assert_eq!(dividers[1].split, vec![1]);
        assert_eq!(dividers[1].bounds, Bounds::new(501.0, 299.0, 499.0, 2.0));
        assert_eq!(
            tree.group_at(700.0, 450.0, AREA, 2.0).unwrap().active(),
            Some(p(3))
        );
        assert!(
            tree.group_at(500.0, 10.0, AREA, 2.0).is_none(),
            "séparation"
        );
    }

    #[test]
    fn glisser_une_separation() {
        let mut tree = two_columns();
        assert!(tree.drag_divider(&[], 0, 250.0, AREA, 0.0, 100.0));
        assert_eq!(weights(&tree), vec![0.25, 0.75]);
        // Taille minimale.
        tree.drag_divider(&[], 0, 10.0, AREA, 0.0, 100.0);
        assert_eq!(weights(&tree), vec![0.1, 0.9]);
        tree.drag_divider(&[], 0, 990.0, AREA, 0.0, 100.0);
        assert_eq!(weights(&tree), vec![0.9, 0.1]);
        assert!(
            !tree.drag_divider(&[], 1, 500.0, AREA, 0.0, 100.0),
            "hors du split"
        );
        assert!(
            !tree.drag_divider(&[0], 0, 500.0, AREA, 0.0, 100.0),
            "pas un split"
        );
        assert!(tree.is_valid());
    }

    #[test]
    fn redimensionner_au_clavier() {
        let mut tree = two_columns();
        tree.split(p(2), Direction::Down, p(3)).unwrap();
        // 3 est dans un V : en largeur, c'est le H de la racine qui change.
        assert!(tree.resize(p(3), Axis::Horizontal, 100.0, AREA, 0.0, 50.0));
        assert_eq!(weights(&tree), vec![0.4, 0.6]);
        // La dernière zone grandit en prenant sur la précédente.
        assert!(tree.resize(p(1), Axis::Horizontal, -1000.0, AREA, 0.0, 50.0));
        assert_eq!(weights(&tree), vec![0.05, 0.95], "taille minimale");
        assert!(
            !tree.resize(p(1), Axis::Vertical, 10.0, AREA, 0.0, 50.0),
            "pas de V"
        );
        assert!(tree.resize(p(3), Axis::Vertical, 30.0, AREA, 0.0, 50.0));
        assert!(!tree.resize(p(9), Axis::Vertical, 30.0, AREA, 0.0, 50.0));
        assert!(tree.is_valid());
    }

    #[test]
    fn voisins() {
        // 1 | V[2 | 3], puis 4 sous 1.
        let mut tree = two_columns();
        tree.split(p(2), Direction::Down, p(3)).unwrap();
        tree.split(p(1), Direction::Down, p(4)).unwrap();
        tree.drag_divider(&[0], 0, 100.0, AREA, 0.0, 10.0);
        let n = |panel, direction| tree.neighbor(p(panel), direction, AREA, 0.0);
        assert_eq!(n(1, Direction::Right), Some(p(2)));
        assert_eq!(n(1, Direction::Down), Some(p(4)));
        assert_eq!(n(4, Direction::Right), Some(p(3)), "le plus recouvert");
        assert_eq!(n(3, Direction::Left), Some(p(4)));
        assert_eq!(n(2, Direction::Up), None);
        assert_eq!(n(1, Direction::Left), None);
        assert_eq!(n(9, Direction::Left), None);
    }

    #[test]
    fn serialisation_aller_retour() {
        let mut tree = two_columns();
        tree.split(p(2), Direction::Down, p(3)).unwrap();
        tree.add(p(4), Some(p(3))).unwrap();
        tree.set_preview(p(4), true).unwrap();
        let json = serde_json::to_string(&tree).unwrap();
        let back: LayoutTree = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tree);
    }

    #[test]
    fn json_abime_repare() {
        let json = r#"{"split": {"axis": "horizontal", "children": [
            {"weight": -1, "node": {"tabs": {"panels": [1, 2], "active": 7}}},
            {"weight": 2, "node": {"tabs": {"panels": []}}},
            {"weight": 1, "node": {"split": {"axis": "vertical", "children": [
                {"weight": 1, "node": {"tabs": {"panels": [2, 3], "preview": 9}}}
            ]}}}
        ]}}"#;
        let tree: LayoutTree = serde_json::from_str(json).unwrap();
        assert!(tree.is_valid());
        assert_eq!(
            shape(&tree),
            "H[(*1 2) | 3]",
            "doublon, zone vide, split à un enfant"
        );
        assert_eq!(weights(&tree), vec![0.5, 0.5], "poids invalides remplacés");
        assert_eq!(tree.group_of(p(3)).unwrap().preview(), None);
    }

    proptest::proptest! {
        /// Quelle que soit la suite d'opérations, réussies ou non, l'arbre reste valide et
        /// survit à la sérialisation.
        #[test]
        fn toujours_valide(
            ops in proptest::collection::vec((0_u8..8, 0_u64..8, 0_u64..8, 0_usize..4), 0..80)
        ) {
            let mut tree = LayoutTree::new();
            for (op, a, b, d) in ops {
                let direction = Direction::ALL[d];
                let before = tree.clone();
                let result = match op {
                    0 => tree.add(p(a), None),
                    1 => tree.add(p(a), Some(p(b))),
                    2 => tree.split(p(b), direction, p(a)),
                    3 => tree.remove(p(a)),
                    4 => tree.move_panel(p(a), p(b), Placement::Side(direction)),
                    5 => tree.move_panel(p(a), p(b), Placement::Tab),
                    6 => tree.cycle(p(a), d % 2 == 0).map(|_| ()),
                    _ => {
                        let delta = 37.0 * (d as f32 - 1.5);
                        tree.resize(p(a), direction.axis(), delta, AREA, 1.0, 20.0);
                        Ok(())
                    }
                };
                proptest::prop_assert!(tree.is_valid(), "{tree:?}");
                if result.is_err() {
                    proptest::prop_assert_eq!(&tree, &before, "une erreur ne change rien");
                }
            }
            let json = serde_json::to_string(&tree).unwrap();
            let back: LayoutTree = serde_json::from_str(&json).unwrap();
            proptest::prop_assert_eq!(back.panels(), tree.panels());
        }
    }

    #[test]
    fn cible_du_glisser_deposer() {
        let tree = two_columns();
        let area = Bounds::new(0.0, 0.0, 1000.0, 400.0);
        let target = |x, y| tree.drop_target(p(1), x, y, area, 0.0);
        // Centre de l'autre zone : en onglet.
        let center = target(750.0, 200.0).unwrap();
        assert_eq!(center.placement, Placement::Tab);
        assert_eq!(center.anchor, p(2));
        assert_eq!(center.preview, Bounds::new(500.0, 0.0, 500.0, 400.0));
        // Près du bord bas de l'autre zone : nouvelle zone en dessous.
        let below = target(750.0, 390.0).unwrap();
        assert_eq!(below.placement, Placement::Side(Direction::Down));
        assert_eq!(below.preview, Bounds::new(500.0, 200.0, 500.0, 200.0));
        assert_eq!(
            target(510.0, 200.0).unwrap().placement,
            Placement::Side(Direction::Left)
        );
        // Sa propre zone, où il est seul : rien à faire.
        assert_eq!(target(250.0, 200.0), None);
        assert_eq!(target(10.0, 200.0), None);
        assert_eq!(target(2000.0, 200.0), None, "hors des zones");
    }
}
