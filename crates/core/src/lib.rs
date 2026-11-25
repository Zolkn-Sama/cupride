//! Noyau de CuprIDE, sans interface graphique.
//!
//! - [`Editor`] : état global de l'éditeur (documents ouverts, document actif, workspace).
//!   Chaque document affiché l'est dans un panneau ; le panneau actif décide du document actif
//!   ([`NewPanel`] : où placer un nouveau panneau).
//! - [`clipboard`] : presse-papiers, fourni par l'application (mémoire par défaut).
//! - [`command`] : registre unique des commandes, utilisé par toutes les sources d'actions.
//! - [`config`] : configuration TOML de l'utilisateur.
//! - [`events`] : événements publiés par le noyau (document ouvert, modifié, enregistré,
//!   fermé, focus changé).
//! - [`notifications`] : informations, avertissements et erreurs, avec leur historique.
//! - [`keymap`] : raccourcis clavier, simples ou en séquence, sans aucune logique.
//! - [`Document`] : un texte, son fichier, son historique et ses sélections.
//! - [`session`] : espace de travail enregistré à la fermeture et restauré au lancement.
//!
//! Ne dépend jamais de `render`, `ui` ou `app` : tout le noyau se teste avec `cargo test` et se
//! pilote depuis le REPL console.

pub mod clipboard;
pub mod command;
pub mod config;
mod document;
mod editor;
pub mod events;
pub mod indent;
pub mod keymap;
pub mod language;
pub mod monitors;
pub mod notifications;
mod panels;
pub mod project_search;
pub mod search;
pub mod session;
mod ui_request;
mod windows;

pub use cupride_layout as layout;
pub use cupride_text as text;
pub use document::{Document, DocumentError, LineEnding};
pub use editor::{DocumentId, Editor, TrashFn};
pub use panels::{LayoutMetrics, NewPanel};
pub use ui_request::UiRequest;
