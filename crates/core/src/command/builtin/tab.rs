//! Groupe `tab` : onglets de la zone active.

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(CommandDef::new(
        "tab",
        "next",
        "Affiche l'onglet suivant de la zone active (revient au premier après le dernier)",
        |editor, _| cycle(editor, true),
    ))?;
    registry.register(CommandDef::new(
        "tab",
        "previous",
        "Affiche l'onglet précédent de la zone active",
        |editor, _| cycle(editor, false),
    ))?;
    registry.register(CommandDef::new(
        "tab",
        "move-left",
        "Déplace l'onglet actif d'un rang vers la gauche",
        |editor, _| shift(editor, false),
    ))?;
    registry.register(CommandDef::new(
        "tab",
        "move-right",
        "Déplace l'onglet actif d'un rang vers la droite",
        |editor, _| shift(editor, true),
    ))?;
    registry.register(CommandDef::new(
        "tab",
        "keep",
        "Garde l'onglet d'aperçu actif : il ne sera plus remplacé",
        |editor, _| {
            let panel = editor.active_panel().ok_or(CommandError::NoActivePanel)?;
            editor
                .workspace_mut()
                .tree_mut()
                .set_preview(panel, false)?;
            Ok(CommandOutput::None)
        },
    ))?;
    Ok(())
}

fn cycle(editor: &mut Editor, forward: bool) -> Result<CommandOutput, CommandError> {
    let panel = editor.active_panel().ok_or(CommandError::NoActivePanel)?;
    let next = editor.workspace_mut().tree_mut().cycle(panel, forward)?;
    editor.focus_panel(next);
    Ok(CommandOutput::None)
}

fn shift(editor: &mut Editor, right: bool) -> Result<CommandOutput, CommandError> {
    let panel = editor.active_panel().ok_or(CommandError::NoActivePanel)?;
    let tree = editor.workspace_mut().tree_mut();
    let index = tree
        .group_of(panel)
        .and_then(|g| g.panels().iter().position(|p| *p == panel))
        .unwrap_or_default();
    let index = if right {
        index + 1
    } else {
        index.saturating_sub(1)
    };
    tree.reorder(panel, index)?;
    Ok(CommandOutput::None)
}

#[cfg(test)]
mod tests {
    use crate::editor::Editor;

    fn tabs(e: &Editor) -> Vec<u64> {
        e.workspace().tree().groups()[0]
            .panels()
            .iter()
            .map(|p| p.get())
            .collect()
    }

    #[test]
    fn suivant_precedent_reordonner() {
        let mut e = Editor::new();
        for _ in 0..3 {
            e.new_document();
        }
        assert_eq!(e.active_panel().unwrap().get(), 3);
        e.execute("tab next").unwrap();
        assert_eq!(
            e.active_panel().unwrap().get(),
            1,
            "après le dernier, le premier"
        );
        let doc = e.panel_document(e.active_panel().unwrap());
        assert_eq!(e.active_id(), doc, "le document actif suit");
        e.execute("tab previous").unwrap();
        assert_eq!(e.active_panel().unwrap().get(), 3);
        e.execute("tab move-left").unwrap();
        assert_eq!(tabs(&e), vec![1, 3, 2]);
        e.execute("tab move-left").unwrap();
        e.execute("tab move-left").unwrap();
        assert_eq!(tabs(&e), vec![3, 1, 2], "reste au bord");
        e.execute("tab move-right").unwrap();
        assert_eq!(tabs(&e), vec![1, 3, 2]);
        assert_eq!(
            e.active_panel().unwrap().get(),
            3,
            "l'onglet actif ne change pas"
        );
        e.execute("tab keep").unwrap();
    }

    #[test]
    fn sans_panneau() {
        let mut e = Editor::new();
        assert!(e.execute("tab next").is_err());
        assert!(e.execute("tab move-right").is_err());
    }
}
