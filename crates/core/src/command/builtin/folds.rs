//! Groupe `fold` : replier et déplier des blocs de code, d'après l'arbre syntaxique.

use clap::Arg;
use cupride_syntax::FoldRegion;
use cupride_text::TextBuffer;

use super::active;
use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry, CommandResult};
use crate::document::Document;
use crate::editor::Editor;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    let line = || {
        Arg::new("line")
            .long("line")
            .value_parser(clap::value_parser!(usize))
            .help("Ligne (à partir de 1) au lieu de celle du curseur principal")
    };
    registry.register(
        CommandDef::new(
            "fold",
            "close",
            "Replie le plus petit bloc qui contient le curseur et n'est pas encore replié",
            close,
        )
        .arg(line()),
    )?;
    registry.register(
        CommandDef::new(
            "fold",
            "open",
            "Déplie le bloc replié sur la ligne du curseur (ou qui la contient)",
            open,
        )
        .arg(line()),
    )?;
    registry.register(
        CommandDef::new(
            "fold",
            "toggle",
            "Déplie le bloc replié sur la ligne du curseur, sinon replie le bloc qui la contient",
            |editor, args| {
                let (_, document) = active(editor)?;
                let line = target_line(document, args);
                if document.unfold(line) {
                    Ok(CommandOutput::None)
                } else {
                    close(editor, args)
                }
            },
        )
        .arg(line()),
    )?;
    registry.register(CommandDef::new(
        "fold",
        "close-all",
        "Replie tous les blocs du document",
        |editor, _| {
            let (_, document) = active(editor)?;
            let regions = regions(document, 0..document.buffer().len_bytes())?;
            let count = document.fold_all(&regions);
            Ok(CommandOutput::message(format!(
                "{count} bloc{} replié{}",
                plural(count),
                plural(count)
            )))
        },
    ))?;
    registry.register(CommandDef::new(
        "fold",
        "open-all",
        "Déplie tous les blocs du document",
        |editor, _| {
            let (_, document) = active(editor)?;
            document.unfold_all();
            Ok(CommandOutput::None)
        },
    ))?;
    registry.register(CommandDef::new(
        "fold",
        "list",
        "Blocs repliés : ligne d'en-tête et lignes masquées",
        |editor, _| {
            let document = editor
                .active_document()
                .ok_or(CommandError::NoActiveDocument)?;
            let buffer = document.buffer();
            let mut folds: Vec<(usize, usize, usize)> = document
                .folds()
                .iter()
                .map(|f| {
                    let header = buffer.byte_to_line(f.start).unwrap_or_default();
                    let last = buffer.byte_to_line(f.end).unwrap_or_default();
                    (header + 1, header + 2, last + 1)
                })
                .collect();
            folds.sort_unstable();
            if folds.is_empty() {
                return Ok(CommandOutput::message("aucun bloc replié"));
            }
            let lines: Vec<String> = folds
                .iter()
                .map(|(header, first, last)| format!("{header} (lignes {first} à {last})"))
                .collect();
            Ok(CommandOutput::message(lines.join("\n")))
        },
    ))?;
    Ok(())
}

fn plural(count: usize) -> &'static str {
    if count > 1 { "s" } else { "" }
}

/// Ligne visée : `--line` (à partir de 1), sinon celle du curseur principal.
fn target_line(document: &Document, args: &clap::ArgMatches) -> usize {
    match args.get_one::<usize>("line") {
        Some(&line) => line.saturating_sub(1),
        None => document
            .buffer()
            .byte_to_line(document.selections().primary().head)
            .unwrap_or_default(),
    }
}

/// Zones repliables qui touchent `range`.
fn regions(
    document: &Document,
    range: std::ops::Range<usize>,
) -> Result<Vec<FoldRegion>, CommandError> {
    let syntax = document
        .syntax()
        .filter(|s| s.tree().is_some())
        .ok_or_else(|| {
            CommandError::Failed(match (document.syntax(), document.language()) {
                (Some(s), _) if s.is_parsing() => "analyse syntaxique en cours".to_owned(),
                (_, Some(language)) => {
                    format!("pas de blocs repliables pour le langage {language}")
                }
                (_, None) => "langage du document inconnu : pas de blocs repliables".to_owned(),
            })
        })?;
    Ok(syntax.fold_regions(document.buffer(), range))
}

/// `fold close`.
fn close(editor: &mut Editor, args: &clap::ArgMatches) -> CommandResult {
    let (_, document) = active(editor)?;
    let line = target_line(document, args);
    let buffer = document.buffer();
    let end = buffer.line_range(line)?.end;
    // Zones qui contiennent la ligne (en-tête compris), de la plus petite à la plus grande.
    let mut candidates: Vec<FoldRegion> = regions(document, 0..buffer.len_bytes().min(end))?
        .into_iter()
        .filter(|r| r.header == line || r.hidden.contains(&line))
        .collect();
    candidates.sort_by_key(|r| r.hidden.len());
    for region in &candidates {
        if document.fold(region) {
            return Ok(CommandOutput::None);
        }
    }
    Ok(CommandOutput::message(if candidates.is_empty() {
        "aucun bloc à replier ici"
    } else {
        "bloc déjà replié"
    }))
}

/// `fold open`.
fn open(editor: &mut Editor, args: &clap::ArgMatches) -> CommandResult {
    let (_, document) = active(editor)?;
    let line = target_line(document, args);
    if document.unfold(line) {
        return Ok(CommandOutput::None);
    }
    // La ligne est dans un bloc replié (avec --line) : le plus proche autour d'elle.
    let header = document
        .folds()
        .iter()
        .filter_map(|f| {
            let buffer = document.buffer();
            let header = buffer.byte_to_line(f.start).ok()?;
            let last = buffer.byte_to_line(f.end).ok()?;
            (header < line && line <= last).then_some(header)
        })
        .max();
    match header {
        Some(header) => {
            document.unfold(header);
            Ok(CommandOutput::None)
        }
        None => Ok(CommandOutput::message("aucun bloc replié ici")),
    }
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::editor::Editor;

    const CODE: &str = "fn a() {\n    if x {\n        y();\n    }\n}\n\nfn b() {\n    z();\n}\n";

    fn editor() -> (Editor, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        std::fs::write(&path, CODE).unwrap();
        let mut editor = Editor::new();
        editor.open(&path).unwrap();
        (editor, dir)
    }

    fn list(editor: &mut Editor) -> String {
        match editor.execute("fold list").unwrap() {
            CommandOutput::Message(m) => m,
            CommandOutput::None => String::new(),
        }
    }

    fn ranges(pairs: &[(usize, usize)]) -> Vec<std::ops::Range<usize>> {
        pairs.iter().map(|&(a, b)| a..b).collect()
    }

    fn hidden(editor: &Editor) -> Vec<std::ops::Range<usize>> {
        editor.active_document().unwrap().hidden_lines().to_vec()
    }

    #[test]
    fn replier_et_deplier() {
        let (mut e, _dir) = editor();
        e.execute("edit goto 3:9").unwrap(); // dans le if
        e.execute("fold close").unwrap();
        assert_eq!(list(&mut e), "2 (lignes 3 à 3)");
        // Le curseur masqué remonte à la fin de l'en-tête.
        let document = e.active_document().unwrap();
        assert_eq!(
            document.selections().primary().head,
            CODE.find(" {\n        y").unwrap() + 2
        );
        // Une seconde fois : le bloc englobant.
        e.execute("fold close").unwrap();
        assert_eq!(list(&mut e), "1 (lignes 2 à 4)\n2 (lignes 3 à 3)");
        assert_eq!(hidden(&e), ranges(&[(1, 4)]));
        e.execute("fold open").unwrap();
        assert_eq!(hidden(&e), ranges(&[(2, 3)]));
        e.execute("fold open-all").unwrap();
        assert_eq!(list(&mut e), "aucun bloc replié");
        // Basculer, sur une autre ligne.
        e.execute("fold toggle --line 7").unwrap();
        assert_eq!(hidden(&e), ranges(&[(7, 8)]));
        e.execute("fold toggle --line 7").unwrap();
        assert!(hidden(&e).is_empty());
        let output = e.execute("fold close --line 6").unwrap();
        assert_eq!(output, CommandOutput::message("aucun bloc à replier ici"));
    }

    #[test]
    fn tout_replier_et_curseur_qui_entre_dans_un_pli() {
        let (mut e, _dir) = editor();
        e.execute("edit goto 3:9").unwrap();
        let output = e.execute("fold close-all").unwrap();
        assert_eq!(output, CommandOutput::message("3 blocs repliés"));
        assert_eq!(hidden(&e), ranges(&[(1, 4), (7, 8)]));
        assert_eq!(
            e.active_document().unwrap().selections().primary().head,
            CODE.find(" {\n    if").unwrap() + 2,
            "curseur à la fin de l'en-tête le plus extérieur"
        );
        // Aller dans un pli le déplie.
        e.execute("edit goto 8:1").unwrap();
        assert_eq!(hidden(&e), ranges(&[(1, 4)]));
    }

    #[test]
    fn le_curseur_saute_les_lignes_masquees() {
        let (mut e, _dir) = editor();
        e.execute("fold close --line 1").unwrap();
        assert_eq!(hidden(&e), ranges(&[(1, 4)]));
        let line = |e: &Editor| {
            let document = e.active_document().unwrap();
            let head = document.selections().primary().head;
            cupride_text::TextBuffer::byte_to_line(document.buffer(), head).unwrap()
        };
        e.execute("edit move doc-start").unwrap();
        e.execute("edit move down").unwrap();
        assert_eq!(line(&e), 4, "de l'en-tête à la ligne après le pli");
        e.execute("edit move up").unwrap();
        assert_eq!(line(&e), 0, "retour à l'en-tête");
        e.execute("edit move line-end").unwrap();
        e.execute("edit move right").unwrap();
        assert_eq!(line(&e), 4, "à droite de l'en-tête : après le pli");
        e.execute("edit move left").unwrap();
        assert_eq!(line(&e), 0);
        assert_eq!(hidden(&e), ranges(&[(1, 4)]), "le pli est resté replié");
    }

    #[test]
    fn plis_decales_par_les_modifications() {
        let (mut e, _dir) = editor();
        e.execute("fold close --line 7").unwrap();
        e.execute("edit move doc-start").unwrap();
        e.execute("edit insert '// début\n'").unwrap();
        assert_eq!(hidden(&e), ranges(&[(8, 9)]));
        // Effacer tout le bloc fait disparaître le pli.
        e.execute("edit select-all").unwrap();
        e.execute("edit delete-backward").unwrap();
        assert!(e.active_document().unwrap().folds().is_empty());
    }
}
