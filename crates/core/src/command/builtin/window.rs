//! Groupe `window` : fenêtres.

use clap::{Arg, ArgAction};

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;
use crate::layout::WindowId;

/// Fenêtre de l'argument `name` (son numéro), ou la fenêtre active.
fn window_arg(
    editor: &Editor,
    args: &clap::ArgMatches,
    name: &str,
) -> Result<WindowId, CommandError> {
    match args.get_one::<u64>(name) {
        Some(&number) => {
            let id = WindowId::new(number);
            editor
                .workspace()
                .window(id)
                .map(|w| w.id())
                .ok_or_else(|| {
                    CommandError::Failed(format!(
                        "fenêtre {number} introuvable ; voir « window list »"
                    ))
                })
        }
        None => editor
            .active_window()
            .ok_or_else(|| CommandError::Failed("aucune fenêtre".into())),
    }
}

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(CommandDef::new(
        "window",
        "new",
        "Ouvre une nouvelle fenêtre, avec un document vide",
        |editor, _| {
            let window = editor.new_window();
            Ok(CommandOutput::message(format!("fenêtre {window} ouverte")))
        },
    ))?;

    registry.register(
        CommandDef::new(
            "window",
            "close",
            "Ferme la fenêtre active (ou la fenêtre N) ; ses documents qui ne sont affichés nulle part ailleurs sont fermés",
            |editor, args| {
                let window = window_arg(editor, args, "fenetre")?;
                editor.close_window(window, args.get_flag("force"))?;
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("fenetre")
                .value_name("N")
                .value_parser(clap::value_parser!(u64))
                .help("Numéro de la fenêtre (voir « window list ») ; par défaut, la fenêtre active"),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Ferme même si des documents ont des modifications non enregistrées"),
        ),
    )?;

    registry.register(
        CommandDef::new(
            "window",
            "focus",
            "Donne le focus à la fenêtre N",
            |editor, args| {
                let window = window_arg(editor, args, "fenetre")?;
                editor.focus_window(window);
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("fenetre")
                .value_name("N")
                .required(true)
                .value_parser(clap::value_parser!(u64))
                .help("Numéro de la fenêtre (voir « window list »)"),
        ),
    )?;

    registry.register(CommandDef::new(
        "window",
        "list",
        "Liste les fenêtres : numéro, document actif, nombre de panneaux (> active)",
        |editor, _| {
            let active = editor.active_window();
            let lines: Vec<String> = editor
                .workspace()
                .windows()
                .iter()
                .map(|w| {
                    let marker = if Some(w.id()) == active { '>' } else { ' ' };
                    let shown = w
                        .active()
                        .and_then(|p| editor.panel_document(p))
                        .and_then(|d| editor.document(d))
                        .map_or_else(|| "—".to_owned(), |d| d.display_name());
                    let panels = w.tree().panels().len();
                    let plural = if panels > 1 { "x" } else { "" };
                    format!("{marker} {} : {shown} ({panels} panneau{plural})", w.id())
                })
                .collect();
            Ok(CommandOutput::message(if lines.is_empty() {
                "aucune fenêtre".to_owned()
            } else {
                lines.join("\n")
            }))
        },
    ))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::editor::Editor;

    #[test]
    fn commandes_des_fenetres() {
        let mut e = Editor::new();
        e.new_document();
        assert_eq!(
            e.execute("window new").unwrap(),
            CommandOutput::message("fenêtre 2 ouverte")
        );
        assert_eq!(
            e.execute("window list").unwrap(),
            CommandOutput::message("  1 : sans titre (1 panneau)\n> 2 : sans titre (1 panneau)")
        );
        e.execute("window focus 1").unwrap();
        assert_eq!(e.active_window().unwrap().get(), 1);
        let error = e.execute("window focus 7").unwrap_err().to_string();
        assert!(error.contains("fenêtre 7 introuvable"), "{error}");
        e.execute("window close 2").unwrap();
        assert_eq!(e.workspace().windows().len(), 1);
        e.execute("window close").unwrap();
        assert_eq!(
            e.execute("window list").unwrap(),
            CommandOutput::message("aucune fenêtre")
        );
    }
}
