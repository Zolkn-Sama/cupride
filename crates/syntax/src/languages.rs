//! Langages pris en charge : grammaire Tree-sitter et requêtes de chaque langage.
//!
//! Les grammaires sont compilées avec CuprIDE ; leurs requêtes sont préparées à la première
//! utilisation du langage.

use std::sync::OnceLock;

use tree_sitter::Language;

/// Un langage analysable : sa grammaire et ses requêtes.
pub struct LanguageConfig {
    /// Nom du langage, celui affiché dans la barre d'état (« Rust »).
    pub name: &'static str,
    pub(crate) language: Language,
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

fn rust() -> LanguageConfig {
    LanguageConfig {
        name: "Rust",
        language: tree_sitter_rust::LANGUAGE.into(),
    }
}

fn c() -> LanguageConfig {
    LanguageConfig {
        name: "C",
        language: tree_sitter_c::LANGUAGE.into(),
    }
}

fn markdown() -> LanguageConfig {
    LanguageConfig {
        name: "Markdown",
        language: tree_sitter_md::LANGUAGE.into(),
    }
}

fn markdown_inline() -> LanguageConfig {
    LanguageConfig {
        name: "Markdown (en ligne)",
        language: tree_sitter_md::INLINE_LANGUAGE.into(),
    }
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
}
