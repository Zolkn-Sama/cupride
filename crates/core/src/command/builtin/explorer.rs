//! Groupe `explorer` : fichiers du projet (créer, renommer, déplacer, supprimer) et panneau de
//! l'explorateur.
//!
//! Un chemin relatif part du dossier du projet (sinon du dossier courant). Les documents ouverts
//! suivent leur fichier renommé ou déplacé ; un fichier supprimé part dans la corbeille de l'OS.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use clap::Arg;

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;
use crate::layout::{Axis, Direction, PanelKind};
use crate::panels::NewPanel;
use crate::ui_request::UiRequest;

/// Part de la fenêtre donnée à l'explorateur quand il s'ouvre.
const EXPLORER_WIDTH: f32 = 0.22;

fn path_arg(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .required(true)
        .value_parser(clap::value_parser!(PathBuf))
        .value_hint(clap::ValueHint::AnyPath)
        .help(help)
}

/// Erreur d'une opération sur `path`.
fn failed(path: &Path, error: &io::Error) -> CommandError {
    CommandError::Failed(format!("{} : {error}", path.display()))
}

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(CommandDef::new(
        "explorer",
        "show",
        "Affiche l'explorateur de fichiers (à gauche s'il n'est pas ouvert) et lui donne le focus",
        |editor, _| {
            show(editor);
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "explorer",
        "reveal",
        "Montre le document actif dans l'explorateur",
        |editor, _| {
            let path = editor
                .active_document()
                .and_then(|d| d.path())
                .map(Path::to_path_buf)
                .ok_or_else(|| {
                    CommandError::Failed("le document actif n'a pas de fichier".into())
                })?;
            show(editor);
            editor.request_ui(UiRequest::Reveal(path));
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(
        CommandDef::new(
            "explorer",
            "new-file",
            "Crée un fichier vide et l'ouvre",
            |editor, args| {
                let path = resolve(editor, args.get_one::<PathBuf>("chemin").expect("requis"));
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|e| failed(&path, &e))?;
                editor.open(&path)?;
                editor.request_ui(UiRequest::Reveal(path.clone()));
                Ok(CommandOutput::message(format!("{} créé", path.display())))
            },
        )
        .arg(path_arg("chemin", "Fichier à créer")),
    )?;

    registry.register(
        CommandDef::new(
            "explorer",
            "new-folder",
            "Crée un dossier",
            |editor, args| {
                let path = resolve(editor, args.get_one::<PathBuf>("chemin").expect("requis"));
                fs::create_dir(&path).map_err(|e| failed(&path, &e))?;
                editor.request_ui(UiRequest::Reveal(path.clone()));
                Ok(CommandOutput::message(format!("{} créé", path.display())))
            },
        )
        .arg(path_arg("chemin", "Dossier à créer")),
    )?;

    for (name, description) in [
        (
            "rename",
            "Renomme un fichier ou un dossier ; les documents ouverts suivent",
        ),
        (
            "move",
            "Déplace un fichier ou un dossier (dans un dossier existant : à l'intérieur) ; les documents ouverts suivent",
        ),
    ] {
        registry.register(
            CommandDef::new("explorer", name, description, move |editor, args| {
                let from = resolve(editor, args.get_one::<PathBuf>("source").expect("requis"));
                let mut to = resolve(editor, args.get_one::<PathBuf>("cible").expect("requis"));
                if name == "move"
                    && to.is_dir()
                    && let Some(file_name) = from.file_name()
                {
                    to = to.join(file_name);
                }
                rename(editor, &from, &to)?;
                editor.request_ui(UiRequest::Reveal(to.clone()));
                Ok(CommandOutput::message(format!(
                    "{} → {}",
                    from.display(),
                    to.display()
                )))
            })
            .arg(path_arg("source", "Fichier ou dossier"))
            .arg(path_arg("cible", "Nouveau chemin")),
        )?;
    }

    registry.register(
        CommandDef::new(
            "explorer",
            "delete",
            "Met un fichier ou un dossier à la corbeille de l'OS",
            |editor, args| {
                let path = resolve(editor, args.get_one::<PathBuf>("chemin").expect("requis"));
                if fs::symlink_metadata(&path).is_err() {
                    return Err(CommandError::Failed(format!(
                        "{} n'existe pas",
                        path.display()
                    )));
                }
                editor.trash(&path).map_err(|e| failed(&path, &e))?;
                // Les documents des fichiers supprimés sont fermés, sauf s'ils sont modifiés.
                let gone: Vec<_> = editor
                    .documents()
                    .filter(|(_, d)| {
                        !d.is_modified() && d.path().is_some_and(|p| p.starts_with(&path))
                    })
                    .map(|(id, _)| id)
                    .collect();
                for id in gone {
                    editor.close(id);
                }
                Ok(CommandOutput::message(format!(
                    "{} mis à la corbeille",
                    path.display()
                )))
            },
        )
        .arg(path_arg("chemin", "Fichier ou dossier à supprimer")),
    )?;

    Ok(())
}

/// Chemin absolu : un chemin relatif part du dossier du terminal (commande venue d'un
/// terminal), sinon du projet, sinon du dossier courant.
fn resolve(editor: &Editor, path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else if let Some(base) = editor.base_dir() {
        base.join(path)
    } else if let Some(project) = editor.project() {
        project.join(path)
    } else {
        std::path::absolute(path).unwrap_or_else(|_| path.to_owned())
    };
    crate::editor::normalize(&absolute).unwrap_or(absolute)
}

/// Renomme `from` en `to` (jamais par-dessus un fichier existant) et fait suivre les documents.
fn rename(editor: &mut Editor, from: &Path, to: &Path) -> Result<(), CommandError> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(CommandError::Failed(format!(
            "{} existe déjà",
            to.display()
        )));
    }
    fs::rename(from, to).map_err(|e| failed(from, &e))?;
    let moved: Vec<_> = editor
        .documents()
        .filter_map(|(id, d)| {
            let relative = d.path()?.strip_prefix(from).ok()?;
            Some((id, to.join(relative)))
        })
        .collect();
    for (id, path) in moved {
        if let Some(document) = editor.document_mut(id) {
            document.set_path(path);
        }
    }
    Ok(())
}

/// Donne le focus à l'explorateur ; s'il n'y en a pas, l'ouvre à gauche, sur une colonne étroite.
fn show(editor: &mut Editor) {
    let existing = editor
        .workspace()
        .panels()
        .find(|p| p.kind().as_str() == PanelKind::EXPLORER)
        .map(|p| p.id());
    if let Some(panel) = existing {
        editor.focus_panel(panel);
        return;
    }
    let had_panels = !editor.workspace().tree().is_empty();
    let place = if had_panels {
        NewPanel::Split(Direction::Left)
    } else {
        NewPanel::Tab
    };
    let panel = editor.open_panel(PanelKind::new(PanelKind::EXPLORER), place);
    if had_panels {
        let metrics = editor.layout_metrics();
        let width = metrics.area.width;
        let current = editor
            .workspace()
            .tree()
            .areas(metrics.area, metrics.gap)
            .iter()
            .find(|a| a.group.contains(panel))
            .map_or(width / 2.0, |a| a.bounds.width);
        editor.workspace_mut().tree_mut().resize(
            panel,
            Axis::Horizontal,
            width * EXPLORER_WIDTH - current,
            metrics.area,
            metrics.gap,
            metrics.min_size,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use crate::editor::Editor;
    use crate::text::TextBuffer;
    use crate::ui_request::UiRequest;

    /// Éditeur sur un projet temporaire, avec une corbeille factice qui note les chemins.
    fn setup() -> (
        tempfile::TempDir,
        Editor,
        Arc<Mutex<Vec<std::path::PathBuf>>>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut editor = Editor::new();
        editor.set_project(dir.path()).unwrap();
        let trashed = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&trashed);
        editor.set_trash(Box::new(move |path| {
            seen.lock().unwrap().push(path.to_owned());
            if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
        }));
        (dir, editor, trashed)
    }

    #[test]
    fn creer_renommer_deplacer() {
        let (dir, mut e, _) = setup();
        let root = dir.path().canonicalize().unwrap();
        e.execute("explorer new-folder src").unwrap();
        e.execute("explorer new-file src/a.rs").unwrap();
        assert!(root.join("src/a.rs").is_file());
        assert_eq!(e.active_document().unwrap().display_name(), "a.rs");
        assert!(
            e.execute("explorer new-file src/a.rs").is_err(),
            "existe déjà"
        );
        e.execute("edit insert 'fn a() {}'").unwrap();
        // Le document suit son fichier renommé, puis son dossier déplacé.
        e.execute("explorer rename src/a.rs src/b.rs").unwrap();
        assert_eq!(
            e.active_document().unwrap().path(),
            Some(root.join("src/b.rs").as_path())
        );
        e.execute("explorer new-folder lib").unwrap();
        e.execute("explorer move src lib").unwrap();
        let moved = root.join("lib/src/b.rs");
        assert_eq!(e.active_document().unwrap().path(), Some(moved.as_path()));
        e.execute("file save").unwrap();
        assert_eq!(fs::read_to_string(&moved).unwrap(), "fn a() {}");
        assert!(
            e.execute("explorer rename lib/src/b.rs lib").is_err(),
            "jamais par-dessus"
        );
        let requests = e.take_ui_requests();
        assert!(
            requests.contains(&UiRequest::Reveal(root.join("lib/src"))),
            "{requests:?}"
        );
    }

    #[test]
    fn supprimer_vers_la_corbeille() {
        let (dir, mut e, trashed) = setup();
        let root = dir.path().canonicalize().unwrap();
        fs::write(root.join("propre.txt"), "x").unwrap();
        fs::write(root.join("modifie.txt"), "y").unwrap();
        e.open(root.join("propre.txt")).unwrap();
        let modified = e.open(root.join("modifie.txt")).unwrap();
        e.execute("edit insert 'z'").unwrap();
        e.execute("explorer delete propre.txt").unwrap();
        e.execute("explorer delete modifie.txt").unwrap();
        assert_eq!(trashed.lock().unwrap().len(), 2);
        assert_eq!(e.document_count(), 1, "le document modifié reste ouvert");
        assert_eq!(e.document(modified).unwrap().buffer().to_text(), "zy");
        assert!(e.execute("explorer delete absent.txt").is_err());
        // Sans corbeille fournie par l'application : refusé, rien n'est supprimé.
        let mut bare = Editor::new();
        fs::write(root.join("garde.txt"), "g").unwrap();
        let path = root.join("garde.txt");
        assert!(
            bare.execute(&format!("explorer delete '{}'", path.display()))
                .is_err()
        );
        assert!(path.exists());
    }

    #[test]
    fn afficher_l_explorateur() {
        let (_dir, mut e, _) = setup();
        e.new_document();
        e.execute("explorer show").unwrap();
        let explorer = e.active_panel().unwrap();
        assert_eq!(
            e.workspace().panel(explorer).unwrap().kind().as_str(),
            "explorer"
        );
        let metrics = e.layout_metrics();
        let areas = e.workspace().tree().areas(metrics.area, metrics.gap);
        assert!(
            (areas[0].bounds.width - 220.0).abs() < 2.0,
            "{:?}",
            areas[0].bounds
        );
        // Déjà ouvert : il reprend le focus, sans second explorateur.
        e.execute("panel focus right").unwrap();
        e.execute("explorer show").unwrap();
        assert_eq!(e.active_panel(), Some(explorer));
        assert_eq!(e.workspace().panel_count(), 2);
        assert!(
            e.execute("explorer reveal").is_err(),
            "le document actif n'a pas de fichier"
        );
    }
}
