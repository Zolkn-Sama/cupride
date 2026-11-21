//! Groupe `notifications` : historique des notifications.

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::notifications::age;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(CommandDef::new(
        "notifications",
        "list",
        "Affiche l'historique des notifications, de la plus récente à la plus ancienne",
        |editor, _| {
            let lines: Vec<String> = editor
                .notifications()
                .history()
                .rev()
                .map(|n| format!("[{}] {} ({})", n.level, n.message, age(n.at.elapsed())))
                .collect();
            Ok(if lines.is_empty() {
                CommandOutput::message("aucune notification")
            } else {
                CommandOutput::Message(lines.join("\n"))
            })
        },
    ))?;

    registry.register(CommandDef::new(
        "notifications",
        "clear",
        "Vide l'historique des notifications",
        |editor, _| {
            editor.notifications_mut().clear();
            Ok(CommandOutput::None)
        },
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::editor::Editor;
    use crate::notifications::Level;

    #[test]
    fn historique() {
        let mut e = Editor::new();
        assert_eq!(
            e.execute("notifications list").unwrap(),
            CommandOutput::message("aucune notification")
        );
        e.notify(Level::Info, "enregistré");
        e.notify(Level::Error, "échec");
        let CommandOutput::Message(list) = e.execute("notifications list").unwrap() else {
            panic!("liste attendue");
        };
        assert_eq!(
            list,
            "[erreur] échec (à l'instant)\n[info] enregistré (à l'instant)"
        );
        e.execute("notifications clear").unwrap();
        assert_eq!(e.notifications().history().count(), 0);
    }
}
