//! Analyse syntaxique de CuprIDE, avec [Tree-sitter](https://tree-sitter.github.io).
//!
//! - [`languages`] : langages pris en charge, leur grammaire et leurs requêtes. Les grammaires
//!   Rust, C et Markdown sont intégrées, en attendant les paquets de langage.
//! - [`Syntax`] : arbre syntaxique d'un document, tenu à jour à chaque modification.
//!
//! Cette crate ne dépend que de `cupride-text` : elle ignore tout des documents, des vues et du
//! thème.

pub mod languages;
mod source;
mod syntax;

pub use languages::{LanguageConfig, config_for};
pub use syntax::Syntax;
pub use tree_sitter::{Node, Tree};
