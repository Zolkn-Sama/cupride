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

/// Part de la fenêtre donnée au panneau de recherche quand il s'ouvre.
const SEARCH_WIDTH: f32 = 0.3;

/// Motif et options des arguments `motif`, `--case`, `--word`, `--regex`.
fn query_from(args: &clap::ArgMatches) -> SearchQuery {
    SearchQuery {
        pattern: args.get_one::<String>("motif").cloned().unwrap_or_default(),
        case_sensitive: args.get_flag("case"),
        whole_word: args.get_flag("word"),
        regex: args.get_flag("regex"),
    }
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
                let query = query_from(args);
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

    registry.register(
        CommandDef::new(
            "search",
            "replace",
            "Remplace l'occurrence sélectionnée, puis sélectionne la suivante ($1, ${nom} : groupes de capture avec --regex)",
            |editor, args| {
                let with = args.get_one::<String>("texte").cloned().unwrap_or_default();
                Ok(match editor.replace_match(&with)? {
                    Some(at) => position(at),
                    None => CommandOutput::message("plus aucune occurrence"),
                })
            },
        )
        .arg(Arg::new("texte").required(true).help("Texte de remplacement")),
    )?;

    registry.register(
        CommandDef::new(
            "search",
            "replace-all",
            "Remplace toutes les occurrences, en une seule modification annulable",
            |editor, args| {
                let with = args.get_one::<String>("texte").cloned().unwrap_or_default();
                let count = editor.replace_all(&with)?;
                Ok(CommandOutput::message(format!(
                    "{count} remplacement{}",
                    if count > 1 { "s" } else { "" }
                )))
            },
        )
        .arg(
            Arg::new("texte")
                .required(true)
                .help("Texte de remplacement"),
        ),
    )?;

    registry.register(CommandDef::new(
        "ui",
        "replace",
        "Ouvre la barre de recherche avec le champ de remplacement",
        |editor, _| {
            editor.request_ui(UiRequest::Replace(prefill(editor)));
            Ok(CommandOutput::None)
        },
    ))?;

    registry.register(
        CommandDef::new(
            "search",
            "project",
            "Cherche un motif dans tous les fichiers du projet ; résultats dans le panneau de recherche",
            |editor, args| {
                let query = query_from(args);
                query
                    .compile()
                    .map_err(|e| CommandError::Failed(e.to_string()))?;
                let root = editor
                    .search_root()
                    .ok_or_else(|| CommandError::Failed("aucun dossier où chercher".into()))?;
                let message = format!("recherche de « {} » dans {}", query.pattern, root.display());
                let generation = editor.start_project_search(query, root);
                editor.request_ui(UiRequest::ProjectSearch(generation));
                editor.show_side_panel(crate::layout::PanelKind::SEARCH, SEARCH_WIDTH);
                Ok(CommandOutput::message(message))
            },
        )
        .arg(Arg::new("motif").required(true).help("Texte (ou expression régulière avec --regex)"))
        .arg(flag("case", "Respecte la casse"))
        .arg(flag("word", "Mot entier seulement"))
        .arg(flag("regex", "Le motif est une expression régulière")),
    )?;

    registry.register(
        CommandDef::new(
            "search",
            "project-replace",
            "Montre dans le panneau de recherche les résultats remplacés par le texte, sans rien modifier",
            |editor, args| {
                let with = args.get_one::<String>("texte").cloned().unwrap_or_default();
                Ok(CommandOutput::message(editor.preview_project_replace(&with)?))
            },
        )
        .arg(Arg::new("texte").required(true).help("Texte de remplacement")),
    )?;

    registry.register(CommandDef::new(
        "search",
        "project-apply",
        "Applique le remplacement montré par « search project-replace » à tous les fichiers",
        |editor, _| {
            let (count, files) = editor.apply_project_replace()?;
            Ok(CommandOutput::message(format!(
                "{count} remplacement{} dans {files} fichier{}",
                if count > 1 { "s" } else { "" },
                if files > 1 { "s" } else { "" }
            )))
        },
    ))?;

    registry.register(CommandDef::new(
        "ui",
        "search-project",
        "Prépare une recherche dans le projet dans la ligne de commande",
        |editor, _| {
            let prefill = prefill(editor);
            let line = if prefill.is_empty() {
                "search project ".to_owned()
            } else {
                format!("search project {}", crate::command::quote(&prefill))
            };
            editor.request_ui(UiRequest::CommandLine(line));
            Ok(CommandOutput::None)
        },
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

    #[test]
    fn recherche_dans_le_projet() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "vert\nbleu vert\n").unwrap();
        let mut e = Editor::new();
        e.set_project(dir.path()).unwrap();
        e.new_document();
        let output = e.execute("search project vert").unwrap();
        assert!(matches!(output, CommandOutput::Message(m) if m.contains("« vert »")));
        assert!(matches!(
            e.take_ui_requests()[..],
            [UiRequest::ProjectSearch(1)]
        ));
        let panel = e.active_panel().unwrap();
        assert_eq!(
            e.workspace().panel(panel).unwrap().kind().as_str(),
            "search"
        );
        // Le panneau de recherche existe déjà : il reprend le focus.
        e.execute("panel focus right").unwrap();
        e.execute("search project bleu").unwrap();
        assert_eq!(e.active_panel(), Some(panel));
        assert!(e.execute("search project-apply").is_err(), "rien en aperçu");
        e.execute("ui search-project").unwrap();
        assert!(
            matches!(&e.take_ui_requests()[..], [_, UiRequest::CommandLine(l)] if l.starts_with("search project"))
        );
    }
}
