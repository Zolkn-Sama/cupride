//! Groupes `split` et `panel` : diviser l'espace, ouvrir et fermer des panneaux.

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction};

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;
use crate::layout::{Bounds, Direction, PanelKind};
use crate::panels::NewPanel;

/// Noms des directions, tels qu'on les tape.
pub(crate) const DIRECTIONS: [&str; 4] = ["left", "right", "up", "down"];

/// Direction de l'argument `name` (déjà validé par clap).
pub(crate) fn direction(args: &clap::ArgMatches, name: &str) -> Option<Direction> {
    args.get_one::<String>(name)
        .and_then(|d| Direction::parse(d))
}

pub(super) fn register(registry: &mut CommandRegistry) -> Result<(), CommandError> {
    for (direction, description) in [
        (
            Direction::Right,
            "Divise la zone active : le document actif s'ouvre aussi à droite",
        ),
        (
            Direction::Down,
            "Divise la zone active : le document actif s'ouvre aussi en dessous",
        ),
        (
            Direction::Left,
            "Divise la zone active : le document actif s'ouvre aussi à gauche",
        ),
        (
            Direction::Up,
            "Divise la zone active : le document actif s'ouvre aussi au-dessus",
        ),
    ] {
        registry.register(CommandDef::new(
            "split",
            direction.name(),
            description,
            move |editor, _| {
                split_editor(editor, NewPanel::Split(direction));
                Ok(CommandOutput::None)
            },
        ))?;
    }

    registry.register(
        CommandDef::new(
            "panel",
            "open",
            "Ouvre un panneau du type indiqué (editor, explorer…), en onglet ou dans une nouvelle zone",
            |editor, args| {
                let kind: &String = args.get_one("type").expect("argument requis");
                let place = direction(args, "split").map_or(NewPanel::Tab, NewPanel::Split);
                open(editor, kind, place)?;
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("type")
                .required(true)
                .help("Type de panneau : editor, explorer, ou un type fourni par un plugin"),
        )
        .arg(
            Arg::new("split")
                .long("split")
                .value_name("direction")
                .value_parser(PossibleValuesParser::new(DIRECTIONS))
                .help("Ouvre le panneau dans une nouvelle zone, de ce côté de la zone active"),
        ),
    )?;

    registry.register(
        CommandDef::new(
            "panel",
            "close",
            "Ferme le panneau actif ; son document est fermé s'il n'est plus affiché ailleurs",
            |editor, args| {
                let panel = editor.active_panel().ok_or(CommandError::NoActivePanel)?;
                editor.close_panel(panel, args.get_flag("force"))?;
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Ferme même si le document a des modifications non enregistrées"),
        ),
    )?;

    registry.register(
        CommandDef::new(
            "panel",
            "focus",
            "Donne le focus à la zone voisine, dans la direction indiquée",
            |editor, args| {
                let direction = direction(args, "direction").expect("argument requis");
                let panel = editor.active_panel().ok_or(CommandError::NoActivePanel)?;
                let neighbor = editor
                    .workspace()
                    .tree()
                    .neighbor(panel, direction, NAVIGATION_AREA, 0.0)
                    .ok_or_else(|| {
                        CommandError::Failed(format!(
                            "aucune zone {}",
                            match direction {
                                Direction::Left => "à gauche",
                                Direction::Right => "à droite",
                                Direction::Up => "au-dessus",
                                Direction::Down => "en dessous",
                            }
                        ))
                    })?;
                editor.focus_panel(neighbor);
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("direction")
                .required(true)
                .value_parser(PossibleValuesParser::new(DIRECTIONS))
                .help("left, right, up ou down"),
        ),
    )?;

    registry.register(CommandDef::new(
        "panel",
        "list",
        "Liste les panneaux, zone par zone (> actif)",
        |editor, _| Ok(CommandOutput::message(list(editor))),
    ))?;

    Ok(())
}

/// Surface sur laquelle on cherche la zone voisine : seules les proportions comptent, la taille
/// de la fenêtre n'y change rien (les recouvrements se comparent le long d'un seul axe).
const NAVIGATION_AREA: Bounds = Bounds::new(0.0, 0.0, 10_000.0, 10_000.0);

/// Ouvre le document actif dans un nouveau panneau ; sans document, un document vide.
fn split_editor(editor: &mut Editor, place: NewPanel) {
    let document = editor
        .active_id()
        .unwrap_or_else(|| editor.create_document());
    editor.open_editor_panel(document, place);
}

/// Ouvre un panneau de type `kind`.
fn open(editor: &mut Editor, kind: &str, place: NewPanel) -> Result<(), CommandError> {
    if kind == PanelKind::EDITOR {
        split_editor(editor, place);
        return Ok(());
    }
    if !editor.panel_kinds().any(|k| k == kind) {
        let known: Vec<&str> = editor.panel_kinds().collect();
        let closest = known
            .iter()
            .map(|k| (strsim::levenshtein(k, kind), *k))
            .min()
            .filter(|(distance, _)| *distance <= 3)
            .map(|(_, k)| format!(" (vouliez-vous dire « {k} » ?)"))
            .unwrap_or_default();
        return Err(CommandError::Failed(format!(
            "type de panneau inconnu : « {kind} »{closest} ; types connus : {}",
            known.join(", ")
        )));
    }
    editor.open_panel(PanelKind::new(kind), place);
    Ok(())
}

/// Liste des panneaux, zone par zone.
fn list(editor: &Editor) -> String {
    let workspace = editor.workspace();
    let active = workspace.active();
    let mut lines = Vec::new();
    for (index, group) in workspace.tree().groups().into_iter().enumerate() {
        if group.is_empty() {
            continue;
        }
        lines.push(format!("zone {}", index + 1));
        for &panel in group.panels() {
            let marker = if Some(panel) == active { '>' } else { ' ' };
            let shown = if group.active() == Some(panel) {
                " (affiché)"
            } else {
                ""
            };
            let kind = workspace
                .panel(panel)
                .map_or_else(String::new, |p| p.kind().to_string());
            let document = editor
                .panel_document(panel)
                .and_then(|id| editor.document(id))
                .map_or_else(String::new, |d| format!(" : {}", d.display_name()));
            lines.push(format!("{marker} {panel} {kind}{document}{shown}"));
        }
    }
    if lines.is_empty() {
        "aucun panneau".to_owned()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use crate::command::{CommandError, CommandOutput};
    use crate::editor::Editor;
    use crate::layout::{Axis, Node};

    fn root_axis(editor: &Editor) -> Option<Axis> {
        match editor.workspace().tree().root() {
            Node::Split(split) => Some(split.axis()),
            Node::Tabs(_) => None,
        }
    }

    #[test]
    fn diviser_a_droite_puis_en_bas() {
        let mut e = Editor::new();
        let doc = e.new_document();
        e.execute("split right").unwrap();
        assert_eq!(root_axis(&e), Some(Axis::Horizontal));
        assert_eq!(e.panels_showing(doc).len(), 2, "même document");
        assert_eq!(e.document_count(), 1);
        e.execute("split down").unwrap();
        assert_eq!(e.workspace().tree().groups().len(), 3);
        // Sans document : un document vide dans la nouvelle zone.
        let mut e = Editor::new();
        e.execute("split right").unwrap();
        assert_eq!(e.document_count(), 1);
        assert!(e.active_document().is_some());
    }

    #[test]
    fn fermer_le_panneau_actif() {
        let mut e = Editor::new();
        e.new_document();
        e.execute("split right").unwrap();
        e.execute("panel close").unwrap();
        assert_eq!(e.workspace().tree().groups().len(), 1);
        assert_eq!(e.document_count(), 1);
        e.execute("edit insert 'x'").unwrap();
        assert!(matches!(
            e.execute("panel close"),
            Err(CommandError::UnsavedChanges { .. })
        ));
        e.execute("panel close --force").unwrap();
        assert_eq!(e.document_count(), 0);
        assert!(matches!(
            e.execute("panel close"),
            Err(CommandError::NoActivePanel)
        ));
    }

    #[test]
    fn ouvrir_un_panneau_par_type() {
        let mut e = Editor::new();
        e.new_document();
        e.execute("panel open explorer --split left").unwrap();
        let active = e.active_panel().unwrap();
        assert_eq!(
            e.workspace().panel(active).unwrap().kind().as_str(),
            "explorer"
        );
        assert_eq!(root_axis(&e), Some(Axis::Horizontal));
        e.execute("panel open editor").unwrap();
        assert_eq!(e.workspace().tree().groups()[0].len(), 2, "en onglet");
        let error = e.execute("panel open explorr").unwrap_err().to_string();
        assert!(error.contains("« explorer »"), "{error}");
        assert!(e.execute("panel open editor --split diagonal").is_err());
        e.register_panel_kind("terminal");
        e.execute("panel open terminal --split down").unwrap();
    }

    #[test]
    fn liste_des_panneaux() {
        let mut e = Editor::new();
        assert_eq!(
            e.execute("panel list").unwrap(),
            CommandOutput::message("aucun panneau")
        );
        e.new_document();
        e.execute("split right").unwrap();
        let CommandOutput::Message(list) = e.execute("panel list").unwrap() else {
            panic!("message attendu");
        };
        assert_eq!(
            list,
            "zone 1\n  1 editor : sans titre (affiché)\nzone 2\n> 2 editor : sans titre (affiché)"
        );
    }

    #[test]
    fn naviguer_entre_les_zones() {
        // 1 | (2 / 3)
        let mut e = Editor::new();
        e.new_document();
        let one = e.active_panel().unwrap();
        e.execute("split right").unwrap();
        let two = e.active_panel().unwrap();
        e.execute("split down").unwrap();
        let three = e.active_panel().unwrap();
        e.execute("panel focus up").unwrap();
        assert_eq!(e.active_panel(), Some(two));
        e.execute("panel focus left").unwrap();
        assert_eq!(e.active_panel(), Some(one));
        let error = e.execute("panel focus left").unwrap_err();
        assert_eq!(error.to_string(), "aucune zone à gauche");
        e.execute("panel focus right").unwrap();
        assert_eq!(
            e.active_panel(),
            Some(two),
            "la plus recouverte, puis la première"
        );
        e.execute("panel focus down").unwrap();
        assert_eq!(e.active_panel(), Some(three));
        assert!(e.execute("panel focus diagonal").is_err());
    }
}
