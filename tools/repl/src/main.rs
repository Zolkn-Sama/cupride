//! REPL console : pilote le noyau de CuprIDE sans interface graphique.
//!
//! Chaque ligne lue est une commande du noyau (`file open main.rs`, `edit insert "x"`…). Après
//! chaque commande, les lignes autour du curseur principal sont affichées.
//!
//! ```text
//! cupride-repl [--no-config] [fichier...]
//! ```
//!
//! Commandes propres au REPL : `help` (liste des commandes), `help <commande>`, `quit` (refuse de
//! quitter s'il reste des modifications non enregistrées) et `quit!`.

mod view;

use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::Context;
use cupride_core::Editor;
use cupride_core::command::CommandOutput;

/// Ce que le REPL doit faire après une ligne.
enum Flow {
    Continue,
    Quit,
}

fn main() -> anyhow::Result<()> {
    let mut editor = Editor::new();
    let mut files = Vec::new();
    let mut load_config = true;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-config" => load_config = false,
            "-h" | "--help" => {
                println!("Utilisation : cupride-repl [--no-config] [fichier...]");
                return Ok(());
            }
            _ => files.push(arg),
        }
    }
    if load_config {
        for message in editor.reload_config() {
            eprintln!("configuration : {message}");
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(
        out,
        "cupride-repl {} — « help » pour la liste des commandes, « quit » pour quitter",
        env!("CARGO_PKG_VERSION")
    )?;
    for file in &files {
        match editor.open(file) {
            Ok(_) => {}
            Err(error) => writeln!(out, "erreur : {error}")?,
        }
    }
    if let Some(document) = editor.active_document() {
        write!(out, "{}", view::render(document))?;
    }

    let interactive = io::stdin().is_terminal();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        if interactive {
            write!(out, "> ")?;
            out.flush()?;
        }
        let Some(line) = lines.next() else {
            break;
        };
        let line = line.context("lecture de l'entrée standard")?;
        if !interactive && !line.trim().is_empty() {
            writeln!(out, "> {line}")?;
        }
        match handle(&mut editor, line.trim(), &mut out)? {
            Flow::Continue => {}
            Flow::Quit => break,
        }
    }
    Ok(())
}

/// Traite une ligne : commande du REPL ou commande du noyau.
fn handle(editor: &mut Editor, line: &str, out: &mut impl Write) -> io::Result<Flow> {
    match line {
        "" => return Ok(Flow::Continue),
        "quit" | "exit" => {
            let modified: Vec<String> = editor
                .documents()
                .filter(|(_, d)| d.is_modified())
                .map(|(_, d)| d.display_name())
                .collect();
            if modified.is_empty() {
                return Ok(Flow::Quit);
            }
            writeln!(
                out,
                "modifications non enregistrées : {} — « quit! » pour quitter quand même",
                modified.join(", ")
            )?;
            return Ok(Flow::Continue);
        }
        "quit!" | "exit!" => return Ok(Flow::Quit),
        "help" => {
            let mut group = "";
            for command in editor.commands().list() {
                if command.group() != group {
                    group = command.group();
                    writeln!(out, "{group}")?;
                }
                writeln!(out, "  {:<16} {}", command.name(), command.description())?;
            }
            writeln!(
                out,
                "repl\n  {:<16} Quitte (quit! sans vérifier les modifications)",
                "quit"
            )?;
            return Ok(Flow::Continue);
        }
        _ => {}
    }
    let line = match line.strip_prefix("help ") {
        Some(command) => format!("{command} --help"),
        None => line.to_owned(),
    };
    match editor.execute(&line) {
        Ok(CommandOutput::Message(message)) => writeln!(out, "{message}")?,
        Ok(CommandOutput::None) => {}
        Err(error) => writeln!(out, "erreur : {error}")?,
    }
    if !line.ends_with("--help")
        && let Some(document) = editor.active_document()
    {
        write!(out, "{}", view::render(document))?;
    }
    Ok(Flow::Continue)
}
