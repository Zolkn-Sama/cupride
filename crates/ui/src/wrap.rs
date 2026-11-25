//! Retour à la ligne visuel : chaque ligne trop large est découpée en rangées.
//!
//! La découpe se fait en colonnes (police à chasse fixe) : un graphème compte pour une colonne,
//! un caractère large (CJK, émoji) pour deux, une tabulation va jusqu'au prochain arrêt. Une
//! rangée se coupe de préférence après une espace ; un mot plus long que la rangée est coupé où
//! il faut. La découpe ne dépend que du texte : elle se calcule sans mettre en forme les lignes,
//! pour tout le document.

use std::ops::Range;

use cupride_core::text::TextBuffer;
use unicode_segmentation::UnicodeSegmentation;

/// Au-delà de cette taille, le retour à la ligne n'est pas appliqué : la découpe de tout le
/// document à chaque modification coûterait trop cher.
pub const MAX_WRAPPED_BYTES: usize = 4 * 1024 * 1024;

/// Le caractère occupe-t-il deux colonnes ?
fn is_wide(c: char) -> bool {
    matches!(u32::from(c),
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD)
}

/// Rangées de la ligne `text` (sans fin de ligne) pour une largeur de `columns` colonnes ; une
/// ligne vide en a une.
pub fn segments(text: &str, columns: usize, tab: usize) -> Vec<Range<usize>> {
    let columns = columns.max(1);
    let tab = tab.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    let mut column = 0;
    // Dernière coupure possible dans la rangée : après une espace (position, colonne).
    let mut break_at: Option<(usize, usize)> = None;
    for (index, grapheme) in text.grapheme_indices(true) {
        let width = if grapheme == "\t" {
            tab - column % tab
        } else if grapheme.chars().any(is_wide) {
            2
        } else {
            1
        };
        // Une espace qui déborde reste en fin de rangée : elle ne provoque pas de coupure.
        let blank = grapheme == " " || grapheme == "\t";
        if column + width > columns && index > start && !blank {
            match break_at.filter(|(at, _)| *at > start) {
                Some((at, at_column)) => {
                    rows.push(start..at);
                    start = at;
                    column -= at_column;
                }
                None => {
                    rows.push(start..index);
                    start = index;
                    column = 0;
                }
            }
            break_at = None;
        }
        column += width;
        if blank {
            break_at = Some((index + grapheme.len(), column));
        }
    }
    rows.push(start..text.len());
    rows
}

/// Découpe de tout un document : première rangée de chaque ligne.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapping {
    /// Largeur, en colonnes.
    pub columns: usize,
    /// Largeur d'une tabulation.
    pub tab: usize,
    /// Révision du document découpé.
    pub revision: u64,
    /// `first_row[l]` : première rangée de la ligne `l` ; la dernière valeur est le nombre total
    /// de rangées.
    first_row: Vec<usize>,
}

impl Wrapping {
    /// Découpe le document `buffer` (révision `revision`).
    pub fn new(buffer: &impl TextBuffer, revision: u64, columns: usize, tab: usize) -> Self {
        let lines = buffer.len_lines();
        let mut first_row = Vec::with_capacity(lines + 1);
        let mut row = 0;
        for line in 0..lines {
            first_row.push(row);
            let text = buffer.line(line).unwrap_or_default();
            row += segments(&text, columns, tab).len();
        }
        first_row.push(row);
        Self {
            columns,
            tab,
            revision,
            first_row,
        }
    }

    /// Nombre de rangées.
    pub fn rows(&self) -> usize {
        self.first_row.last().copied().unwrap_or(0)
    }

    /// Nombre de lignes découpées.
    pub fn lines(&self) -> usize {
        self.first_row.len().saturating_sub(1)
    }

    /// Première rangée de la ligne `line`.
    pub fn first_row(&self, line: usize) -> usize {
        self.first_row[line.min(self.lines())]
    }

    /// Ligne de la rangée `row`.
    pub fn line_of(&self, row: usize) -> usize {
        self.first_row
            .partition_point(|&r| r <= row)
            .saturating_sub(1)
            .min(self.lines().saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cupride_core::text::Rope;

    fn rows(text: &str, columns: usize) -> Vec<&str> {
        segments(text, columns, 4)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn coupure_apres_une_espace() {
        assert_eq!(rows("un deux trois", 8), ["un deux ", "trois"]);
        assert_eq!(rows("court", 8), ["court"]);
        assert_eq!(rows("", 8), [""]);
        // Mot plus long que la rangée : coupé où il faut.
        assert_eq!(
            rows("anticonstitutionnel", 8),
            ["anticons", "titution", "nel"]
        );
        // Tabulation jusqu'au prochain arrêt (4) ; caractères larges sur deux colonnes.
        assert_eq!(rows("\tab cd", 6), ["\tab ", "cd"]);
        assert_eq!(rows("日本語の", 4), ["日本", "語の"]);
        // Rien n'est perdu ni dupliqué.
        let text = "fn main() { println!(\"une très longue ligne de code\"); }";
        assert_eq!(rows(text, 10).concat(), text);
    }

    #[test]
    fn rangees_du_document() {
        let rope = Rope::from_text("court\nune ligne bien trop longue\n\nfin");
        let wrapping = Wrapping::new(&rope, 3, 10, 4);
        assert_eq!(wrapping.rows(), 1 + 3 + 1 + 1);
        assert_eq!(wrapping.first_row(1), 1);
        assert_eq!(wrapping.first_row(2), 4);
        assert_eq!(wrapping.line_of(0), 0);
        assert_eq!(wrapping.line_of(3), 1);
        assert_eq!(wrapping.line_of(4), 2);
        assert_eq!(wrapping.line_of(5), 3);
        assert_eq!(wrapping.line_of(99), 3);
    }
}
