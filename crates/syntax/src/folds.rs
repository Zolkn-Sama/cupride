//! Zones repliables, d'après l'arbre : blocs, listes, commentaires sur plusieurs lignes, sections
//! Markdown…

use std::ops::Range;

use cupride_text::{Rope, TextBuffer};
use streaming_iterator::StreamingIterator;
use tree_sitter::QueryCursor;

use crate::syntax::Syntax;

/// Une zone repliable : la ligne `header` reste visible, les lignes `hidden` sont masquées quand
/// elle est repliée. La ligne d'une accolade fermante seule reste visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRegion {
    /// Ligne d'en-tête (à partir de 0).
    pub header: usize,
    /// Lignes masquées : de `header + 1` à la fin de la zone.
    pub hidden: Range<usize>,
    /// Début du nœud, en octets.
    pub start: usize,
}

impl Syntax {
    /// Zones repliables dont le nœud touche la plage `range` (en octets), triées par ligne
    /// d'en-tête. Pour une même ligne d'en-tête, seule la plus grande est gardée.
    pub fn fold_regions(&self, text: &Rope, range: Range<usize>) -> Vec<FoldRegion> {
        let (Some(tree), Some(query)) = (self.tree(), &self.config().folds) else {
            return Vec::new();
        };
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        let mut regions: Vec<FoldRegion> = Vec::new();
        let mut matches = cursor.matches(query, tree.root_node(), &[] as &[u8]);
        while let Some(m) = matches.next() {
            for capture in m.captures() {
                let node = capture.node;
                if let Some(region) = region(text, node.start_byte(), node.end_byte()) {
                    regions.push(region);
                }
            }
        }
        regions.sort_by(|a, b| {
            a.header
                .cmp(&b.header)
                .then(b.hidden.end.cmp(&a.hidden.end))
        });
        regions.dedup_by(|later, first| later.header == first.header);
        regions
    }
}

/// Zone repliable d'un nœud de `start` à `end`, s'il couvre au moins deux lignes à masquer… ou
/// une.
fn region(text: &Rope, start: usize, end: usize) -> Option<FoldRegion> {
    if end <= start {
        return None;
    }
    let header = text.byte_to_line(start).ok()?;
    let mut last = text.byte_to_line(end - 1).ok()?;
    // Une dernière ligne qui ne contient que la fermeture (`}`, `)`, `]`) reste visible.
    let closing = text.line(last).ok()?;
    if closing.trim_start().starts_with(['}', ')', ']']) {
        last = last.checked_sub(1)?;
    }
    (last > header).then(|| FoldRegion {
        header,
        hidden: header + 1..last + 1,
        start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_for;

    fn regions(language: &str, text: &str) -> Vec<(usize, Range<usize>)> {
        let rope = Rope::from_text(text);
        let syntax = Syntax::new(config_for(language).unwrap(), &rope, None);
        syntax
            .fold_regions(&rope, 0..text.len())
            .into_iter()
            .map(|r| (r.header, r.hidden))
            .collect()
    }

    #[test]
    fn zones_rust() {
        let text = "fn f(\n    a: u8,\n) {\n    if a > 0 {\n        g();\n    }\n}\n/* un\n   deux */\nfn h() {}\n";
        assert_eq!(
            regions("Rust", text),
            [
                (0, 1..2), // paramètres : « ) {» reste visible
                (2, 3..6), // corps
                (3, 4..5), // bloc du if
                (7, 8..9), // commentaire : la ligne de « */ » est masquée
            ]
        );
    }

    #[test]
    fn zones_c_et_markdown() {
        let c = "int main(void) {\n    return 0;\n}\nstruct S {\n    int a;\n    int b;\n};\n";
        assert_eq!(regions("C", c), [(0, 1..2), (3, 4..6)]);
        let md = "# Un\n\ntexte\n\n## Deux\n\nfin\n\n```\ncode\nencore\n```\n";
        let found = regions("Markdown", md);
        assert_eq!(
            found[0],
            (0, 1..12),
            "section de premier niveau : {found:?}"
        );
        assert!(found.contains(&(4, 5..12)), "{found:?}");
        assert!(found.contains(&(8, 9..12)), "bloc de code : {found:?}");
    }
}
