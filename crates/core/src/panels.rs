//! Panneaux de l'éditeur : chaque document affiché l'est dans un panneau du workspace.
//!
//! Le modèle des panneaux (identifiant, type, état) vit dans `layout` ; le noyau y ajoute le lien
//! entre un panneau éditeur et son document, qui n'existe qu'en mémoire. Le panneau actif décide
//! du document actif : les commandes d'édition agissent sur le document du panneau qui a le
//! focus, avec les sélections de ce panneau.

use cupride_layout::{Bounds, Direction, LayoutError, PanelId, PanelKind};

use crate::command::CommandError;
use crate::editor::{DocumentId, Editor};

/// Géométrie des zones, fournie par l'interface : le noyau n'a pas de fenêtre, mais le
/// redimensionnement au clavier et la navigation entre zones en ont besoin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutMetrics {
    /// Place de l'ensemble des zones (pixels physiques).
    pub area: Bounds,
    /// Épaisseur des séparations.
    pub gap: f32,
    /// Taille minimale d'une zone, en largeur et en hauteur.
    pub min_size: f32,
}

impl Default for LayoutMetrics {
    /// Valeurs tant que l'interface n'a rien fourni (sans interface : REPL, tests).
    fn default() -> Self {
        Self {
            area: Bounds::new(0.0, 0.0, 1_000.0, 700.0),
            gap: 1.0,
            min_size: 80.0,
        }
    }
}

/// Où placer un nouveau panneau, par rapport au panneau actif.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewPanel {
    /// Nouvel onglet, juste après l'onglet actif.
    Tab,
    /// Nouvelle zone, du côté indiqué de la zone active.
    Split(Direction),
}

impl Editor {
    /// Panneau qui a le focus.
    pub fn active_panel(&self) -> Option<PanelId> {
        self.workspace().active()
    }

    /// Document affiché par le panneau `panel`, si c'est un éditeur.
    pub fn panel_document(&self, panel: PanelId) -> Option<DocumentId> {
        self.panel_documents.get(&panel).copied()
    }

    /// Panneaux qui affichent le document `document`.
    pub fn panels_showing(&self, document: DocumentId) -> Vec<PanelId> {
        let mut panels: Vec<PanelId> = self
            .panel_documents
            .iter()
            .filter(|(_, doc)| **doc == document)
            .map(|(panel, _)| *panel)
            .collect();
        panels.sort();
        panels
    }

    /// Donne le focus au panneau `panel` : il est affiché dans sa zone et, si c'est un éditeur,
    /// son document devient le document actif, avec les sélections de ce panneau. Renvoie
    /// `false` si le panneau n'existe pas.
    pub fn focus_panel(&mut self, panel: PanelId) -> bool {
        if self.workspace_mut().focus(panel).is_err() {
            return false;
        }
        self.sync_active_document();
        self.flush_events();
        true
    }

    /// Affiche le document `document` dans la zone active et lui donne le focus : l'onglet qui
    /// l'affiche déjà dans cette zone, sinon un nouvel onglet. Renvoie le panneau, ou `None` si
    /// le document n'existe pas.
    pub fn show_document(&mut self, document: DocumentId) -> Option<PanelId> {
        self.show_document_as(document, false)
    }

    /// Comme [`Editor::show_document`], en aperçu si l'option `[tabs] preview` est activée : le
    /// document remplace l'onglet d'aperçu de la zone active, ou s'ouvre dans un nouvel onglet
    /// d'aperçu. Le document que l'aperçu affichait est fermé s'il n'est affiché nulle part
    /// ailleurs.
    pub fn show_document_preview(&mut self, document: DocumentId) -> Option<PanelId> {
        self.show_document_as(document, self.config().tabs.preview)
    }

    fn show_document_as(&mut self, document: DocumentId, preview: bool) -> Option<PanelId> {
        self.document(document)?;
        let active = self.active_panel();
        let group = active.and_then(|p| self.workspace().tree().group_of(p));
        let existing = group.and_then(|g| {
            g.panels()
                .iter()
                .copied()
                .find(|p| self.panel_document(*p) == Some(document))
        });
        let preview_panel = group.and_then(|g| g.preview());
        let panel = match (existing, preview_panel) {
            (Some(panel), _) => {
                // Ouvert normalement, un onglet d'aperçu est gardé.
                if !preview {
                    let _ = self.workspace_mut().tree_mut().set_preview(panel, false);
                }
                panel
            }
            (None, Some(panel)) if preview => {
                self.rebind(panel, document);
                panel
            }
            (None, _) => {
                let panel = self.place_editor(document, NewPanel::Tab);
                if preview {
                    let _ = self.workspace_mut().tree_mut().set_preview(panel, true);
                }
                panel
            }
        };
        self.focus_panel(panel);
        Some(panel)
    }

    /// Le panneau éditeur `panel` affiche désormais `document` ; l'ancien document est fermé
    /// s'il n'est plus affiché nulle part et n'a pas de modifications.
    fn rebind(&mut self, panel: PanelId, document: DocumentId) {
        let Some(old) = self.panel_documents.insert(panel, document) else {
            return;
        };
        if let Some(previous) = self.document_mut(old) {
            previous.remove_view(panel);
        }
        if old != document
            && self.panels_showing(old).is_empty()
            && self.document(old).is_some_and(|d| !d.is_modified())
        {
            self.remove_document(old);
        }
    }

    /// Garde les onglets d'aperçu dont le document a été modifié.
    pub(crate) fn keep_modified_previews(&mut self) {
        let previews: Vec<PanelId> = self
            .workspace()
            .tree()
            .groups()
            .iter()
            .filter_map(|g| g.preview())
            .collect();
        for panel in previews {
            let modified = self
                .panel_document(panel)
                .and_then(|doc| self.document(doc))
                .is_some_and(|d| d.is_modified());
            if modified {
                let _ = self.workspace_mut().tree_mut().set_preview(panel, false);
            }
        }
    }

    /// Ouvre un nouveau panneau éditeur sur `document`, placé selon `place`, et lui donne le
    /// focus. Ses sélections partent de celles du document.
    pub fn open_editor_panel(&mut self, document: DocumentId, place: NewPanel) -> Option<PanelId> {
        self.document(document)?;
        let panel = self.place_editor(document, place);
        self.focus_panel(panel);
        Some(panel)
    }

    /// Ouvre un panneau de type `kind` (sans document), placé selon `place`, et lui donne le
    /// focus.
    pub fn open_panel(&mut self, kind: PanelKind, place: NewPanel) -> PanelId {
        let panel = self.workspace_mut().create(kind);
        self.place(panel, place);
        self.focus_panel(panel);
        panel
    }

    /// Crée le panneau éditeur et le place, sans lui donner le focus.
    fn place_editor(&mut self, document: DocumentId, place: NewPanel) -> PanelId {
        let panel = self
            .workspace_mut()
            .create(PanelKind::new(PanelKind::EDITOR));
        self.panel_documents.insert(panel, document);
        self.place(panel, place);
        panel
    }

    /// Place un panneau créé, à côté du panneau actif (ou dans la première zone).
    fn place(&mut self, panel: PanelId, place: NewPanel) {
        let active = self.active_panel();
        let tree = self.workspace_mut().tree_mut();
        let placed = match (place, active) {
            (NewPanel::Split(direction), Some(active)) => tree.split(active, direction, panel),
            (_, active) => tree.add(panel, active),
        };
        if let Err(error) = placed {
            debug_assert!(false, "panneau neuf non placé : {error}");
        }
    }

    /// Ferme le panneau `panel`. Un document qui n'est plus affiché nulle part est fermé avec
    /// lui : s'il a des modifications non enregistrées, la fermeture est refusée, sauf avec
    /// `force`.
    pub fn close_panel(&mut self, panel: PanelId, force: bool) -> Result<(), CommandError> {
        if self.workspace().panel(panel).is_none() {
            return Err(LayoutError::UnknownPanel(panel).into());
        }
        let document = self.panel_document(panel);
        let last_view = document.is_some_and(|doc| self.panels_showing(doc).len() == 1);
        if let Some(doc) = document.filter(|_| last_view)
            && let Some(document) = self.document(doc)
            && document.is_modified()
            && !force
        {
            return Err(CommandError::UnsavedChanges {
                name: document.display_name(),
            });
        }
        self.remove_panel(panel);
        if let Some(doc) = document.filter(|_| last_view) {
            self.remove_document(doc);
        }
        self.sync_active_document();
        self.flush_events();
        Ok(())
    }

    /// Retire le panneau `panel` du workspace et du document qu'il affichait.
    pub(crate) fn remove_panel(&mut self, panel: PanelId) {
        self.workspace_mut().remove(panel);
        if let Some(doc) = self.panel_documents.remove(&panel)
            && let Some(document) = self.document_mut(doc)
        {
            document.remove_view(panel);
        }
    }

    /// Accorde le document actif avec le panneau actif : le document d'un panneau éditeur
    /// devient actif, avec les sélections de ce panneau. Un autre type de panneau laisse le
    /// document actif tel quel, s'il existe encore.
    pub(crate) fn sync_active_document(&mut self) {
        let panel = self.active_panel();
        if let Some(panel) = panel
            && let Some(doc) = self.panel_document(panel)
        {
            if let Some(document) = self.document_mut(doc) {
                document.focus_view(panel);
            }
            self.set_active_raw(Some(doc));
            return;
        }
        let current = self.active_id().filter(|id| self.document(*id).is_some());
        let fallback = current.or_else(|| self.panel_documents.values().next().copied());
        self.set_active_raw(fallback);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{Edit, EditKind, Selections, TextBuffer};

    fn type_text(editor: &mut Editor, text: &str) {
        editor
            .active_document_mut()
            .unwrap()
            .transact(EditKind::Insert, |b, tx| {
                let at = tx.selections_after().primary().head;
                tx.apply(b, Edit::insert(at, text))
            })
            .unwrap();
    }

    #[test]
    fn chaque_document_ouvert_a_son_panneau() {
        let mut editor = Editor::new();
        let a = editor.new_document();
        let pa = editor.active_panel().unwrap();
        assert_eq!(editor.panel_document(pa), Some(a));
        let b = editor.new_document();
        let pb = editor.active_panel().unwrap();
        assert_ne!(pa, pb);
        assert_eq!(
            editor.workspace().tree().panels(),
            vec![pa, pb],
            "en onglets"
        );
        // Réafficher a reprend son onglet, sans en créer un autre.
        assert!(editor.set_active(a));
        assert_eq!(editor.active_panel(), Some(pa));
        assert_eq!(editor.workspace().panel_count(), 2);
        assert!(editor.focus_panel(pb));
        assert_eq!(editor.active_id(), Some(b));
        assert!(editor.workspace().is_valid());
    }

    #[test]
    fn meme_document_deux_panneaux() {
        let mut editor = Editor::new();
        let doc = editor.new_document();
        type_text(&mut editor, "abc");
        let left = editor.active_panel().unwrap();
        let right = editor
            .open_editor_panel(doc, NewPanel::Split(Direction::Right))
            .unwrap();
        assert_eq!(editor.document_count(), 1, "un seul document en mémoire");
        assert_eq!(editor.panels_showing(doc), vec![left, right]);
        // Chacun ses curseurs.
        editor
            .active_document_mut()
            .unwrap()
            .set_selections(Selections::cursor(0));
        type_text(&mut editor, ">");
        let document = editor.document(doc).unwrap();
        assert_eq!(document.buffer().to_text(), ">abc");
        assert_eq!(document.selections_in(right), &Selections::cursor(1));
        assert_eq!(
            document.selections_in(left),
            &Selections::cursor(4),
            "décalé"
        );
        editor.focus_panel(left);
        assert_eq!(
            editor.active_document().unwrap().selections(),
            &Selections::cursor(4)
        );
    }

    #[test]
    fn fermer_un_panneau() {
        let mut editor = Editor::new();
        let doc = editor.new_document();
        let first = editor.active_panel().unwrap();
        let second = editor
            .open_editor_panel(doc, NewPanel::Split(Direction::Down))
            .unwrap();
        type_text(&mut editor, "brouillon");
        // Le document reste ouvert tant qu'un panneau l'affiche.
        editor.close_panel(second, false).unwrap();
        assert_eq!(editor.document_count(), 1);
        assert_eq!(editor.active_panel(), Some(first));
        // Dernière vue d'un document modifié : refusé sans --force.
        assert!(matches!(
            editor.close_panel(first, false),
            Err(CommandError::UnsavedChanges { .. })
        ));
        editor.close_panel(first, true).unwrap();
        assert_eq!(editor.document_count(), 0);
        assert_eq!(editor.active_id(), None);
        assert!(editor.workspace().tree().is_empty());
        assert!(editor.close_panel(first, true).is_err());
    }

    #[test]
    fn fermer_un_document_ferme_ses_panneaux() {
        let mut editor = Editor::new();
        let a = editor.new_document();
        editor
            .open_editor_panel(a, NewPanel::Split(Direction::Right))
            .unwrap();
        let b = editor.new_document();
        let pb = editor.active_panel().unwrap();
        editor.close(a);
        assert_eq!(editor.workspace().tree().panels(), vec![pb]);
        assert_eq!(editor.active_id(), Some(b));
        assert!(editor.workspace().is_valid());
    }

    #[test]
    fn onglet_d_apercu() {
        let mut editor = Editor::new();
        let mut config = editor.config().clone();
        config.tabs.preview = true;
        editor.apply_config(config);
        let first = editor.new_document();
        let a = editor.create_document();
        let b = editor.create_document();
        let preview = editor.show_document_preview(a).unwrap();
        let group = |editor: &Editor| editor.workspace().tree().groups()[0].clone();
        assert_eq!(group(&editor).preview(), Some(preview));
        // L'aperçu suivant remplace le premier, qui est fermé.
        assert_eq!(editor.show_document_preview(b), Some(preview));
        assert_eq!(editor.panel_document(preview), Some(b));
        assert!(editor.document(a).is_none());
        assert_eq!(group(&editor).len(), 2);
        // Modifié, l'onglet est gardé : l'aperçu suivant ouvre un nouvel onglet.
        editor.execute("edit insert 'x'").unwrap();
        assert_eq!(group(&editor).preview(), None);
        let c = editor.create_document();
        let next = editor.show_document_preview(c).unwrap();
        assert_ne!(next, preview);
        assert_eq!(group(&editor).len(), 3);
        // Ouvert normalement, l'aperçu est gardé.
        editor.show_document(c);
        assert_eq!(group(&editor).preview(), None);
        assert!(editor.document(first).is_some());

        // Option désactivée : jamais d'aperçu.
        let mut editor = Editor::new();
        let doc = editor.new_document();
        editor.show_document_preview(doc);
        assert_eq!(group(&editor).preview(), None);
    }

    #[test]
    fn panneau_sans_document() {
        let mut editor = Editor::new();
        let doc = editor.new_document();
        let explorer = editor.open_panel(
            PanelKind::new(PanelKind::EXPLORER),
            NewPanel::Split(Direction::Left),
        );
        assert_eq!(editor.active_panel(), Some(explorer));
        assert_eq!(editor.panel_document(explorer), None);
        assert_eq!(editor.active_id(), Some(doc), "le document actif reste");
        editor.close_panel(explorer, false).unwrap();
        assert_eq!(editor.document_count(), 1);
    }
}
