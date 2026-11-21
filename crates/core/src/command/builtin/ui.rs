//! Groupe `ui` : éléments de l'interface (ligne de commande, palette…).

use clap::Arg;

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::ui_request::UiRequest;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "ui",
            "command-line",
            "Ouvre la ligne de commande, éventuellement préremplie",
            |editor, args| {
                let text = args.get_one::<String>("texte").cloned().unwrap_or_default();
                editor.request_ui(UiRequest::CommandLine(text));
                Ok(CommandOutput::None)
            },
        )
        .arg(Arg::new("texte").help("Texte placé dans la ligne de commande")),
    )?;
    registry.register(CommandDef::new(
        "ui",
        "palette",
        "Ouvre la palette : recherche floue parmi toutes les commandes",
        |editor, _| {
            editor.request_ui(UiRequest::Palette);
            Ok(CommandOutput::None)
        },
    ))?;
    registry.register(CommandDef::new(
        "ui",
        "quick-open",
        "Ouvre un fichier du projet en tapant une partie de son chemin (recherche floue)",
        |editor, _| {
            editor.request_ui(UiRequest::QuickOpen);
            Ok(CommandOutput::None)
        },
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::editor::Editor;
    use crate::ui_request::UiRequest;

    #[test]
    fn demande_d_ouverture() {
        let mut e = Editor::new();
        e.execute("ui command-line 'file open '").unwrap();
        assert_eq!(
            e.take_ui_requests(),
            [UiRequest::CommandLine("file open ".into())]
        );
        assert!(e.take_ui_requests().is_empty(), "lues une seule fois");
    }
}
