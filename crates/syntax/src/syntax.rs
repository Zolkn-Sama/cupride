//! Arbre syntaxique d'un document.

use cupride_text::Rope;
use tree_sitter::{Node, Parser, Tree};

use crate::languages::LanguageConfig;
use crate::source::Source;

/// Arbre syntaxique d'un document, dans un langage.
pub struct Syntax {
    config: &'static LanguageConfig,
    parser: Parser,
    tree: Option<Tree>,
}

impl std::fmt::Debug for Syntax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Syntax")
            .field("language", &self.config.name)
            .field("parsed", &self.tree.is_some())
            .finish_non_exhaustive()
    }
}

impl Syntax {
    /// Analyse `text` dans le langage `config`.
    pub fn new(config: &'static LanguageConfig, text: &Rope) -> Self {
        let mut parser = Parser::new();
        if let Err(error) = parser.set_language(&config.language) {
            tracing::warn!(langage = config.name, %error, "grammaire incompatible");
        }
        let mut syntax = Self {
            config,
            parser,
            tree: None,
        };
        syntax.reparse(text);
        syntax
    }

    /// Langage analysé.
    pub fn config(&self) -> &'static LanguageConfig {
        self.config
    }

    /// Réanalyse tout le texte, après une modification.
    pub fn reparse(&mut self, text: &Rope) {
        self.tree = parse(&mut self.parser, text, None);
    }

    /// Arbre syntaxique, s'il a pu être construit.
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// Plus petit nœud nommé qui contient la plage `range` (en octets).
    pub fn named_node_at(&self, range: std::ops::Range<usize>) -> Option<Node<'_>> {
        self.tree()?
            .root_node()
            .named_descendant_for_byte_range(range.start, range.end)
    }
}

/// Analyse `text`, en réutilisant `old` (déjà mis à jour des modifications) s'il est donné.
pub(crate) fn parse(parser: &mut Parser, text: &Rope, old: Option<&Tree>) -> Option<Tree> {
    let source = Source::new(text);
    let tree = parser.parse_with_options(&mut |byte, _| source.at(byte), old, None);
    if tree.is_none() {
        parser.reset();
    }
    tree
}

#[cfg(test)]
mod tests {
    use cupride_text::TextBuffer;

    use super::*;
    use crate::config_for;

    fn syntax(language: &str, text: &str) -> (Syntax, Rope) {
        let rope = Rope::from_text(text);
        let config = config_for(language).expect("langage intégré");
        (Syntax::new(config, &rope), rope)
    }

    #[test]
    fn analyse_rust() {
        let (syntax, _) = syntax("Rust", "fn main() {\n    println!(\"salut\");\n}\n");
        let root = syntax.tree().expect("arbre").root_node();
        assert_eq!(root.kind(), "source_file");
        assert!(!root.has_error());
        assert_eq!(root.named_child(0).map(|n| n.kind()), Some("function_item"));
        let node = syntax.named_node_at(3..4).expect("nœud");
        assert_eq!(node.kind(), "identifier");
        assert_eq!(node.parent().map(|n| n.kind()), Some("function_item"));
    }

    #[test]
    fn analyse_c_et_markdown() {
        let (c, _) = syntax("C", "int main(void) { return 0; }\n");
        let root = c.tree().expect("arbre").root_node();
        assert_eq!(root.kind(), "translation_unit");
        assert!(!root.has_error());
        let (md, _) = syntax("Markdown", "# Titre\n\nUn paragraphe.\n");
        assert_eq!(md.tree().expect("arbre").root_node().kind(), "document");
    }

    #[test]
    fn erreurs_signalees_dans_l_arbre() {
        let (syntax, _) = syntax("Rust", "fn main( {\n");
        assert!(syntax.tree().expect("arbre").root_node().has_error());
    }

    #[test]
    fn reanalyse_apres_modification() {
        let (mut syntax, mut rope) = syntax("Rust", "fn a() {}\n");
        rope.insert(rope.len_bytes(), "struct B;\n").unwrap();
        syntax.reparse(&rope);
        let root = syntax.tree().expect("arbre").root_node();
        assert_eq!(root.named_child_count(), 2);
        assert_eq!(root.named_child(1).map(|n| n.kind()), Some("struct_item"));
    }

    #[test]
    fn texte_sur_plusieurs_morceaux() {
        let text: String = (0..3000).map(|i| format!("fn f{i}() {{}}\n")).collect();
        let (syntax, _) = syntax("Rust", &text);
        let root = syntax.tree().expect("arbre").root_node();
        assert!(!root.has_error());
        assert_eq!(root.named_child_count(), 3000);
    }
}
