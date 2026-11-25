//! Groupe `layout` : layouts nommés, pour basculer d'une disposition à l'autre.
//!
//! Un layout nommé a le format d'une session (zones, onglets, fichiers, curseurs, défilement). Il
//! est enregistré en JSON dans le dossier de données de l'OS, par projet :
//! `layouts/<projet>/<nom>.json`.

use std::fs;
use std::io;
use std::path::PathBuf;

use clap::{Arg, ArgAction};

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::{DocumentId, Editor};
use crate::notifications::Level;
use crate::session;

fn name_arg() -> Arg {
    Arg::new("nom")
        .required(true)
        .help("Nom du layout : lettres, chiffres, espaces, - et _")
}

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "layout",
            "save",
            "Enregistre la disposition actuelle sous un nom (remplace un layout du même nom)",
            |editor, args| {
                let name = valid_name(args.get_one::<String>("nom").expect("requis"))?;
                let path = layout_path(editor, name)?;
                session::save(&editor.capture_session(), &path)
                    .map_err(|e| CommandError::Failed(e.to_string()))?;
                Ok(CommandOutput::message(format!(
                    "layout « {name} » enregistré"
                )))
            },
        )
        .arg(name_arg()),
    )?;

    registry.register(
        CommandDef::new(
            "layout",
            "load",
            "Remplace la disposition actuelle par un layout enregistré",
            |editor, args| {
                let name = valid_name(args.get_one::<String>("nom").expect("requis"))?;
                load(editor, name, args.get_flag("force"))?;
                Ok(CommandOutput::message(format!("layout « {name} » chargé")))
            },
        )
        .arg(name_arg())
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Ferme les documents modifiés que le layout n'affiche pas, sans les enregistrer"),
        ),
    )?;

    registry.register(CommandDef::new(
        "layout",
        "list",
        "Liste les layouts enregistrés pour ce projet",
        |editor, _| {
            let names = names(editor)?;
            if names.is_empty() {
                Ok(CommandOutput::message(
                    "aucun layout enregistré (voir « layout save »)",
                ))
            } else {
                Ok(CommandOutput::message(names.join("\n")))
            }
        },
    ))?;

    registry.register(
        CommandDef::new(
            "layout",
            "delete",
            "Supprime un layout enregistré",
            |editor, args| {
                let name = valid_name(args.get_one::<String>("nom").expect("requis"))?;
                let path = layout_path(editor, name)?;
                match fs::remove_file(&path) {
                    Ok(()) => Ok(CommandOutput::message(format!(
                        "layout « {name} » supprimé"
                    ))),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        Err(unknown(editor, name))
                    }
                    Err(error) => Err(CommandError::Failed(format!(
                        "{} : {error}",
                        path.display()
                    ))),
                }
            },
        )
        .arg(name_arg()),
    )?;

    Ok(())
}

/// Vérifie un nom de layout : il sert de nom de fichier.
fn valid_name(name: &str) -> Result<&str, CommandError> {
    let name = name.trim();
    let valid = !name.is_empty()
        && name.chars().count() <= 64
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '));
    if valid {
        Ok(name)
    } else {
        Err(CommandError::Failed(format!(
            "nom de layout invalide : « {name} » (lettres, chiffres, espaces, - et _)"
        )))
    }
}

/// Dossier des layouts du projet ouvert.
fn layouts_dir(editor: &Editor) -> Result<PathBuf, CommandError> {
    let root = editor.layouts_dir().ok_or_else(|| {
        CommandError::Failed("dossier de données introuvable : layouts indisponibles".into())
    })?;
    let project = session::session_file_name(editor.project());
    let project = project.strip_suffix(".json").unwrap_or(&project);
    Ok(root.join(project))
}

fn layout_path(editor: &Editor, name: &str) -> Result<PathBuf, CommandError> {
    Ok(layouts_dir(editor)?.join(format!("{name}.json")))
}

/// Noms des layouts enregistrés, triés.
fn names(editor: &Editor) -> Result<Vec<String>, CommandError> {
    let dir = layouts_dir(editor)?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(CommandError::Failed(format!("{} : {error}", dir.display())));
        }
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".json").map(ToOwned::to_owned)
        })
        .collect();
    names.sort();
    Ok(names)
}

/// Layout inconnu, avec le nom le plus proche.
fn unknown(editor: &Editor, name: &str) -> CommandError {
    let closest = names(editor)
        .unwrap_or_default()
        .into_iter()
        .map(|n| (strsim::levenshtein(&n, name), n))
        .min()
        .filter(|(distance, _)| *distance <= 3)
        .map(|(_, n)| format!(" (vouliez-vous dire « {n} » ?)"))
        .unwrap_or_default();
    CommandError::Failed(format!(
        "layout inconnu : « {name} »{closest} ; voir « layout list »"
    ))
}

/// Charge le layout `name`. Les documents qu'il n'affiche pas sont fermés ; un document modifié
/// ne l'est jamais sans `force` : la commande échoue en le nommant.
fn load(editor: &mut Editor, name: &str, force: bool) -> Result<(), CommandError> {
    let path = layout_path(editor, name)?;
    let saved = session::load(&path)
        .map_err(|e| CommandError::Failed(e.to_string()))?
        .ok_or_else(|| unknown(editor, name))?;
    // Fichiers que le layout affichera.
    let shown: Vec<PathBuf> = saved
        .workspace
        .panels()
        .filter_map(|p| p.state_as::<session::EditorState>())
        .filter_map(|state| crate::editor::normalize(&state.path).ok())
        .collect();
    let hidden: Vec<DocumentId> = editor
        .documents()
        .filter(|(_, doc)| doc.path().is_none_or(|p| !shown.iter().any(|s| s == p)))
        .map(|(id, _)| id)
        .collect();
    let modified: Vec<String> = hidden
        .iter()
        .filter_map(|id| editor.document(*id))
        .filter(|doc| doc.is_modified())
        .map(|doc| doc.display_name())
        .collect();
    if !modified.is_empty() && !force {
        return Err(CommandError::Failed(format!(
            "{} : modifications non enregistrées que ce layout n'affiche pas ; enregistrer \
             d'abord, ou ajouter --force pour les abandonner",
            modified
                .iter()
                .map(|n| format!("« {n} »"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    for problem in editor.restore_session(saved) {
        editor.notify(Level::Warning, problem);
    }
    for id in hidden {
        if editor.panels_showing(id).is_empty() {
            editor.close(id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::command::CommandOutput;
    use crate::editor::Editor;

    fn editor_in(dir: &std::path::Path) -> Editor {
        let mut editor = Editor::new();
        editor.set_layouts_dir(Some(dir.join("layouts")));
        editor
    }

    fn message(output: CommandOutput) -> String {
        match output {
            CommandOutput::Message(m) => m,
            CommandOutput::None => String::new(),
        }
    }

    #[test]
    fn enregistrer_lister_charger_supprimer() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.rs"), dir.path().join("b.rs"));
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let mut e = editor_in(dir.path());
        assert_eq!(
            message(e.execute("layout list").unwrap()),
            "aucun layout enregistré (voir « layout save »)"
        );
        e.open(&a).unwrap();
        e.execute("layout save écriture").unwrap();
        e.execute("split right").unwrap();
        e.open(&b).unwrap();
        e.execute("layout save 'revue de code'").unwrap();
        assert_eq!(
            message(e.execute("layout list").unwrap()),
            "revue de code\nécriture"
        );

        e.execute("layout load écriture").unwrap();
        assert_eq!(e.workspace().tree().groups().len(), 1);
        assert_eq!(e.document_count(), 1, "b, plus affiché, est fermé");
        e.execute("layout load 'revue de code'").unwrap();
        assert_eq!(e.workspace().tree().groups().len(), 2);
        assert_eq!(e.active_document().unwrap().display_name(), "b.rs");
        assert!(e.workspace().is_valid());

        let error = e.execute("layout load ecriture").unwrap_err().to_string();
        assert!(error.contains("vouliez-vous dire « écriture »"), "{error}");
        e.execute("layout delete écriture").unwrap();
        assert!(e.execute("layout delete écriture").is_err());
        assert_eq!(message(e.execute("layout list").unwrap()), "revue de code");
        assert!(
            e.execute("layout save ../evasion").is_err(),
            "nom de fichier sûr"
        );
        assert!(e.execute("layout save ''").is_err());
    }

    #[test]
    fn document_modifie_jamais_ferme_sans_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.rs"), dir.path().join("b.rs"));
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let mut e = editor_in(dir.path());
        e.open(&a).unwrap();
        e.execute("layout save seul-a").unwrap();
        e.open(&b).unwrap();
        e.execute("edit insert 'modif '").unwrap();
        let error = e.execute("layout load seul-a").unwrap_err().to_string();
        assert!(error.contains("« b.rs »"), "{error}");
        assert_eq!(e.document_count(), 2, "rien n'a changé");
        assert_eq!(e.workspace().panel_count(), 2);
        // Un document modifié que le layout affiche ne pose pas de problème.
        e.execute("file save").unwrap();
        e.execute("layout save a-et-b").unwrap();
        e.execute("edit insert 'encore '").unwrap();
        e.execute("layout load a-et-b").unwrap();
        assert!(e.active_document().unwrap().is_modified(), "gardé tel quel");
        // Avec --force, les modifications sont abandonnées.
        e.execute("layout load seul-a --force").unwrap();
        assert_eq!(e.document_count(), 1);
    }
}
