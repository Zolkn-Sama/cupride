//! Groupe `keys` : raccourcis actifs.

use clap::Arg;

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;
use crate::keymap::{KeySequence, Platform};

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "keys",
            "list",
            "Affiche tous les raccourcis actifs, triés par commande ; « * » marque ceux de la configuration",
            |editor, args| {
                let filter = args
                    .get_one::<String>("filtre")
                    .map(|f| f.to_lowercase())
                    .unwrap_or_default();
                let user = user_sequences(editor);
                let mut rows: Vec<(String, String, bool)> = editor
                    .keymap()
                    .bindings()
                    .iter()
                    .map(|b| (b.command.clone(), b.keys.to_string(), user.contains(&b.keys)))
                    .filter(|(command, keys, _)| {
                        command.to_lowercase().contains(&filter) || keys.contains(&filter)
                    })
                    .collect();
                if rows.is_empty() {
                    return Ok(CommandOutput::message("aucun raccourci"));
                }
                rows.sort();
                let width = rows.iter().map(|(_, k, _)| k.chars().count()).max().unwrap_or(0);
                let lines: Vec<String> = rows
                    .iter()
                    .map(|(command, keys, custom)| {
                        let mark = if *custom { "*" } else { " " };
                        let pad = width - keys.chars().count();
                        format!("{mark} {keys}{}  {command}", " ".repeat(pad))
                    })
                    .collect();
                Ok(CommandOutput::Message(lines.join("\n")))
            },
        )
        .arg(Arg::new("filtre").help("N'affiche que les raccourcis dont la commande ou les touches contiennent ce texte")),
    )?;

    registry.register(
        CommandDef::new(
            "keys",
            "describe",
            "Indique la commande exécutée par une combinaison de touches",
            |editor, args| {
                let text: &String = args.get_one("touches").expect("argument requis");
                let keys = KeySequence::parse(text, Platform::current()).map_err(|error| {
                    CommandError::InvalidArguments {
                        command: "keys describe".to_owned(),
                        message: error.to_string(),
                    }
                })?;
                Ok(CommandOutput::Message(
                    match editor.keymap().command_for(&keys) {
                        Some(command) => format!("{keys} : {command}"),
                        None => format!("{keys} : aucun raccourci"),
                    },
                ))
            },
        )
        .arg(
            Arg::new("touches")
                .required(true)
                .help("Combinaison ou séquence : « mod+s », « ctrl+k ctrl+w »"),
        ),
    )?;
    Ok(())
}

/// Séquences définies dans la section `[keys]` de la configuration.
fn user_sequences(editor: &Editor) -> Vec<KeySequence> {
    editor
        .config()
        .keys
        .iter()
        .filter(|(_, command)| !command.trim().is_empty())
        .filter_map(|(keys, _)| KeySequence::parse(keys, Platform::current()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::config::Config;
    use crate::editor::Editor;

    fn output(editor: &mut Editor, line: &str) -> String {
        match editor.execute(line).unwrap() {
            CommandOutput::Message(text) => text,
            CommandOutput::None => String::new(),
        }
    }

    #[test]
    fn liste_et_raccourcis_personnels() {
        let mut e = Editor::new();
        let mut config = Config::default();
        config.keys.insert("f5".into(), "file reload".into());
        // Remplace un raccourci par défaut.
        config
            .keys
            .insert("mod+s".into(), "file save-as copie.txt".into());
        assert!(e.apply_config(config).is_empty());

        let list = output(&mut e, "keys list");
        assert!(list.lines().count() > 20, "{list}");
        let reload = list.lines().find(|l| l.contains("file reload")).unwrap();
        assert!(reload.starts_with("* f5"), "{reload}");
        assert!(
            list.lines()
                .any(|l| l.starts_with("* ") && l.ends_with("file save-as copie.txt"))
        );
        assert!(
            !list.lines().any(|l| l.ends_with("  file save")),
            "le défaut est remplacé"
        );
        let undo = list.lines().find(|l| l.ends_with("edit undo")).unwrap();
        assert!(undo.starts_with("  "), "raccourci par défaut non marqué");

        let filtered = output(&mut e, "keys list zoom");
        assert!(filtered.lines().all(|l| l.contains("zoom")), "{filtered}");
        assert_eq!(output(&mut e, "keys list rien-de-tel"), "aucun raccourci");
    }

    #[test]
    fn decrire_des_touches() {
        let mut e = Editor::new();
        let text = output(&mut e, "keys describe mod+z");
        assert!(text.ends_with(": edit undo"), "{text}");
        assert!(output(&mut e, "keys describe f12").ends_with("aucun raccourci"));
        assert!(e.execute("keys describe ctrl+banane").is_err());
    }
}
