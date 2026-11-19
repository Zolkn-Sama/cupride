//! Tests par propriétés : séquences aléatoires d'opérations, générées et réduites par `proptest`.
//!
//! Compilés avec les tests unitaires, donc avec des feuilles de rope minuscules : quelques
//! centaines d'octets suffisent à obtenir des arbres profonds et à exercer découpages et fusions.

use std::time::{Duration, Instant};

use proptest::prelude::*;

use crate::position::{
    offset_to_grapheme_point, offset_to_point, offset_to_utf16_point, point_to_offset,
};
use crate::{Edit, EditKind, History, Rope, Selections, StringBuffer, TextBuffer, Transaction};

/// Opération aléatoire ; les positions sont ramenées dans le texte au moment de l'appliquer.
#[derive(Debug, Clone)]
enum Op {
    Insert { at: usize, text: String },
    Remove { a: usize, b: usize },
}

/// Texte court mêlant ASCII, multi-octets, emojis, accents combinants et fins de ligne.
fn text() -> impl Strategy<Value = String> {
    "(a|b|z| |é|日|😀|e\u{301}|\n|\r\n|\t){0,40}"
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (any::<usize>(), text()).prop_map(|(at, text)| Op::Insert { at, text }),
        2 => (any::<usize>(), any::<usize>()).prop_map(|(a, b)| Op::Remove { a, b }),
    ]
}

/// Ramène `raw` sur une frontière de caractère de `text`.
fn boundary(text: &str, raw: usize) -> usize {
    let mut offset = raw % (text.len() + 1);
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Vérifie que la rope et la référence répondent de la même façon à toutes les requêtes.
fn assert_equivalent(rope: &Rope, reference: &StringBuffer) -> Result<(), TestCaseError> {
    let text = reference.as_str();
    prop_assert_eq!(rope.to_text(), text);
    prop_assert_eq!(rope.len_bytes(), reference.len_bytes());
    prop_assert_eq!(rope.len_lines(), reference.len_lines());
    prop_assert_eq!(
        rope.lines().collect::<Vec<_>>(),
        reference.lines().collect::<Vec<_>>()
    );
    for line in 0..reference.len_lines() {
        prop_assert_eq!(rope.line_to_byte(line), reference.line_to_byte(line));
        prop_assert_eq!(rope.line(line), reference.line(line));
    }
    for offset in (0..=text.len()).filter(|&o| text.is_char_boundary(o)) {
        prop_assert_eq!(rope.byte_to_line(offset), reference.byte_to_line(offset));
    }
    if let Err(error) = rope.check_invariants() {
        return Err(TestCaseError::fail(error));
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// La rope et l'implémentation de référence donnent toujours le même résultat.
    #[test]
    fn rope_equivalente_a_la_reference(initial in text(), ops in prop::collection::vec(op(), 1..60)) {
        let mut rope = Rope::from_text(&initial);
        let mut reference = StringBuffer::from_text(&initial);
        for op in ops {
            let current = reference.as_str().to_owned();
            match op {
                Op::Insert { at, text } => {
                    let at = boundary(&current, at);
                    prop_assert_eq!(rope.insert(at, &text), reference.insert(at, &text));
                }
                Op::Remove { a, b } => {
                    let (a, b) = (boundary(&current, a), boundary(&current, b));
                    let range = a.min(b)..a.max(b);
                    prop_assert_eq!(rope.remove(range.clone()), reference.remove(range));
                }
            }
            prop_assert_eq!(rope.len_bytes(), reference.len_bytes());
            if let Err(error) = rope.check_invariants() {
                return Err(TestCaseError::fail(error));
            }
        }
        assert_equivalent(&rope, &reference)?;
    }

    /// Les positions brutes (y compris invalides) donnent les mêmes erreurs des deux côtés.
    #[test]
    fn memes_erreurs_que_la_reference(initial in text(), offset in 0usize..200, line in 0usize..20) {
        let mut rope = Rope::from_text(&initial);
        let mut reference = StringBuffer::from_text(&initial);
        prop_assert_eq!(rope.byte_to_line(offset), reference.byte_to_line(offset));
        prop_assert_eq!(rope.line_to_byte(line), reference.line_to_byte(line));
        prop_assert_eq!(rope.slice(offset / 2..offset), reference.slice(offset / 2..offset));
        prop_assert_eq!(rope.insert(offset, "x"), reference.insert(offset, "x"));
        prop_assert_eq!(rope.to_text(), reference.as_str());
    }

    /// Les conversions de positions sont identiques sur la rope et la référence, et réversibles.
    #[test]
    fn conversions_identiques_et_reversibles(initial in text(), suffix in text()) {
        // Assez de texte pour que la rope ait plusieurs niveaux.
        let full = format!("{}{initial}{suffix}", "x\n".repeat(30));
        let rope = Rope::from_text(&full);
        let reference = StringBuffer::from_text(&full);
        for offset in (0..=full.len()).filter(|&o| full.is_char_boundary(o)) {
            let point = offset_to_point(&rope, offset).unwrap();
            prop_assert_eq!(Ok(point), offset_to_point(&reference, offset));
            prop_assert_eq!(point_to_offset(&rope, point), Ok(offset));
            prop_assert_eq!(
                offset_to_grapheme_point(&rope, offset),
                offset_to_grapheme_point(&reference, offset)
            );
            prop_assert_eq!(
                offset_to_utf16_point(&rope, offset),
                offset_to_utf16_point(&reference, offset)
            );
        }
    }

    /// Après des modifications aléatoires, tout annuler ramène au texte initial, et tout
    /// rétablir ramène au texte final, quel que soit le regroupement des transactions.
    #[test]
    fn tout_annuler_ramene_au_texte_initial(
        initial in text(),
        steps in prop::collection::vec((op(), 0u8..3, 0u64..1_500), 1..60),
    ) {
        let mut rope = Rope::from_text(&initial);
        let mut history = History::new();
        let mut now = Instant::now();
        let mut selections = Selections::cursor(0);
        for (op, kind, pause_ms) in steps {
            let current = rope.to_text();
            let kind = [EditKind::Insert, EditKind::Delete, EditKind::Other][kind as usize];
            let edit = match op {
                Op::Insert { at, text } => Edit::insert(boundary(&current, at), text),
                Op::Remove { a, b } => {
                    let (a, b) = (boundary(&current, a), boundary(&current, b));
                    Edit::delete(&rope, a.min(b)..a.max(b)).unwrap()
                }
            };
            let mut tx = Transaction::with_kind(kind, selections.clone());
            tx.apply(&mut rope, edit).unwrap();
            selections = tx.selections_after().clone();
            now += Duration::from_millis(pause_ms);
            history.commit_at(tx, now);
        }
        let final_text = rope.to_text();
        while history.undo(&mut rope).unwrap().is_some() {}
        prop_assert_eq!(rope.to_text(), initial);
        while history.redo(&mut rope).unwrap().is_some() {}
        prop_assert_eq!(rope.to_text(), final_text);
        if let Err(error) = rope.check_invariants() {
            return Err(TestCaseError::fail(error));
        }
    }
}
