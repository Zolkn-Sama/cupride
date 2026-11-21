//! Groupe `theme` : thèmes de couleurs.

use clap::Arg;

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "theme",
            "set",
            "Change de thème (pour cette session ; [view] theme dans la configuration pour le garder)",
            |editor, args| {
                let name: &String = args.get_one("nom").expect("argument requis");
                editor.set_theme(name)?;
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("nom")
                .required(true)
                .help("Identifiant du thème : padparadscha, clair, sombre, ou un thème de l'utilisateur"),
        ),
    )?;

    registry.register(CommandDef::new(
        "theme",
        "list",
        "Liste les thèmes disponibles ; « > » marque le thème en cours",
        |editor, _| {
            let lines: Vec<String> = editor
                .themes()
                .iter()
                .map(|t| {
                    let mark = if t == editor.theme() { ">" } else { " " };
                    format!("{mark} {t}")
                })
                .collect();
            Ok(CommandOutput::Message(if lines.is_empty() {
                format!("> {}", editor.theme())
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
    fn changer_de_theme() {
        let mut e = Editor::new();
        assert_eq!(e.theme(), "padparadscha");
        e.set_themes(vec!["clair".into(), "padparadscha".into(), "sombre".into()]);
        e.execute("theme set Clair").unwrap();
        assert_eq!(e.theme(), "clair");
        let error = e.execute("theme set sombr").unwrap_err().to_string();
        assert!(error.contains("vouliez-vous dire « sombre »"), "{error}");
        assert_eq!(
            e.execute("theme list").unwrap(),
            CommandOutput::Message("> clair\n  padparadscha\n  sombre".into())
        );
        // La configuration rechargée remet son thème.
        e.apply_config(crate::config::Config::default());
        assert_eq!(e.theme(), "padparadscha");
    }
}
