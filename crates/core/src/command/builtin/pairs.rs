//! Frappe avec fermeture automatique des paires (`edit type`), et parenthèse correspondante
//! (`edit goto-bracket`).

use std::ops::Range;

use clap::Arg;
use cupride_text::{Edit, EditKind, Rope, Selection, Selections, TextBuffer, TextError};

use super::active;
use crate::brackets::{bracket_pair, char_at, char_before, pairs};
use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry, CommandResult};
use crate::editor::Editor;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "edit",
            "type",
            "Tape du texte à chaque curseur, comme au clavier : les paires (parenthèses, \
             guillemets…) se ferment toutes seules ([editor] auto_close)",
            type_text,
        )
        .arg(Arg::new("texte").required(true).help("Texte tapé")),
    )?;
    registry.register(CommandDef::new(
        "edit",
        "goto-bracket",
        "Va à la parenthèse (crochet, accolade) qui correspond à celle à côté du curseur",
        goto_bracket,
    ))?;
    Ok(())
}

/// Ce que devient une sélection à la frappe : texte remplacé, texte inséré, puis sélection, en
/// positions relatives au début du remplacement.
struct Typed {
    range: Range<usize>,
    insert: String,
    anchor: usize,
    head: usize,
}

/// Paires de `language` actives, ou aucune si `[editor] auto_close` est désactivé.
pub(super) fn active_pairs(editor: &Editor, language: Option<&str>) -> &'static [(char, char)] {
    if editor.config().auto_close_for(language) {
        pairs(language)
    } else {
        &[]
    }
}

/// Une paire peut-elle être fermée devant `head` ? Seulement avant une espace, une fin de ligne,
/// une parenthèse fermante ou une ponctuation ; et pour un guillemet, pas juste après une lettre
/// (apostrophe d'un mot).
fn can_close(buffer: &Rope, head: usize, open: char, close: char) -> bool {
    let next_ok = char_at(buffer, head)
        .is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';' | ':' | '.'));
    if open != close {
        return next_ok;
    }
    let previous = char_before(buffer, head).map(|(_, c)| c);
    next_ok && previous.is_none_or(|p| !p.is_alphanumeric() && p != '_' && p != open)
}

fn plan(buffer: &Rope, selection: Selection, c: char, pairs: &[(char, char)]) -> Typed {
    let len = c.len_utf8();
    let head = selection.head;
    // Fermante déjà là (tapée automatiquement) : on passe par-dessus.
    if selection.is_empty()
        && pairs.iter().any(|&(_, close)| close == c)
        && char_at(buffer, head) == Some(c)
    {
        return Typed {
            range: head..head,
            insert: String::new(),
            anchor: len,
            head: len,
        };
    }
    if let Some(&(open, close)) = pairs.iter().find(|&&(open, _)| open == c) {
        // Sélection entourée, et gardée.
        if !selection.is_empty() {
            let range = selection.range();
            let inner = buffer.slice(range.clone()).unwrap_or_default();
            let (from, to) = (len, len + inner.len());
            let (anchor, head) = if selection.is_forward() {
                (from, to)
            } else {
                (to, from)
            };
            return Typed {
                range,
                insert: format!("{open}{inner}{close}"),
                anchor,
                head,
            };
        }
        if can_close(buffer, head, open, close) {
            return Typed {
                range: head..head,
                insert: format!("{open}{close}"),
                anchor: len,
                head: len,
            };
        }
    }
    let range = selection.range();
    let mut text = [0; 4];
    Typed {
        range,
        insert: c.encode_utf8(&mut text).to_owned(),
        anchor: len,
        head: len,
    }
}

/// `edit type`.
fn type_text(editor: &mut Editor, args: &clap::ArgMatches) -> CommandResult {
    let text: &String = args.get_one("texte").expect("argument requis");
    let language = editor.active_document().and_then(|d| d.language());
    let pairs = active_pairs(editor, language);
    let (_, document) = active(editor)?;
    let mut chars = text.chars();
    let typed = match (chars.next(), chars.next()) {
        (Some(c), None) if !pairs.is_empty() => c,
        // Plusieurs caractères (méthode de saisie), ou paires désactivées : texte inséré tel quel.
        _ => return editor.run("edit insert", std::slice::from_ref(text)),
    };
    let selections = document.selections().clone();
    let plans: Vec<Typed> = selections
        .iter()
        .map(|s| plan(document.buffer(), *s, typed, pairs))
        .collect();
    document.transact(EditKind::Insert, |buffer, tx| {
        // De la fin vers le début : les positions des suivantes ne bougent pas.
        for typed in plans.iter().rev() {
            if !typed.range.is_empty() || !typed.insert.is_empty() {
                let edit = Edit::replace(buffer, typed.range.clone(), &typed.insert)?;
                tx.apply(buffer, edit)?;
            }
        }
        let mut shift = 0isize;
        let mut after = Vec::with_capacity(plans.len());
        for typed in &plans {
            let start = typed.range.start.saturating_add_signed(shift);
            after.push(Selection::new(start + typed.anchor, start + typed.head));
            shift += typed.insert.len() as isize - typed.range.len() as isize;
        }
        let after = Selections::from_vec(after, selections.primary_index()).ok_or(
            TextError::InvalidRange {
                start: 0,
                end: 0,
                len: 0,
            },
        )?;
        tx.set_selections_after(after);
        Ok(())
    })?;
    // Aucune modification (on est passé par-dessus une fermante) : seules les sélections changent.
    if document.selections() == &selections {
        let moved: Vec<Selection> = plans
            .iter()
            .map(|t| Selection::new(t.range.start + t.anchor, t.range.start + t.head))
            .collect();
        if let Some(moved) = Selections::from_vec(moved, selections.primary_index()) {
            document.set_selections(moved);
        }
    }
    Ok(CommandOutput::None)
}

/// Plage d'une paire vide autour du curseur `head` (« (|) »), à effacer d'un coup.
pub(super) fn empty_pair_around(
    buffer: &Rope,
    head: usize,
    pairs: &[(char, char)],
) -> Option<Range<usize>> {
    let (start, open) = char_before(buffer, head)?;
    let close = char_at(buffer, head)?;
    pairs
        .contains(&(open, close))
        .then(|| start..head + close.len_utf8())
}

/// `edit goto-bracket`.
fn goto_bracket(editor: &mut Editor, _: &clap::ArgMatches) -> CommandResult {
    let (_, document) = active(editor)?;
    let current = document.selections().clone();
    let mut moved = false;
    let cursors: Vec<Selection> = current
        .iter()
        .map(|s| match bracket_pair(document, s.head) {
            Some((_, partner)) => {
                moved = true;
                Selection::cursor(partner)
            }
            None => *s,
        })
        .collect();
    if !moved {
        return Ok(CommandOutput::message(
            "aucune parenthèse à côté du curseur",
        ));
    }
    if let Some(cursors) = Selections::from_vec(cursors, current.primary_index()) {
        document.set_selections(cursors);
    }
    Ok(CommandOutput::None)
}

#[cfg(test)]
mod tests {
    use crate::editor::Editor;

    fn editor(name: &str, text: &str) -> (Editor, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        let mut editor = Editor::new();
        editor.open(&path).unwrap();
        (editor, dir)
    }

    /// Texte, avec « | » à chaque curseur (ou « [ » et « ] » autour des sélections).
    fn shown(editor: &Editor) -> String {
        let document = editor.active_document().unwrap();
        let mut text = document.buffer().to_string();
        for s in document.selections().iter().rev() {
            if s.is_empty() {
                text.insert(s.head, '|');
            } else {
                text.insert(s.end(), ']');
                text.insert(s.start(), '[');
            }
        }
        text
    }

    fn type_all(editor: &mut Editor, keys: &str) {
        for c in keys.chars() {
            editor.run("edit type", &[c.to_string()]).unwrap();
        }
    }

    #[test]
    fn paires_fermees_et_passees() {
        let (mut e, _dir) = editor("main.c", "");
        type_all(&mut e, "f(");
        assert_eq!(shown(&e), "f(|)");
        type_all(&mut e, "a[1");
        assert_eq!(shown(&e), "f(a[1|])");
        type_all(&mut e, "])");
        assert_eq!(shown(&e), "f(a[1])|", "les fermantes sont passées");
        type_all(&mut e, " \"x");
        assert_eq!(shown(&e), "f(a[1]) \"x|\"");
        type_all(&mut e, "\"");
        assert_eq!(shown(&e), "f(a[1]) \"x\"|");
        // Pas de fermeture devant un mot.
        e.execute("edit move doc-start").unwrap();
        type_all(&mut e, "(");
        assert_eq!(shown(&e), "(|f(a[1]) \"x\"");
    }

    #[test]
    fn apostrophes_et_rust() {
        let (mut e, _dir) = editor("notes.txt", "");
        type_all(&mut e, "l'arbre (");
        assert_eq!(shown(&e), "l'arbre (|)");
        let (mut e, _dir) = editor("main.rs", "");
        type_all(&mut e, "&'a");
        assert_eq!(shown(&e), "&'a|", "durée de vie");
        let (mut e, _dir) = editor("main.c", "");
        type_all(&mut e, "c = '");
        assert_eq!(shown(&e), "c = '|'");
        type_all(&mut e, "it'");
        assert_eq!(
            shown(&e),
            "c = 'it'|",
            "apostrophe après une lettre : passée"
        );
    }

    #[test]
    fn selection_entouree() {
        let (mut e, _dir) = editor("main.c", "un deux");
        e.execute("edit move word-right --extend").unwrap();
        type_all(&mut e, "(");
        assert_eq!(shown(&e), "([un]) deux");
        type_all(&mut e, "[");
        assert_eq!(shown(&e), "([[un]]) deux");
    }

    #[test]
    fn retour_arriere_efface_la_paire() {
        let (mut e, _dir) = editor("main.c", "");
        type_all(&mut e, "x(");
        e.execute("edit delete-backward").unwrap();
        assert_eq!(shown(&e), "x|");
        type_all(&mut e, "(a");
        e.execute("edit delete-backward").unwrap();
        e.execute("edit delete-backward").unwrap();
        assert_eq!(shown(&e), "x|");
    }

    #[test]
    fn plusieurs_curseurs_et_annulation() {
        let (mut e, _dir) = editor("main.c", "a\nb\n");
        e.execute("edit move line-end").unwrap();
        e.execute("edit add-cursor down").unwrap();
        type_all(&mut e, "(");
        assert_eq!(shown(&e), "a(|)\nb(|)\n");
        type_all(&mut e, "1)");
        assert_eq!(shown(&e), "a(1)|\nb(1)|\n");
        e.execute("edit undo").unwrap();
        assert_eq!(e.active_document().unwrap().buffer().to_string(), "a\nb\n");
    }

    #[test]
    fn desactive_par_la_configuration() {
        let (mut e, dir) = editor("main.c", "");
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "[languages.C]\nauto_close = false\n").unwrap();
        let loaded = crate::config::load(&config);
        e.apply_config(loaded.config);
        type_all(&mut e, "f(\"");
        assert_eq!(shown(&e), "f(\"|");
    }

    #[test]
    fn aller_a_la_parenthese() {
        let (mut e, _dir) = editor("main.rs", "fn f() { g(1); }");
        e.execute("edit goto 1:8").unwrap();
        e.execute("edit goto-bracket").unwrap();
        assert_eq!(shown(&e), "fn f() { g(1); |}");
        e.execute("edit goto-bracket").unwrap();
        assert_eq!(shown(&e), "fn f() |{ g(1); }");
        e.execute("edit move doc-start").unwrap();
        let output = e.execute("edit goto-bracket").unwrap();
        assert_eq!(
            output,
            crate::command::CommandOutput::message("aucune parenthèse à côté du curseur")
        );
    }
}
