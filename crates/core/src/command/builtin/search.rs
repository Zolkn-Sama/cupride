//! Groupe `search` : rechercher dans le document actif.

use clap::{Arg, ArgAction};

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;
use crate::search::SearchQuery;
use crate::text::TextBuffer;
use crate::ui_request::UiRequest;

fn flag(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .long(name)
        .action(ArgAction::SetTrue)
        .help(help)
}

/// « 3/12 ».
fn position((index, count): (usize, usize)) -> CommandOutput {
    CommandOutput::message(format!("{index}/{count}"))
}

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    registry.register(
        CommandDef::new(
            "search",
            "find",
            "Cherche un motif dans le document actif et sélectionne l'occurrence suivante",
            |editor, args| {
                let query = SearchQuery {
                    pattern: args.get_one::<String>("motif").cloned().unwrap_or_default(),
                    case_sensitive: args.get_flag("case"),
                    whole_word: args.get_flag("word"),
                    regex: args.get_flag("regex"),
                };
                query
                    .compile()
                    .map_err(|e| CommandError::Failed(e.to_string()))?;
                editor.set_search_query(Some(query));
                // À partir du début de la sélection : une occurrence sous le curseur compte.
                let from = editor
                    .active_document()
                    .map(|d| d.selections().primary().start());
                Ok(position(editor.select_match(true, from)?))
            },
        )
        .arg(
            Arg::new("motif")
                .required(true)
                .help("Texte (ou expression régulière avec --regex)"),
        )
        .arg(flag("case", "Respecte la casse"))
        .arg(flag("word", "Mot entier seulement"))
        .arg(flag("regex", "Le motif est une expression régulière")),
    )?;

    registry.register(CommandDef::new(
        "search",
        "next",
        "Sélectionne l'occurrence suivante de la recherche en cours",
        |editor, _| Ok(position(editor.select_match(true, None)?)),
    ))?;

    registry.register(CommandDef::new(
        "search",
        "previous",
        "Sélectionne l'occurrence précédente de la recherche en cours",
        |editor, _| Ok(position(editor.select_match(false, None)?)),
    ))?;

    registry.register(CommandDef::new(
        "search",
        "clear",
        "Arrête la recherche en cours (les occurrences ne sont plus surlignées)",
        |editor, _| {
            editor.set_search_query(None);
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(CommandDef::new(
        "ui",
        "find",
        "Ouvre la barre de recherche (préremplie avec la sélection)",
        |editor, _| {
            editor.request_ui(UiRequest::Find(prefill(editor)));
            Ok(CommandOutput::None)
        },
    ))?;

    Ok(())
}

/// Motif proposé à l'ouverture de la barre : la sélection si elle tient sur une ligne, sinon le
/// motif en cours.
fn prefill(editor: &Editor) -> String {
    let selected = editor.active_document().and_then(|d| {
        let selection = d.selections().primary();
        let text = d.buffer().slice(selection.range()).ok()?;
        (!text.is_empty() && !text.contains('\n')).then_some(text)
    });
    selected
        .or_else(|| editor.search().query().map(|q| q.pattern.clone()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::command::CommandOutput;
    use crate::editor::Editor;
    use crate::ui_request::UiRequest;

    #[test]
    fn chercher() {
        let mut e = Editor::new();
        e.insert_document(crate::Document::from_text("Rouge vert rouge bleu ROUGE"));
        assert_eq!(
            e.execute("search find rouge").unwrap(),
            CommandOutput::message("1/3")
        );
        assert_eq!(
            e.execute("search next").unwrap(),
            CommandOutput::message("2/3")
        );
        assert_eq!(
            e.execute("search find rouge --case").unwrap(),
            CommandOutput::message("1/1"),
            "à partir de la sélection : celle sous le curseur compte"
        );
        assert_eq!(
            e.execute("search previous").unwrap(),
            CommandOutput::message("1/1")
        );
        assert!(e.execute("search find '('  --regex").is_err());
        e.execute("search clear").unwrap();
        assert!(e.execute("search next").is_err());
        // La barre est préremplie avec la sélection.
        e.execute("search find vert").unwrap();
        e.execute("search clear").unwrap();
        e.execute("ui find").unwrap();
        assert_eq!(e.take_ui_requests(), [UiRequest::Find("vert".into())]);
    }
}
