//! Opérations sur les lignes : aller à une ligne, dupliquer, déplacer, supprimer, joindre,
//! commenter. Elles agissent sur les lignes touchées par les sélections, en une seule
//! modification annulable.

use clap::Arg;
use clap::builder::PossibleValuesParser;

use super::active;
use super::edit::selected_lines;
use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry, CommandResult};
use crate::editor::Editor;
use crate::text::{Edit, EditKind, Rope, Selection, Selections, TextBuffer, TextError};
use crate::ui_request::UiRequest;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "edit",
            "goto",
            "Place le curseur à la ligne et à la colonne indiquées (ligne[:colonne], à partir de 1)",
            |editor, args| {
                let target: &String = args.get_one("position").expect("argument requis");
                let (line, column) = parse_position(target)?;
                editor.goto(line, column)?;
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("position")
                .required(true)
                .help("ligne, ou ligne:colonne (colonne en caractères)"),
        ),
    )?;

    registry.register(CommandDef::new(
        "ui",
        "goto",
        "Prépare « edit goto » dans la ligne de commande",
        |editor, _| {
            editor.request_ui(UiRequest::CommandLine("edit goto ".into()));
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "duplicate-lines",
        "Duplique les lignes des sélections ; les curseurs passent sur la copie du dessous",
        |editor, _| {
            let (_, document) = active(editor)?;
            let blocks = blocks(&selected_lines(document)?);
            let newline = document.line_ending().as_str().to_owned();
            document.transact(EditKind::Other, |buffer, tx| {
                for &(first, last) in blocks.iter().rev() {
                    let start = buffer.line_to_byte(first)?;
                    let end = buffer.line_content_range(last)?.end;
                    let copy = format!("{}{newline}", buffer.slice(start..end)?);
                    // La copie est insérée avant : les curseurs restent sur celle du dessous.
                    tx.apply(buffer, Edit::insert(start, copy))?;
                }
                Ok(())
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(
        CommandDef::new(
            "edit",
            "move-lines",
            "Déplace les lignes des sélections d'une ligne vers le haut ou vers le bas",
            |editor, args| {
                let up = args
                    .get_one::<String>("direction")
                    .is_some_and(|d| d == "up");
                move_lines(editor, up)
            },
        )
        .arg(
            Arg::new("direction")
                .required(true)
                .value_parser(PossibleValuesParser::new(["up", "down"]))
                .help("up ou down"),
        ),
    )?;

    registry.register(CommandDef::new(
        "edit",
        "delete-lines",
        "Supprime les lignes des sélections",
        |editor, _| {
            let (_, document) = active(editor)?;
            let blocks = blocks(&selected_lines(document)?);
            document.transact(EditKind::Other, |buffer, tx| {
                for &(first, last) in blocks.iter().rev() {
                    let range = if last + 1 < buffer.len_lines() {
                        buffer.line_to_byte(first)?..buffer.line_to_byte(last + 1)?
                    } else if first > 0 {
                        // Dernière ligne : le saut de ligne d'avant part avec elle.
                        buffer.line_content_range(first - 1)?.end..buffer.len_bytes()
                    } else {
                        0..buffer.len_bytes()
                    };
                    let edit = Edit::delete(buffer, range)?;
                    tx.apply(buffer, edit)?;
                }
                Ok(())
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "join-lines",
        "Joint chaque ligne des sélections à la suivante (avec une espace, sans l'indentation)",
        |editor, _| {
            let (_, document) = active(editor)?;
            let buffer = document.buffer();
            let mut joins = Vec::new();
            for selection in document.selections().iter() {
                let first = buffer.byte_to_line(selection.start())?;
                let last = buffer.byte_to_line(selection.end())?.max(first + 1);
                for line in first..last.min(buffer.len_lines().saturating_sub(1)) {
                    joins.push(line);
                }
            }
            joins.sort_unstable();
            joins.dedup();
            document.transact(EditKind::Other, |buffer, tx| {
                for &line in joins.iter().rev() {
                    let end = buffer.line_content_range(line)?.end;
                    let next = buffer.line_content_range(line + 1)?;
                    let text = buffer.slice(next.clone())?;
                    let indent = text.len() - text.trim_start().len();
                    let before = buffer.slice(buffer.line_content_range(line)?)?;
                    let separator = if text.trim().is_empty() || before.ends_with(' ') {
                        ""
                    } else {
                        " "
                    };
                    let edit = Edit::replace(buffer, end..next.start + indent, separator)?;
                    tx.apply(buffer, edit)?;
                }
                Ok(())
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "toggle-comment",
        "Commente les lignes des sélections, ou les décommente si elles le sont toutes",
        toggle_comment,
    ))?;

    Ok(())
}

/// `12` ou `12:5`.
fn parse_position(text: &str) -> Result<(usize, usize), CommandError> {
    let invalid = || {
        CommandError::Failed(format!(
            "position invalide : « {text} » (ligne, ou ligne:colonne)"
        ))
    };
    let (line, column) = match text.split_once(':') {
        Some((line, column)) => (line, Some(column)),
        None => (text, None),
    };
    let line = line.trim().parse().map_err(|_| invalid())?;
    let column = match column {
        Some(column) => column.trim().parse().map_err(|_| invalid())?,
        None => 1,
    };
    Ok((line, column))
}

/// Lignes regroupées en blocs consécutifs : (première, dernière).
fn blocks(lines: &[usize]) -> Vec<(usize, usize)> {
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    for &line in lines {
        match blocks.last_mut() {
            Some((_, last)) if *last + 1 == line => *last = line,
            _ => blocks.push((line, line)),
        }
    }
    blocks
}

/// Déplace les blocs de lignes sélectionnées d'une ligne : la ligne voisine passe de l'autre
/// côté. Le texte touché est réécrit d'un coup, et les sélections suivent leurs lignes.
fn move_lines(editor: &mut Editor, up: bool) -> CommandResult {
    let (_, document) = active(editor)?;
    let buffer = document.buffer();
    let count = buffer.len_lines();
    let blocks = blocks(&selected_lines(document)?);
    // Ordre des lignes après le déplacement.
    let mut order: Vec<usize> = (0..count).collect();
    for &(first, last) in &blocks {
        if up && first > 0 {
            order[first - 1..=last].rotate_left(1);
        } else if !up && last + 1 < count {
            order[first..=last + 1].rotate_right(1);
        }
    }
    let span = order
        .iter()
        .enumerate()
        .filter(|(i, l)| i != *l)
        .map(|(i, _)| i);
    let (Some(from), Some(to)) = (span.clone().min(), span.max()) else {
        return Ok(CommandOutput::None);
    };
    let newline = document.line_ending().as_str();
    let start = buffer.line_to_byte(from)?;
    let end = buffer.line_content_range(to)?.end;
    let contents: Vec<String> = (from..=to)
        .map(|i| buffer.slice(buffer.line_content_range(order[i])?))
        .collect::<Result<_, TextError>>()?;
    // Nouvelle place de chaque ligne déplacée.
    let mut new_start = std::collections::HashMap::new();
    let mut offset = start;
    for (i, content) in (from..=to).zip(&contents) {
        new_start.insert(order[i], offset);
        offset += content.len() + newline.len();
    }
    let relocate = |position: usize| -> usize {
        let Ok(line) = buffer.byte_to_line(position) else {
            return position;
        };
        match (new_start.get(&line), buffer.line_to_byte(line)) {
            (Some(&at), Ok(line_start)) => at + (position - line_start),
            _ => position,
        }
    };
    let selections = relocated(document.selections(), &relocate);
    let text = contents.join(newline);
    document.transact(EditKind::Other, |buffer: &mut Rope, tx| {
        let edit = Edit::replace(buffer, start..end, text.clone())?;
        tx.apply(buffer, edit)?;
        tx.set_selections_after(selections.clone());
        Ok(())
    })?;
    Ok(CommandOutput::None)
}

fn relocated(selections: &Selections, relocate: &dyn Fn(usize) -> usize) -> Selections {
    let ranges = selections
        .iter()
        .map(|s| Selection::new(relocate(s.anchor), relocate(s.head)))
        .collect();
    Selections::from_vec(ranges, selections.primary_index()).unwrap_or_else(|| selections.clone())
}

/// `edit toggle-comment`.
fn toggle_comment(editor: &mut Editor, _: &clap::ArgMatches) -> CommandResult {
    let (id, document) = active(editor)?;
    let language = document.path().and_then(crate::language::language_of);
    let token = editor
        .config()
        .languages
        .get(language.unwrap_or_default())
        .and_then(|l| l.comment.clone())
        .or_else(|| {
            language
                .and_then(crate::language::line_comment)
                .map(str::to_owned)
        })
        .ok_or_else(|| {
            CommandError::Failed(format!(
                "pas de commentaire de ligne connu pour {} : [languages.<Nom>] comment",
                language.unwrap_or("ce fichier")
            ))
        })?;
    let document = editor
        .document_mut(id)
        .ok_or(CommandError::NoActiveDocument)?;
    let lines = selected_lines(document)?;
    let buffer = document.buffer();
    let contents: Vec<(usize, String)> = lines
        .iter()
        .map(|&l| Ok((l, buffer.slice(buffer.line_content_range(l)?)?)))
        .collect::<Result<_, TextError>>()?;
    let filled: Vec<&(usize, String)> = contents
        .iter()
        .filter(|(_, t)| !t.trim().is_empty())
        .collect();
    if filled.is_empty() {
        return Ok(CommandOutput::None);
    }
    let commented = filled
        .iter()
        .all(|(_, t)| t.trim_start().starts_with(&token));
    // Les commentaires s'alignent sur l'indentation la plus faible du bloc.
    let indent = filled
        .iter()
        .map(|(_, t)| t.len() - t.trim_start().len())
        .min()
        .unwrap_or(0);
    document.transact(EditKind::Other, |buffer, tx| {
        for (line, text) in filled.iter().rev() {
            let start = buffer.line_to_byte(*line)?;
            if commented {
                let at = text.len() - text.trim_start().len();
                let after = &text[at + token.len()..];
                let length = token.len() + usize::from(after.starts_with(' '));
                let edit = Edit::delete(buffer, start + at..start + at + length)?;
                tx.apply(buffer, edit)?;
            } else {
                tx.apply(buffer, Edit::insert(start + indent, format!("{token} ")))?;
            }
        }
        Ok(())
    })?;
    Ok(CommandOutput::None)
}

#[cfg(test)]
mod tests {
    use crate::Document;
    use crate::editor::Editor;
    use crate::text::{Selection, Selections, TextBuffer};

    fn editor(text: &str, selection: (usize, usize)) -> Editor {
        let mut editor = Editor::new();
        let mut document = Document::from_text(text);
        document.set_selections(Selections::single(Selection::new(selection.0, selection.1)));
        editor.insert_document(document);
        editor
    }

    fn text(editor: &Editor) -> String {
        editor.active_document().unwrap().buffer().to_text()
    }

    fn head(editor: &Editor) -> usize {
        editor
            .active_document()
            .unwrap()
            .selections()
            .primary()
            .head
    }

    #[test]
    fn aller_a() {
        let mut e = editor("un\ndeux\ntrois", (0, 0));
        e.execute("edit goto 2:3").unwrap();
        assert_eq!(head(&e), 5);
        e.execute("edit goto 3").unwrap();
        assert_eq!(head(&e), 8);
        assert!(e.execute("edit goto x:2").is_err());
    }

    #[test]
    fn dupliquer_supprimer() {
        let mut e = editor("a\nb\nc", (2, 2));
        e.execute("edit duplicate-lines").unwrap();
        assert_eq!(text(&e), "a\nb\nb\nc");
        assert_eq!(head(&e), 4, "sur la copie du dessous");
        e.execute("edit delete-lines").unwrap();
        assert_eq!(text(&e), "a\nb\nc");
        // Dernière ligne : le saut de ligne d'avant part avec elle.
        let mut e = editor("a\nb", (3, 3));
        e.execute("edit delete-lines").unwrap();
        assert_eq!(text(&e), "a");
    }

    #[test]
    fn deplacer() {
        let mut e = editor("un\ndeux\ntrois\nquatre", (3, 7));
        e.execute("edit move-lines down").unwrap();
        assert_eq!(text(&e), "un\ntrois\ndeux\nquatre");
        assert_eq!(head(&e), 13, "la sélection suit sa ligne");
        e.execute("edit move-lines up").unwrap();
        e.execute("edit move-lines up").unwrap();
        assert_eq!(text(&e), "deux\nun\ntrois\nquatre");
        e.execute("edit move-lines up").unwrap();
        assert_eq!(text(&e), "deux\nun\ntrois\nquatre", "déjà en haut");
        // Plusieurs lignes, jusqu'à la fin.
        let mut e = editor("a\nb\nc\nd", (2, 5));
        e.execute("edit move-lines down").unwrap();
        assert_eq!(text(&e), "a\nd\nb\nc");
        e.execute("edit undo").unwrap();
        assert_eq!(text(&e), "a\nb\nc\nd", "une seule modification");
    }

    #[test]
    fn joindre() {
        let mut e = editor("un\n    deux\ntrois", (0, 0));
        e.execute("edit join-lines").unwrap();
        assert_eq!(text(&e), "un deux\ntrois");
        let mut e = editor("a\nb\nc\nd", (0, 5));
        e.execute("edit join-lines").unwrap();
        assert_eq!(text(&e), "a b c\nd");
    }

    #[test]
    fn commenter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.rs");
        std::fs::write(&path, "fn a() {\n    x();\n\n    y();\n}\n").unwrap();
        let mut e = Editor::new();
        e.open(&path).unwrap();
        e.active_document_mut()
            .unwrap()
            .set_selections(Selections::single(Selection::new(9, 27)));
        e.execute("edit toggle-comment").unwrap();
        assert_eq!(text(&e), "fn a() {\n    // x();\n\n    // y();\n}\n");
        e.execute("edit toggle-comment").unwrap();
        assert_eq!(text(&e), "fn a() {\n    x();\n\n    y();\n}\n");
        // Sans syntaxe connue : erreur claire.
        let mut e = editor("texte", (0, 0));
        assert!(e.execute("edit toggle-comment").is_err());
    }
}
