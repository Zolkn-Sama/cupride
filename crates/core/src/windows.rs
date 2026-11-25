//! Fenêtres de l'éditeur : chacune a son layout ; toutes partagent les documents.
//!
//! Le noyau tient le modèle des fenêtres ; l'application ouvre et ferme les fenêtres du système
//! pour qu'elles lui correspondent.

use cupride_layout::{PanelId, WindowId};

use crate::command::CommandError;
use crate::editor::{DocumentId, Editor};

impl Editor {
    /// Fenêtre active : celle qui a le focus.
    pub fn active_window(&self) -> Option<WindowId> {
        self.workspace().active_window()
    }

    /// Ouvre une fenêtre, lui donne le focus, et y affiche un document vide.
    pub fn new_window(&mut self) -> WindowId {
        let window = self.workspace_mut().create_window();
        self.focus_window(window);
        self.new_document();
        window
    }

    /// Donne le focus à la fenêtre `window` (et à son panneau actif). Renvoie faux si elle
    /// n'existe pas.
    pub fn focus_window(&mut self, window: WindowId) -> bool {
        if !self.workspace_mut().focus_window(window) {
            return false;
        }
        self.sync_active_document();
        self.flush_events();
        true
    }

    /// Envoie le panneau `panel` dans la fenêtre `window`, à côté de son panneau actif, et lui
    /// donne le focus. Il garde son document, ses curseurs et son défilement.
    pub fn move_panel_to_window(
        &mut self,
        panel: PanelId,
        window: WindowId,
    ) -> Result<(), CommandError> {
        self.workspace_mut().move_to_window(panel, window)?;
        self.focus_panel(panel);
        Ok(())
    }

    /// Panneaux de la fenêtre `window`.
    pub fn window_panels(&self, window: WindowId) -> Vec<PanelId> {
        self.workspace()
            .window(window)
            .map(|w| w.tree().panels())
            .unwrap_or_default()
    }

    /// Documents qui ne sont affichés que dans la fenêtre `window` : ils seraient fermés avec
    /// elle.
    pub fn documents_only_in(&self, window: WindowId) -> Vec<DocumentId> {
        let panels = self.window_panels(window);
        let mut documents: Vec<DocumentId> = panels
            .iter()
            .filter_map(|p| self.panel_document(*p))
            .filter(|doc| self.panels_showing(*doc).iter().all(|p| panels.contains(p)))
            .collect();
        documents.sort();
        documents.dedup();
        documents
    }

    /// Ferme la fenêtre `window` et ses panneaux. Les documents qui n'étaient affichés que là sont
    /// fermés ; s'il y en a de modifiés, la fermeture est refusée, sauf avec `force`.
    pub fn close_window(&mut self, window: WindowId, force: bool) -> Result<(), CommandError> {
        if self.workspace().window(window).is_none() {
            return Err(CommandError::Failed(format!(
                "fenêtre {window} introuvable"
            )));
        }
        let documents = self.documents_only_in(window);
        let modified: Vec<String> = documents
            .iter()
            .filter_map(|id| self.document(*id))
            .filter(|d| d.is_modified())
            .map(|d| d.display_name())
            .collect();
        if !modified.is_empty() && !force {
            return Err(CommandError::UnsavedChanges {
                name: modified.join("», «"),
            });
        }
        for panel in self.window_panels(window) {
            self.remove_panel(panel);
        }
        self.workspace_mut().remove_window(window);
        for document in documents {
            self.remove_document(document);
        }
        self.sync_active_document();
        self.flush_events();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::command::CommandError;
    use crate::editor::Editor;

    #[test]
    fn nouvelle_fenetre_avec_son_layout() {
        let mut e = Editor::new();
        let doc = e.new_document();
        let first = e.active_window().unwrap();
        let second = e.new_window();
        assert_ne!(first, second);
        assert_eq!(e.active_window(), Some(second));
        assert_eq!(e.window_panels(second).len(), 1, "un document vide");
        assert_ne!(e.active_id(), Some(doc));
        // Ouvrir un document dans la fenêtre active : il y est affiché, pas dans l'autre.
        e.show_document(doc);
        assert_eq!(e.window_panels(second).len(), 2);
        assert_eq!(e.window_panels(first).len(), 1);
        assert_eq!(e.panels_showing(doc).len(), 2, "un document, deux fenêtres");
        assert!(e.focus_window(first));
        assert_eq!(e.active_id(), Some(doc));
        assert!(e.workspace().is_valid());
    }

    #[test]
    fn fermer_une_fenetre() {
        let mut e = Editor::new();
        let shared = e.new_document();
        let first = e.active_window().unwrap();
        let second = e.new_window();
        e.show_document(shared);
        let only_here = e.new_document();
        e.execute("edit insert 'brouillon'").unwrap();
        let only = e.documents_only_in(second);
        assert_eq!(
            only.len(),
            2,
            "le document vide de la fenêtre, et le brouillon"
        );
        assert!(only.contains(&only_here));
        assert!(!only.contains(&shared));
        assert!(matches!(
            e.close_window(second, false),
            Err(CommandError::UnsavedChanges { .. })
        ));
        assert_eq!(e.workspace().windows().len(), 2, "rien n'a changé");
        e.close_window(second, true).unwrap();
        assert_eq!(e.workspace().windows().len(), 1);
        assert_eq!(e.active_window(), Some(first));
        assert!(e.document(shared).is_some(), "affiché ailleurs : gardé");
        assert!(e.document(only_here).is_none());
        assert_eq!(e.active_id(), Some(shared));
        // Dernière fenêtre : plus de fenêtre ni de document actif.
        e.close_window(first, false).unwrap();
        assert_eq!(e.active_window(), None);
        assert_eq!(e.active_id(), None);
        assert_eq!(e.document_count(), 0);
        assert!(e.close_window(first, false).is_err());
        // Ouvrir un document sans fenêtre en ouvre une.
        e.new_document();
        assert!(e.active_window().is_some());
        assert!(e.workspace().is_valid());
    }
}
