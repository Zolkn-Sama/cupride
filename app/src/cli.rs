//! Ligne de commande de `cupride` : fichiers et dossier à ouvrir au lancement.
//!
//! ```text
//! cupride                      document vide
//! cupride src/main.rs          ouvre le fichier (créé au premier enregistrement s'il n'existe pas)
//! cupride .                    ouvre le dossier comme projet
//! cupride . README.md a.rs     dossier du projet et fichiers ; le premier fichier est actif
//! cupride --no-restore         sans restaurer la session du projet
//! cupride --version | --help
//! ```
//!
//! La session du projet (zones, onglets, fichiers) est restaurée avant d'ouvrir les fichiers
//! demandés, qui s'ajoutent dans la zone active.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, Command, value_parser};
use cupride_core::Editor;
use cupride_core::command::parser::describe_clap_error;
use cupride_core::session;

/// Lancement demandé.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Launch {
    /// Fichiers ou dossier.
    pub(crate) paths: Vec<PathBuf>,
    /// Restaurer la session du projet (sauf `--no-restore`).
    pub(crate) restore: bool,
}

/// Ce que demande la ligne de commande.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cli {
    /// Lancer l'IDE sur ces chemins (fichiers ou dossier).
    Run(Launch),
    /// Afficher ce texte sur la sortie standard et quitter (aide, version).
    Print(String),
    /// Ligne de commande invalide : message d'erreur, à afficher avant de quitter avec le code 2.
    Error(String),
}

fn command() -> Command {
    Command::new("cupride")
        .version(env!("CARGO_PKG_VERSION"))
        .about("CuprIDE, IDE léger, rapide et entièrement personnalisable")
        .help_template(
            "{about}\n\nUtilisation : {usage}\n\n{all-args}\n\nUn chemin inexistant ouvre un nouveau document, créé au premier enregistrement.",
        )
        .override_usage("cupride [--no-restore] [chemin…]")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("chemins")
                .num_args(0..)
                .value_parser(value_parser!(PathBuf))
                .help("Fichiers à ouvrir ; un dossier est ouvert comme projet")
                .help_heading("Arguments"),
        )
        .arg(
            Arg::new("no-restore")
                .long("no-restore")
                .action(ArgAction::SetTrue)
                .help("Démarre sans restaurer l'espace de travail du projet")
                .help_heading("Options"),
        )
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help("Affiche cette aide")
                .help_heading("Options"),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help("Affiche la version")
                .help_heading("Options"),
        )
}

/// Analyse les arguments (sans le nom du programme).
pub(crate) fn parse<I, T>(args: I) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args = std::iter::once(OsString::from("cupride")).chain(args.into_iter().map(Into::into));
    match command().try_get_matches_from(args) {
        Ok(matches) => Cli::Run(Launch {
            paths: matches
                .get_many::<PathBuf>("chemins")
                .map(|paths| paths.cloned().collect())
                .unwrap_or_default(),
            restore: !matches.get_flag("no-restore"),
        }),
        Err(error) => match error.kind() {
            clap::error::ErrorKind::DisplayHelp => Cli::Print(error.render().to_string()),
            clap::error::ErrorKind::DisplayVersion => {
                Cli::Print(format!("cupride {}\n", env!("CARGO_PKG_VERSION")))
            }
            // Le message indique déjà de voir « cupride --help ».
            _ => Cli::Error(describe_clap_error("cupride", &error).to_string()),
        },
    }
}

/// Ouvre les chemins demandés : le dossier devient le projet ; la session du projet est
/// restaurée depuis le dossier `sessions` s'il est donné ; les fichiers sont ouverts et le premier
/// devient actif. Sans fichier ni session, un document vide est créé. Renvoie les problèmes à
/// signaler ; l'IDE démarre quand même.
pub(crate) fn open(editor: &mut Editor, paths: &[PathBuf], sessions: Option<&Path>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut first = None;
    for path in paths.iter().filter(|p| p.is_dir()) {
        if let Some(project) = editor.project() {
            problems.push(format!(
                "{} ignoré : un seul dossier par fenêtre ({} est déjà ouvert)",
                path.display(),
                project.display()
            ));
        } else if let Err(error) = editor.set_project(path) {
            problems.push(format!("{} : {error}", path.display()));
        }
    }
    if let Some(sessions) = sessions {
        let file = sessions.join(session::session_file_name(editor.project()));
        match session::load(&file) {
            Ok(Some(saved)) => {
                tracing::info!(session = %file.display(), "session restaurée");
                problems.extend(editor.restore_session(saved));
            }
            Ok(None) => {}
            Err(error) => problems.push(error.to_string()),
        }
    }
    for path in paths.iter().filter(|p| !p.is_dir()) {
        match editor.open(path) {
            Ok(id) => {
                first.get_or_insert(id);
            }
            Err(error) => problems.push(error.to_string()),
        }
    }
    match first {
        Some(id) => {
            editor.set_active(id);
        }
        None if editor.workspace().tree().is_empty() => {
            editor.new_document();
        }
        None => {}
    }
    problems
}

/// Nom affiché d'un projet : le nom de son dossier.
pub(crate) fn project_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyse() {
        let run = |paths: &[&str], restore| {
            Cli::Run(Launch {
                paths: paths.iter().map(PathBuf::from).collect(),
                restore,
            })
        };
        assert_eq!(parse::<_, &str>([]), run(&[], true));
        assert_eq!(parse(["a.rs", "dossier"]), run(&["a.rs", "dossier"], true));
        assert_eq!(parse(["--no-restore", "a.rs"]), run(&["a.rs"], false));
        assert_eq!(
            parse(["--version"]),
            Cli::Print(format!("cupride {}\n", env!("CARGO_PKG_VERSION")))
        );
        let Cli::Print(help) = parse(["-h"]) else {
            panic!("aide attendue");
        };
        assert!(
            help.contains("Utilisation : cupride [--no-restore] [chemin…]"),
            "{help}"
        );
        assert!(help.contains("ouvert comme projet"), "{help}");
        let Cli::Error(error) = parse(["--inconnue"]) else {
            panic!("erreur attendue");
        };
        assert!(
            error.contains("argument inattendu : « --inconnue »"),
            "{error}"
        );
        // Un fichier qui s'appelle comme une option reste ouvrable.
        assert_eq!(parse(["--", "--x"]), run(&["--x"], true));
    }

    #[test]
    fn ouverture_des_chemins() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        std::fs::write(&file, "// lib").unwrap();
        let missing = dir.path().join("nouveau.rs");

        let mut editor = Editor::new();
        let problems = open(
            &mut editor,
            &[dir.path().into(), missing.clone(), file.clone()],
            None,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            editor.project(),
            Some(dir.path().canonicalize().unwrap().as_path())
        );
        assert_eq!(editor.document_count(), 2);
        let active = editor.active_document().unwrap();
        assert_eq!(active.display_name(), "nouveau.rs", "premier fichier actif");
        assert!(
            active.buffer().to_string().is_empty(),
            "fichier inexistant : document vide"
        );

        // Dossier seul : document vide ; second dossier : signalé, ignoré.
        let mut editor = Editor::new();
        let other = tempfile::tempdir().unwrap();
        let problems = open(&mut editor, &[dir.path().into(), other.path().into()], None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(editor.document_count(), 1);
        assert_eq!(editor.active_document().unwrap().path(), None);

        // Fichier illisible (binaire) : signalé, l'IDE démarre sur un document vide.
        let binary = dir.path().join("image.bin");
        std::fs::write(&binary, b"\0\0\0").unwrap();
        let mut editor = Editor::new();
        assert_eq!(open(&mut editor, &[binary], None).len(), 1);
        assert_eq!(editor.document_count(), 1);
    }

    #[test]
    fn session_restauree_puis_fichiers() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        let project = dir.path().join("projet");
        std::fs::create_dir(&project).unwrap();
        let (a, b) = (project.join("a.rs"), project.join("b.rs"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();

        // Première session : a à gauche, b à droite.
        let mut editor = Editor::new();
        open(&mut editor, &[project.clone(), a.clone()], Some(&sessions));
        editor.execute("split right").unwrap();
        editor.open(&b).unwrap();
        let file = sessions.join(session::session_file_name(editor.project()));
        session::save(&editor.capture_session(), &file).unwrap();

        // Relancé sur le projet : les deux zones reviennent, sans document vide.
        let mut editor = Editor::new();
        let problems = open(&mut editor, std::slice::from_ref(&project), Some(&sessions));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(editor.workspace().tree().groups().len(), 2);
        assert_eq!(editor.document_count(), 2);
        assert_eq!(editor.active_document().unwrap().display_name(), "b.rs");

        // Un fichier demandé s'ajoute dans la zone active et devient actif.
        let c = project.join("c.rs");
        let mut editor = Editor::new();
        open(&mut editor, &[project.clone(), c], Some(&sessions));
        assert_eq!(editor.active_document().unwrap().display_name(), "c.rs");
        assert_eq!(editor.workspace().tree().groups()[1].len(), 3);

        // Sans restauration : comme avant.
        let mut editor = Editor::new();
        open(&mut editor, &[project], None);
        assert_eq!(editor.workspace().tree().groups().len(), 1);
        assert_eq!(editor.document_count(), 1, "document vide");
    }
}
