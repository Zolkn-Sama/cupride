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

/// Type de panneau : `editor`, `explorer`, ou un type fourni par un plugin. La vue de chaque type
/// est fabriquée par l'interface, à partir de son registre.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PanelKind(String);

impl PanelKind {
    /// Éditeur de texte : affiche un document.
    pub const EDITOR: &'static str = "editor";
    /// Explorateur de fichiers du projet.
    pub const EXPLORER: &'static str = "explorer";
    /// Résultats de la recherche dans le projet.
    pub const SEARCH: &'static str = "search";

    /// Type nommé `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Nom du type.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Est-ce un éditeur de texte ?
    pub fn is_editor(&self) -> bool {
        self.0 == Self::EDITOR
    }
}

impl fmt::Display for PanelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Modèle d'un panneau : identité, type et état sérialisable. Il ne sait rien de son affichage,
/// qui est l'affaire de sa vue, dans l'interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    id: PanelId,
    kind: PanelKind,
    /// État propre au type (document affiché, défilement…), libre pour les types des plugins.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    state: serde_json::Value,
}

impl Panel {
    /// Panneau `id` de type `kind`, sans état.
    pub fn new(id: PanelId, kind: PanelKind) -> Self {
        Self {
            id,
            kind,
            state: serde_json::Value::Null,
        }
    }

    /// Identifiant.
    pub fn id(&self) -> PanelId {
        self.id
    }

    pub(crate) fn set_id(&mut self, id: PanelId) {
        self.id = id;
    }

    /// Type.
    pub fn kind(&self) -> &PanelKind {
        &self.kind
    }

    /// État sérialisé.
    pub fn state(&self) -> &serde_json::Value {
        &self.state
    }

    /// Remplace l'état.
    pub fn set_state(&mut self, state: serde_json::Value) {
        self.state = state;
    }

    /// État lu comme un `T` ; `None` s'il est absent ou d'une autre forme.
    pub fn state_as<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_value(self.state.clone()).ok()
    }

    /// Enregistre `state` comme état du panneau.
    pub fn store_state<T: Serialize>(&mut self, state: &T) {
        match serde_json::to_value(state) {
            Ok(value) => self.state = value,
            Err(error) => {
                debug_assert!(false, "état de panneau non sérialisable : {error}");
            }
        }
    }
}
