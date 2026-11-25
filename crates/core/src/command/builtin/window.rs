//! Groupe `window` : fenêtres.

use clap::{Arg, ArgAction};

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;
use crate::layout::WindowId;
use crate::ui_request::UiRequest;

/// Argument `--monitor N`.
fn monitor_arg(required: bool) -> Arg {
    Arg::new("monitor")
        .long("monitor")
        .value_name("N")
        .required(required)
        .value_parser(clap::value_parser!(usize))
        .help("Numéro de l'écran (voir « window list-monitors »)")
}

/// Écran demandé par `--monitor N`, vérifié.
fn monitor(editor: &Editor, args: &clap::ArgMatches) -> Result<Option<usize>, CommandError> {
    let Some(&number) = args.get_one::<usize>("monitor") else {
        return Ok(None);
    };
    let count = editor.monitors().len();
    if number == 0 || (count > 0 && number > count) {
        return Err(CommandError::Failed(format!(
            "écran {number} introuvable : {count} écran{} (voir « window list-monitors »)",
            if count > 1 { "s" } else { "" }
        )));
    }
    Ok(Some(number))
}

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
    registry.register(
        CommandDef::new(
            "window",
            "new",
            "Ouvre une nouvelle fenêtre, avec un document vide, éventuellement sur l'écran N",
            |editor, args| {
                let monitor = monitor(editor, args)?;
                let window = editor.new_window();
                if let Some(monitor) = monitor {
                    editor.request_ui(UiRequest::PlaceWindow { window, monitor });
                }
                Ok(CommandOutput::message(format!("fenêtre {window} ouverte")))
            },
        )
        .arg(monitor_arg(false)),
    )?;

    registry.register(
        CommandDef::new(
            "window",
            "move",
            "Déplace la fenêtre active (ou la fenêtre N) sur l'écran indiqué",
            |editor, args| {
                let window = window_arg(editor, args, "fenetre")?;
                let monitor = monitor(editor, args)?.expect("argument requis");
                editor.request_ui(UiRequest::PlaceWindow { window, monitor });
                Ok(CommandOutput::None)
            },
        )
        .arg(monitor_arg(true))
        .arg(
            Arg::new("fenetre")
                .value_name("N")
                .value_parser(clap::value_parser!(u64))
                .help("Numéro de la fenêtre ; par défaut, la fenêtre active"),
        ),
    )?;

    registry.register(CommandDef::new(
        "window",
        "list-monitors",
        "Liste les écrans : numéro, nom, résolution, échelle",
        |editor, _| {
            let lines: Vec<String> = editor
                .monitors()
                .iter()
                .enumerate()
                .map(|(i, m)| format!("{} : {}", i + 1, m.describe()))
                .collect();
            Ok(CommandOutput::message(if lines.is_empty() {
                "aucun écran connu".to_owned()
            } else {
                lines.join("\n")
            }))
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

    #[test]
    fn ecrans() {
        use crate::monitors::MonitorInfo;
        use crate::ui_request::UiRequest;
        let screen = |name: &str, x| MonitorInfo {
            name: name.to_owned(),
            x,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
            primary: x == 0,
        };
        let mut e = Editor::new();
        e.new_document();
        e.set_monitors(vec![screen("B", 1920), screen("A", 0)]);
        assert_eq!(
            e.execute("window list-monitors").unwrap(),
            CommandOutput::message(
                "1 : A — 1920×1080, échelle 1, principal\n2 : B — 1920×1080, échelle 1"
            )
        );
        e.execute("window new --monitor 2").unwrap();
        let window = e.active_window().unwrap();
        assert_eq!(
            e.take_ui_requests(),
            [UiRequest::PlaceWindow { window, monitor: 2 }]
        );
        e.execute("window move --monitor 1 1").unwrap();
        assert_eq!(
            e.take_ui_requests(),
            [UiRequest::PlaceWindow {
                window: crate::layout::WindowId::new(1),
                monitor: 1
            }]
        );
        let error = e
            .execute("window move --monitor 3")
            .unwrap_err()
            .to_string();
        assert!(error.contains("écran 3 introuvable : 2 écrans"), "{error}");
        assert!(e.execute("window new --monitor 0").is_err());
        assert_eq!(
            e.workspace().windows().len(),
            2,
            "rien d'ouvert en cas d'erreur"
        );
    }
}
