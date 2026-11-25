//! Groupe `view` : affichage, indépendant du document.

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::{Editor, ZOOM_STEPS};

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(CommandDef::new(
        "view",
        "zoom-in",
        "Agrandit l'interface d'un cran",
        |editor, _| Ok(zoom_to(editor, next_step(editor.zoom(), true))),
    ))?;

    registry.register(CommandDef::new(
        "view",
        "zoom-out",
        "Réduit l'interface d'un cran",
        |editor, _| Ok(zoom_to(editor, next_step(editor.zoom(), false))),
    ))?;

    registry.register(CommandDef::new(
        "view",
        "zoom-reset",
        "Revient à la taille normale de l'interface (100 %)",
        |editor, _| Ok(zoom_to(editor, 1.0)),
    ))?;

    registry.register(
        CommandDef::new(
            "view",
            "wrap",
            "Retour à la ligne visuel des lignes trop longues : on, off, ou toggle (par défaut)",
            |editor, args| {
                let language = editor
                    .active_document()
                    .and_then(|d| d.path())
                    .and_then(crate::language::language_of);
                let current = editor.wrap_for(language);
                let wrap = match args.get_one::<String>("etat").map(String::as_str) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => !current,
                };
                editor.set_wrap_override(Some(wrap));
                Ok(CommandOutput::message(if wrap {
                    "retour à la ligne : activé"
                } else {
                    "retour à la ligne : désactivé"
                }))
            },
        )
        .arg(
            clap::Arg::new("etat")
                .value_parser(clap::builder::PossibleValuesParser::new([
                    "on", "off", "toggle",
                ]))
                .help("on, off ou toggle"),
        ),
    )?;

    Ok(())
}

/// Cran suivant (ou précédent) de la liste, à partir du zoom actuel ; reste au bout de la liste.
fn next_step(zoom: f32, up: bool) -> f32 {
    let found = if up {
        ZOOM_STEPS.iter().copied().find(|&step| step > zoom + 1e-3)
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&step| step < zoom - 1e-3)
    };
    found.unwrap_or(zoom)
}

fn zoom_to(editor: &mut Editor, zoom: f32) -> CommandOutput {
    editor.set_zoom(zoom);
    CommandOutput::message(format!("zoom : {} %", (editor.zoom() * 100.0).round()))
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::editor::Editor;

    #[test]
    fn crans_de_zoom() {
        let mut e = Editor::new();
        assert_eq!(e.zoom(), 1.0);
        assert_eq!(
            e.execute("view zoom-in").unwrap(),
            CommandOutput::message("zoom : 110 %")
        );
        e.execute("view zoom-in").unwrap();
        assert_eq!(e.zoom(), 1.25);
        for _ in 0..20 {
            e.execute("view zoom-in").unwrap();
        }
        assert_eq!(e.zoom(), 3.0, "plafond");
        e.execute("view zoom-reset").unwrap();
        assert_eq!(e.zoom(), 1.0);
        for _ in 0..20 {
            e.execute("view zoom-out").unwrap();
        }
        assert_eq!(e.zoom(), 0.5, "plancher");
        // Un zoom hors de la liste reprend au cran le plus proche dans le bon sens.
        e.set_zoom(1.05);
        e.execute("view zoom-in").unwrap();
        assert_eq!(e.zoom(), 1.1);
    }

    #[test]
    fn retour_a_la_ligne() {
        let mut e = Editor::new();
        assert!(!e.wrap_for(None));
        assert_eq!(
            e.execute("view wrap").unwrap(),
            CommandOutput::message("retour à la ligne : activé")
        );
        assert!(e.wrap_for(Some("Rust")));
        e.execute("view wrap off").unwrap();
        assert!(!e.wrap_for(None));
    }
}
