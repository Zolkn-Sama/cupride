//! Erreurs du rendu.

use thiserror::Error;

/// Erreur d'initialisation ou de rendu.
#[derive(Debug, Error)]
pub enum RenderError {
    /// Aucune carte graphique utilisable, même avec le repli OpenGL.
    #[error(
        "aucune carte graphique compatible trouvée (essayé : {tried}). Vérifier les pilotes graphiques, ou choisir un backend avec WGPU_BACKEND (vulkan, metal, dx12, gl)"
    )]
    NoAdapter {
        /// Backends essayés.
        tried: String,
    },

    /// La carte graphique a refusé de créer un device.
    #[error("la carte graphique « {adapter} » n'a pas pu être initialisée : {source}")]
    Device {
        /// Nom de la carte.
        adapter: String,
        /// Erreur d'origine.
        #[source]
        source: wgpu::RequestDeviceError,
    },

    /// La surface de la fenêtre n'a pas pu être créée.
    #[error("impossible de dessiner dans la fenêtre : {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),

    /// La surface n'accepte aucun format d'image utilisable.
    #[error("la fenêtre n'accepte aucun format d'image compatible avec la carte graphique")]
    NoSurfaceFormat,

    /// Échec du dessin d'une frame.
    #[error("échec du dessin : {0}")]
    Draw(String),

    /// Lecture d'une image rendue impossible.
    #[error("lecture de l'image rendue impossible : {0}")]
    Readback(String),
}
