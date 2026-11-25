//! Structure du code, d'après l'arbre : nœud parent ou enfant d'une sélection, fonctions.

use std::ops::Range;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, QueryCursor};

use crate::syntax::Syntax;

impl Syntax {
    /// Plage du plus petit nœud qui contient strictement `range` : la sélection étendue au nœud
    /// parent. Un curseur s'étend au nœud sous lui (un identifiant, par exemple).
    pub fn expand(&self, range: Range<usize>) -> Option<Range<usize>> {
        let root = self.tree()?.root_node();
        let mut node = root.named_descendant_for_byte_range(range.start, range.end)?;
        while node.start_byte() == range.start && node.end_byte() == range.end
            || !contains(node, &range)
        {
            node = node.parent()?;
        }
        Some(node.byte_range())
    }

    /// Plage du premier nœud enfant de la sélection `range`, qui contient `head` s'il y en a un :
    /// la sélection réduite d'un niveau. `None` si la sélection n'a pas d'enfant.
    pub fn shrink(&self, range: Range<usize>, head: usize) -> Option<Range<usize>> {
        let root = self.tree()?.root_node();
        let mut node = root.named_descendant_for_byte_range(range.start, range.end)?;
        // Le plus haut nœud qui couvre exactement la sélection.
        while let Some(parent) = node.parent()
            && parent.byte_range() == node.byte_range()
        {
            node = parent;
        }
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node
            .named_children(&mut cursor)
            .filter(|c| c.byte_range() != range && !c.byte_range().is_empty())
            .collect();
        children
            .iter()
            .find(|c| c.start_byte() <= head && head <= c.end_byte())
            .or(children.first())
            .map(Node::byte_range)
    }

    /// Plages des fonctions (et méthodes) qui commencent dans `range`, dans l'ordre du texte. En
    /// Markdown, les titres.
    pub fn functions(&self, range: Range<usize>) -> Vec<Range<usize>> {
        let (Some(tree), Some(query)) = (self.tree(), &self.config().functions) else {
            return Vec::new();
        };
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut found = Vec::new();
        let mut matches = cursor.matches(query, tree.root_node(), &[] as &[u8]);
        while let Some(m) = matches.next() {
            for capture in m.captures() {
                let r = capture.node.byte_range();
                if range.contains(&r.start) {
                    found.push(r);
                }
            }
        }
        found.sort_by_key(|r| r.start);
        found.dedup();
        found
    }

    /// Début de la fonction suivante (`forward`) ou précédente de la position `offset`.
    pub fn function_near(&self, offset: usize, forward: bool, len: usize) -> Option<usize> {
        if forward {
            self.functions(offset.saturating_add(1)..len)
                .first()
                .map(|r| r.start)
        } else {
            self.functions(0..offset).last().map(|r| r.start)
        }
    }
}

impl Syntax {
    /// Parenthèse (crochet, accolade) qui correspond à celle qui commence à `offset`, d'après
    /// l'arbre. `None` si le caractère n'est pas une parenthèse de l'arbre (dans une chaîne, un
    /// commentaire, ou sans arbre) ; `Some(None)` si elle n'a pas de partenaire (code
    /// incomplet) ; sinon `Some(Some(position))`.
    pub fn matching_bracket(&self, offset: usize) -> Option<Option<usize>> {
        let root = self.tree()?.root_node();
        let node = root.descendant_for_byte_range(offset, offset + 1)?;
        if node.start_byte() != offset || node.end_byte() != offset + 1 || node.is_named() {
            return None;
        }
        let kind = node.kind();
        let partner = partner(kind.chars().next()?)?;
        let opening = matches!(kind, "(" | "[" | "{");
        let mut sibling = node;
        loop {
            let next = if opening {
                sibling.next_sibling()
            } else {
                sibling.prev_sibling()
            };
            let Some(next) = next else {
                return Some(None);
            };
            sibling = next;
            if !sibling.is_named()
                && sibling.kind().starts_with(partner)
                && sibling.kind().len() == 1
            {
                // Une parenthèse manquante est insérée par Tree-sitter, de longueur nulle.
                return Some((!sibling.is_missing()).then(|| sibling.start_byte()));
            }
        }
    }
}

/// Parenthèse qui correspond à `c`, ou `None` si `c` n'en est pas une.
pub fn partner(c: char) -> Option<char> {
    Some(match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        _ => return None,
    })
}

fn contains(node: Node<'_>, range: &Range<usize>) -> bool {
    node.start_byte() <= range.start && range.end <= node.end_byte()
}

#[cfg(test)]
mod tests {
    use cupride_text::{Rope, TextBuffer};

    use crate::{Syntax, config_for};

    fn syntax(language: &str, text: &str) -> Syntax {
        Syntax::new(config_for(language).unwrap(), &Rope::from_text(text), None)
    }

    #[test]
    fn etendre_puis_reduire() {
        let text = "fn f() {\n    let x = a + b * c;\n}\n";
        let s = syntax("Rust", text);
        let b = text.find("b *").unwrap();
        let steps: Vec<&str> = std::iter::successors(Some(b..b), |r| s.expand(r.clone()))
            .skip(1)
            .map(|r| &text[r])
            .collect();
        assert_eq!(
            steps,
            [
                "b",
                "b * c",
                "a + b * c",
                "let x = a + b * c;",
                "{\n    let x = a + b * c;\n}",
                "fn f() {\n    let x = a + b * c;\n}",
                "fn f() {\n    let x = a + b * c;\n}\n",
            ]
        );
        // Réduire : l'enfant qui contient le curseur.
        let sum = text.find("a + b").unwrap();
        let shrunk = s.shrink(sum..sum + 9, b).unwrap();
        assert_eq!(&text[shrunk], "b * c");
        let shrunk = s.shrink(sum..sum + 9, sum).unwrap();
        assert_eq!(&text[shrunk], "a");
        assert_eq!(
            s.shrink(b..b + 1, b),
            None,
            "un identifiant n'a pas d'enfant"
        );
    }

    #[test]
    fn parentheses_correspondantes() {
        let text = "fn f(a: [u8; 2]) {\n    g(\")\", (1));\n}\n";
        let s = syntax("Rust", text);
        let at = |pattern: &str| text.find(pattern).unwrap();
        let m = |offset| s.matching_bracket(offset).flatten();
        assert_eq!(m(at("(a")), Some(at(") {")));
        assert_eq!(m(at(") {")), Some(at("(a")));
        assert_eq!(m(at("[u8")), Some(at("]")));
        assert_eq!(m(at("{\n")), Some(text.rfind('}').unwrap()));
        let call = at("g(") + 1;
        assert_eq!(m(call), Some(at("));") + 1));
        // La parenthèse de la chaîne n'est pas une parenthèse de l'arbre.
        assert_eq!(s.matching_bracket(at("\")\"") + 1), None);
        assert_eq!(s.matching_bracket(at("fn")), None);
        // Code incomplet : pas de partenaire.
        let s = syntax("C", "int f(void) { return (1; }\n");
        assert_eq!(s.matching_bracket(21), Some(None));
    }

    #[test]
    fn fonctions_suivante_et_precedente() {
        let text = "fn a() {}\nstruct S;\nimpl S {\n    fn m(&self) {}\n}\nfn b() {}\n";
        let s = syntax("Rust", text);
        let starts: Vec<&str> = s
            .functions(0..text.len())
            .into_iter()
            .map(|r| &text[r.start..r.start + 4])
            .collect();
        assert_eq!(starts, ["fn a", "fn m", "fn b"]);
        let m = text.find("fn m").unwrap();
        assert_eq!(s.function_near(0, true, text.len()), Some(m));
        assert_eq!(s.function_near(m, true, text.len()), text.find("fn b"));
        assert_eq!(s.function_near(m, false, text.len()), Some(0));
        assert_eq!(s.function_near(0, false, text.len()), None);
        let c = syntax("C", "int a(void) { return 0; }\nint b;\nvoid c(void) {}\n");
        assert_eq!(c.functions(0..100).len(), 2);
        let md = syntax("Markdown", "# Un\n\ntexte\n\n## Deux\n");
        assert_eq!(md.function_near(0, true, 100), Some(13));
    }
}
