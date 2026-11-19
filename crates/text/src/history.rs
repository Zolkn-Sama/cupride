//! Historique des modifications : annuler et rétablir.
//!
//! Une [`Transaction`] regroupe les modifications d'une action (une frappe, une commande) et les
//! sélections avant et après. L'[`History`] empile les transactions : annuler applique l'inverse
//! de la dernière et restaure les sélections d'avant ; rétablir la réapplique.
//!
//! ```
//! use cupride_text::{Edit, History, Rope, Selections, TextBuffer, Transaction};
//!
//! let mut buffer = Rope::from_text("monde");
//! let mut history = History::new();
//!
//! let mut tx = Transaction::new(Selections::cursor(0));
//! tx.apply(&mut buffer, Edit::insert(0, "le "))?;
//! history.commit(tx);
//! assert_eq!(buffer.to_text(), "le monde");
//!
//! let selections = history.undo(&mut buffer)?;
//! assert_eq!(buffer.to_text(), "monde");
//! assert_eq!(selections, Some(Selections::cursor(0)));
//!
//! history.redo(&mut buffer)?;
//! assert_eq!(buffer.to_text(), "le monde");
//! # Ok::<(), cupride_text::TextError>(())
//! ```

use crate::buffer::TextBuffer;
use crate::edit::Edit;
use crate::error::Result;
use crate::selection::Selections;

/// Modifications d'une même action, annulées et rétablies ensemble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    edits: Vec<Edit>,
    selections_before: Selections,
    selections_after: Selections,
}

impl Transaction {
    /// Commence une transaction ; `selections` sont les sélections avant la première modification.
    pub fn new(selections: Selections) -> Self {
        Self {
            edits: Vec::new(),
            selections_after: selections.clone(),
            selections_before: selections,
        }
    }

    /// Applique `edit` à `buffer`, l'enregistre et décale les sélections d'après.
    pub fn apply<B: TextBuffer + ?Sized>(&mut self, buffer: &mut B, edit: Edit) -> Result<()> {
        edit.apply(buffer)?;
        self.selections_after.map(&edit);
        self.edits.push(edit);
        Ok(())
    }

    /// Remplace les sélections d'après, par exemple pour réduire une sélection remplacée à un
    /// curseur.
    pub fn set_selections_after(&mut self, selections: Selections) {
        self.selections_after = selections;
    }

    /// Modifications, dans l'ordre où elles ont été appliquées.
    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    /// Sélections avant la transaction.
    pub fn selections_before(&self) -> &Selections {
        &self.selections_before
    }

    /// Sélections après la transaction.
    pub fn selections_after(&self) -> &Selections {
        &self.selections_after
    }

    /// Indique si la transaction ne contient aucune modification.
    pub fn is_empty(&self) -> bool {
        self.edits.iter().all(Edit::is_noop)
    }

    /// Annule la transaction : inverses des modifications, de la dernière à la première.
    fn revert<B: TextBuffer + ?Sized>(&self, buffer: &mut B) -> Result<()> {
        self.edits
            .iter()
            .rev()
            .try_for_each(|edit| edit.inverse().apply(buffer))
    }

    /// Réapplique la transaction.
    fn replay<B: TextBuffer + ?Sized>(&self, buffer: &mut B) -> Result<()> {
        self.edits.iter().try_for_each(|edit| edit.apply(buffer))
    }
}

/// Piles d'annulation et de rétablissement d'un document.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
}

impl History {
    /// Historique vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enregistre une transaction déjà appliquée. Une nouvelle modification vide la pile de
    /// rétablissement ; une transaction vide est ignorée.
    pub fn commit(&mut self, transaction: Transaction) {
        if transaction.is_empty() {
            return;
        }
        self.redo.clear();
        self.undo.push(transaction);
    }

    /// Annule la dernière transaction. Renvoie les sélections à restaurer, ou `None` s'il n'y a
    /// rien à annuler.
    pub fn undo<B: TextBuffer + ?Sized>(&mut self, buffer: &mut B) -> Result<Option<Selections>> {
        let Some(transaction) = self.undo.pop() else {
            return Ok(None);
        };
        if let Err(error) = transaction.revert(buffer) {
            self.undo.push(transaction);
            return Err(error);
        }
        let selections = transaction.selections_before.clone();
        self.redo.push(transaction);
        Ok(Some(selections))
    }

    /// Rétablit la dernière transaction annulée. Renvoie les sélections à restaurer, ou `None`
    /// s'il n'y a rien à rétablir.
    pub fn redo<B: TextBuffer + ?Sized>(&mut self, buffer: &mut B) -> Result<Option<Selections>> {
        let Some(transaction) = self.redo.pop() else {
            return Ok(None);
        };
        if let Err(error) = transaction.replay(buffer) {
            self.redo.push(transaction);
            return Err(error);
        }
        let selections = transaction.selections_after.clone();
        self.undo.push(transaction);
        Ok(Some(selections))
    }

    /// Indique s'il y a une transaction à annuler.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Indique s'il y a une transaction à rétablir.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Nombre de transactions annulables.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Vide l'historique.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suite::Lcg;
    use crate::{Rope, Selection};

    /// Tape `text` à chaque curseur, comme le ferait l'éditeur.
    fn type_text(buffer: &mut Rope, selections: &mut Selections, text: &str) -> Transaction {
        let mut tx = Transaction::new(selections.clone());
        for i in 0..selections.len() {
            let range = tx.selections_after().as_slice()[i].range();
            let edit = Edit::replace(buffer, range, text).unwrap();
            tx.apply(buffer, edit).unwrap();
        }
        *selections = tx.selections_after().clone();
        tx
    }

    #[test]
    fn annuler_et_retablir() {
        let mut b = Rope::from_text("abc");
        let mut s = Selections::cursor(3);
        let mut h = History::new();
        h.commit(type_text(&mut b, &mut s, "d"));
        h.commit(type_text(&mut b, &mut s, "e"));
        assert_eq!(b.to_text(), "abcde");
        assert_eq!(h.undo(&mut b).unwrap(), Some(Selections::cursor(4)));
        assert_eq!(b.to_text(), "abcd");
        assert_eq!(h.undo(&mut b).unwrap(), Some(Selections::cursor(3)));
        assert_eq!(b.to_text(), "abc");
        assert_eq!(h.undo(&mut b).unwrap(), None);
        assert_eq!(h.redo(&mut b).unwrap(), Some(Selections::cursor(4)));
        assert_eq!(h.redo(&mut b).unwrap(), Some(Selections::cursor(5)));
        assert_eq!(h.redo(&mut b).unwrap(), None);
        assert_eq!(b.to_text(), "abcde");
    }

    #[test]
    fn nouvelle_modification_vide_le_retablissement() {
        let mut b = Rope::from_text("");
        let mut s = Selections::cursor(0);
        let mut h = History::new();
        h.commit(type_text(&mut b, &mut s, "a"));
        h.undo(&mut b).unwrap();
        assert!(h.can_redo());
        s = Selections::cursor(0);
        h.commit(type_text(&mut b, &mut s, "b"));
        assert!(!h.can_redo());
        assert_eq!(h.redo(&mut b).unwrap(), None);
        assert_eq!(b.to_text(), "b");
    }

    #[test]
    fn selections_restaurees() {
        let mut b = Rope::from_text("un deux trois");
        let before =
            Selections::from_vec(vec![Selection::new(0, 2), Selection::new(8, 13)], 1).unwrap();
        let mut s = before.clone();
        let mut h = History::new();
        let mut tx = type_text(&mut b, &mut s, "X");
        let collapsed = Selections::from_vec(
            tx.selections_after()
                .iter()
                .map(|s| Selection::cursor(s.end()))
                .collect(),
            1,
        )
        .unwrap();
        tx.set_selections_after(collapsed.clone());
        h.commit(tx);
        assert_eq!(b.to_text(), "X deux X");
        assert_eq!(h.undo(&mut b).unwrap(), Some(before));
        assert_eq!(b.to_text(), "un deux trois");
        assert_eq!(h.redo(&mut b).unwrap(), Some(collapsed));
    }

    #[test]
    fn transaction_vide_ignoree() {
        let mut h = History::new();
        h.commit(Transaction::new(Selections::cursor(0)));
        assert!(!h.can_undo());
    }

    #[test]
    fn aller_retour_complet() {
        let initial = "fn main() {\r\n    println!(\"é😀\");\n}\n".repeat(20);
        let mut b = Rope::from_text(&initial);
        let mut h = History::new();
        let mut rng = Lcg::new(99);
        for _ in 0..500 {
            let text = b.to_text();
            let a = rng.boundary(&text);
            let c = rng.boundary(&text);
            let range = a.min(c)..a.max(c);
            let mut tx = Transaction::new(Selections::cursor(range.start));
            let edit = if rng.below(1) == 0 {
                Edit::replace(&b, range, rng.text(10)).unwrap()
            } else {
                Edit::delete(&b, range).unwrap()
            };
            tx.apply(&mut b, edit).unwrap();
            h.commit(tx);
        }
        let final_text = b.to_text();
        while h.undo(&mut b).unwrap().is_some() {}
        assert_eq!(b.to_text(), initial);
        while h.redo(&mut b).unwrap().is_some() {}
        assert_eq!(b.to_text(), final_text);
    }
}
