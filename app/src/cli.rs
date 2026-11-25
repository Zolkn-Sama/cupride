//! Ligne de commande de `cupride` : fichiers et dossier à ouvrir au lancement.
//!
//! ```text
//! cupride                      document vide
//! cupride src/main.rs          ouvre le fichier (créé au premier enregistrement s'il n'existe pas)
//! cupride .                    ouvre le dossier comme projet
//! cupride . README.md a.rs     dossier du projet et fichiers ; le premier fichier est actif
//! cupride --no-restore         sans restaurer la session du projet
//! cupride --window 2 a.rs      dans la fenêtre 2 de l'instance
//! cupride --wait COMMIT_MSG    attend la fermeture du fichier (éditeur de Git)
//! cupride panel focus left     commande interne, même syntaxe que la ligne de commande
//! cupride --version | --help
//! ```
//!
//! Un premier argument qui est un groupe de commandes (`file`, `panel`, `window`…) fait de la
//! suite une commande ; un fichier qui porterait ce nom s'ouvre avec `./nom`.
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
    /// Fenêtre où ouvrir les fichiers ou exécuter la commande (`--window N`).
    pub(crate) window: Option<u64>,
    /// Attendre la fermeture des fichiers ouverts (`--wait`).
    pub(crate) wait: bool,
    /// Commande interne (`groupe commande arguments…`), au lieu de fichiers.
    pub(crate) command: Option<Vec<String>>,
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
        .override_usage(
            "cupride [options] [chemin…]\n       cupride [--window N] <groupe> <commande> [arguments]",
        )
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
            Arg::new("window")
                .long("window")
                .value_name("N")
                .value_parser(value_parser!(u64))
                .help("Ouvre dans la fenêtre N de l'instance (voir « window list »)")
                .help_heading("Options"),
        )
        .arg(
            Arg::new("wait")
                .long("wait")
                .action(ArgAction::SetTrue)
                .help("Attend que les fichiers ouverts soient fermés (éditeur de Git…)")
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

/// Options qui prennent une valeur : leur valeur n'est pas un groupe de commandes.
const OPTIONS_WITH_VALUE: [&str; 1] = ["--window"];

/// Analyse les arguments (sans le nom du programme). `is_group` reconnaît les groupes de
/// commandes (`file`, `panel`…) : le premier argument qui en est un, après les options, fait de
/// la suite une commande interne.
pub(crate) fn parse<I, T>(args: I, is_group: impl Fn(&str) -> bool) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let mut index = 0;
    let mut split = None;
    while let Some(arg) = args.get(index) {
        let arg = arg.to_string_lossy();
        if arg == "--" {
            break;
        }
        if arg.starts_with('-') {
            index += if OPTIONS_WITH_VALUE.contains(&arg.as_ref()) {
                2
            } else {
                1
            };
            continue;
        }
        if is_group(&arg) {
            split = Some(index);
        }
        break;
    }
    let (options, internal) = match split {
        Some(index) => (
            &args[..index],
            Some(
                args[index..]
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            ),
        ),
        None => (&args[..], None),
    };
    let options = std::iter::once(OsString::from("cupride")).chain(options.iter().cloned());
    match command().try_get_matches_from(options) {
        Ok(matches) => Cli::Run(Launch {
            paths: matches
                .get_many::<PathBuf>("chemins")
                .map(|paths| paths.cloned().collect())
                .unwrap_or_default(),
            restore: !matches.get_flag("no-restore"),
            window: matches.get_one::<u64>("window").copied(),
            wait: matches.get_flag("wait"),
            command: internal,
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
pub(crate) fn open(editor: &mut Editor, paths: &[PathBuf], sessions: Option<&Path>) -> Opened {
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
    let mut documents = Vec::new();
    for path in paths.iter().filter(|p| !p.is_dir()) {
        match editor.open(path) {
            Ok(id) => {
                first.get_or_insert(id);
                documents.push(id);
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
    Opened {
        problems,
        documents,
    }
}

/// Résultat de [`open`].
#[derive(Debug, Default)]
pub(crate) struct Opened {
    /// Problèmes à signaler.
    pub(crate) problems: Vec<String>,
    /// Documents des fichiers demandés.
    pub(crate) documents: Vec<cupride_core::DocumentId>,
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

    /// Analyse avec les groupes de commandes intégrés.
    fn p<const N: usize>(args: [&str; N]) -> Cli {
        let registry = cupride_core::command::builtin_registry();
        parse(args, |group| registry.has_group(group))
    }

    #[test]
    fn analyse() {
        let run = |paths: &[&str], restore| {
            Cli::Run(Launch {
                paths: paths.iter().map(PathBuf::from).collect(),
                restore,
                ..Launch::default()
            })
        };
        assert_eq!(p([]), run(&[], true));
        assert_eq!(p(["a.rs", "dossier"]), run(&["a.rs", "dossier"], true));
        assert_eq!(p(["--no-restore", "a.rs"]), run(&["a.rs"], false));
        assert_eq!(
            p(["--version"]),
            Cli::Print(format!("cupride {}\n", env!("CARGO_PKG_VERSION")))
        );
        let Cli::Print(help) = p(["-h"]) else {
            panic!("aide attendue");
        };
        assert!(
            help.contains("Utilisation : cupride [options] [chemin…]"),
            "{help}"
        );
        assert!(help.contains("ouvert comme projet"), "{help}");
        let Cli::Error(error) = p(["--inconnue"]) else {
            panic!("erreur attendue");
        };
        assert!(
            error.contains("argument inattendu : « --inconnue »"),
            "{error}"
        );
        // Un fichier qui s'appelle comme une option reste ouvrable.
        assert_eq!(p(["--", "--x"]), run(&["--x"], true));
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
        )
        .problems;
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
        let problems = open(&mut editor, &[dir.path().into(), other.path().into()], None).problems;
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(editor.document_count(), 1);
        assert_eq!(editor.active_document().unwrap().path(), None);

        // Fichier illisible (binaire) : signalé, l'IDE démarre sur un document vide.
        let binary = dir.path().join("image.bin");
        std::fs::write(&binary, b"\0\0\0").unwrap();
        let mut editor = Editor::new();
        assert_eq!(open(&mut editor, &[binary], None).problems.len(), 1);
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
        let problems = open(&mut editor, std::slice::from_ref(&project), Some(&sessions)).problems;
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

    #[test]
    fn commandes_et_options() {
        let Cli::Run(launch) = p(["--window", "2", "panel", "focus", "left", "--force"]) else {
            panic!("commande attendue");
        };
        assert_eq!(launch.window, Some(2));
        assert_eq!(
            launch.command.as_deref(),
            Some(
                &[
                    "panel".to_owned(),
                    "focus".into(),
                    "left".into(),
                    "--force".into()
                ][..]
            ),
            "les options après le groupe appartiennent à la commande"
        );
        assert!(launch.paths.is_empty());
        let Cli::Run(launch) = p(["--wait", "COMMIT_EDITMSG"]) else {
            panic!("fichier attendu");
        };
        assert!(launch.wait);
        assert_eq!(launch.command, None);
        assert_eq!(launch.paths, vec![PathBuf::from("COMMIT_EDITMSG")]);
        // Un fichier qui porte le nom d'un groupe : avec ./
        let Cli::Run(launch) = p(["./file"]) else {
            panic!("fichier attendu");
        };
        assert_eq!(launch.command, None);
        assert!(matches!(p(["--window", "x"]), Cli::Error(_)));
    }
}
