//! Ligne de commande de `cupride` : fichiers et dossier à ouvrir au lancement.
//!
//! ```text
//! cupride                      document vide
//! cupride src/main.rs          ouvre le fichier (créé au premier enregistrement s'il n'existe pas)
//! cupride .                    ouvre le dossier comme projet
//! cupride . README.md a.rs     dossier du projet et fichiers ; le premier fichier est actif
//! cupride --version | --help
//! ```

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, Command, value_parser};
use cupride_core::Editor;
use cupride_core::command::parser::describe_clap_error;

/// Ce que demande la ligne de commande.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cli {
    /// Lancer l'IDE sur ces chemins (fichiers ou dossier).
    Run(Vec<PathBuf>),
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
        .override_usage("cupride [chemin…]")
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
        Ok(matches) => Cli::Run(
            matches
                .get_many::<PathBuf>("chemins")
                .map(|paths| paths.cloned().collect())
                .unwrap_or_default(),
        ),
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

/// Ouvre les chemins demandés : le dossier devient le projet, les fichiers sont ouverts et le
/// premier devient actif. Sans fichier, un document vide est créé. Renvoie les problèmes à
/// signaler ; l'IDE démarre quand même.
pub(crate) fn open(editor: &mut Editor, paths: &[PathBuf]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut first = None;
    for path in paths {
        if path.is_dir() {
            if let Some(project) = editor.project() {
                problems.push(format!(
                    "{} ignoré : un seul dossier par fenêtre ({} est déjà ouvert)",
                    path.display(),
                    project.display()
                ));
                continue;
            }
            if let Err(error) = editor.set_project(path) {
                problems.push(format!("{} : {error}", path.display()));
            }
            continue;
        }
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
        None => {
            editor.new_document();
        }
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
        assert_eq!(parse::<_, &str>([]), Cli::Run(vec![]));
        assert_eq!(
            parse(["a.rs", "dossier"]),
            Cli::Run(vec!["a.rs".into(), "dossier".into()])
        );
        assert_eq!(
            parse(["--version"]),
            Cli::Print(format!("cupride {}\n", env!("CARGO_PKG_VERSION")))
        );
        let Cli::Print(help) = parse(["-h"]) else {
            panic!("aide attendue");
        };
        assert!(help.contains("Utilisation : cupride [chemin…]"), "{help}");
        assert!(help.contains("ouvert comme projet"), "{help}");
        let Cli::Error(error) = parse(["--inconnue"]) else {
            panic!("erreur attendue");
        };
        assert!(
            error.contains("argument inattendu : « --inconnue »"),
            "{error}"
        );
        // Un fichier qui s'appelle comme une option reste ouvrable.
        assert_eq!(parse(["--", "--x"]), Cli::Run(vec!["--x".into()]));
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
        let problems = open(&mut editor, &[dir.path().into(), other.path().into()]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(editor.document_count(), 1);
        assert_eq!(editor.active_document().unwrap().path(), None);

        // Fichier illisible (binaire) : signalé, l'IDE démarre sur un document vide.
        let binary = dir.path().join("image.bin");
        std::fs::write(&binary, b"\0\0\0").unwrap();
        let mut editor = Editor::new();
        assert_eq!(open(&mut editor, &[binary]).len(), 1);
        assert_eq!(editor.document_count(), 1);
    }
}
