//! Une modification du texte : plage remplacée, texte retiré et texte inséré.

use std::ops::Range;

use crate::buffer::TextBuffer;
use crate::error::Result;

/// Côté vers lequel une position est poussée quand du texte est inséré exactement à cet endroit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    /// La position reste avant le texte inséré.
    Before,
    /// La position passe après le texte inséré (comportement d'un curseur qui tape).
    After,
}

/// Une modification : à la position `start`, le texte `removed` est remplacé par `inserted`.
///
/// Une modification conserve le texte retiré : elle peut donc être inversée, ce qui sert à
/// l'historique. Une insertion a un `removed` vide, une suppression un `inserted` vide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Position du début de la modification, en octets.
    pub start: usize,
    /// Texte retiré, présent à `start` avant la modification.
    pub removed: String,
    /// Texte inséré à `start`.
    pub inserted: String,
}

impl Edit {
    /// Insertion de `text` à `offset`.
    pub fn insert(offset: usize, text: impl Into<String>) -> Self {
        Self {
            start: offset,
            removed: String::new(),
            inserted: text.into(),
        }
    }

    /// Suppression de `range` ; le texte retiré est lu dans `buffer`.
    pub fn delete<B: TextBuffer + ?Sized>(buffer: &B, range: Range<usize>) -> Result<Self> {
        Self::replace(buffer, range, String::new())
    }

    /// Remplacement de `range` par `text` ; le texte retiré est lu dans `buffer`.
    pub fn replace<B: TextBuffer + ?Sized>(
        buffer: &B,
        range: Range<usize>,
        text: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            start: range.start,
            removed: buffer.slice(range)?,
            inserted: text.into(),
        })
    }

    /// Plage du texte retiré, avant la modification.
    pub fn removed_range(&self) -> Range<usize> {
        self.start..self.start + self.removed.len()
    }

    /// Plage du texte inséré, après la modification.
    pub fn inserted_range(&self) -> Range<usize> {
        self.start..self.start + self.inserted.len()
    }

    /// Indique si la modification ne change rien.
    pub fn is_noop(&self) -> bool {
        self.removed == self.inserted
    }

    /// Applique la modification à `buffer`.
    pub fn apply<B: TextBuffer + ?Sized>(&self, buffer: &mut B) -> Result<()> {
        debug_assert_eq!(
            buffer.slice(self.removed_range()).as_deref(),
            Ok(self.removed.as_str()),
            "le texte retiré ne correspond pas au buffer"
        );
        if !self.removed.is_empty() {
            buffer.remove(self.removed_range())?;
        }
        if !self.inserted.is_empty() {
            buffer.insert(self.start, &self.inserted)?;
        }
        Ok(())
    }

    /// Modification inverse : appliquée après celle-ci, elle restaure le texte d'origine.
    pub fn inverse(&self) -> Self {
        Self {
            start: self.start,
            removed: self.inserted.clone(),
            inserted: self.removed.clone(),
        }
    }

    /// Nouvelle valeur d'une position après la modification.
    ///
    /// - Avant la modification : inchangée.
    /// - Après le texte retiré : décalée de la différence de longueur.
    /// - Dans le texte retiré : ramenée au début (`Before`) ou après le texte inséré (`After`).
    /// - Au point d'une insertion pure : selon `assoc`.
    pub fn map_offset(&self, offset: usize, assoc: Assoc) -> usize {
        let old_end = self.start + self.removed.len();
        let new_end = self.start + self.inserted.len();
        if offset < self.start {
            offset
        } else if offset > old_end {
            offset - self.removed.len() + self.inserted.len()
        } else if offset == old_end && !self.removed.is_empty() {
            new_end
        } else if offset == self.start && !self.removed.is_empty() {
            self.start
        } else {
            match assoc {
                Assoc::Before => self.start,
                Assoc::After => new_end,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rope, StringBuffer};

    #[test]
    fn appliquer_puis_inverser() {
        let mut b = Rope::from_text("le vaste monde");
        let edit = Edit::replace(&b, 3..8, "petit").unwrap();
        assert_eq!(edit.removed, "vaste");
        edit.apply(&mut b).unwrap();
        assert_eq!(b.to_text(), "le petit monde");
        edit.inverse().apply(&mut b).unwrap();
        assert_eq!(b.to_text(), "le vaste monde");
    }

    #[test]
    fn insertion_et_suppression() {
        let mut b = StringBuffer::from_text("abc");
        Edit::insert(1, "xy").apply(&mut b).unwrap();
        assert_eq!(b.as_str(), "axybc");
        let del = Edit::delete(&b, 0..3).unwrap();
        assert_eq!(del.removed, "axy");
        del.apply(&mut b).unwrap();
        assert_eq!(b.as_str(), "bc");
        assert!(Edit::insert(0, "").is_noop());
    }

    #[test]
    fn decalage_des_positions() {
        // "0123456789" : remplace 3..6 par "ab".
        let edit = Edit {
            start: 3,
            removed: "345".into(),
            inserted: "ab".into(),
        };
        assert_eq!(edit.map_offset(2, Assoc::After), 2);
        assert_eq!(edit.map_offset(3, Assoc::After), 3);
        assert_eq!(edit.map_offset(4, Assoc::Before), 3);
        assert_eq!(edit.map_offset(4, Assoc::After), 5);
        assert_eq!(edit.map_offset(6, Assoc::Before), 5);
        assert_eq!(edit.map_offset(9, Assoc::Before), 8);

        let insert = Edit::insert(3, "xyz");
        assert_eq!(insert.map_offset(3, Assoc::Before), 3);
        assert_eq!(insert.map_offset(3, Assoc::After), 6);
        assert_eq!(insert.map_offset(4, Assoc::Before), 7);
    }
}
