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
    Ok(())
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
    let tree = syntax
        .tree()
        .ok_or_else(|| CommandError::Failed("analyse syntaxique impossible".to_owned()))?;
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
