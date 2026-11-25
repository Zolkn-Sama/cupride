//! Ce qu'une fenêtre affiche, pour ne redessiner que les fenêtres concernées par un changement.
//!
//! Après une commande, la fenêtre courante est toujours redessinée ; une autre fenêtre ne l'est
//! que si son empreinte a changé : son layout, son panneau actif, ou le texte, l'état modifié ou
//! les curseurs d'un document qu'elle affiche. Modifier un document affiché dans deux fenêtres
//! redessine les deux ; modifier un document que l'autre fenêtre n'affiche pas ne la touche pas.

use cupride_core::layout::{LayoutTree, PanelId, WindowId};
use cupride_core::text::Selections;
use cupride_core::{DocumentId, Editor};

/// Ce que l'onglet d'un document montre : document, révision du texte, état modifié et
/// sélections du panneau.
type Shown = (DocumentId, u64, bool, Selections);

/// Empreinte de ce qu'une fenêtre affiche.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stamp {
    tree: LayoutTree,
    active: Option<PanelId>,
    focused: bool,
    /// Pour chaque panneau : son document, son état modifié et, pour un onglet affiché, la
    /// révision du texte et les sélections du panneau.
    panels: Vec<(PanelId, Option<Shown>)>,
    /// Apparence (thème, police, zoom) : change pour toutes les fenêtres à la fois.
    look: u64,
}

impl Stamp {
    /// Empreinte de la fenêtre `window`, ou `None` si elle n'existe plus.
    pub(crate) fn of(editor: &Editor, window: WindowId, look: u64) -> Option<Self> {
        let layout = editor.workspace().window(window)?;
        let tree = layout.tree().clone();
        // Tous les onglets montrent l'état modifié de leur document ; seuls les onglets affichés
        // montrent son texte et leurs curseurs.
        let shown: Vec<PanelId> = tree.groups().iter().filter_map(|g| g.active()).collect();
        let panels = tree
            .panels()
            .into_iter()
            .map(|panel| {
                let document = editor.panel_document(panel).and_then(|id| {
                    let document = editor.document(id)?;
                    let visible = shown.contains(&panel);
                    Some((
                        id,
                        if visible { document.revision() } else { 0 },
                        document.is_modified(),
                        if visible {
                            document.selections_in(panel).clone()
                        } else {
                            Selections::cursor(0)
                        },
                    ))
                });
                (panel, document)
            })
            .collect();
        Some(Self {
            active: layout.active(),
            focused: editor.active_window() == Some(window),
            tree,
            panels,
            look,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seules_les_fenetres_concernees_changent() {
        let mut editor = Editor::new();
        let shared = editor.new_document();
        let first = editor.active_window().unwrap();
        let second = editor.new_window();
        editor.show_document(shared);
        let alone = editor.new_document();
        let stamps = |editor: &Editor| {
            (
                Stamp::of(editor, first, 0).unwrap(),
                Stamp::of(editor, second, 0).unwrap(),
            )
        };

        // Un document que la première fenêtre n'affiche pas : elle ne change pas.
        let before = stamps(&editor);
        editor.execute("edit insert 'x'").unwrap();
        assert_eq!(editor.active_id(), Some(alone));
        let after = stamps(&editor);
        assert_eq!(before.0, after.0);
        assert_ne!(before.1, after.1);

        // Le document partagé : les deux fenêtres changent.
        editor.show_document(shared);
        let before = stamps(&editor);
        editor.execute("edit insert 'y'").unwrap();
        let after = stamps(&editor);
        assert_ne!(before.0, after.0, "même document dans l'autre fenêtre");
        assert_ne!(before.1, after.1);

        // L'apparence change partout.
        assert_ne!(Stamp::of(&editor, first, 0), Stamp::of(&editor, first, 1));
        editor.close_window(second, true).unwrap();
        assert!(Stamp::of(&editor, second, 0).is_none());
    }
}
