//! Identité des panneaux.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Identifiant stable d'un panneau. Il n'est jamais réutilisé pour un autre panneau du même
/// workspace, et survit à la sauvegarde du layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PanelId(u64);

impl PanelId {
    /// Identifiant de valeur `value`.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Valeur de l'identifiant.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PanelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
