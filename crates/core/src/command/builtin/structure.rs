//! Sélection et déplacements structurels, d'après l'arbre syntaxique : étendre ou réduire la
//! sélection d'un nœud, aller à la fonction suivante ou précédente.

use cupride_syntax::Syntax;
use cupride_text::{Selection, Selections, TextBuffer};

use super::active;
use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry, CommandResult};
use crate::document::Document;
use crate::editor::Editor;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(CommandDef::new(
        "edit",
        "expand-selection",
        "Étend chaque sélection au nœud syntaxique qui la contient (mot, expression, bloc, \
         fonction…)",
        expand,
    ))?;
    registry.register(CommandDef::new(
        "edit",
        "shrink-selection",
        "Réduit chaque sélection : revient à l'étape précédente, ou au premier nœud enfant",
        shrink,
    ))?;
    registry.register(CommandDef::new(
        "edit",
        "next-function",
        "Va au début de la fonction suivante (au titre suivant en Markdown)",
        |editor, _| goto_function(editor, true),
    ))?;
    registry.register(CommandDef::new(
        "edit",
        "previous-function",
        "Va au début de la fonction précédente (au titre précédent en Markdown)",
        |editor, _| goto_function(editor, false),
    ))?;
    Ok(())
}

/// Étapes de `edit expand-selection`, pour que `edit shrink-selection` les défasse.
#[derive(Debug, Default)]
pub(crate) struct Expansions {
    /// Sélections avant chaque extension.
    stack: Vec<Selections>,
    /// Sélections après la dernière extension : si elles ont changé depuis, les étapes sont
    /// oubliées.
    after: Option<Selections>,
}

/// Arbre syntaxique du document, ou erreur.
fn syntax(document: &Document) -> Result<&Syntax, CommandError> {
    document
        .syntax()
        .filter(|s| s.tree().is_some())
        .ok_or_else(|| {
            CommandError::Failed(match (document.syntax(), document.language()) {
                (Some(syntax), _) if syntax.is_parsing() => {
                    "analyse syntaxique en cours".to_owned()
                }
                (_, Some(language)) => format!("pas d'arbre syntaxique pour le langage {language}"),
                (_, None) => "langage du document inconnu : pas d'arbre syntaxique".to_owned(),
            })
        })
}

/// Sélection de `range`, dans le sens de `like`.
fn oriented(range: std::ops::Range<usize>, like: Selection) -> Selection {
    if like.is_forward() || like.is_empty() {
        Selection::new(range.start, range.end)
    } else {
        Selection::new(range.end, range.start)
    }
}

/// `edit expand-selection`.
fn expand(editor: &mut Editor, _: &clap::ArgMatches) -> CommandResult {
    let (_, document) = active(editor)?;
    let syntax = syntax(document)?;
    let current = document.selections().clone();
    let ranges: Vec<Selection> = current
        .iter()
        .map(|s| syntax.expand(s.range()).map_or(*s, |r| oriented(r, *s)))
        .collect();
    let Some(expanded) = Selections::from_vec(ranges, current.primary_index()) else {
        return Ok(CommandOutput::None);
    };
    if expanded == current {
        return Ok(CommandOutput::message(
            "la sélection couvre déjà tout le document",
        ));
    }
    let expansions = &mut document.expansions;
    if expansions.after.as_ref() != Some(&current) {
        expansions.stack.clear();
    }
    expansions.stack.push(current);
    expansions.after = Some(expanded.clone());
    document.set_selections(expanded);
    Ok(CommandOutput::None)
}

/// `edit shrink-selection`.
fn shrink(editor: &mut Editor, _: &clap::ArgMatches) -> CommandResult {
    let (_, document) = active(editor)?;
    let current = document.selections().clone();
    // Juste après des extensions : on revient sur ses pas.
    if document.expansions.after.as_ref() == Some(&current)
        && let Some(previous) = document.expansions.stack.pop()
    {
        document.expansions.after = Some(previous.clone());
        document.set_selections(previous);
        return Ok(CommandOutput::None);
    }
    let syntax = syntax(document)?;
    let ranges: Vec<Selection> = current
        .iter()
        .map(|s| {
            if s.is_empty() {
                return *s;
            }
            syntax
                .shrink(s.range(), s.head)
                .map_or(*s, |r| oriented(r, *s))
        })
        .collect();
    if let Some(shrunk) = Selections::from_vec(ranges, current.primary_index()) {
        document.set_selections(shrunk);
    }
    Ok(CommandOutput::None)
}

/// `edit next-function`, `edit previous-function` : chaque curseur va au début de la fonction
/// suivante ou précédente.
fn goto_function(editor: &mut Editor, forward: bool) -> CommandResult {
    let (_, document) = active(editor)?;
    let syntax = syntax(document)?;
    let len = document.buffer().len_bytes();
    let current = document.selections().clone();
    let mut moved = false;
    let cursors: Vec<Selection> = current
        .iter()
        .map(|s| match syntax.function_near(s.head, forward, len) {
            Some(start) => {
                moved = true;
                Selection::cursor(start)
            }
            None => *s,
        })
        .collect();
    if !moved {
        return Ok(CommandOutput::message(if forward {
            "aucune fonction après le curseur"
        } else {
            "aucune fonction avant le curseur"
        }));
    }
    if let Some(cursors) = Selections::from_vec(cursors, current.primary_index()) {
        document.set_selections(cursors);
    }
    Ok(CommandOutput::None)
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::editor::Editor;

    fn editor(name: &str, text: &str) -> (Editor, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        let mut editor = Editor::new();
        editor.open(&path).unwrap();
        (editor, dir)
    }

    fn selected(editor: &Editor) -> Vec<String> {
        let document = editor.active_document().unwrap();
        let text = document.buffer().to_string();
        document
            .selections()
            .iter()
            .map(|s| text[s.range()].to_owned())
            .collect()
    }

    #[test]
    fn etendre_et_reduire_la_selection() {
        let (mut e, _dir) = editor("main.rs", "fn f() {\n    let x = a + b * c;\n}\n");
        e.execute("edit goto 2:17").unwrap(); // sur « b »
        e.execute("edit expand-selection").unwrap();
        assert_eq!(selected(&e), ["b"]);
        e.execute("edit expand-selection").unwrap();
        assert_eq!(selected(&e), ["b * c"]);
        e.execute("edit expand-selection").unwrap();
        assert_eq!(selected(&e), ["a + b * c"]);
        // Réduire défait les extensions, dans l'ordre.
        e.execute("edit shrink-selection").unwrap();
        assert_eq!(selected(&e), ["b * c"]);
        e.execute("edit shrink-selection").unwrap();
        assert_eq!(selected(&e), ["b"]);
        e.execute("edit shrink-selection").unwrap();
        assert_eq!(selected(&e), [""]);
        // Sans extension avant : premier enfant qui contient le curseur.
        e.execute("edit goto 2:13").unwrap(); // sur « a »
        for _ in 0..2 {
            e.execute("edit expand-selection").unwrap();
        }
        assert_eq!(selected(&e), ["a + b * c"]);
        e.execute("edit move right --extend").unwrap();
        e.execute("edit move left --extend").unwrap();
        e.execute("edit shrink-selection").unwrap();
        assert_eq!(selected(&e), ["a"]);
    }

    #[test]
    fn plusieurs_curseurs() {
        let (mut e, _dir) = editor("main.c", "int a = f(1);\nint b = g(2);\n");
        e.execute("edit goto 1:11").unwrap();
        e.execute("edit add-cursor down").unwrap();
        e.execute("edit expand-selection").unwrap();
        e.execute("edit expand-selection").unwrap();
        assert_eq!(selected(&e), ["(1)", "(2)"]);
    }

    #[test]
    fn fonctions_suivante_et_precedente() {
        let (mut e, _dir) = editor(
            "main.rs",
            "fn a() {}\n\nimpl S {\n    fn m(&self) {}\n}\n\nfn b() {}\n",
        );
        e.execute("edit next-function").unwrap();
        let head = |e: &Editor| e.active_document().unwrap().selections().primary().head;
        assert_eq!(head(&e), 24, "fn m");
        e.execute("edit next-function").unwrap();
        assert_eq!(head(&e), 42, "fn b");
        let output = e.execute("edit next-function").unwrap();
        assert_eq!(
            output,
            CommandOutput::message("aucune fonction après le curseur")
        );
        e.execute("edit previous-function").unwrap();
        e.execute("edit previous-function").unwrap();
        assert_eq!(head(&e), 0);
    }

    #[test]
    fn sans_arbre() {
        let mut e = Editor::new();
        e.insert_document(crate::Document::from_text("texte"));
        assert!(e.execute("edit expand-selection").is_err());
    }
}
