//! Groupe `syntax` : arbre syntaxique du document (Tree-sitter).

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry, CommandResult};
use crate::editor::Editor;

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "syntax",
            "tree",
            "Nœuds de l'arbre syntaxique sous le curseur, du plus large au plus précis ; --full : \
             tout l'arbre",
            tree,
        )
        .arg(
            clap::Arg::new("full")
                .long("full")
                .action(clap::ArgAction::SetTrue)
                .help("Tout l'arbre, en S-expression"),
        ),
    )?;
    registry.register(CommandDef::new(
        "outline",
        "list",
        "Plan du document : fonctions, types, modules (titres en Markdown), avec leur ligne",
        |editor, _| {
            let symbols = outline(editor)?;
            if symbols.is_empty() {
                return Ok(CommandOutput::message("aucun symbole dans le document"));
            }
            let lines: Vec<String> = symbols
                .iter()
                .map(|(line, column, symbol)| {
                    format!("{line}:{column} {} {}", symbol.kind, symbol.name)
                })
                .collect();
            Ok(CommandOutput::message(lines.join("\n")))
        },
    ))?;
    registry.register(CommandDef::new(
        "ui",
        "outline",
        "Plan du document dans un sélecteur : taper une partie d'un nom, Entrée pour y aller",
        |editor, _| {
            outline(editor)?;
            editor.request_ui(crate::UiRequest::Outline);
            Ok(CommandOutput::None)
        },
    ))?;
    Ok(())
}

/// Symboles du document actif, avec la ligne et la colonne (à partir de 1, colonne en
/// caractères) de leur nom.
pub(crate) fn outline(
    editor: &Editor,
) -> Result<Vec<(usize, usize, cupride_syntax::Symbol)>, CommandError> {
    use cupride_text::TextBuffer;

    let document = editor
        .active_document()
        .ok_or(CommandError::NoActiveDocument)?;
    let syntax = document
        .syntax()
        .filter(|s| s.tree().is_some())
        .ok_or_else(|| {
            CommandError::Failed(match (document.syntax(), document.language()) {
                (Some(s), _) if s.is_parsing() => "analyse syntaxique en cours".to_owned(),
                (_, Some(language)) => format!("pas de plan pour le langage {language}"),
                (_, None) => "langage du document inconnu : pas de plan".to_owned(),
            })
        })?;
    let buffer = document.buffer();
    Ok(syntax
        .symbols(buffer)
        .into_iter()
        .map(|symbol| {
            let at = symbol.name_range.start;
            let line = buffer.byte_to_line(at).unwrap_or_default();
            let start = buffer.line_to_byte(line).unwrap_or_default();
            let column = buffer
                .slice(start..at)
                .map_or(0, |before| before.chars().count());
            (line + 1, column + 1, symbol)
        })
        .collect())
}

/// `syntax tree`.
fn tree(editor: &mut Editor, args: &clap::ArgMatches) -> CommandResult {
    let document = editor
        .active_document()
        .ok_or(CommandError::NoActiveDocument)?;
    let syntax = document.syntax().ok_or_else(|| {
        CommandError::Failed(match document.language() {
            Some(language) => format!("pas de grammaire pour le langage {language}"),
            None => "langage du document inconnu : pas d'arbre syntaxique".to_owned(),
        })
    })?;
    let tree = syntax.tree().ok_or_else(|| {
        CommandError::Failed(if syntax.is_parsing() {
            "analyse syntaxique en cours".to_owned()
        } else {
            "analyse syntaxique impossible".to_owned()
        })
    })?;
    if args.get_flag("full") {
        return Ok(CommandOutput::message(tree.root_node().to_sexp()));
    }
    let selection = document.selections().primary();
    let mut node = syntax.named_node_at(selection.range());
    let mut path = Vec::new();
    while let Some(current) = node {
        let start = current.start_position();
        path.push(format!(
            "{} ({}:{})",
            current.kind(),
            start.row + 1,
            start.column + 1
        ));
        node = current.parent();
    }
    path.reverse();
    Ok(CommandOutput::message(path.join(" › ")))
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::editor::Editor;

    #[test]
    fn noeuds_sous_le_curseur() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        std::fs::write(&path, "fn main() {\n    let x = 1;\n}\n").unwrap();
        let mut editor = Editor::new();
        editor.open(&path).unwrap();
        editor.execute("edit goto 2:9").unwrap();
        let CommandOutput::Message(path) = editor.execute("syntax tree").unwrap() else {
            panic!("message attendu");
        };
        assert_eq!(
            path,
            "source_file (1:1) › function_item (1:1) › block (1:11) › let_declaration (2:5) › \
             identifier (2:9)"
        );
        let CommandOutput::Message(full) = editor.execute("syntax tree --full").unwrap() else {
            panic!("message attendu");
        };
        assert!(full.starts_with("(source_file (function_item"), "{full}");
    }

    #[test]
    fn gros_fichier_analyse_en_arriere_plan() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gros.c");
        let text: String = (0..30_000).map(|i| format!("int v{i} = {i};\n")).collect();
        std::fs::write(&path, text).unwrap();
        let mut editor = Editor::new();
        let woken = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&woken);
        editor.set_syntax_waker(cupride_syntax::Waker::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        editor.open(&path).unwrap();
        let error = editor.execute("syntax tree").unwrap_err();
        assert_eq!(error.to_string(), "analyse syntaxique en cours");
        let started = std::time::Instant::now();
        while woken.load(Ordering::SeqCst) == 0 {
            assert!(started.elapsed().as_secs() < 30, "analyse trop longue");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(editor.poll_syntax());
        let CommandOutput::Message(path) = editor.execute("syntax tree").unwrap() else {
            panic!("message attendu");
        };
        assert!(path.starts_with("translation_unit"), "{path}");
    }

    #[test]
    fn plan_du_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        std::fs::write(&path, "struct S;\n\nimpl S {\n    fn é() {}\n}\n").unwrap();
        let mut editor = Editor::new();
        editor.open(&path).unwrap();
        let CommandOutput::Message(list) = editor.execute("outline list").unwrap() else {
            panic!("message attendu");
        };
        assert_eq!(list, "1:8 type S\n4:8 méthode é");
        editor.execute("ui outline").unwrap();
        assert_eq!(editor.take_ui_requests(), [crate::UiRequest::Outline]);
        editor.insert_document(crate::Document::from_text("texte"));
        assert!(editor.execute("ui outline").is_err());
    }

    #[test]
    fn sans_grammaire() {
        let mut editor = Editor::new();
        editor.insert_document(crate::Document::from_text("texte"));
        let error = editor.execute("syntax tree").unwrap_err();
        assert!(error.to_string().contains("langage du document inconnu"));
    }

    #[test]
    fn langage_d_apres_la_ligne_shebang() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outil");
        std::fs::write(&path, "#!/usr/bin/env -S cargo -Zscript\nfn main() {}\n").unwrap();
        let mut editor = Editor::new();
        editor.open(&path).unwrap();
        let document = editor.active_document().unwrap();
        assert_eq!(document.language(), Some("Rust"));
        assert!(document.syntax().is_some());
    }

    #[test]
    fn arbre_tenu_a_jour_et_langage_change_au_renommage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "").unwrap();
        let mut editor = Editor::new();
        editor.open(&path).unwrap();
        assert!(editor.active_document().unwrap().syntax().is_none());
        editor
            .execute("edit insert 'int main(void) { return 0; }'")
            .unwrap();
        editor
            .execute(&format!(
                "file save-as {}",
                crate::command::quote(&dir.path().join("main.c").to_string_lossy())
            ))
            .unwrap();
        let document = editor.active_document().unwrap();
        assert_eq!(document.language(), Some("C"));
        let root = document.syntax().unwrap().tree().unwrap().root_node();
        assert_eq!(root.kind(), "translation_unit");
        assert!(!root.has_error());
        editor.execute("edit insert ' int x;'").unwrap();
        let root = editor
            .active_document()
            .unwrap()
            .syntax()
            .unwrap()
            .tree()
            .unwrap()
            .root_node();
        assert_eq!(root.named_child_count(), 2);
    }
}
