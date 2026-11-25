//! Coloration syntaxique : catégories des nœuds d'une plage du texte, langages imbriqués
//! compris.
//!
//! Les requêtes de coloration des grammaires capturent des nœuds sous des noms
//! (`@keyword`, `@string.escape`, `@function.macro`…), ramenés ici à quelques catégories, celles
//! des thèmes. Quand plusieurs captures couvrent un même caractère, la plus précise (le plus petit
//! nœud) l'emporte ; pour un même nœud, le premier motif de la requête l'emporte.

use std::ops::Range;

use cupride_text::{Rope, TextBuffer};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, QueryCursor};

use crate::languages::{LanguageConfig, config_for};
use crate::source::Source;
use crate::syntax::Syntax;

/// Au-delà, la plage à colorer est tronquée (une ligne immense, par exemple).
pub const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;

/// Profondeur maximale des langages imbriqués (Markdown › bloc de code Rust : 1).
const MAX_DEPTH: usize = 3;

/// Catégorie de coloration, associée à une couleur par le thème.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Highlight {
    Comment,
    Keyword,
    String,
    Number,
    Constant,
    Function,
    Type,
    Variable,
    Operator,
    Punctuation,
}

impl Highlight {
    /// Toutes les catégories.
    pub const ALL: [Self; 10] = [
        Self::Comment,
        Self::Keyword,
        Self::String,
        Self::Number,
        Self::Constant,
        Self::Function,
        Self::Type,
        Self::Variable,
        Self::Operator,
        Self::Punctuation,
    ];

    /// Nom de la catégorie, celui des thèmes (`[syntax] keyword = "#…"`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::Keyword => "keyword",
            Self::String => "string",
            Self::Number => "number",
            Self::Constant => "constant",
            Self::Function => "function",
            Self::Type => "type",
            Self::Variable => "variable",
            Self::Operator => "operator",
            Self::Punctuation => "punctuation",
        }
    }
}

/// Effet d'une capture de la requête de coloration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Paint {
    /// Capture sans couleur (`@text.emphasis`…) : ignorée.
    Skip,
    /// `@none` : retire la couleur (contenu d'un bloc de code, coloré par son propre langage).
    Clear,
    /// Une catégorie.
    Color(Highlight),
}

/// Captures et leur catégorie, dans l'ordre où elles sont essayées : un nom de capture prend la
/// catégorie de la première règle dont un préfixe lui correspond (`keyword` couvre
/// `keyword.return`, mais pas `keywords`).
const RULES: &[(&[&str], Highlight)] = &[
    (&["comment"], Highlight::Comment),
    (
        &[
            "variable.builtin",
            "punctuation.special",
            "text.title",
            "markup.heading",
            "keyword",
            "conditional",
            "repeat",
            "include",
            "exception",
            "storageclass",
        ],
        Highlight::Keyword,
    ),
    (
        &["string.escape", "string.special", "escape"],
        Highlight::Constant,
    ),
    (
        &["string", "character", "text.literal", "markup.raw"],
        Highlight::String,
    ),
    (&["number", "float"], Highlight::Number),
    (&["constant", "boolean", "label"], Highlight::Constant),
    (
        &[
            "function",
            "method",
            "attribute",
            "text.uri",
            "markup.link.url",
        ],
        Highlight::Function,
    ),
    (
        &["type", "constructor", "text.reference", "markup.link.label"],
        Highlight::Type,
    ),
    (
        &["variable", "property", "field", "parameter"],
        Highlight::Variable,
    ),
    (&["operator"], Highlight::Operator),
    (&["punctuation", "delimiter"], Highlight::Punctuation),
];

impl Paint {
    /// Effet de la capture nommée `name`.
    pub(crate) fn of(name: &str) -> Self {
        if name == "none" {
            return Self::Clear;
        }
        let is = |prefix: &&str| {
            name.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        };
        RULES
            .iter()
            .find(|(prefixes, _)| prefixes.iter().any(is))
            .map_or(Self::Skip, |(_, highlight)| Self::Color(*highlight))
    }
}

/// Catégorie de chaque octet de la plage colorée.
struct Painter {
    start: usize,
    paint: Vec<Option<Highlight>>,
}

impl Painter {
    fn paint(&mut self, range: Range<usize>, paint: Paint) {
        let value = match paint {
            Paint::Skip => return,
            Paint::Clear => None,
            Paint::Color(highlight) => Some(highlight),
        };
        let end = self.start + self.paint.len();
        let (from, to) = (range.start.max(self.start), range.end.min(end));
        if from < to {
            self.paint[from - self.start..to - self.start].fill(value);
        }
    }

    /// Plages colorées successives, sans les octets sans couleur.
    fn spans(&self) -> Vec<(Range<usize>, Highlight)> {
        let mut spans: Vec<(Range<usize>, Highlight)> = Vec::new();
        for (index, paint) in self.paint.iter().enumerate() {
            let Some(highlight) = *paint else {
                continue;
            };
            let at = self.start + index;
            match spans.last_mut() {
                Some((range, last)) if range.end == at && *last == highlight => range.end += 1,
                _ => spans.push((at..at + 1, highlight)),
            }
        }
        spans
    }
}

impl Syntax {
    /// Plages colorées de `range` (en octets, triées, sans chevauchement), langages imbriqués
    /// compris. Vide sans arbre. À n'appeler que sur ce qui est visible : le coût est
    /// proportionnel à la plage.
    pub fn highlights(&self, text: &Rope, range: Range<usize>) -> Vec<(Range<usize>, Highlight)> {
        let Some(tree) = self.tree() else {
            return Vec::new();
        };
        let end = range
            .end
            .min(text.len_bytes())
            .min(range.start.saturating_add(MAX_HIGHLIGHT_BYTES));
        let start = range.start.min(end);
        let source = Source::new(text);
        let mut painter = Painter {
            start,
            paint: vec![None; end - start],
        };
        let mut parser = Parser::new();
        paint_layer(
            self.config(),
            tree.root_node(),
            &source,
            start..end,
            &mut painter,
            &mut parser,
            0,
        );
        painter.spans()
    }
}

/// Colore la plage `range` d'après l'arbre `root` du langage `config`, puis ses langages
/// imbriqués.
fn paint_layer(
    config: &LanguageConfig,
    root: Node<'_>,
    source: &Source<'_>,
    range: Range<usize>,
    painter: &mut Painter,
    parser: &mut Parser,
    depth: usize,
) {
    let text = |node: Node<'_>| source.slices(node.byte_range()).into_iter();
    if let Some(query) = &config.highlights {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        // (début, fin, motif, effet)
        let mut found: Vec<(usize, usize, usize, Paint)> = Vec::new();
        let mut matches = cursor.matches(query, root, text);
        while let Some(m) = matches.next() {
            for capture in m.captures() {
                let paint = config.paints[capture.index as usize];
                if paint != Paint::Skip {
                    let r = capture.node.byte_range();
                    found.push((r.start, r.end, m.pattern_index, paint));
                }
            }
        }
        // Les plus grands nœuds d'abord, les plus précis par-dessus ; pour un même nœud, le
        // premier motif en dernier, pour qu'il l'emporte.
        found.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(b.2.cmp(&a.2)));
        for (start, end, _, paint) in found {
            painter.paint(start..end, paint);
        }
    }
    if depth >= MAX_DEPTH {
        return;
    }
    for (injected, ranges) in injections(config, root, source, &range) {
        if parser.set_language(&injected.language).is_err()
            || parser.set_included_ranges(&ranges).is_err()
        {
            continue;
        }
        let tree = parser.parse_with_options(&mut |byte, _| source.at(byte), None, None);
        if let Some(tree) = tree {
            paint_layer(
                injected,
                tree.root_node(),
                source,
                range.clone(),
                painter,
                parser,
                depth + 1,
            );
        } else {
            parser.reset();
        }
    }
}

/// Langages imbriqués dans la plage `range` : le langage, et les plages de texte qui en sont.
fn injections(
    config: &LanguageConfig,
    root: Node<'_>,
    source: &Source<'_>,
    range: &Range<usize>,
) -> Vec<(&'static LanguageConfig, Vec<tree_sitter::Range>)> {
    let Some(query) = &config.injections else {
        return Vec::new();
    };
    let Some(content) = query.capture_index_for_name("injection.content") else {
        return Vec::new();
    };
    let language_capture = query.capture_index_for_name("injection.language");
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    let mut found = Vec::new();
    let text = |node: Node<'_>| source.slices(node.byte_range()).into_iter();
    let mut matches = cursor.matches(query, root, text);
    while let Some(m) = matches.next() {
        let settings = query.property_settings(m.pattern_index);
        let setting = |key: &str| settings.iter().find(|p| &*p.key == key);
        // Langage fixé par la requête, ou écrit dans le texte (```rust,ignore).
        let name = match setting("injection.language").and_then(|p| p.value.as_deref()) {
            Some(name) => name.to_owned(),
            None => {
                let Some(node) = language_capture.and_then(|i| m.nodes_for_capture_index(i).next())
                else {
                    continue;
                };
                let written = source.text(node.byte_range());
                written
                    .split(|c: char| c == ',' || c.is_whitespace() || c == '{')
                    .find(|w| !w.is_empty())
                    .unwrap_or_default()
                    .to_owned()
            }
        };
        let Some(injected) = config_for(&name) else {
            continue;
        };
        let include_children = setting("injection.include-children").is_some();
        for node in m.nodes_for_capture_index(content) {
            let ranges = if include_children {
                vec![node.range()]
            } else {
                without_children(node)
            };
            if !ranges.is_empty() {
                found.push((injected, ranges));
            }
        }
    }
    found
}

/// Plages du nœud `node`, sans celles de ses enfants nommés (une marque de citation `>` au
/// début d'une ligne de bloc de code, par exemple). Les enfants anonymes sont des morceaux du
/// texte lui-même, gardés.
fn without_children(node: Node<'_>) -> Vec<tree_sitter::Range> {
    let mut ranges = Vec::new();
    let mut start = (node.start_byte(), node.start_position());
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.start_byte() > start.0 {
            ranges.push(tree_sitter::Range {
                start_byte: start.0,
                end_byte: child.start_byte(),
                start_point: start.1,
                end_point: child.start_position(),
            });
        }
        start = (child.end_byte(), child.end_position());
    }
    if node.end_byte() > start.0 {
        ranges.push(tree_sitter::Range {
            start_byte: start.0,
            end_byte: node.end_byte(),
            start_point: start.1,
            end_point: node.end_position(),
        });
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_for;

    /// Texte de chaque plage colorée, et sa catégorie.
    fn colored(language: &str, text: &str) -> Vec<(String, Highlight)> {
        colored_in(language, text, 0..text.len())
    }

    fn colored_in(language: &str, text: &str, range: Range<usize>) -> Vec<(String, Highlight)> {
        let rope = Rope::from_text(text);
        let syntax = Syntax::new(config_for(language).unwrap(), &rope, None);
        syntax
            .highlights(&rope, range)
            .into_iter()
            .map(|(r, h)| (text[r].to_owned(), h))
            .collect()
    }

    fn has(found: &[(String, Highlight)], text: &str, highlight: Highlight) -> bool {
        found.iter().any(|(t, h)| t == text && *h == highlight)
    }

    #[test]
    fn noms_des_captures() {
        assert_eq!(Paint::of("keyword"), Paint::Color(Highlight::Keyword));
        assert_eq!(
            Paint::of("keyword.return"),
            Paint::Color(Highlight::Keyword)
        );
        assert_eq!(Paint::of("keywordish"), Paint::Skip);
        assert_eq!(
            Paint::of("string.escape"),
            Paint::Color(Highlight::Constant)
        );
        assert_eq!(
            Paint::of("function.macro"),
            Paint::Color(Highlight::Function)
        );
        assert_eq!(
            Paint::of("variable.builtin"),
            Paint::Color(Highlight::Keyword)
        );
        assert_eq!(Paint::of("none"), Paint::Clear);
        assert_eq!(Paint::of("text.emphasis"), Paint::Skip);
    }

    #[test]
    fn coloration_rust() {
        let found = colored(
            "Rust",
            "// note\nfn main() {\n    let s = \"a\\n\";\n    println!(\"{}\", MAX + 1);\n}\n",
        );
        assert!(
            has(&found, "// note\n", Highlight::Comment)
                || has(&found, "// note", Highlight::Comment),
            "{found:?}"
        );
        assert!(has(&found, "fn", Highlight::Keyword), "{found:?}");
        assert!(has(&found, "main", Highlight::Function), "{found:?}");
        assert!(
            has(&found, "\\n", Highlight::Constant),
            "échappement : {found:?}"
        );
        assert!(
            has(&found, "MAX", Highlight::Constant),
            "constante : {found:?}"
        );
        assert!(has(&found, "1", Highlight::Constant), "{found:?}");
        assert!(has(&found, "+", Highlight::Operator), "{found:?}");
    }

    #[test]
    fn coloration_c() {
        let found = colored(
            "C",
            "#include <stdio.h>\nint main(void) { return BUFSIZ; }\n",
        );
        assert!(has(&found, "#include", Highlight::Keyword), "{found:?}");
        assert!(has(&found, "<stdio.h>", Highlight::String), "{found:?}");
        assert!(has(&found, "int", Highlight::Type), "{found:?}");
        assert!(has(&found, "return", Highlight::Keyword), "{found:?}");
        assert!(has(&found, "BUFSIZ", Highlight::Constant), "{found:?}");
    }

    #[test]
    fn seule_la_plage_demandee() {
        let text = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let found = colored_in("Rust", text, 10..20);
        assert!(has(&found, "b", Highlight::Function), "{found:?}");
        assert!(
            !found.iter().any(|(t, _)| t == "a" || t == "c"),
            "{found:?}"
        );
    }

    #[test]
    fn langages_imbriques_dans_markdown() {
        let text = "# Titre\n\nDu *texte* et du `code`.\n\n```rust\nfn main() { let x = 1; }\n```\n\n```c\nint y;\n```\n";
        let found = colored("Markdown", text);
        assert!(
            has(&found, "Titre", Highlight::Keyword),
            "titre : {found:?}"
        );
        assert!(
            has(&found, "code", Highlight::String),
            "code en ligne : {found:?}"
        );
        assert!(
            has(&found, "fn", Highlight::Keyword),
            "bloc Rust : {found:?}"
        );
        assert!(
            has(&found, "main", Highlight::Function),
            "bloc Rust : {found:?}"
        );
        assert!(has(&found, "int", Highlight::Type), "bloc C : {found:?}");
        // Le contenu d'un bloc d'un langage inconnu n'est pas coloré comme du code.
        let found = colored("Markdown", "```inconnu\nfn x\n```\n");
        assert!(!found.iter().any(|(t, _)| t.contains("fn x")), "{found:?}");
    }
}
