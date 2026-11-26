//! Symboles d'un document : fonctions, types, modules, titres… d'après sa requête de symboles
//! (`@name` dans un nœud `@definition.<genre>`).

use std::ops::Range;

use cupride_text::Rope;
use streaming_iterator::StreamingIterator;
use tree_sitter::QueryCursor;

use crate::source::Source;
use crate::syntax::Syntax;

/// Un symbole défini dans le document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Nom.
    pub name: String,
    /// Genre, en clair : « fonction », « méthode », « type »…
    pub kind: &'static str,
    /// Nœud de la définition, en octets.
    pub range: Range<usize>,
    /// Position du nom, en octets.
    pub name_range: Range<usize>,
}

/// Genre affiché pour une capture `@definition.<genre>`.
fn kind_name(kind: &str) -> &'static str {
    match kind {
        "function" => "fonction",
        "method" => "méthode",
        "class" | "type" | "struct" | "enum" => "type",
        "interface" => "trait",
        "module" => "module",
        "macro" => "macro",
        "constant" => "constante",
        "heading" => "titre",
        _ => "symbole",
    }
}

impl Syntax {
    /// Symboles du document, dans l'ordre du texte. Une définition capturée par plusieurs motifs
    /// (une méthode est aussi une fonction) n'apparaît qu'une fois, avec le genre du premier.
    pub fn symbols(&self, text: &Rope) -> Vec<Symbol> {
        let (Some(tree), Some(query)) = (self.tree(), &self.config().symbols) else {
            return Vec::new();
        };
        let Some(name_index) = query.capture_index_for_name("name") else {
            return Vec::new();
        };
        let source = Source::new(text);
        let provider = |node: tree_sitter::Node<'_>| source.slices(node.byte_range()).into_iter();
        let mut cursor = QueryCursor::new();
        // (motif, symbole)
        let mut found: Vec<(usize, Symbol)> = Vec::new();
        let mut matches = cursor.matches(query, tree.root_node(), provider);
        while let Some(m) = matches.next() {
            let Some(name) = m.nodes_for_capture_index(name_index).next() else {
                continue;
            };
            for capture in m.captures() {
                let capture_name = query.capture_names()[capture.index as usize];
                let Some(kind) = capture_name.strip_prefix("definition.") else {
                    continue;
                };
                found.push((
                    m.pattern_index,
                    Symbol {
                        name: source.text(name.byte_range()).trim().to_owned(),
                        kind: kind_name(kind),
                        range: capture.node.byte_range(),
                        name_range: name.byte_range(),
                    },
                ));
            }
        }
        found.sort_by(|a, b| {
            a.1.name_range
                .start
                .cmp(&b.1.name_range.start)
                .then(a.0.cmp(&b.0))
        });
        found.dedup_by(|later, first| later.1.name_range == first.1.name_range);
        found.into_iter().map(|(_, symbol)| symbol).collect()
    }
}

#[cfg(test)]
mod tests {
    use cupride_text::TextBuffer;

    use super::*;
    use crate::config_for;

    fn symbols(language: &str, text: &str) -> Vec<(String, &'static str)> {
        let rope = Rope::from_text(text);
        let syntax = Syntax::new(config_for(language).unwrap(), &rope, None);
        syntax
            .symbols(&rope)
            .into_iter()
            .map(|s| (s.name, s.kind))
            .collect()
    }

    #[test]
    fn symboles_rust() {
        let text = "mod m {}\nstruct S;\nenum E { A }\ntrait T {}\nimpl S {\n    fn methode(&self) {}\n}\nfn main() {}\nmacro_rules! mac { () => {} }\n";
        assert_eq!(
            symbols("Rust", text),
            [
                ("m".to_owned(), "module"),
                ("S".to_owned(), "type"),
                ("E".to_owned(), "type"),
                ("T".to_owned(), "trait"),
                ("methode".to_owned(), "méthode"),
                ("main".to_owned(), "fonction"),
                ("mac".to_owned(), "macro"),
            ]
        );
    }

    #[test]
    fn symboles_c_et_markdown() {
        let c = "struct point { int x; };\ntypedef int entier;\nint main(void) { return 0; }\n";
        assert_eq!(
            symbols("C", c),
            [
                ("point".to_owned(), "type"),
                ("entier".to_owned(), "type"),
                ("main".to_owned(), "fonction"),
            ]
        );
        let md = "# Titre\n\ntexte\n\n## Partie *une*\n\nSous-titre\n---\n";
        assert_eq!(
            symbols("Markdown", md),
            [
                ("Titre".to_owned(), "titre"),
                ("Partie *une*".to_owned(), "titre"),
                ("Sous-titre".to_owned(), "titre"),
            ]
        );
    }
}
