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
use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};

/// Directions acceptées par `edit move`.
const DIRECTIONS: [&str; 10] = [
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
        "Insère un saut de ligne à chaque curseur, avec la fin de ligne du document",
        |editor, _| {
            let (_, document) = active(editor)?;
            let newline = document.line_ending().as_str();
            document.transact(EditKind::Insert, |buffer, tx| {
                replace_selections(buffer, tx, newline)
            })?;
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "edit",
        "delete-backward",
        "Supprime la sélection, ou le caractère avant chaque curseur (retour arrière)",
        |editor, _| {
            let (_, document) = active(editor)?;
            document.transact(EditKind::Delete, |buffer, tx| {
                delete_each(buffer, tx, movement::delete_backward_range)
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
            let (_, document) = active(editor)?;
            let selections = move_selections(document.buffer(), document.selections(), direction, extend)?;
            document.set_selections(selections);
            Ok(CommandOutput::None)
        })
        .arg(
            Arg::new("direction")
                .required(true)
                .value_parser(PossibleValuesParser::new(DIRECTIONS))
                .help("left, right, up, down, word-left, word-right, line-start, line-end, doc-start ou doc-end"),
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
                let target = vertical(document.buffer(), head, direction == "up")?;
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
    range_for: fn(&Rope, usize) -> Result<std::ops::Range<usize>, TextError>,
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

/// Position atteinte en montant ou en descendant d'une ligne depuis `offset`, à la même colonne
/// (en graphèmes) si la ligne est assez longue, sinon à sa fin. Au-delà de la première ou de la
/// dernière ligne : début ou fin du texte.
fn vertical(buffer: &Rope, offset: usize, up: bool) -> Result<usize, TextError> {
    let point = offset_to_grapheme_point(buffer, offset)?;
    let line = if up {
        match point.line.checked_sub(1) {
            Some(line) => line,
            None => return Ok(0),
        }
    } else if point.line + 1 < buffer.len_lines() {
        point.line + 1
    } else {
        return Ok(buffer.len_bytes());
    };
    let target = Point::new(line, point.column);
    grapheme_point_to_offset(buffer, target).or_else(|_| Ok(buffer.line_content_range(line)?.end))
}

/// Nouvelle position de la tête d'une sélection après un déplacement.
fn moved_head(
    buffer: &Rope,
    selection: Selection,
    direction: &str,
    extend: bool,
) -> Result<usize, TextError> {
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
        "left" => movement::prev_grapheme(buffer, head),
        "right" => movement::next_grapheme(buffer, head),
        "up" => vertical(buffer, head, true),
        "down" => vertical(buffer, head, false),
        "word-left" => movement::prev_word(buffer, head),
        "word-right" => movement::next_word(buffer, head),
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
    direction: &str,
    extend: bool,
) -> Result<Selections, TextError> {
    let moved = selections
        .iter()
        .map(|&selection| {
            let head = moved_head(buffer, selection, direction, extend)?;
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
}
