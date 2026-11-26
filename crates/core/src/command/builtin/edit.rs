//! Groupe `edit` : insertion, suppression, annulation, déplacements et sélections.
//!
//! Toute action clavier de l'éditeur correspond à l'une de ces commandes : le keymap ne fait que
//! les associer à des touches. Les commandes agissent sur **toutes les sélections** du document
//! actif (multi-curseur).

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction};
use cupride_text::movement;
use cupride_text::position::{grapheme_point_to_offset, offset_to_grapheme_point};
use cupride_text::{
    Edit, EditKind, Point, Rope, Selection, Selections, TextBuffer, TextError, Transaction,
};

use super::active;
use crate::clipboard::{ClipboardKind, LastCopy, split_for_cursors};
use crate::command::CommandResult;
use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;

/// Directions acceptées par `edit move`.
const DIRECTIONS: [&str; 12] = [
    "left",
    "right",
    "up",
    "down",
    "word-left",
    "word-right",
    "line-start",
    "line-end",
    "doc-start",
    "doc-end",
    "page-up",
    "page-down",
];

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "edit",
            "insert",
            "Insère du texte à chaque curseur (remplace les sélections)",
            |editor, args| {
                let text: &String = args.get_one("texte").expect("argument requis");
                let (_, document) = active(editor)?;
                document.transact(EditKind::Insert, |buffer, tx| {
                    replace_selections(buffer, tx, text)
                })?;
                Ok(CommandOutput::None)
            },
        )
        .arg(Arg::new("texte").required(true).help("Texte à insérer")),
    )?;

    registry.register(CommandDef::new(
        "edit",
        "newline",
        "Insère un saut de ligne à chaque curseur, avec la fin de ligne du document ; la nouvelle ligne garde l'indentation",
        |editor, _| {
            let (_, document) = active(editor)?;
            let newline = document.line_ending().as_str();
            let buffer = document.buffer();
            // Pour chaque curseur : saut de ligne, puis l'indentation de sa ligne (jusqu'au
            // curseur).
            let texts = document
                .selections()
                .iter()
                .map(|selection| {
                    let start = selection.start();
                    let line = buffer.byte_to_line(start)?;
                    let line_start = buffer.line_to_byte(line)?;
                    let before = buffer.slice(line_start..start)?;
                    let indent: String = before
                        .chars()
                        .take_while(|c| *c == ' ' || *c == '\t')
                        .collect();
                    Ok(format!("{newline}{indent}"))
                })
                .collect::<Result<Vec<String>, TextError>>()?;
            document.transact(EditKind::Insert, |buffer, tx| {
                replace_each(buffer, tx, &texts)
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "indent",
        "Indente : un niveau au début de chaque ligne sélectionnée, ou, sans sélection sur plusieurs lignes, jusqu'au prochain niveau au curseur",
        |editor, _| {
            let (_, document) = active(editor)?;
            let indent = document.indent();
            let multiline = spans_lines(document);
            if multiline {
                let lines = selected_lines(document)?;
                let unit = indent.unit();
                document.transact(EditKind::Other, |buffer, tx| {
                    for &line in lines.iter().rev() {
                        let start = buffer.line_to_byte(line)?;
                        tx.apply(buffer, Edit::insert(start, unit.clone()))?;
                    }
                    Ok(())
                })?;
            } else {
                let buffer = document.buffer();
                let texts = document
                    .selections()
                    .iter()
                    .map(|selection| {
                        if indent.tabs {
                            return Ok("\t".to_owned());
                        }
                        let column = offset_to_grapheme_point(buffer, selection.start())?.column;
                        Ok(" ".repeat(indent.width - column % indent.width))
                    })
                    .collect::<Result<Vec<String>, TextError>>()?;
                document.transact(EditKind::Insert, |buffer, tx| {
                    replace_each(buffer, tx, &texts)
                })?;
            }
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "outdent",
        "Désindente : retire un niveau au début de chaque ligne des sélections",
        |editor, _| {
            let (_, document) = active(editor)?;
            let indent = document.indent();
            let lines = selected_lines(document)?;
            let buffer = document.buffer();
            let mut removals = Vec::new();
            for &line in &lines {
                let start = buffer.line_to_byte(line)?;
                let content = buffer.line(line)?;
                let length = if content.starts_with('\t') {
                    1
                } else {
                    content
                        .chars()
                        .take(indent.width)
                        .take_while(|c| *c == ' ')
                        .count()
                };
                if length > 0 {
                    removals.push(start..start + length);
                }
            }
            document.transact(EditKind::Other, |buffer, tx| {
                for range in removals.iter().rev() {
                    let edit = Edit::delete(buffer, range.clone())?;
                    tx.apply(buffer, edit)?;
                }
                Ok(())
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(
        CommandDef::new(
            "edit",
            "indentation",
            "Change l'indentation du document actif : tabulations ou espaces, et largeur",
            |editor, args| {
                let kind: &String = args.get_one("type").expect("argument requis");
                let (_, document) = active(editor)?;
                let mut indent = document.indent();
                indent.tabs = kind == "tabs";
                if let Some(&width) = args.get_one::<usize>("width") {
                    indent.width = width.clamp(1, 16);
                }
                document.set_indent(indent, true);
                Ok(CommandOutput::message(indent.describe()))
            },
        )
        .arg(
            Arg::new("type")
                .required(true)
                .value_parser(PossibleValuesParser::new(["tabs", "spaces"]))
                .help("tabs ou spaces"),
        )
        .arg(
            Arg::new("width")
                .long("width")
                .value_name("N")
                .value_parser(clap::value_parser!(usize))
                .help("Largeur d'un niveau (1 à 16)"),
        ),
    )?;

    registry.register(CommandDef::new(
        "edit",
        "delete-backward",
        "Supprime la sélection, ou le caractère avant chaque curseur (retour arrière) ; une \
         paire vide autour du curseur, « (|) », est effacée d'un coup",
        |editor, _| {
            let language = editor.active_document().and_then(|d| d.language());
            let pairs = super::pairs::active_pairs(editor, language);
            let (_, document) = active(editor)?;
            document.transact(EditKind::Delete, |buffer, tx| {
                delete_each(
                    buffer,
                    tx,
                    |buffer: &Rope, head| match super::pairs::empty_pair_around(buffer, head, pairs)
                    {
                        Some(range) => Ok(range),
                        None => movement::delete_backward_range(buffer, head),
                    },
                )
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "delete-forward",
        "Supprime la sélection, ou le caractère après chaque curseur (suppr)",
        |editor, _| {
            let (_, document) = active(editor)?;
            document.transact(EditKind::Delete, |buffer, tx| {
                delete_each(buffer, tx, movement::delete_forward_range)
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "undo",
        "Annule la dernière modification",
        |editor, _| {
            let (_, document) = active(editor)?;
            Ok(if document.undo()? {
                CommandOutput::None
            } else {
                CommandOutput::message("rien à annuler")
            })
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "redo",
        "Rétablit la dernière modification annulée",
        |editor, _| {
            let (_, document) = active(editor)?;
            Ok(if document.redo()? {
                CommandOutput::None
            } else {
                CommandOutput::message("rien à rétablir")
            })
        },
    ))?;

    registry.register(
        CommandDef::new("edit", "move", "Déplace les curseurs", |editor, args| {
            let direction: &String = args.get_one("direction").expect("argument requis");
            let extend = args.get_flag("extend");
            let page = editor.page_lines();
            let (_, document) = active(editor)?;
            let hidden = document.hidden_lines();
            let moved = Moved {
                direction,
                extend,
                page,
                hidden,
            };
            let selections = move_selections(document.buffer(), document.selections(), &moved)?;
            document.set_selections(selections);
            Ok(CommandOutput::None)
        })
        .arg(
            Arg::new("direction")
                .required(true)
                .value_parser(PossibleValuesParser::new(DIRECTIONS))
                .help("left, right, up, down, word-left, word-right, line-start, line-end, doc-start, doc-end, page-up ou page-down"),
        )
        .arg(
            Arg::new("extend")
                .long("extend")
                .action(ArgAction::SetTrue)
                .help("Étend la sélection au lieu de déplacer le curseur"),
        ),
    )?;

    registry.register(CommandDef::new(
        "edit",
        "select-all",
        "Sélectionne tout le texte",
        |editor, _| {
            let (_, document) = active(editor)?;
            let end = document.buffer().len_bytes();
            document.set_selections(Selections::single(Selection::new(0, end)));
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "collapse",
        "Réduit chaque sélection à un curseur",
        |editor, _| {
            let (_, document) = active(editor)?;
            let mut selections = document.selections().clone();
            selections.transform(Selection::collapse);
            document.set_selections(selections);
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "keep-primary",
        "Ne garde que la sélection principale (sortie du multi-curseur)",
        |editor, _| {
            let (_, document) = active(editor)?;
            let mut selections = document.selections().clone();
            selections.keep_primary();
            document.set_selections(selections);
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(
        CommandDef::new(
            "edit",
            "add-cursor",
            "Ajoute un curseur sur la ligne au-dessus ou en dessous du curseur principal",
            |editor, args| {
                let direction: &String = args.get_one("direction").expect("argument requis");
                let (_, document) = active(editor)?;
                let head = document.selections().primary().head;
                let hidden = document.hidden_lines();
                let target = vertical(document.buffer(), head, direction == "up", 1, hidden)?;
                if target != head {
                    let mut selections = document.selections().clone();
                    selections.push(Selection::cursor(target));
                    document.set_selections(selections);
                }
                Ok(CommandOutput::None)
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
        "add-next-occurrence",
        "Sélectionne le mot sous le curseur, puis ajoute une sélection à chaque occurrence suivante",
        |editor, _| next_occurrence(editor),
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "select-all-occurrences",
        "Place une sélection sur chaque occurrence du texte sélectionné (ou du mot sous le curseur)",
        |editor, _| all_occurrences(editor),
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "copy",
        "Copie les sélections dans le presse-papiers (un fragment par sélection)",
        |editor, _| {
            Ok(match copy(editor)? {
                true => CommandOutput::None,
                false => CommandOutput::message("rien à copier : aucune sélection"),
            })
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "cut",
        "Copie les sélections dans le presse-papiers, puis les supprime",
        |editor, _| {
            if !copy(editor)? {
                return Ok(CommandOutput::message("rien à couper : aucune sélection"));
            }
            let (_, document) = active(editor)?;
            document.transact(EditKind::Other, |buffer, tx| {
                delete_each(buffer, tx, |_, offset| Ok(offset..offset))
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(
        CommandDef::new(
            "edit",
            "paste",
            "Colle le presse-papiers à chaque curseur (un fragment par curseur si les nombres correspondent)",
            |editor, args| {
                let kind = if args.get_flag("primary") {
                    ClipboardKind::Primary
                } else {
                    ClipboardKind::Standard
                };
                let cursors = active(editor)?.1.selections().len();
                let Some(text) = editor.clipboard_mut().get(kind).filter(|t| !t.is_empty()) else {
                    return Ok(CommandOutput::message("presse-papiers vide"));
                };
                let fragments = split_for_cursors(&text, cursors, editor.last_copy());
                let (_, document) = active(editor)?;
                let newline = document.line_ending().as_str();
                let adapt = |text: &str| text.replace("\r\n", "\n").replace('\n', newline);
                document.transact(EditKind::Other, |buffer, tx| match &fragments {
                    Some(fragments) => {
                        let fragments: Vec<String> = fragments.iter().map(|f| adapt(f)).collect();
                        replace_each(buffer, tx, &fragments)
                    }
                    None => replace_selections(buffer, tx, &adapt(&text)),
                })?;
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("primary")
                .long("primary")
                .action(ArgAction::SetTrue)
                .help("Colle la sélection primaire (X11, Wayland) au lieu du presse-papiers"),
        ),
    )?;

    Ok(())
}

/// Une des sélections s'étend-elle sur plusieurs lignes ?
fn spans_lines(document: &crate::Document) -> bool {
    let buffer = document.buffer();
    document.selections().iter().any(|s| {
        !s.is_empty() && buffer.byte_to_line(s.start()).ok() != buffer.byte_to_line(s.end()).ok()
    })
}

/// Lignes touchées par les sélections, sans doublon, dans l'ordre. Une sélection qui finit au
/// tout début d'une ligne ne compte pas cette ligne.
pub(super) fn selected_lines(document: &crate::Document) -> Result<Vec<usize>, TextError> {
    let buffer = document.buffer();
    let mut lines = Vec::new();
    for selection in document.selections().iter() {
        let first = buffer.byte_to_line(selection.start())?;
        let mut last = buffer.byte_to_line(selection.end())?;
        if last > first && buffer.line_to_byte(last)? == selection.end() {
            last -= 1;
        }
        lines.extend(first..=last);
    }
    lines.sort_unstable();
    lines.dedup();
    Ok(lines)
}

/// Texte à chercher pour les occurrences : la sélection principale, ou le mot sous le curseur
/// (qui devient alors la sélection principale ; le booléen le signale). `None` hors de tout mot.
fn occurrence_text(document: &mut crate::Document) -> Result<Option<(String, bool)>, CommandError> {
    let primary = document.selections().primary();
    if !primary.is_empty() {
        return Ok(Some((document.buffer().slice(primary.range())?, false)));
    }
    let word = movement::word_range(document.buffer(), primary.head)?;
    if word.is_empty() {
        return Ok(None);
    }
    let mut selections = document.selections().clone();
    let index = selections.primary_index();
    let mut ranges = selections.as_slice().to_vec();
    ranges[index] = Selection::new(word.start, word.end);
    if let Some(updated) = Selections::from_vec(ranges, index) {
        selections = updated;
    }
    document.set_selections(selections);
    Ok(Some((document.buffer().slice(word)?, true)))
}

/// `edit add-next-occurrence` : sur un curseur, sélectionne le mot ; sur une sélection, ajoute la
/// prochaine occurrence de son texte (en revenant au début), qui devient la principale.
fn next_occurrence(editor: &mut Editor) -> CommandResult {
    let (_, document) = active(editor)?;
    // Sur un curseur, la première fois, le mot est seulement sélectionné.
    let Some((needle, false)) = occurrence_text(document)? else {
        return Ok(CommandOutput::None);
    };
    let text = document.buffer().to_text();
    let after = document.selections().primary().end();
    let taken = |start: usize| {
        document
            .selections()
            .iter()
            .any(|s| s.start() == start && s.end() == start + needle.len())
    };
    let found = text[after..]
        .match_indices(&needle)
        .map(|(i, _)| after + i)
        .chain(text[..after].match_indices(&needle).map(|(i, _)| i))
        .find(|&start| !taken(start));
    let Some(start) = found else {
        return Ok(CommandOutput::message(
            "toutes les occurrences sont sélectionnées",
        ));
    };
    let mut selections = document.selections().clone();
    selections.push(Selection::new(start, start + needle.len()));
    document.set_selections(selections);
    Ok(CommandOutput::None)
}

/// `edit select-all-occurrences` : une sélection par occurrence ; la principale reste où elle était.
fn all_occurrences(editor: &mut Editor) -> CommandResult {
    let (_, document) = active(editor)?;
    let Some((needle, _)) = occurrence_text(document)? else {
        return Ok(CommandOutput::None);
    };
    let primary = document.selections().primary();
    let text = document.buffer().to_text();
    let ranges: Vec<Selection> = text
        .match_indices(&needle)
        .map(|(i, _)| Selection::new(i, i + needle.len()))
        .collect();
    let index = ranges
        .iter()
        .position(|s| s.start() == primary.start())
        .unwrap_or(0);
    let count = ranges.len();
    if let Some(selections) = Selections::from_vec(ranges, index) {
        document.set_selections(selections);
    }
    Ok(CommandOutput::message(format!(
        "{count} occurrence{}",
        if count > 1 { "s" } else { "" }
    )))
}

/// Copie le texte des sélections non vides dans le presse-papiers, séparé par la fin de ligne du
/// document. Renvoie faux s'il n'y avait rien à copier.
fn copy(editor: &mut Editor) -> Result<bool, CommandError> {
    let (_, document) = active(editor)?;
    let buffer = document.buffer();
    let fragments = document
        .selections()
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| buffer.slice(s.range()))
        .collect::<Result<Vec<String>, TextError>>()?;
    if fragments.is_empty() {
        return Ok(false);
    }
    let text = fragments.join(document.line_ending().as_str());
    editor
        .clipboard_mut()
        .set(ClipboardKind::Standard, text.clone())
        .map_err(|e| CommandError::Failed(format!("presse-papiers inaccessible : {e}")))?;
    editor.set_last_copy(LastCopy { text, fragments });
    Ok(true)
}

/// Remplace la `i`-ième sélection par `texts[i]` ; les curseurs se retrouvent après le texte.
fn replace_each(
    buffer: &mut Rope,
    tx: &mut Transaction,
    texts: &[String],
) -> Result<(), TextError> {
    let mut i = 0;
    while i < tx.selections_after().len() && i < texts.len() {
        let range = tx.selections_after().as_slice()[i].range();
        let edit = Edit::replace(buffer, range, &texts[i])?;
        tx.apply(buffer, edit)?;
        i += 1;
    }
    collapse_to_end(tx);
    Ok(())
}

/// Remplace chaque sélection par `text` ; les curseurs se retrouvent après le texte inséré.
fn replace_selections(
    buffer: &mut Rope,
    tx: &mut Transaction,
    text: &str,
) -> Result<(), TextError> {
    let mut i = 0;
    while i < tx.selections_after().len() {
        let range = tx.selections_after().as_slice()[i].range();
        let edit = Edit::replace(buffer, range, text)?;
        tx.apply(buffer, edit)?;
        i += 1;
    }
    collapse_to_end(tx);
    Ok(())
}

/// Supprime chaque sélection non vide, ou la plage donnée par `range_for` autour de chaque curseur.
fn delete_each(
    buffer: &mut Rope,
    tx: &mut Transaction,
    range_for: impl Fn(&Rope, usize) -> Result<std::ops::Range<usize>, TextError>,
) -> Result<(), TextError> {
    let mut i = 0;
    // Des curseurs voisins peuvent fusionner pendant la boucle : la longueur est relue à chaque tour.
    while i < tx.selections_after().len() {
        let selection = tx.selections_after().as_slice()[i];
        let range = if selection.is_empty() {
            range_for(buffer, selection.head)?
        } else {
            selection.range()
        };
        if !range.is_empty() {
            let edit = Edit::delete(buffer, range)?;
            tx.apply(buffer, edit)?;
        }
        i += 1;
    }
    collapse_to_end(tx);
    Ok(())
}

/// Réduit les sélections d'après la transaction à des curseurs placés à leur fin.
fn collapse_to_end(tx: &mut Transaction) {
    let mut selections = tx.selections_after().clone();
    selections.transform(|s| Selection::cursor(s.end()));
    tx.set_selections_after(selections);
}

/// Position atteinte en montant ou en descendant de `lines` lignes depuis `offset`, à la même
/// colonne (en graphèmes) si la ligne est assez longue, sinon à sa fin. Au-delà de la première ou
/// de la dernière ligne : début ou fin du texte.
///
/// Une ligne masquée par un pli (`hidden`) est sautée : en montant, vers la ligne d'en-tête du
/// pli ; en descendant, vers la ligne qui le suit.
fn vertical(
    buffer: &Rope,
    offset: usize,
    up: bool,
    lines: usize,
    hidden: &[std::ops::Range<usize>],
) -> Result<usize, TextError> {
    let point = offset_to_grapheme_point(buffer, offset)?;
    let mut line = if up {
        match point.line.checked_sub(lines) {
            Some(line) => line,
            None => return Ok(0),
        }
    } else if point.line + lines < buffer.len_lines() {
        point.line + lines
    } else {
        return Ok(buffer.len_bytes());
    };
    if let Some(range) = hidden.iter().find(|r| r.contains(&line)) {
        line = if up {
            range.start.saturating_sub(1)
        } else if range.end < buffer.len_lines() {
            range.end
        } else {
            return Ok(buffer.len_bytes());
        };
    }
    let target = Point::new(line, point.column);
    grapheme_point_to_offset(buffer, target).or_else(|_| Ok(buffer.line_content_range(line)?.end))
}

/// Un déplacement : direction, extension de la sélection, taille d'une page, lignes masquées par
/// des plis.
struct Moved<'a> {
    direction: &'a str,
    extend: bool,
    page: usize,
    hidden: &'a [std::ops::Range<usize>],
}

/// Position `target`, atteinte en avançant (`forward`) ou en reculant, ramenée hors des lignes
/// masquées : au début de la ligne qui suit le pli, ou à la fin de sa ligne d'en-tête.
fn skip_hidden(
    buffer: &Rope,
    target: usize,
    forward: bool,
    hidden: &[std::ops::Range<usize>],
) -> Result<usize, TextError> {
    let line = buffer.byte_to_line(target)?;
    let Some(range) = hidden.iter().find(|r| r.contains(&line)) else {
        return Ok(target);
    };
    if forward {
        if range.end < buffer.len_lines() {
            buffer.line_to_byte(range.end)
        } else {
            Ok(buffer.len_bytes())
        }
    } else {
        Ok(buffer
            .line_content_range(range.start.saturating_sub(1))?
            .end)
    }
}

/// Nouvelle position de la tête d'une sélection après un déplacement.
fn moved_head(buffer: &Rope, selection: Selection, moved: &Moved<'_>) -> Result<usize, TextError> {
    let Moved {
        direction,
        extend,
        page,
        hidden,
    } = *moved;
    let head = selection.head;
    // Sans extension, gauche et droite réduisent d'abord une sélection à son bord.
    if !extend && !selection.is_empty() {
        match direction {
            "left" => return Ok(selection.start()),
            "right" => return Ok(selection.end()),
            _ => {}
        }
    }
    match direction {
        "left" => skip_hidden(
            buffer,
            movement::prev_grapheme(buffer, head)?,
            false,
            hidden,
        ),
        "right" => skip_hidden(buffer, movement::next_grapheme(buffer, head)?, true, hidden),
        "up" => vertical(buffer, head, true, 1, hidden),
        "down" => vertical(buffer, head, false, 1, hidden),
        "page-up" => vertical(buffer, head, true, page.max(1), hidden),
        "page-down" => vertical(buffer, head, false, page.max(1), hidden),
        "word-left" => skip_hidden(buffer, movement::prev_word(buffer, head)?, false, hidden),
        "word-right" => skip_hidden(buffer, movement::next_word(buffer, head)?, true, hidden),
        "line-start" => movement::line_start(buffer, head),
        "line-end" => movement::line_end(buffer, head),
        "doc-start" => Ok(movement::document_start(buffer)),
        "doc-end" => Ok(movement::document_end(buffer)),
        _ => Ok(head),
    }
}

/// Sélections après un déplacement de toutes les têtes.
fn move_selections(
    buffer: &Rope,
    selections: &Selections,
    moved: &Moved<'_>,
) -> Result<Selections, TextError> {
    let extend = moved.extend;
    let moved = selections
        .iter()
        .map(|&selection| {
            let head = moved_head(buffer, selection, moved)?;
            Ok(if extend {
                Selection::new(selection.anchor, head)
            } else {
                Selection::cursor(head)
            })
        })
        .collect::<Result<Vec<_>, TextError>>()?;
    Ok(Selections::from_vec(moved, selections.primary_index())
        .unwrap_or_else(|| selections.clone()))
}

#[cfg(test)]
mod tests {
    use cupride_text::{Selection, Selections};

    use crate::command::{CommandError, CommandOutput};
    use crate::document::Document;
    use crate::editor::Editor;

    fn editor_with(text: &str, cursor: usize) -> Editor {
        let mut editor = Editor::new();
        let mut document = Document::from_text(text);
        document.set_selections(Selections::cursor(cursor));
        editor.insert_document(document);
        editor
    }

    fn text(editor: &Editor) -> String {
        editor.active_document().unwrap().buffer().to_string()
    }

    fn heads(editor: &Editor) -> Vec<usize> {
        editor
            .active_document()
            .unwrap()
            .selections()
            .iter()
            .map(|s| s.head)
            .collect()
    }

    fn run(editor: &mut Editor, lines: &[&str]) {
        for line in lines {
            editor
                .execute(line)
                .unwrap_or_else(|e| panic!("{line} : {e}"));
        }
    }

    #[test]
    fn inserer_et_supprimer() {
        let mut e = editor_with("monde", 0);
        run(&mut e, &["edit insert 'bonjour '"]);
        assert_eq!(text(&e), "bonjour monde");
        assert_eq!(heads(&e), [8]);
        run(&mut e, &["edit delete-backward", "edit delete-backward"]);
        assert_eq!(text(&e), "bonjoumonde");
        run(&mut e, &["edit delete-forward"]);
        assert_eq!(text(&e), "bonjouonde");
    }

    #[test]
    fn suppression_par_grapheme() {
        let mut e = editor_with("a👨‍👩‍👧b", 0);
        run(
            &mut e,
            &["edit move right", "edit move right", "edit delete-backward"],
        );
        assert_eq!(text(&e), "ab");
    }

    #[test]
    fn saut_de_ligne_avec_la_fin_de_ligne_du_document() {
        let mut e = editor_with("un\r\ndeux", 2);
        run(&mut e, &["edit newline"]);
        assert_eq!(text(&e), "un\r\n\r\ndeux");
        let mut e = editor_with("un\ndeux", 2);
        run(&mut e, &["edit newline"]);
        assert_eq!(text(&e), "un\n\ndeux");
    }

    #[test]
    fn annuler_et_retablir() {
        let mut e = editor_with("", 0);
        run(&mut e, &["edit insert a", "edit insert b"]);
        // Frappes consécutives : un seul bloc.
        run(&mut e, &["edit undo"]);
        assert_eq!(text(&e), "");
        run(&mut e, &["edit redo"]);
        assert_eq!(text(&e), "ab");
        assert_eq!(heads(&e), [2]);
        run(&mut e, &["edit undo"]);
        assert_eq!(
            e.execute("edit undo").unwrap(),
            CommandOutput::message("rien à annuler")
        );
    }

    #[test]
    fn deplacements() {
        let mut e = editor_with("let x = 1;\nfoo(bar);\nz", 0);
        run(&mut e, &["edit move word-right"]);
        assert_eq!(heads(&e), [3]);
        run(&mut e, &["edit move line-end"]);
        assert_eq!(heads(&e), [10]);
        run(&mut e, &["edit move down"]);
        assert_eq!(heads(&e), [20], "ligne plus courte : fin de ligne");
        run(&mut e, &["edit move up"]);
        assert_eq!(heads(&e), [9], "même colonne, sans mémoire de colonne");
        run(&mut e, &["edit move doc-end"]);
        assert_eq!(heads(&e), [22]);
        run(&mut e, &["edit move down"]);
        assert_eq!(heads(&e), [22]);
        run(
            &mut e,
            &["edit move doc-start", "edit move up", "edit move left"],
        );
        assert_eq!(heads(&e), [0]);
        run(&mut e, &["edit move line-end", "edit move line-start"]);
        assert_eq!(heads(&e), [0]);
    }

    #[test]
    fn copier_couper_coller() {
        let mut e = editor_with("un deux trois", 0);
        assert_eq!(
            e.execute("edit copy").unwrap(),
            CommandOutput::message("rien à copier : aucune sélection")
        );
        run(&mut e, &["edit move word-right --extend", "edit copy"]);
        run(
            &mut e,
            &["edit move doc-end", "edit insert ' '", "edit paste"],
        );
        assert_eq!(text(&e), "un deux trois un");
        run(
            &mut e,
            &[
                "edit move doc-start",
                "edit move word-right --extend",
                "edit cut",
            ],
        );
        assert_eq!(text(&e), " deux trois un");
        run(&mut e, &["edit undo"]);
        assert_eq!(text(&e), "un deux trois un", "couper s'annule");
        run(&mut e, &["edit undo"]);
        assert_eq!(
            text(&e),
            "un deux trois ",
            "coller s'annule séparément de la frappe"
        );
    }

    #[test]
    fn un_fragment_par_curseur() {
        let mut e = editor_with("ab\ncd\nef", 0);
        run(
            &mut e,
            &[
                "edit add-cursor down",
                "edit add-cursor down",
                "edit move right --extend",
                "edit copy",
                "edit move line-end",
                "edit paste",
            ],
        );
        assert_eq!(
            text(&e),
            "aba\ncdc\nefe",
            "chaque curseur reçoit son fragment"
        );
        // Un seul curseur : les trois fragments, séparés par des fins de ligne.
        run(
            &mut e,
            &["edit keep-primary", "edit move doc-end", "edit paste"],
        );
        assert_eq!(text(&e), "aba\ncdc\nefea\nc\ne");
    }

    #[test]
    fn coller_du_texte_d_ailleurs() {
        let mut e = editor_with("1\r\n2", 0);
        e.clipboard_mut()
            .set(crate::clipboard::ClipboardKind::Standard, "x\ny\n".into())
            .unwrap();
        run(&mut e, &["edit paste"]);
        assert_eq!(text(&e), "x\r\ny\r\n1\r\n2", "fins de ligne du document");
        // Deux curseurs, deux lignes : une par curseur.
        let mut e = editor_with("a\nb", 1);
        e.clipboard_mut()
            .set(crate::clipboard::ClipboardKind::Standard, "X\nY".into())
            .unwrap();
        run(&mut e, &["edit add-cursor down", "edit paste"]);
        assert_eq!(text(&e), "aX\nbY");
        // Sélection primaire.
        e.clipboard_mut()
            .set(crate::clipboard::ClipboardKind::Primary, "!".into())
            .unwrap();
        run(&mut e, &["edit keep-primary", "edit paste --primary"]);
        assert!(text(&e).contains('!'));
        assert_eq!(
            Editor::new()
                .execute("edit paste")
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Err("aucun document ouvert".to_owned())
        );
    }

    #[test]
    fn tabulation_du_raccourci_par_defaut() {
        let keymap = crate::keymap::Keymap::with_defaults(crate::keymap::Platform::Other);
        let tab = crate::keymap::KeySequence::parse("tab", crate::keymap::Platform::Other).unwrap();
        let command = keymap.command_for(&tab).unwrap().to_owned();
        let mut e = editor_with("x", 0);
        run(&mut e, &[&command]);
        assert_eq!(
            text(&e),
            "    x",
            "un niveau d'indentation (4 espaces par défaut)"
        );
    }

    #[test]
    fn page_suivante_et_precedente() {
        let text: String = (0..100).map(|i| format!("{i:03}\n")).collect();
        let mut e = editor_with(&text, 1);
        e.set_page_lines(30);
        run(&mut e, &["edit move page-down"]);
        assert_eq!(heads(&e), [30 * 4 + 1], "30 lignes plus bas, même colonne");
        run(&mut e, &["edit move page-down --extend"]);
        let sel = e.active_document().unwrap().selections().primary();
        assert_eq!((sel.anchor, sel.head), (121, 60 * 4 + 1));
        run(
            &mut e,
            &[
                "edit move page-up",
                "edit move page-up",
                "edit move page-up",
            ],
        );
        assert_eq!(heads(&e), [0], "au-delà du début : début du texte");
    }

    #[test]
    fn colonne_verticale_en_graphemes() {
        let mut e = editor_with("é😀x\nabcd", 0);
        run(
            &mut e,
            &["edit move right", "edit move right", "edit move down"],
        );
        assert_eq!(
            heads(&e),
            [10],
            "colonne 2 en graphèmes sur la ligne suivante"
        );
    }

    #[test]
    fn selection_etendue_puis_remplacee() {
        let mut e = editor_with("le vaste monde", 3);
        run(&mut e, &["edit move word-right --extend"]);
        let sel = e.active_document().unwrap().selections().primary();
        assert_eq!(sel, Selection::new(3, 8));
        run(&mut e, &["edit insert petit"]);
        assert_eq!(text(&e), "le petit monde");
        assert_eq!(heads(&e), [8]);
        run(&mut e, &["edit move left --extend", "edit move left"]);
        assert_eq!(heads(&e), [7], "gauche réduit la sélection à son début");
    }

    #[test]
    fn tout_selectionner_et_supprimer() {
        let mut e = editor_with("tout\nle texte", 2);
        run(&mut e, &["edit select-all", "edit delete-backward"]);
        assert_eq!(text(&e), "");
    }

    #[test]
    fn multi_curseur() {
        let mut e = editor_with("ab\ncd\nef", 0);
        run(&mut e, &["edit add-cursor down", "edit add-cursor down"]);
        assert_eq!(heads(&e), [0, 3, 6]);
        run(&mut e, &["edit insert '> '"]);
        assert_eq!(text(&e), "> ab\n> cd\n> ef");
        assert_eq!(heads(&e), [2, 7, 12]);
        run(&mut e, &["edit move line-end", "edit delete-backward"]);
        assert_eq!(text(&e), "> a\n> c\n> e");
        run(&mut e, &["edit undo"]);
        assert_eq!(text(&e), "> ab\n> cd\n> ef");
        run(&mut e, &["edit keep-primary"]);
        assert_eq!(e.active_document().unwrap().selections().len(), 1);
    }

    #[test]
    fn curseurs_voisins_fusionnes_par_la_suppression() {
        let mut e = editor_with("abcd", 0);
        let mut selections = Selections::cursor(2);
        selections.push(Selection::cursor(3));
        e.active_document_mut().unwrap().set_selections(selections);
        run(&mut e, &["edit delete-backward", "edit delete-backward"]);
        assert_eq!(text(&e), "d");
        assert_eq!(heads(&e), [0]);
    }

    #[test]
    fn sans_document() {
        let mut e = Editor::new();
        assert!(matches!(
            e.execute("edit insert x"),
            Err(CommandError::NoActiveDocument)
        ));
        assert!(matches!(
            e.execute("edit move nulle-part"),
            Err(CommandError::InvalidArguments { .. })
        ));
    }

    #[test]
    fn occurrences_et_edition_a_chaque_curseur() {
        let mut e = editor_with("chat chien chat\nun chat", 1);
        run(&mut e, &["edit add-next-occurrence"]);
        let sel = |e: &Editor| {
            e.active_document()
                .unwrap()
                .selections()
                .iter()
                .map(|s| s.range())
                .collect::<Vec<_>>()
        };
        assert_eq!(sel(&e), vec![0..4], "le mot sous le curseur");
        run(
            &mut e,
            &["edit add-next-occurrence", "edit add-next-occurrence"],
        );
        assert_eq!(sel(&e), vec![0..4, 11..15, 19..23]);
        assert_eq!(
            e.execute("edit add-next-occurrence").unwrap(),
            CommandOutput::message("toutes les occurrences sont sélectionnées")
        );
        // Toutes les commandes d'édition agissent à chaque curseur.
        run(&mut e, &["edit insert 'loup'"]);
        assert_eq!(text(&e), "loup chien loup\nun loup");
        run(
            &mut e,
            &["edit delete-backward", "edit move left", "edit insert X"],
        );
        assert_eq!(text(&e), "loXu chien loXu\nun loXu");
        run(&mut e, &["edit keep-primary"]);
        assert_eq!(heads(&e).len(), 1, "Échap : un seul curseur");
        let mut e = editor_with("a b a b a", 0);
        assert_eq!(
            e.execute("edit select-all-occurrences").unwrap(),
            CommandOutput::message("3 occurrences")
        );
    }

    #[test]
    fn indenter_et_desindenter() {
        // Au curseur : jusqu'au prochain niveau.
        let mut e = editor_with("ab", 1);
        run(&mut e, &["edit indent"]);
        assert_eq!(text(&e), "a   b", "colonne 1 → colonne 4");
        // Sélection sur plusieurs lignes : chaque ligne, la dernière vide exclue.
        let mut e = editor_with("un\ndeux\ntrois", 0);
        e.active_document_mut()
            .unwrap()
            .set_selections(Selections::single(Selection::new(0, 8)));
        run(&mut e, &["edit indent"]);
        assert_eq!(text(&e), "    un\n    deux\ntrois");
        run(&mut e, &["edit outdent", "edit outdent"]);
        assert_eq!(text(&e), "un\ndeux\ntrois");
        // Tabulations.
        run(&mut e, &["edit indentation tabs"]);
        run(&mut e, &["edit indent"]);
        assert_eq!(text(&e), "\tun\n\tdeux\ntrois");
        run(&mut e, &["edit outdent"]);
        assert_eq!(text(&e), "un\ndeux\ntrois");
        assert_eq!(
            e.execute("edit indentation spaces --width 2").unwrap(),
            CommandOutput::message("Espaces : 2")
        );
    }

    #[test]
    fn entree_garde_l_indentation() {
        let mut e = editor_with("    if x {", 10);
        run(&mut e, &["edit newline", "edit insert y"]);
        assert_eq!(text(&e), "    if x {\n    y");
        // Au milieu de l'indentation : seulement ce qui précède le curseur.
        let mut e = editor_with("\t\tz", 1);
        run(&mut e, &["edit newline"]);
        assert_eq!(text(&e), "\t\n\t\tz");
    }

    #[test]
    fn indentation_detectee_a_l_ouverture() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Makefile");
        std::fs::write(&path, "all:\n\techo\n").unwrap();
        let mut e = Editor::new();
        e.open(&path).unwrap();
        assert!(e.active_document().unwrap().indent().tabs);
        let other = dir.path().join("deux.js");
        std::fs::write(&other, "a {\n  b\n}\n").unwrap();
        e.open(&other).unwrap();
        assert_eq!(e.active_document().unwrap().indent().width, 2);
    }
}
