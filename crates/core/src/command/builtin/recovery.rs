//! Groupe `recovery` : modifications retrouvées après un arrêt inattendu.

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(CommandDef::new(
        "recovery",
        "list",
        "Liste les documents non enregistrés retrouvés après un arrêt inattendu",
        |editor, _| {
            let found = editor.found_backups();
            if found.is_empty() {
                return Ok(CommandOutput::message("rien à récupérer"));
            }
            let lines: Vec<String> = found
                .iter()
                .map(|b| match &b.path {
                    Some(path) => format!("{} ({})", b.name, path.display()),
                    None => b.name.clone(),
                })
                .collect();
            Ok(CommandOutput::message(lines.join("\n")))
        },
    ))?;

    registry.register(CommandDef::new(
        "recovery",
        "restore",
        "Restaure les modifications retrouvées : chaque document les reçoit, non enregistrées",
        |editor, _| {
            let found = editor.found_backups().to_vec();
            if found.is_empty() {
                return Ok(CommandOutput::message("rien à récupérer"));
            }
            let count = editor.restore_backups(&found)?;
            Ok(CommandOutput::message(format!(
                "{count} document{} restauré{} : à vérifier, puis à enregistrer",
                if count > 1 { "s" } else { "" },
                if count > 1 { "s" } else { "" }
            )))
        },
    ))?;

    registry.register(CommandDef::new(
        "recovery",
        "discard",
        "Oublie les modifications retrouvées après un arrêt inattendu",
        |editor, _| {
            let found = editor.found_backups().to_vec();
            editor.discard_backups(&found);
            Ok(CommandOutput::message("modifications retrouvées oubliées"))
        },
    ))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::editor::Editor;

    #[test]
    fn commandes() {
        let dir = tempfile::tempdir().unwrap();
        let mut crashed = Editor::new();
        crashed.start_recovery(dir.path().to_path_buf()).unwrap();
        crashed.new_document();
        crashed.execute("edit insert 'perdu ?'").unwrap();
        crashed.backup_documents().unwrap();
        drop(crashed);

        let mut e = Editor::new();
        let found = e.start_recovery(dir.path().to_path_buf()).unwrap();
        e.set_found_backups(found);
        assert_eq!(
            e.execute("recovery list").unwrap(),
            CommandOutput::message("sans titre")
        );
        assert_eq!(
            e.execute("recovery restore").unwrap(),
            CommandOutput::message("1 document restauré : à vérifier, puis à enregistrer")
        );
        assert_eq!(
            e.execute("recovery restore").unwrap(),
            CommandOutput::message("rien à récupérer")
        );
        e.execute("recovery discard").unwrap();
    }
}
