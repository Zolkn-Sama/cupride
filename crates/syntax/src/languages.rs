//! Langages pris en charge : grammaire Tree-sitter et requêtes de chaque langage.
//!
//! Les grammaires sont compilées avec CuprIDE ; leurs requêtes sont préparées à la première
//! utilisation du langage.

use std::sync::OnceLock;

use tree_sitter::{Language, Query};

#[cfg(test)]
use crate::highlight::Highlight;
use crate::highlight::Paint;

/// Un langage analysable : sa grammaire et ses requêtes.
pub struct LanguageConfig {
    /// Nom du langage, celui affiché dans la barre d'état (« Rust »).
    pub name: &'static str,
    pub(crate) language: Language,
    /// Requête de coloration, et ce que devient chacune de ses captures.
    pub(crate) highlights: Option<Query>,
    pub(crate) paints: Vec<Paint>,
    /// Requête des langages imbriqués (blocs de code dans du Markdown…).
    pub(crate) injections: Option<Query>,
    /// Requête des fonctions (`@function`), pour aller de l'une à l'autre.
    pub(crate) functions: Option<Query>,
    /// Requête des zones repliables (`@fold`).
    pub(crate) folds: Option<Query>,
}

impl std::fmt::Debug for LanguageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageConfig")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl LanguageConfig {
    /// Grammaire Tree-sitter du langage.
    pub fn language(&self) -> &Language {
        &self.language
    }
}

/// Un langage intégré : son nom, les autres noms sous lesquels il est désigné (blocs de code
/// Markdown, par exemple), et de quoi préparer sa configuration.
struct Builtin {
    name: &'static str,
    aliases: &'static [&'static str],
    build: fn() -> LanguageConfig,
    config: OnceLock<LanguageConfig>,
}

static BUILTINS: [Builtin; 4] = [
    Builtin {
        name: "Rust",
        aliases: &["rust", "rs"],
        build: rust,
        config: OnceLock::new(),
    },
    Builtin {
        name: "C",
        aliases: &["c", "h"],
        build: c,
        config: OnceLock::new(),
    },
    Builtin {
        name: "Markdown",
        aliases: &["markdown", "md"],
        build: markdown,
        config: OnceLock::new(),
    },
    // Partie « en ligne » de Markdown (emphase, liens, code), analysée dans chaque paragraphe.
    Builtin {
        name: "Markdown (en ligne)",
        aliases: &["markdown_inline"],
        build: markdown_inline,
        config: OnceLock::new(),
    },
];

/// Configuration du langage `name` (nom affiché, ou autre nom : `rs`, `markdown_inline`…), sans
/// tenir compte de la casse. `None` si aucune grammaire n'est disponible.
pub fn config_for(name: &str) -> Option<&'static LanguageConfig> {
    let builtin = BUILTINS.iter().find(|b| {
        b.name.eq_ignore_ascii_case(name) || b.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    })?;
    Some(builtin.config.get_or_init(builtin.build))
}

/// Langages dont la grammaire est disponible, par nom affiché.
pub fn supported() -> impl Iterator<Item = &'static str> {
    BUILTINS
        .iter()
        .map(|b| b.name)
        .filter(|name| !name.contains('('))
}

impl LanguageConfig {
    /// Configuration du langage `name`, avec sa grammaire et ses requêtes. Une requête invalide
    /// est journalisée et ignorée : le langage reste analysable.
    fn new(
        name: &'static str,
        language: Language,
        highlights: &[&str],
        injections: Option<&str>,
        functions: &[&str],
        folds: &[&str],
    ) -> Self {
        let highlights = query(name, "coloration", &language, &highlights.concat());
        let paints = highlights.as_ref().map_or_else(Vec::new, |q| {
            q.capture_names().iter().map(|n| Paint::of(n)).collect()
        });
        let injections = injections.and_then(|source| query(name, "injections", &language, source));
        let kinds_query = |kinds: &[&str], capture: &str, kind: &str| {
            (!kinds.is_empty())
                .then(|| {
                    let kinds: String = kinds.iter().map(|k| format!("({k}) ")).collect();
                    query(name, kind, &language, &format!("[{kinds}] @{capture}"))
                })
                .flatten()
        };
        let functions = kinds_query(functions, "function", "fonctions");
        let folds = kinds_query(folds, "fold", "repliage");
        Self {
            name,
            language,
            highlights,
            paints,
            injections,
            functions,
            folds,
        }
    }
}

fn query(name: &str, kind: &str, language: &Language, source: &str) -> Option<Query> {
    match Query::new(language, source) {
        Ok(query) => Some(query),
        Err(error) => {
            tracing::warn!(langage = name, requête = kind, %error, "requête invalide ignorée");
            None
        }
    }
}

/// Motifs ajoutés avant la requête de coloration de la grammaire : le premier motif qui
/// s'applique à un nœud l'emporte.
const RUST_HIGHLIGHTS: &str = r#"
((identifier) @constant
 (#match? @constant "^[A-Z][A-Z\d_]+$"))
[
  "+" "-" "/" "%" "^" "|" "!" "=" "==" "!=" "<=" ">=" "&&" "||" "<<" ">>"
  "+=" "-=" "*=" "/=" "%=" "^=" "|=" "&=" "<<=" ">>=" ".." "..=" "?"
] @operator
"#;

const C_HIGHLIGHTS: &str = r#"
((identifier) @constant
 (#match? @constant "^[A-Z][A-Z\d_]*$"))
(primitive_type) @type.builtin
"#;

fn rust() -> LanguageConfig {
    LanguageConfig::new(
        "Rust",
        tree_sitter_rust::LANGUAGE.into(),
        &[RUST_HIGHLIGHTS, tree_sitter_rust::HIGHLIGHTS_QUERY],
        // Les macros (`token_tree`) ne sont pas réanalysées comme du Rust : coûteux, et leur
        // contenu n'est souvent pas une expression valide.
        None,
        &[
            "function_item",
            "function_signature_item",
            "macro_definition",
        ],
        &[
            "block",
            "declaration_list",
            "field_declaration_list",
            "ordered_field_declaration_list",
            "enum_variant_list",
            "field_initializer_list",
            "match_block",
            "use_list",
            "token_tree",
            "arguments",
            "parameters",
            "array_expression",
            "tuple_expression",
            "block_comment",
            "macro_definition",
        ],
    )
}

fn c() -> LanguageConfig {
    LanguageConfig::new(
        "C",
        tree_sitter_c::LANGUAGE.into(),
        &[C_HIGHLIGHTS, tree_sitter_c::HIGHLIGHT_QUERY],
        None,
        &["function_definition"],
        &[
            "compound_statement",
            "field_declaration_list",
            "enumerator_list",
            "initializer_list",
            "parameter_list",
            "argument_list",
            "comment",
            "preproc_if",
            "preproc_ifdef",
            "preproc_function_def",
        ],
    )
}

fn markdown() -> LanguageConfig {
    LanguageConfig::new(
        "Markdown",
        tree_sitter_md::LANGUAGE.into(),
        &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
        Some(tree_sitter_md::INJECTION_QUERY_BLOCK),
        &["atx_heading", "setext_heading"],
        &[
            "section",
            "fenced_code_block",
            "indented_code_block",
            "list",
            "block_quote",
            "html_block",
        ],
    )
}

fn markdown_inline() -> LanguageConfig {
    LanguageConfig::new(
        "Markdown (en ligne)",
        tree_sitter_md::INLINE_LANGUAGE.into(),
        &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
        Some(tree_sitter_md::INJECTION_QUERY_INLINE),
        &[],
        &[],
    )
}

/// Catégories de coloration que la requête du langage peut produire (pour les tests).
#[cfg(test)]
pub(crate) fn highlights_of(config: &LanguageConfig) -> Vec<Highlight> {
    let mut found: Vec<Highlight> = config
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Color(h) => Some(*h),
            _ => None,
        })
        .collect();
    found.sort_by_key(|h| *h as u8);
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langages_trouves_par_nom_ou_alias() {
        assert_eq!(config_for("Rust").map(|c| c.name), Some("Rust"));
        assert_eq!(config_for("rs").map(|c| c.name), Some("Rust"));
        assert_eq!(config_for("C").map(|c| c.name), Some("C"));
        assert_eq!(config_for("MARKDOWN").map(|c| c.name), Some("Markdown"));
        assert!(config_for("markdown_inline").is_some());
        assert!(config_for("Python").is_none());
        assert_eq!(supported().collect::<Vec<_>>(), ["Rust", "C", "Markdown"]);
    }

    #[test]
    fn requetes_valides() {
        for name in ["Rust", "C", "Markdown", "markdown_inline"] {
            let config = config_for(name).unwrap();
            assert!(config.highlights.is_some(), "coloration de {name}");
            assert!(!highlights_of(config).is_empty(), "{name}");
        }
        assert!(config_for("Markdown").unwrap().injections.is_some());
        for name in ["Rust", "C", "Markdown"] {
            assert!(config_for(name).unwrap().functions.is_some(), "{name}");
            assert!(config_for(name).unwrap().folds.is_some(), "{name}");
        }
    }
}
