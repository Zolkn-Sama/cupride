//! Demandes reçues d'un autre `cupride` lancé dans un terminal (instance unique).
//!
//! La ligne de commande transmise est analysée comme au lancement ; les chemins relatifs partent
//! du dossier courant du terminal. Les fichiers s'ouvrent dans la fenêtre active (une fenêtre est
//! ouverte s'il n'y en a plus).

use std::path::{Path, PathBuf};

use cupride_core::Editor;
use cupride_ipc::{Reply, Request};

use crate::cli;

/// Ce qu'il faut répondre au terminal : messages et code de sortie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Answer {
    pub(crate) replies: Vec<Reply>,
    pub(crate) code: i32,
}

impl Answer {
    fn ok() -> Self {
        Self {
            replies: Vec::new(),
            code: 0,
        }
    }

    fn error(text: String, code: i32) -> Self {
        Self {
            replies: vec![Reply::Error { text }],
            code,
        }
    }
}

/// `path` absolu, relatif au dossier `cwd` du terminal.
fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    }
}

/// Exécute la demande d'un terminal dans l'instance.
pub(crate) fn handle(editor: &mut Editor, request: &Request) -> Answer {
    match cli::parse(&request.args) {
        cli::Cli::Print(text) => Answer {
            replies: vec![Reply::Output { text }],
            code: 0,
        },
        cli::Cli::Error(message) => Answer::error(message, 2),
        cli::Cli::Run(launch) => {
            if editor.active_window().is_none() {
                editor.new_window();
            }
            let paths: Vec<PathBuf> = launch
                .paths
                .iter()
                .map(|p| absolute(&request.cwd, p))
                .collect();
            let problems = cli::open(editor, &paths, None);
            if problems.is_empty() {
                Answer::ok()
            } else {
                Answer {
                    replies: problems
                        .into_iter()
                        .map(|text| Reply::Error { text })
                        .collect(),
                    code: 1,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(cwd: &Path, args: &[&str]) -> Request {
        Request::new(
            cwd.to_owned(),
            args.iter().map(|a| (*a).to_owned()).collect(),
        )
    }

    #[test]
    fn ouvrir_dans_l_instance() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "a").unwrap();
        let mut editor = Editor::new();
        editor.new_document();
        // Chemin relatif au dossier du terminal.
        let answer = handle(&mut editor, &request(dir.path(), &["a.rs"]));
        assert_eq!(answer, Answer::ok());
        assert_eq!(editor.active_document().unwrap().display_name(), "a.rs");
        assert_eq!(
            editor.workspace().windows().len(),
            1,
            "dans la fenêtre active"
        );
        // Fichier illisible : erreur, code 1.
        std::fs::write(dir.path().join("b.bin"), b"\0\0").unwrap();
        let answer = handle(&mut editor, &request(dir.path(), &["b.bin"]));
        assert_eq!(answer.code, 1);
        assert!(matches!(&answer.replies[0], Reply::Error { .. }));
        // Aide, option inconnue.
        let help = handle(&mut editor, &request(dir.path(), &["--help"]));
        assert!(matches!(&help.replies[0], Reply::Output { text } if text.contains("Utilisation")));
        assert_eq!(
            handle(&mut editor, &request(dir.path(), &["--nope"])).code,
            2
        );
    }

    #[test]
    fn sans_fenetre_une_fenetre_s_ouvre() {
        let dir = tempfile::tempdir().unwrap();
        let mut editor = Editor::new();
        editor.new_document();
        let window = editor.active_window().unwrap();
        editor.close_window(window, true).unwrap();
        assert_eq!(handle(&mut editor, &request(dir.path(), &[])), Answer::ok());
        assert_eq!(editor.workspace().windows().len(), 1);
        assert!(editor.active_document().is_some());
    }
}
