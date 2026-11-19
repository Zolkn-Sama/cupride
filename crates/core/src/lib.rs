//! Noyau de CuprIDE, sans interface graphique.
//!
//! Contiendra l'état global de l'éditeur, les documents, le registre de
//! commandes, le keymap et la configuration. Ne dépend jamais de `render`,
//! `ui` ou `app`.

pub use cupride_layout as layout;
pub use cupride_text as text;
