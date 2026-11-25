//! Groupes `split` et `panel` : diviser l'espace, ouvrir et fermer des panneaux.

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction};

use crate::command::{CommandDef, CommandError, CommandOutput, CommandRegistry};
use crate::editor::Editor;
use crate::layout::{Axis, Direction, LayoutError, PanelKind};
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
                let metrics = editor.layout_metrics();
                let neighbor = editor
                    .workspace()
                    .tree()
                    .neighbor(panel, direction, metrics.area, metrics.gap)
                    .ok_or_else(|| {
                        CommandError::Failed(format!("aucune zone {}", direction_name(direction)))
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

    registry.register(
        CommandDef::new(
            "panel",
            "resize",
            "Agrandit ou réduit la zone active, en largeur ou en hauteur",
            |editor, args| {
                let how: &String = args.get_one("sens").expect("argument requis");
                let (axis, sign) = match how.as_str() {
                    "wider" => (Axis::Horizontal, 1.0),
                    "narrower" => (Axis::Horizontal, -1.0),
                    "taller" => (Axis::Vertical, 1.0),
                    _ => (Axis::Vertical, -1.0),
                };
                let panel = editor.active_panel().ok_or(CommandError::NoActivePanel)?;
                let metrics = editor.layout_metrics();
                let step = metrics.area.length(axis) * RESIZE_STEP;
                let changed = editor.workspace_mut().tree_mut().resize(
                    panel,
                    axis,
                    sign * step,
                    metrics.area,
                    metrics.gap,
                    metrics.min_size,
                );
                if !changed {
                    return Err(CommandError::Failed(format!(
                        "la zone ne peut pas être {}",
                        match how.as_str() {
                            "wider" => "élargie",
                            "narrower" => "rétrécie",
                            "taller" => "agrandie en hauteur",
                            _ => "réduite en hauteur",
                        }
                    )));
                }
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("sens")
                .required(true)
                .value_parser(PossibleValuesParser::new([
                    "wider", "narrower", "taller", "shorter",
                ]))
                .help("wider, narrower (largeur), taller, shorter (hauteur)"),
        ),
    )?;

    registry.register(
        CommandDef::new(
            "panel",
            "move",
            "Déplace le panneau actif dans la zone voisine, dans une nouvelle zone de ce côté, ou dans une autre fenêtre",
            |editor, args| {
                if let Some(&window) = args.get_one::<u64>("window") {
                    let panel = editor.active_panel().ok_or(CommandError::NoActivePanel)?;
                    editor.move_panel_to_window(panel, crate::layout::WindowId::new(window))?;
                    return Ok(CommandOutput::None);
                }
                let direction = direction(args, "direction").expect("argument requis");
                editor
                    .move_active_panel(direction, args.get_flag("split"))
                    .map_err(|error| match error {
                        CommandError::Layout(LayoutError::AloneInGroup(_)) => {
                            CommandError::Failed(format!(
                                "aucune zone {} : le panneau est déjà seul dans la sienne",
                                direction_name(direction)
                            ))
                        }
                        other => other,
                    })?;
                Ok(CommandOutput::None)
            },
        )
        .arg(
            Arg::new("direction")
                .required_unless_present("window")
                .value_parser(PossibleValuesParser::new(DIRECTIONS))
                .help("left, right, up ou down"),
        )
        .arg(
            Arg::new("split")
                .long("split")
                .action(ArgAction::SetTrue)
                .conflicts_with("window")
                .help("Toujours dans une nouvelle zone, de ce côté de la zone active"),
        )
        .arg(
            Arg::new("window")
                .long("window")
                .value_name("N")
                .value_parser(clap::value_parser!(u64))
                .conflicts_with("direction")
                .help("Dans la fenêtre N (voir « window list »), à côté de son panneau actif"),
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

/// « à gauche », « à droite »…
fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Left => "à gauche",
        Direction::Right => "à droite",
        Direction::Up => "au-dessus",
        Direction::Down => "en dessous",
    }
}

/// Part de la fenêtre gagnée ou perdue par `panel resize`.
const RESIZE_STEP: f32 = 0.05;

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

    #[test]
    fn redimensionner_au_clavier() {
        use crate::layout::Node;
        let weights = |e: &Editor| match e.workspace().tree().root() {
            Node::Split(split) => split
                .children()
                .iter()
                .map(|c| (c.weight() * 100.0).round())
                .collect::<Vec<_>>(),
            Node::Tabs(_) => vec![],
        };
        let mut e = Editor::new();
        e.new_document();
        assert!(e.execute("panel resize wider").is_err(), "une seule zone");
        e.execute("split right").unwrap();
        e.execute("panel resize wider").unwrap();
        assert_eq!(weights(&e), vec![45.0, 55.0]);
        e.execute("panel resize narrower").unwrap();
        e.execute("panel resize narrower").unwrap();
        assert_eq!(weights(&e), vec![55.0, 45.0]);
        assert!(
            e.execute("panel resize taller").is_err(),
            "pas de zone au-dessus"
        );
        // Taille minimale : 80 px sur 1000.
        for _ in 0..30 {
            let _ = e.execute("panel resize narrower");
        }
        assert_eq!(weights(&e), vec![92.0, 8.0]);
        let error = e.execute("panel resize narrower").unwrap_err();
        assert_eq!(error.to_string(), "la zone ne peut pas être rétrécie");
    }

    #[test]
    fn deplacer_un_panneau() {
        let groups = |e: &Editor| -> Vec<Vec<u64>> {
            e.workspace()
                .tree()
                .groups()
                .iter()
                .map(|g| g.panels().iter().map(|p| p.get()).collect())
                .collect()
        };
        let mut e = Editor::new();
        e.new_document();
        e.new_document();
        assert_eq!(groups(&e), vec![vec![1, 2]]);
        // Pas de zone à droite : une nouvelle zone.
        e.execute("panel move right").unwrap();
        assert_eq!(groups(&e), vec![vec![1], vec![2]]);
        assert_eq!(e.active_panel().unwrap().get(), 2, "garde le focus");
        // Seul dans sa zone, sans voisine en bas : rien à faire.
        let error = e.execute("panel move down").unwrap_err();
        assert_eq!(
            error.to_string(),
            "aucune zone en dessous : le panneau est déjà seul dans la sienne"
        );
        // Vers la zone de gauche : en onglet ; la zone vidée disparaît.
        e.execute("panel move left").unwrap();
        assert_eq!(groups(&e), vec![vec![1, 2]]);
        e.execute("panel move up --split").unwrap();
        assert_eq!(groups(&e), vec![vec![2], vec![1]]);
        assert!(e.workspace().is_valid());
    }

    #[test]
    fn envoyer_dans_une_autre_fenetre() {
        use crate::text::Selections;
        let mut e = Editor::new();
        let doc = e.new_document();
        e.execute("edit insert 'abc'").unwrap();
        let panel = e.active_panel().unwrap();
        e.new_window();
        e.execute("window focus 1").unwrap();
        e.active_document_mut()
            .unwrap()
            .set_selections(Selections::cursor(1));
        e.execute("panel move --window 2").unwrap();
        assert_eq!(
            e.active_window().unwrap().get(),
            2,
            "le focus suit le panneau"
        );
        assert_eq!(e.active_panel(), Some(panel), "même panneau");
        assert_eq!(e.active_id(), Some(doc));
        assert_eq!(
            e.active_document().unwrap().selections(),
            &Selections::cursor(1),
            "curseurs gardés"
        );
        assert!(e.window_panels(crate::layout::WindowId::new(1)).is_empty());
        let error = e.execute("panel move --window 7").unwrap_err().to_string();
        assert_eq!(error, "fenêtre 7 introuvable");
        assert!(
            e.execute("panel move left --window 1").is_err(),
            "l'un ou l'autre"
        );
        assert!(e.execute("panel move").is_err());
        assert!(e.workspace().is_valid());
    }
}
