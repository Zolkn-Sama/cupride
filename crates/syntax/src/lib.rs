//! Analyse syntaxique de CuprIDE, avec [Tree-sitter](https://tree-sitter.github.io).
//!
//! - [`languages`] : langages pris en charge, leur grammaire et leurs requêtes. Les grammaires
//!   Rust, C et Markdown sont intégrées, en attendant les paquets de langage.
//! - [`Syntax`] : arbre syntaxique d'un document, tenu à jour à chaque modification : seule la
//!   zone modifiée est réanalysée, et une analyse longue passe dans un thread.
//! - [`Highlight`] : catégories de coloration d'une plage ([`Syntax::highlights`]), langages
//!   imbriqués compris (blocs de code dans du Markdown).
//! - Structure du code : nœud parent ou enfant d'une sélection ([`Syntax::expand`],
//!   [`Syntax::shrink`]), fonctions ([`Syntax::functions`]).
//!
//! Cette crate ne dépend que de `cupride-text` : elle ignore tout des documents, des vues et du
//! thème.

mod edits;
mod highlight;
pub mod languages;
mod source;
mod structure;
mod syntax;

pub use edits::{MAX_INCREMENTAL_EDITS, input_edits};
pub use highlight::{Highlight, MAX_HIGHLIGHT_BYTES};
pub use languages::{LanguageConfig, config_for};
pub use syntax::{SYNC_BUDGET, Syntax, Waker};
pub use tree_sitter::{Node, Tree};
