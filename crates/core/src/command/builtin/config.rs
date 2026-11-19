//! Groupe `config` : fichier de configuration.

use std::fs;

use cupride_text::{Edit, EditKind};

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::config::DEFAULT_CONFIG_TEMPLATE;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(CommandDef::new(
        "config",
        "open",
        "Ouvre le fichier de configuration ; s'il n'existe pas, il est prérempli avec les options par défaut",
        |editor, _| {
            let path = editor
                .config_path()
                .map(ToOwned::to_owned)
                .ok_or_else(|| CommandError::Failed("dossier de configuration introuvable".into()))?;
            let exists = path.exists();
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(|source| crate::DocumentError::Io {
                    path: dir.to_owned(),
                    source,
                })?;
            }
            let id = editor.open(&path)?;
            let document = editor.document_mut(id).ok_or(CommandError::NoActiveDocument)?;
            if !exists && document.buffer().to_string().is_empty() {
                document.transact(EditKind::Other, |buffer, tx| {
                    tx.apply(buffer, Edit::insert(0, DEFAULT_CONFIG_TEMPLATE))
                })?;
                document.set_selections(cupride_text::Selections::cursor(0));
            }
            Ok(CommandOutput::message(format!("{} ouvert", path.display())))
        },
    ))?;

    registry.register(CommandDef::new(
        "config",
        "reload",
        "Relit le fichier de configuration et l'applique",
        |editor, _| {
            let messages = editor.reload_config();
            Ok(CommandOutput::message(if messages.is_empty() {
                "configuration rechargée".to_owned()
            } else {
                messages.join("\n")
            }))
        },
    ))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::command::CommandOutput;
    use crate::config::DEFAULT_CONFIG_TEMPLATE;
    use crate::keymap::{KeyMatch, KeySequence, Platform};
    use crate::{Editor, LineEnding};

    fn editor_with_config(text: Option<&str>) -> (Editor, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cupride").join("config.toml");
        if let Some(text) = text {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
        }
        let mut editor = Editor::new();
        editor.set_config_path(Some(path));
        (editor, dir)
    }

    fn keys(text: &str) -> KeySequence {
        KeySequence::parse(text, Platform::current()).unwrap()
    }

    #[test]
    fn ouvrir_un_fichier_absent_le_preremplit() {
        let (mut editor, _dir) = editor_with_config(None);
        editor.execute("config open").unwrap();
        let document = editor.active_document().unwrap();
        assert_eq!(document.buffer().to_string(), DEFAULT_CONFIG_TEMPLATE);
        editor.execute("file save").unwrap();
        assert_eq!(
            fs::read_to_string(editor.config_path().unwrap()).unwrap(),
            DEFAULT_CONFIG_TEMPLATE
        );
    }

    #[test]
    fn ouvrir_un_fichier_existant_ne_le_modifie_pas() {
        let (mut editor, _dir) = editor_with_config(Some("[editor]\n"));
        editor.execute("config open").unwrap();
        let document = editor.active_document().unwrap();
        assert_eq!(document.buffer().to_string(), "[editor]\n");
        assert!(!document.is_modified());
    }

    #[test]
    fn recharger_applique_la_configuration() {
        let (mut editor, _dir) = editor_with_config(Some(
            "[editor]\nline_ending = \"crlf\"\n\n[keys]\n\"mod+shift+s\" = \"file save-as copie.txt\"\n\"mod+s\" = \"\"\n",
        ));
        assert_eq!(
            editor.execute("config reload").unwrap(),
            CommandOutput::message("configuration rechargée")
        );
        assert_eq!(
            editor.keymap().lookup(&keys("mod+shift+s").0),
            KeyMatch::Command("file save-as copie.txt")
        );
        assert_eq!(editor.keymap().lookup(&keys("mod+s").0), KeyMatch::None);
        // Les autres raccourcis par défaut sont conservés.
        assert_eq!(
            editor.keymap().lookup(&keys("mod+z").0),
            KeyMatch::Command("edit undo")
        );
        let id = editor.new_document();
        assert_eq!(editor.document(id).unwrap().line_ending(), LineEnding::Crlf);
    }

    #[test]
    fn erreur_de_configuration_signalee_valeurs_par_defaut() {
        let (mut editor, _dir) = editor_with_config(Some("[editor\n"));
        let CommandOutput::Message(message) = editor.execute("config reload").unwrap() else {
            panic!("message attendu");
        };
        assert!(message.contains("config.toml:1:"), "{message}");
        assert!(
            message.contains("valeurs par défaut utilisées"),
            "{message}"
        );
        assert_eq!(editor.config(), &crate::config::Config::default());
    }

    #[test]
    fn raccourci_invalide_signale() {
        let (mut editor, _dir) =
            editor_with_config(Some("[keys]\n\"ctrl+banane\" = \"file save\"\n"));
        let CommandOutput::Message(message) = editor.execute("config reload").unwrap() else {
            panic!("message attendu");
        };
        assert_eq!(
            message,
            "raccourci « ctrl+banane » ignoré : touche inconnue : « banane »"
        );
    }

    #[test]
    fn delai_de_regroupement_applique() {
        let (mut editor, _dir) = editor_with_config(Some("[editor]\nundo_group_delay_ms = 0\n"));
        let id = editor.new_document();
        editor.execute("config reload").unwrap();
        assert_eq!(
            editor.document(id).unwrap().history().group_delay(),
            std::time::Duration::ZERO
        );
    }
}
