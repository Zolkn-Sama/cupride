//! Modifications du texte, traduites pour Tree-sitter.

use cupride_text::{Edit, Rope, TextBuffer};
use tree_sitter::{InputEdit, Point};

/// Au-delà, une transaction (un « tout remplacer », par exemple) est suivie d'une analyse
/// complète plutôt que de la mise à jour de l'arbre, modification par modification.
pub const MAX_INCREMENTAL_EDITS: usize = 512;

/// Modifications `applied`, appliquées dans l'ordre pour arriver au texte `text`, traduites pour
/// Tree-sitter.
///
/// Tree-sitter veut la ligne et la colonne du début de chaque modification dans le texte tel
/// qu'il était juste avant elle : le texte est ramené à son état d'avant les modifications (en
/// appliquant leurs inverses), puis elles sont rejouées une à une. `text` est identique à la fin.
/// `None` si les modifications ne correspondent pas au texte, ou sont trop nombreuses
/// ([`MAX_INCREMENTAL_EDITS`]).
pub fn input_edits(text: &mut Rope, applied: &[Edit]) -> Option<Vec<InputEdit>> {
    if applied.len() > MAX_INCREMENTAL_EDITS {
        return None;
    }
    // Une seule modification (le cas de la frappe) : le texte avant son début n'a pas changé.
    if let [edit] = applied {
        let start = point_at(text, edit.start)?;
        return Some(vec![input_edit(edit, start)]);
    }
    let mut undone = 0;
    for edit in applied.iter().rev() {
        if edit.inverse().apply(text).is_err() {
            break;
        }
        undone += 1;
    }
    let mut edits = Vec::with_capacity(applied.len());
    let mut valid = undone == applied.len();
    for edit in &applied[applied.len() - undone..] {
        if valid {
            match point_at(text, edit.start) {
                Some(start) => edits.push(input_edit(edit, start)),
                None => valid = false,
            }
        }
        // Le texte doit revenir à son état final, quoi qu'il arrive.
        if edit.apply(text).is_err() {
            tracing::error!("modification impossible à rejouer : texte de l'arbre syntaxique");
            return None;
        }
    }
    valid.then_some(edits)
}

fn input_edit(edit: &Edit, start: Point) -> InputEdit {
    InputEdit {
        start_byte: edit.start,
        old_end_byte: edit.start + edit.removed.len(),
        new_end_byte: edit.start + edit.inserted.len(),
        start_position: start,
        old_end_position: advance(start, &edit.removed),
        new_end_position: advance(start, &edit.inserted),
    }
}

/// Ligne et colonne (en octets) de la position `byte`.
pub(crate) fn point_at(text: &Rope, byte: usize) -> Option<Point> {
    let row = text.byte_to_line(byte).ok()?;
    let column = byte - text.line_to_byte(row).ok()?;
    Some(Point { row, column })
}

/// Position après le texte `inserted`, inséré à `start`.
fn advance(start: Point, inserted: &str) -> Point {
    match inserted.rfind('\n') {
        Some(last) => Point {
            row: start.row + inserted.matches('\n').count(),
            column: inserted.len() - last - 1,
        },
        None => Point {
            row: start.row,
            column: start.column + inserted.len(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_modification() {
        let mut text = Rope::from_text("ab\ncd\nef");
        let edit = Edit::replace(&text, 4..5, "X\nY").unwrap();
        edit.apply(&mut text).unwrap();
        let edits = input_edits(&mut text, &[edit]).unwrap();
        assert_eq!(
            edits,
            [InputEdit {
                start_byte: 4,
                old_end_byte: 5,
                new_end_byte: 7,
                start_position: Point { row: 1, column: 1 },
                old_end_position: Point { row: 1, column: 2 },
                new_end_position: Point { row: 2, column: 1 },
            }]
        );
    }

    #[test]
    fn positions_dans_le_texte_d_avant_chaque_modification() {
        let mut text = Rope::from_text("un\ndeux\ntrois");
        // Ligne insérée au début, puis modification de « trois » : sa ligne était 3 au moment
        // de la seconde modification.
        let first = Edit::insert(0, "zéro\n");
        first.apply(&mut text).unwrap();
        let second = Edit::insert(text.line_to_byte(3).unwrap(), "* ");
        second.apply(&mut text).unwrap();
        let final_text = text.to_text();
        let edits = input_edits(&mut text, &[first, second]).unwrap();
        assert_eq!(text.to_text(), final_text, "texte inchangé à la fin");
        assert_eq!(edits[0].start_position, Point { row: 0, column: 0 });
        assert_eq!(edits[0].new_end_position, Point { row: 1, column: 0 });
        assert_eq!(edits[1].start_position, Point { row: 3, column: 0 });
        assert_eq!(edits[1].new_end_byte, edits[1].start_byte + 2);
    }

    #[test]
    fn trop_de_modifications() {
        let mut text = Rope::from_text(&"a".repeat(1000));
        let edits: Vec<_> = (0..=MAX_INCREMENTAL_EDITS)
            .map(|i| Edit::insert(i, "b"))
            .collect();
        for edit in &edits {
            edit.apply(&mut text).unwrap();
        }
        assert!(input_edits(&mut text, &edits).is_none());
    }
}
