//! Sélections multiples.
//!
//! Une [`Selection`] a une **ancre** (là où elle a commencé) et une **tête** (là où est le
//! curseur) ; un curseur est une sélection vide. Les [`Selections`] d'un document forment une
//! liste triée, sans chevauchement, avec une sélection principale : le multi-curseur est prévu
//! dès le départ.

use std::ops::Range;

use crate::edit::{Assoc, Edit};

/// Une sélection : de l'ancre à la tête, en octets. Vide, c'est un curseur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Selection {
    /// Position où la sélection a commencé ; reste fixe quand on étend la sélection.
    pub anchor: usize,
    /// Position du curseur.
    pub head: usize,
}

impl Selection {
    /// Sélection de `anchor` à `head`.
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    /// Curseur (sélection vide) à `offset`.
    pub fn cursor(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    /// Indique si la sélection est un simple curseur.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Début de la sélection (le plus petit de l'ancre et de la tête).
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    /// Fin de la sélection (le plus grand de l'ancre et de la tête).
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// Plage sélectionnée.
    pub fn range(&self) -> Range<usize> {
        self.start()..self.end()
    }

    /// Indique si la tête est après l'ancre (ou confondue avec elle).
    pub fn is_forward(&self) -> bool {
        self.head >= self.anchor
    }

    /// Sélection réduite à un curseur sur sa tête.
    pub fn collapse(self) -> Self {
        Self::cursor(self.head)
    }

    /// Sélection avec la même plage, orientée comme `forward`.
    fn oriented(start: usize, end: usize, forward: bool) -> Self {
        if forward {
            Self::new(start, end)
        } else {
            Self::new(end, start)
        }
    }

    /// Sélection après la modification `edit`.
    ///
    /// Un curseur passe après le texte inséré à sa position (il « tape »). Une sélection non vide
    /// n'absorbe pas le texte inséré juste avant ou juste après elle.
    pub fn map(self, edit: &Edit) -> Self {
        if self.is_empty() {
            return Self::cursor(edit.map_offset(self.head, Assoc::After));
        }
        let start = edit.map_offset(self.start(), Assoc::After);
        let end = edit.map_offset(self.end(), Assoc::Before).max(start);
        Self::oriented(start, end, self.is_forward())
    }
}

/// Liste non vide de sélections triées et sans chevauchement, avec une sélection principale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selections {
    ranges: Vec<Selection>,
    primary: usize,
}

impl Selections {
    /// Une seule sélection, principale.
    pub fn single(selection: Selection) -> Self {
        Self {
            ranges: vec![selection],
            primary: 0,
        }
    }

    /// Un seul curseur à `offset`.
    pub fn cursor(offset: usize) -> Self {
        Self::single(Selection::cursor(offset))
    }

    /// Sélections à partir d'une liste quelconque, dont `primary` est l'indice de la principale.
    /// La liste est triée et les sélections qui se chevauchent sont fusionnées. Renvoie `None`
    /// si la liste est vide.
    pub fn from_vec(ranges: Vec<Selection>, primary: usize) -> Option<Self> {
        if ranges.is_empty() {
            return None;
        }
        let primary = primary.min(ranges.len() - 1);
        let mut selections = Self { ranges, primary };
        selections.normalize();
        Some(selections)
    }

    /// Sélection principale.
    pub fn primary(&self) -> Selection {
        self.ranges[self.primary]
    }

    /// Indice de la sélection principale.
    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// Change la sélection principale (indice ramené dans la liste).
    pub fn set_primary(&mut self, index: usize) {
        self.primary = index.min(self.ranges.len() - 1);
    }

    /// Nombre de sélections (au moins une).
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    /// Toujours faux : il y a au moins une sélection.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Sélections, dans l'ordre du texte.
    pub fn as_slice(&self) -> &[Selection] {
        &self.ranges
    }

    /// Parcourt les sélections, dans l'ordre du texte.
    pub fn iter(&self) -> std::slice::Iter<'_, Selection> {
        self.ranges.iter()
    }

    /// Ajoute une sélection, qui devient la principale. Elle est fusionnée avec celles qu'elle
    /// chevauche.
    pub fn push(&mut self, selection: Selection) {
        self.ranges.push(selection);
        self.primary = self.ranges.len() - 1;
        self.normalize();
    }

    /// Ne garde que la sélection principale.
    pub fn keep_primary(&mut self) {
        let primary = self.primary();
        self.ranges = vec![primary];
        self.primary = 0;
    }

    /// Applique `f` à chaque sélection, puis trie et fusionne.
    pub fn transform(&mut self, mut f: impl FnMut(Selection) -> Selection) {
        for selection in &mut self.ranges {
            *selection = f(*selection);
        }
        self.normalize();
    }

    /// Décale toutes les sélections après la modification `edit`.
    pub fn map(&mut self, edit: &Edit) {
        self.transform(|s| s.map(edit));
    }

    /// Décale toutes les sélections après une suite de modifications appliquées dans l'ordre.
    pub fn map_all<'a>(&mut self, edits: impl IntoIterator<Item = &'a Edit>) {
        for edit in edits {
            self.map(edit);
        }
    }

    /// Trie les sélections et fusionne celles qui se chevauchent, en suivant la principale.
    fn normalize(&mut self) {
        let primary = self.ranges[self.primary];
        let mut indexed: Vec<(usize, Selection)> = self.ranges.drain(..).enumerate().collect();
        let primary_index = self.primary;
        indexed.sort_by_key(|&(_, s)| (s.start(), s.end()));

        let mut merged: Vec<Selection> = Vec::with_capacity(indexed.len());
        let mut new_primary = 0;
        for (index, selection) in indexed {
            let is_primary = index == primary_index;
            let last_index = merged.len().wrapping_sub(1);
            if let Some(last) = merged.last_mut() {
                let overlaps = selection.start() < last.end()
                    || (selection.start() == last.end()
                        && (last.is_empty() || selection.is_empty()));
                if overlaps {
                    // Le sens de la sélection principale l'emporte.
                    let forward = if is_primary {
                        primary.is_forward()
                    } else {
                        last.is_forward()
                    };
                    let end = last.end().max(selection.end());
                    *last = Selection::oriented(last.start(), end, forward);
                    if is_primary {
                        new_primary = last_index;
                    }
                    continue;
                }
            }
            merged.push(selection);
            if is_primary {
                new_primary = merged.len() - 1;
            }
        }
        self.ranges = merged;
        self.primary = new_primary;
    }
}

impl<'a> IntoIterator for &'a Selections {
    type Item = &'a Selection;
    type IntoIter = std::slice::Iter<'a, Selection>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rope, TextBuffer};

    fn sels(list: &[(usize, usize)], primary: usize) -> Selections {
        let ranges = list.iter().map(|&(a, h)| Selection::new(a, h)).collect();
        Selections::from_vec(ranges, primary).unwrap()
    }

    fn pairs(s: &Selections) -> Vec<(usize, usize)> {
        s.iter().map(|s| (s.anchor, s.head)).collect()
    }

    #[test]
    fn selection_de_base() {
        let s = Selection::new(5, 2);
        assert_eq!(s.range(), 2..5);
        assert!(!s.is_forward());
        assert!(!s.is_empty());
        assert_eq!(s.collapse(), Selection::cursor(2));
        assert!(Selection::cursor(3).is_empty());
    }

    #[test]
    fn liste_vide_refusee() {
        assert!(Selections::from_vec(Vec::new(), 0).is_none());
    }

    #[test]
    fn tri_et_principale_suivie() {
        let s = sels(&[(10, 12), (0, 2), (5, 5)], 0);
        assert_eq!(pairs(&s), vec![(0, 2), (5, 5), (10, 12)]);
        assert_eq!(s.primary(), Selection::new(10, 12));
        assert_eq!(s.primary_index(), 2);
    }

    #[test]
    fn fusion_des_chevauchements() {
        // Chevauchement, curseur dans une sélection, curseurs confondus, curseur collé.
        let s = sels(
            &[
                (0, 5),
                (3, 8),
                (6, 6),
                (10, 10),
                (10, 10),
                (12, 14),
                (14, 14),
            ],
            1,
        );
        assert_eq!(pairs(&s), vec![(0, 8), (10, 10), (12, 14)]);
        assert_eq!(s.primary_index(), 0);
        // Deux sélections non vides qui se touchent restent séparées.
        let s = sels(&[(0, 2), (2, 4)], 0);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn fusion_garde_le_sens_de_la_principale() {
        let s = sels(&[(0, 5), (8, 3)], 1);
        assert_eq!(pairs(&s), vec![(8, 0)]);
        assert_eq!(s.primary(), Selection::new(8, 0));
    }

    #[test]
    fn ajout_devient_principale() {
        let mut s = Selections::cursor(0);
        s.push(Selection::cursor(10));
        s.push(Selection::new(4, 6));
        assert_eq!(s.len(), 3);
        assert_eq!(s.primary(), Selection::new(4, 6));
        s.keep_primary();
        assert_eq!(pairs(&s), vec![(4, 6)]);
    }

    #[test]
    fn decalage_apres_insertion() {
        // Curseurs à 0, 4 et 8 dans "abc\ndef\nghi" ; on tape "X" au premier.
        let mut s = sels(&[(0, 0), (4, 4), (8, 8)], 0);
        s.map(&Edit::insert(0, "X"));
        assert_eq!(pairs(&s), vec![(1, 1), (5, 5), (9, 9)]);
    }

    #[test]
    fn frappe_multi_curseur() {
        let mut b = Rope::from_text("ab\ncd\nef");
        let mut s = sels(&[(0, 0), (3, 3), (6, 6)], 2);
        // Une modification par curseur, dans l'ordre : chaque modification décale les suivants.
        let mut edits = Vec::new();
        for i in 0..s.len() {
            let at = s.as_slice()[i].head;
            let edit = Edit::insert(at, "> ");
            edit.apply(&mut b).unwrap();
            s.map(&edit);
            edits.push(edit);
        }
        assert_eq!(b.to_text(), "> ab\n> cd\n> ef");
        assert_eq!(pairs(&s), vec![(2, 2), (7, 7), (12, 12)]);
        assert_eq!(s.primary_index(), 2);
    }

    #[test]
    fn selection_n_absorbe_pas_les_insertions_voisines() {
        let s = Selection::new(2, 5);
        assert_eq!(s.map(&Edit::insert(2, "xx")), Selection::new(4, 7));
        assert_eq!(s.map(&Edit::insert(5, "xx")), Selection::new(2, 5));
        assert_eq!(s.map(&Edit::insert(3, "xx")), Selection::new(2, 7));
    }

    #[test]
    fn suppression_englobante_fusionne() {
        let mut b = Rope::from_text("0123456789");
        let mut s = sels(&[(1, 1), (3, 4), (8, 8)], 0);
        let edit = Edit::delete(&b, 0..5).unwrap();
        edit.apply(&mut b).unwrap();
        s.map(&edit);
        assert_eq!(pairs(&s), vec![(0, 0), (3, 3)]);
    }

    #[test]
    fn remplacement_de_la_selection() {
        let b = Rope::from_text("le vaste monde");
        let s = Selection::new(3, 8);
        let edit = Edit::replace(&b, s.range(), "petit").unwrap();
        assert_eq!(s.map(&edit), Selection::new(3, 8));
        assert_eq!(Selection::new(8, 3).map(&edit), Selection::new(8, 3));
        let edit = Edit::replace(&b, s.range(), "x").unwrap();
        assert_eq!(s.map(&edit), Selection::new(3, 4));
    }
}
