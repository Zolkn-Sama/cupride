//! Demandes faites à l'interface par les commandes.
//!
//! Le noyau n'a pas d'interface : une commande comme `ui command-line` dépose une demande, que
//! l'application lit après chaque commande (`Editor::take_ui_requests`) et exécute.

/// Ce que l'interface doit faire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiRequest {
    /// Ouvrir la ligne de commande, préremplie avec ce texte.
    CommandLine(String),
    /// Ouvrir la palette de commandes.
    Palette,
    /// Ouvrir l'ouverture rapide de fichiers.
    QuickOpen,
    /// Ouvrir la barre de recherche, préremplie avec ce motif.
    Find(String),
    /// Montrer ce fichier ou ce dossier dans l'explorateur (dossiers parents dépliés).
    Reveal(std::path::PathBuf),
    /// Placer la fenêtre sur l'écran `monitor` (numéro, à partir de 1).
    PlaceWindow {
        /// Fenêtre à placer.
        window: cupride_layout::WindowId,
        /// Numéro de l'écran.
        monitor: usize,
    },
}
