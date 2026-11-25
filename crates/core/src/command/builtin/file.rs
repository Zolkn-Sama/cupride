//! Groupe `file` : ouvrir, enregistrer, fermer et recharger des fichiers.

use std::path::PathBuf;

use clap::{Arg, ArgAction};

use super::active;
use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::normalize;

fn path_arg(help: &'static str) -> Arg {
    Arg::new("chemin")
        .required(true)
        .value_parser(clap::value_parser!(PathBuf))
        .value_hint(clap::ValueHint::AnyPath)
        .help(help)
}

fn force_arg(help: &'static str) -> Arg {
    Arg::new("force")
        .long("force")
        .action(ArgAction::SetTrue)
        .help(help)
}

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "file",
            "open",
            "Ouvre un fichier (créé au premier enregistrement s'il n'existe pas)",
            |editor, args| {
                let path: &PathBuf = args.get_one("chemin").expect("argument requis");
                let path = editor.resolve_path(path);
                let id = if args.get_flag("preview") {
                    editor.open_preview(&path)?
                } else {
                    editor.open(&path)?
                };
                let name = editor
                    .document(id)
                    .map(|d| d.display_name())
                    .unwrap_or_default();
                Ok(CommandOutput::message(format!("« {name} » ouvert")))
            },
        )
        .arg(path_arg("Fichier à ouvrir"))
        .arg(
            Arg::new("preview")
                .long("preview")
                .action(ArgAction::SetTrue)
                .help("Ouvre dans l'onglet d'aperçu, si l'option [tabs] preview est activée"),
        ),
    )?;

    registry.register(CommandDef::new(
        "file",
        "save",
        "Enregistre le document actif",
        |editor, _| {
            let (_, document) = active(editor)?;
            document.save()?;
            Ok(CommandOutput::message(format!(
                "« {} » enregistré",
                document.display_name()
            )))
        },
    ))?;

    registry.register(
        CommandDef::new(
            "file",
            "save-as",
            "Enregistre le document actif sous un autre chemin, qui devient son fichier",
            |editor, args| {
                let path: &PathBuf = args.get_one("chemin").expect("argument requis");
                let path = editor.resolve_path(path);
                let path = normalize(&path).map_err(|source| crate::DocumentError::Io {
                    path: path.clone(),
                    source,
                })?;
                let (_, document) = active(editor)?;
                document.save_as(path)?;
                Ok(CommandOutput::message(format!(
                    "« {} » enregistré",
                    document.display_name()
                )))
            },
        )
        .arg(path_arg("Nouveau chemin du document")),
    )?;

    registry.register(
        CommandDef::new(
            "file",
            "close",
            "Ferme le document actif",
            |editor, args| {
                let (id, document) = active(editor)?;
                let name = document.display_name();
                if document.is_modified() && !args.get_flag("force") {
                    return Err(CommandError::UnsavedChanges { name });
                }
                editor.close(id);
                Ok(CommandOutput::message(format!("« {name} » fermé")))
            },
        )
        .arg(force_arg(
            "Ferme même si des modifications ne sont pas enregistrées",
        )),
    )?;

    registry.register(
        CommandDef::new(
            "file",
            "reload",
            "Recharge le document actif depuis le disque",
            |editor, args| {
                let (_, document) = active(editor)?;
                let name = document.display_name();
                if document.is_modified() && !args.get_flag("force") {
                    return Err(CommandError::UnsavedChanges { name });
                }
                document.reload()?;
                Ok(CommandOutput::message(format!("« {name} » rechargé")))
            },
        )
        .arg(force_arg(
            "Recharge même si des modifications ne sont pas enregistrées",
        )),
    )?;

    registry.register(CommandDef::new(
        "file",
        "list",
        "Liste les documents ouverts (> actif, * modifié)",
        |editor, _| {
            let active = editor.active_id();
            let lines: Vec<String> = editor
                .documents()
                .map(|(id, doc)| {
                    let marker = if Some(id) == active { '>' } else { ' ' };
                    let modified = if doc.is_modified() { " *" } else { "" };
                    let location = doc
                        .path()
                        .map_or_else(String::new, |p| format!("  ({})", p.display()));
                    format!("{marker} {}{modified}{location}", doc.display_name())
                })
                .collect();
            if lines.is_empty() {
                Ok(CommandOutput::message("aucun document ouvert"))
            } else {
                Ok(CommandOutput::message(lines.join("\n")))
            }
        },
    ))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use cupride_text::{Edit, EditKind};

    use crate::command::{CommandError, CommandOutput};
    use crate::editor::Editor;

    fn type_text(editor: &mut Editor, text: &str) {
        editor
            .active_document_mut()
            .unwrap()
            .transact(EditKind::Insert, |b, tx| tx.apply(b, Edit::insert(0, text)))
            .unwrap();
    }

    fn quoted(path: &std::path::Path) -> String {
        format!("'{}'", path.display())
    }

    #[test]
    fn ouvrir_modifier_enregistrer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "monde").unwrap();
        let mut editor = Editor::new();
        assert_eq!(
            editor
                .execute(&format!("file open {}", quoted(&path)))
                .unwrap(),
            CommandOutput::message("« a.txt » ouvert")
        );
        type_text(&mut editor, "bonjour ");
        editor.execute("file save").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "bonjour monde");
        assert!(!editor.active_document().unwrap().is_modified());
    }

    #[test]
    fn enregistrer_sous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nouveau.rs");
        let mut editor = Editor::new();
        editor.new_document();
        type_text(&mut editor, "fn main() {}");
        assert!(matches!(
            editor.execute("file save"),
            Err(CommandError::Document(crate::DocumentError::NoPath))
        ));
        editor
            .execute(&format!("file save-as {}", quoted(&path)))
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "fn main() {}");
        // Le document est maintenant associé au fichier : le rouvrir ne crée pas de doublon.
        editor
            .execute(&format!("file open {}", quoted(&path)))
            .unwrap();
        assert_eq!(editor.document_count(), 1);
    }

    #[test]
    fn fermer_demande_confirmation() {
        let mut editor = Editor::new();
        editor.new_document();
        type_text(&mut editor, "brouillon");
        let error = editor.execute("file close").unwrap_err();
        assert!(matches!(error, CommandError::UnsavedChanges { .. }));
        assert_eq!(
            error.to_string(),
            "« sans titre » a des modifications non enregistrées : enregistrer d'abord, ou ajouter --force"
        );
        assert_eq!(editor.document_count(), 1);
        editor.execute("file close --force").unwrap();
        assert_eq!(editor.document_count(), 0);
        assert!(matches!(
            editor.execute("file close"),
            Err(CommandError::NoActiveDocument)
        ));
    }

    #[test]
    fn fermer_sans_modification() {
        let mut editor = Editor::new();
        editor.new_document();
        editor.execute("file close").unwrap();
        assert_eq!(editor.document_count(), 0);
    }

    #[test]
    fn recharger() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "v1").unwrap();
        let mut editor = Editor::new();
        editor
            .execute(&format!("file open {}", quoted(&path)))
            .unwrap();
        fs::write(&path, "v2").unwrap();
        editor.execute("file reload").unwrap();
        assert_eq!(editor.active_document().unwrap().buffer().to_string(), "v2");
        type_text(&mut editor, "x");
        assert!(matches!(
            editor.execute("file reload"),
            Err(CommandError::UnsavedChanges { .. })
        ));
        editor.execute("file reload --force").unwrap();
        assert_eq!(editor.active_document().unwrap().buffer().to_string(), "v2");
    }

    #[test]
    fn lister() {
        let mut editor = Editor::new();
        assert_eq!(
            editor.execute("file list").unwrap(),
            CommandOutput::message("aucun document ouvert")
        );
        editor.new_document();
        type_text(&mut editor, "x");
        editor.new_document();
        assert_eq!(
            editor.execute("file list").unwrap(),
            CommandOutput::message("  sans titre *\n> sans titre")
        );
    }
}
