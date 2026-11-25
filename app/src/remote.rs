//! Demandes reçues d'un autre `cupride` lancé dans un terminal (instance unique).
//!
//! La ligne de commande transmise est analysée comme au lancement, avec le registre de
//! l'instance (commandes des plugins comprises) ; les chemins relatifs partent du dossier courant
//! du terminal.
//!
//! - Fichiers : ouverts dans la fenêtre active, ou dans la fenêtre `--window N` ; une fenêtre est
//!   ouverte s'il n'y en a plus. Avec `--wait`, la réponse attend leur fermeture.
//! - Commande (`cupride panel focus left`) : exécutée comme dans la ligne de commande interne ;
//!   son message s'affiche dans le terminal, une erreur aussi, avec un code de sortie non nul.

use std::path::{Path, PathBuf};

use cupride_core::command::CommandOutput;
use cupride_core::layout::WindowId;
use cupride_core::{DocumentId, Editor};
use cupride_ipc::{Reply, Request};

use crate::cli;

/// Ce qu'il faut répondre au terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Answer {
    /// Messages.
    pub(crate) replies: Vec<Reply>,
    /// Code de sortie.
    pub(crate) code: i32,
    /// Documents dont il faut attendre la fermeture avant de conclure (`--wait`).
    pub(crate) wait: Vec<DocumentId>,
}

impl Answer {
    fn ok() -> Self {
        Self {
            replies: Vec::new(),
            code: 0,
            wait: Vec::new(),
        }
    }

    fn error(text: String, code: i32) -> Self {
        Self {
            replies: vec![Reply::Error { text }],
            code,
            wait: Vec::new(),
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

/// Donne le focus à la fenêtre `--window N`, si elle est demandée.
fn focus_window(editor: &mut Editor, window: Option<u64>) -> Result<(), Answer> {
    match window {
        Some(number) if !editor.focus_window(WindowId::new(number)) => Err(Answer::error(
            format!("fenêtre {number} introuvable ; voir « cupride window list »"),
            1,
        )),
        _ => Ok(()),
    }
}

/// Exécute la demande d'un terminal dans l'instance.
pub(crate) fn handle(editor: &mut Editor, request: &Request) -> Answer {
    let parsed = {
        let commands = editor.commands();
        cli::parse(&request.args, |group| commands.has_group(group))
    };
    let launch = match parsed {
        cli::Cli::Print(text) => {
            return Answer {
                replies: vec![Reply::Output { text }],
                ..Answer::ok()
            };
        }
        cli::Cli::Error(message) => return Answer::error(message, 2),
        cli::Cli::Run(launch) => launch,
    };
    if let Err(answer) = focus_window(editor, launch.window) {
        return answer;
    }
    if let Some(command) = &launch.command {
        return match editor.run_in(&request.cwd, command) {
            Ok(CommandOutput::None) => Answer::ok(),
            Ok(CommandOutput::Message(text)) => Answer {
                replies: vec![Reply::Output {
                    text: format!("{text}\n"),
                }],
                ..Answer::ok()
            },
            Err(error) => Answer::error(error.to_string(), 1),
        };
    }
    if editor.active_window().is_none() {
        editor.new_window();
    }
    let paths: Vec<PathBuf> = launch
        .paths
        .iter()
        .map(|p| absolute(&request.cwd, p))
        .collect();
    let opened = cli::open(editor, &paths, None);
    Answer {
        code: i32::from(!opened.problems.is_empty()),
        replies: opened
            .problems
            .into_iter()
            .map(|text| Reply::Error { text })
            .collect(),
        wait: if launch.wait {
            opened.documents
        } else {
            Vec::new()
        },
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

    #[test]
    fn commandes_depuis_le_terminal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("note.txt"), "x").unwrap();
        let mut editor = Editor::new();
        editor.new_document();
        let answer = handle(
            &mut editor,
            &request(dir.path(), &["file", "open", "note.txt"]),
        );
        assert_eq!(
            answer.replies,
            vec![Reply::Output {
                text: "« note.txt » ouvert\n".into()
            }]
        );
        assert_eq!(answer.code, 0);
        let answer = handle(
            &mut editor,
            &request(dir.path(), &["panel", "focus", "left"]),
        );
        assert_eq!(answer.code, 1, "erreur : code non nul");
        assert!(
            matches!(&answer.replies[0], Reply::Error { text } if text.contains("aucune zone"))
        );
        let answer = handle(&mut editor, &request(dir.path(), &["file", "open"]));
        assert_eq!(answer.code, 1, "argument manquant");
    }

    #[test]
    fn fenetre_choisie_et_attente() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("msg"), "x").unwrap();
        let mut editor = Editor::new();
        editor.new_document();
        editor.new_window();
        let answer = handle(&mut editor, &request(dir.path(), &["--window", "1", "msg"]));
        assert_eq!(answer.code, 0);
        assert_eq!(editor.active_window(), Some(WindowId::new(1)));
        assert_eq!(editor.window_panels(WindowId::new(1)).len(), 2);
        let answer = handle(&mut editor, &request(dir.path(), &["--window", "9", "msg"]));
        assert_eq!(answer.code, 1);
        // --wait : les documents à attendre.
        let answer = handle(&mut editor, &request(dir.path(), &["--wait", "msg"]));
        assert_eq!(answer.wait, vec![editor.active_id().unwrap()]);
        let answer = handle(&mut editor, &request(dir.path(), &["msg"]));
        assert!(answer.wait.is_empty());
    }
}
