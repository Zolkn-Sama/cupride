//! Historique des modifications : annuler et rétablir.
//!
//! Une [`Transaction`] regroupe les modifications d'une action (une frappe, une commande) et les
//! sélections avant et après. L'[`History`] empile les transactions : annuler applique l'inverse
//! de la dernière et restaure les sélections d'avant ; rétablir la réapplique.
//!
//! # Regroupement
//!
//! Pour qu'annuler revienne par blocs logiques et non lettre par lettre, les transactions de
//! frappe ([`EditKind::Insert`]) ou d'effacement ([`EditKind::Delete`]) consécutives sont
//! fusionnées. Le bloc est coupé :
//!
//! - après une pause plus longue que le délai de regroupement ([`History::set_group_delay`]) ;
//! - quand le curseur a bougé entre deux transactions (les sélections ne se suivent plus) ;
//! - quand le type d'action change (frappe puis effacement) ;
//! - explicitement, avec [`History::break_group`].
//!
//! Une transaction [`EditKind::Other`] (une commande, un remplacement global…) n'est jamais
//! fusionnée : elle forme toujours un bloc à elle seule, quel que soit son nombre de modifications.
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

use std::time::{Duration, Instant};

use crate::buffer::TextBuffer;
use crate::edit::Edit;
use crate::error::Result;
use crate::selection::Selections;

/// Délai de regroupement par défaut : au-delà, une nouvelle frappe commence un nouveau bloc.
pub const DEFAULT_GROUP_DELAY: Duration = Duration::from_millis(1_000);

/// Type d'action d'une transaction, qui décide si elle peut être fusionnée avec la précédente.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EditKind {
    /// Frappe de texte : fusionnée avec les frappes consécutives.
    Insert,
    /// Effacement (retour arrière, suppr) : fusionné avec les effacements consécutifs.
    Delete,
    /// Toute autre action (commande, collage, remplacement global) : jamais fusionnée.
    #[default]
    Other,
}

impl EditKind {
    fn is_groupable(self) -> bool {
        matches!(self, Self::Insert | Self::Delete)
    }
}

/// Modifications d'une même action, annulées et rétablies ensemble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    kind: EditKind,
    edits: Vec<Edit>,
    selections_before: Selections,
    selections_after: Selections,
}

impl Transaction {
    /// Commence une transaction de type [`EditKind::Other`] ; `selections` sont les sélections
    /// avant la première modification.
    pub fn new(selections: Selections) -> Self {
        Self::with_kind(EditKind::Other, selections)
    }

    /// Commence une transaction du type donné.
    pub fn with_kind(kind: EditKind, selections: Selections) -> Self {
        Self {
            kind,
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

    /// Type d'action.
    pub fn kind(&self) -> EditKind {
        self.kind
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
#[derive(Debug, Clone)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    group_delay: Duration,
    /// Instant de la dernière transaction enregistrée, si son bloc est encore ouvert.
    open_group: Option<Instant>,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    /// Historique vide, avec le délai de regroupement par défaut.
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            group_delay: DEFAULT_GROUP_DELAY,
            open_group: None,
        }
    }

    /// Délai de regroupement : une pause plus longue entre deux frappes coupe le bloc.
    pub fn group_delay(&self) -> Duration {
        self.group_delay
    }

    /// Change le délai de regroupement. `Duration::ZERO` désactive le regroupement par le temps.
    pub fn set_group_delay(&mut self, delay: Duration) {
        self.group_delay = delay;
    }

    /// Coupe le bloc en cours : la prochaine transaction ne sera pas fusionnée avec la précédente.
    pub fn break_group(&mut self) {
        self.open_group = None;
    }

    /// Enregistre une transaction déjà appliquée, à l'instant présent. Voir [`History::commit_at`].
    pub fn commit(&mut self, transaction: Transaction) {
        self.commit_at(transaction, Instant::now());
    }

    /// Enregistre une transaction déjà appliquée, à l'instant `now`.
    ///
    /// Une nouvelle modification vide la pile de rétablissement ; une transaction vide est
    /// ignorée. La transaction est fusionnée avec la précédente si elles forment un même bloc
    /// (voir le [regroupement](self#regroupement)).
    pub fn commit_at(&mut self, transaction: Transaction, now: Instant) {
        if transaction.is_empty() {
            return;
        }
        self.redo.clear();
        let groupable = transaction.kind.is_groupable();
        let merge_into = match (self.open_group, self.undo.last_mut()) {
            (Some(last_time), Some(last))
                if groupable
                    && last.kind == transaction.kind
                    && now.saturating_duration_since(last_time) <= self.group_delay
                    && last.selections_after == transaction.selections_before =>
            {
                Some(last)
            }
            _ => None,
        };
        match merge_into {
            Some(last) => {
                last.edits.extend(transaction.edits);
                last.selections_after = transaction.selections_after;
            }
            None => self.undo.push(transaction),
        }
        self.open_group = groupable.then_some(now);
    }

    /// Annule la dernière transaction. Renvoie les sélections à restaurer, ou `None` s'il n'y a
    /// rien à annuler.
    pub fn undo<B: TextBuffer + ?Sized>(&mut self, buffer: &mut B) -> Result<Option<Selections>> {
        self.break_group();
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
        self.break_group();
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
        self.break_group();
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

    /// Tape un caractère au curseur, comme une frappe clavier, à l'instant `at`.
    fn key(b: &mut Rope, s: &mut Selections, h: &mut History, text: &str, at: Instant) {
        let mut tx = Transaction::with_kind(EditKind::Insert, s.clone());
        let at_cursor = s.primary().head;
        tx.apply(b, Edit::insert(at_cursor, text)).unwrap();
        *s = tx.selections_after().clone();
        h.commit_at(tx, at);
    }

    /// Retour arrière d'un caractère ASCII au curseur, à l'instant `at`.
    fn backspace(b: &mut Rope, s: &mut Selections, h: &mut History, at: Instant) {
        let mut tx = Transaction::with_kind(EditKind::Delete, s.clone());
        let head = s.primary().head;
        let edit = Edit::delete(b, head - 1..head).unwrap();
        tx.apply(b, edit).unwrap();
        *s = tx.selections_after().clone();
        h.commit_at(tx, at);
    }

    #[test]
    fn frappes_consecutives_regroupees() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut b = Rope::from_text("");
        let mut s = Selections::cursor(0);
        let mut h = History::new();
        for (i, c) in ["b", "o", "n", "j", "o", "u", "r"].iter().enumerate() {
            key(&mut b, &mut s, &mut h, c, t0 + ms(100 * i as u64));
        }
        assert_eq!(h.undo_len(), 1);
        h.undo(&mut b).unwrap();
        assert_eq!(b.to_text(), "");
    }

    #[test]
    fn coupure_apres_une_pause() {
        let t0 = Instant::now();
        let mut b = Rope::from_text("");
        let mut s = Selections::cursor(0);
        let mut h = History::new();
        h.set_group_delay(Duration::from_millis(300));
        key(&mut b, &mut s, &mut h, "a", t0);
        key(&mut b, &mut s, &mut h, "b", t0 + Duration::from_millis(200));
        key(&mut b, &mut s, &mut h, "c", t0 + Duration::from_millis(900));
        assert_eq!(h.undo_len(), 2);
        h.undo(&mut b).unwrap();
        assert_eq!(b.to_text(), "ab");
    }

    #[test]
    fn coupure_apres_un_deplacement() {
        let t0 = Instant::now();
        let mut b = Rope::from_text("");
        let mut s = Selections::cursor(0);
        let mut h = History::new();
        key(&mut b, &mut s, &mut h, "a", t0);
        key(&mut b, &mut s, &mut h, "b", t0);
        // Le curseur revient au début : la frappe suivante ne prolonge pas le bloc.
        s = Selections::cursor(0);
        key(&mut b, &mut s, &mut h, "c", t0);
        assert_eq!(b.to_text(), "cab");
        assert_eq!(h.undo_len(), 2);
        h.undo(&mut b).unwrap();
        assert_eq!(b.to_text(), "ab");
    }

    #[test]
    fn coupure_au_changement_de_type() {
        let t0 = Instant::now();
        let mut b = Rope::from_text("");
        let mut s = Selections::cursor(0);
        let mut h = History::new();
        key(&mut b, &mut s, &mut h, "a", t0);
        key(&mut b, &mut s, &mut h, "b", t0);
        backspace(&mut b, &mut s, &mut h, t0);
        backspace(&mut b, &mut s, &mut h, t0);
        key(&mut b, &mut s, &mut h, "c", t0);
        assert_eq!(h.undo_len(), 3);
        h.undo(&mut b).unwrap();
        assert_eq!(b.to_text(), "");
        h.undo(&mut b).unwrap();
        assert_eq!(b.to_text(), "ab");
        h.undo(&mut b).unwrap();
        assert_eq!(b.to_text(), "");
    }

    #[test]
    fn coupure_explicite_et_apres_annulation() {
        let t0 = Instant::now();
        let mut b = Rope::from_text("");
        let mut s = Selections::cursor(0);
        let mut h = History::new();
        key(&mut b, &mut s, &mut h, "a", t0);
        h.break_group();
        key(&mut b, &mut s, &mut h, "b", t0);
        assert_eq!(h.undo_len(), 2);
        s = h.undo(&mut b).unwrap().unwrap();
        key(&mut b, &mut s, &mut h, "c", t0);
        assert_eq!(h.undo_len(), 2);
    }

    #[test]
    fn commande_a_plusieurs_modifications_en_une_transaction() {
        let t0 = Instant::now();
        let mut b = Rope::from_text("un chat, un chien, un chat");
        let mut s = Selections::cursor(0);
        let mut h = History::new();
        key(&mut b, &mut s, &mut h, ">", t0);
        // Remplacement global : toutes les occurrences dans une seule transaction.
        let mut tx = Transaction::new(s.clone());
        while let Some(pos) = b.to_text().find("chat") {
            let edit = Edit::replace(&b, pos..pos + 4, "loup").unwrap();
            tx.apply(&mut b, edit).unwrap();
        }
        assert_eq!(tx.edits().len(), 2);
        h.commit_at(tx, t0);
        // Une frappe juste après n'est pas fusionnée avec la commande.
        let mut s2 = s.clone();
        key(&mut b, &mut s2, &mut h, "!", t0);
        assert_eq!(b.to_text(), ">!un loup, un chien, un loup");
        assert_eq!(h.undo_len(), 3);
        h.undo(&mut b).unwrap();
        h.undo(&mut b).unwrap();
        assert_eq!(b.to_text(), ">un chat, un chien, un chat");
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
