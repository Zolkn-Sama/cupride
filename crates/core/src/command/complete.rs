//! Complétion de la ligne de commande et aperçu de l'aide.
//!
//! La complétion porte sur le mot sous le curseur, d'après sa place dans la ligne :
//!
//! - premier mot : groupes de commandes (`file`, `edit`…) ;
//! - deuxième mot : commandes du groupe ;
//! - ensuite : options (`--force`), valeurs possibles d'un argument (`edit move <direction>`),
//!   ou chemins pour un argument qui en attend un (`file open <chemin>`), relatifs au dossier
//!   courant ; un dossier se termine par `/`.

use std::path::Path;

use clap::ValueHint;

use super::registry::CommandRegistry;

/// Propositions pour le mot sous le curseur.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completion {
    /// Début du mot à remplacer, en octets.
    pub start: usize,
    /// Propositions, triées ; chacune remplace le mot entier.
    pub candidates: Vec<String>,
}

impl Completion {
    /// Plus long début commun à toutes les propositions.
    pub fn common_prefix(&self) -> String {
        let Some(first) = self.candidates.first() else {
            return String::new();
        };
        let mut end = first.len();
        for candidate in &self.candidates[1..] {
            end = first
                .char_indices()
                .zip(candidate.chars())
                .find(|((_, a), b)| a != b)
                .map_or(end.min(candidate.len()), |((i, _), _)| i.min(end));
        }
        first[..end].to_owned()
    }
}

/// Mots de `line` jusqu'à la position `cursor` : (début, texte) de chacun, en séparant par les
/// espaces. Le dernier est le mot en cours (vide après un espace).
fn words(line: &str, cursor: usize) -> Vec<(usize, &str)> {
    let before = &line[..cursor.min(line.len())];
    let mut words: Vec<(usize, &str)> = Vec::new();
    let mut start = None;
    for (i, c) in before.char_indices() {
        match (c.is_whitespace(), start) {
            (true, Some(s)) => {
                words.push((s, &before[s..i]));
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    match start {
        Some(s) => words.push((s, &before[s..])),
        None => words.push((before.len(), "")),
    }
    words
}

/// Propositions pour le mot sous le curseur de `line`.
pub fn complete(registry: &CommandRegistry, line: &str, cursor: usize) -> Completion {
    let words = words(line, cursor);
    let (start, current) = words[words.len() - 1];
    let filter = |items: Vec<String>| -> Vec<String> {
        let mut items: Vec<String> = items
            .into_iter()
            .filter(|item| item.starts_with(current))
            .collect();
        items.sort();
        items.dedup();
        items
    };
    let candidates = match words.len() {
        1 => filter(registry.list().map(|c| c.group().to_owned()).collect()),
        2 => {
            let group = words[0].1;
            filter(
                registry
                    .list()
                    .filter(|c| c.group() == group)
                    .map(|c| c.name().to_owned())
                    .collect(),
            )
        }
        _ => {
            let name = format!("{} {}", words[0].1, words[1].1);
            match registry.get(&name) {
                Some(command) => argument_candidates(command.args(), &words[2..], current),
                None => Vec::new(),
            }
        }
    };
    Completion { start, candidates }
}

/// Propositions pour un argument : options, valeurs possibles ou chemins.
fn argument_candidates(
    args: &clap::Command,
    words: &[(usize, &str)],
    current: &str,
) -> Vec<String> {
    if current.starts_with('-') {
        let mut options: Vec<String> = args
            .get_arguments()
            .filter_map(|a| a.get_long().map(|long| format!("--{long}")))
            .filter(|o| o.starts_with(current))
            .collect();
        options.sort();
        return options;
    }
    // Rang de l'argument positionnel en cours : mots précédents qui ne sont pas des options.
    let index = words[..words.len() - 1]
        .iter()
        .filter(|(_, w)| !w.starts_with('-'))
        .count();
    let Some(arg) = args.get_positionals().nth(index) else {
        return Vec::new();
    };
    let values: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_owned())
        .filter(|v| v.starts_with(current))
        .collect();
    if !values.is_empty() {
        return values;
    }
    match arg.get_value_hint() {
        ValueHint::AnyPath | ValueHint::FilePath | ValueHint::DirPath => paths(current),
        _ => Vec::new(),
    }
}

/// Chemins qui commencent par `typed`, relatifs au dossier courant ; dossiers terminés par `/`.
/// Les fichiers cachés n'apparaissent que si le nom tapé commence par un point.
fn paths(typed: &str) -> Vec<String> {
    let (dir, prefix) = match typed.rfind(['/', std::path::MAIN_SEPARATOR]) {
        Some(i) => (&typed[..=i], &typed[i + 1..]),
        None => ("", typed),
    };
    let read_from = if dir.is_empty() {
        Path::new(".")
    } else {
        Path::new(dir)
    };
    let Ok(entries) = std::fs::read_dir(read_from) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !name.starts_with(prefix) || (name.starts_with('.') && !prefix.starts_with('.')) {
                return None;
            }
            let slash = if entry.path().is_dir() { "/" } else { "" };
            Some(format!("{dir}{name}{slash}"))
        })
        .collect();
    found.sort();
    found
}

/// Aide de la commande en cours de saisie : utilisation et description, dès que les deux
/// premiers mots désignent une commande.
pub fn help_hint(registry: &CommandRegistry, line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    let name = format!("{} {}", words.next()?, words.next()?);
    let command = registry.get(&name)?;
    Some(format!("{} — {}", command.usage(), command.description()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Editor;

    fn candidates(line: &str) -> Vec<String> {
        let editor = Editor::new();
        complete(editor.commands(), line, line.len()).candidates
    }

    #[test]
    fn groupes_et_commandes() {
        assert_eq!(candidates("fi"), ["file"]);
        assert!(candidates("").contains(&"edit".to_owned()));
        let file = candidates("file s");
        assert_eq!(file, ["save", "save-as"]);
        let completion = complete(Editor::new().commands(), "file s", 6);
        assert_eq!(completion.start, 5);
        assert_eq!(completion.common_prefix(), "save");
        assert!(candidates("inconnu x").is_empty());
    }

    #[test]
    fn valeurs_options_et_chemins() {
        assert_eq!(candidates("edit move wo"), ["word-left", "word-right"]);
        assert_eq!(candidates("edit move left --"), ["--extend"]);
        assert_eq!(candidates("file close --f"), ["--force"]);
        // Chemins relatifs au dossier courant : celui du test est la crate.
        assert_eq!(candidates("file open Carg"), ["Cargo.toml"]);
        assert_eq!(candidates("file open sr"), ["src/"]);
        assert!(candidates("file open src/com").contains(&"src/command/".to_owned()));
    }

    #[test]
    fn mot_au_milieu_de_la_ligne() {
        let editor = Editor::new();
        let completion = complete(editor.commands(), "edit mo left", 7);
        assert_eq!(completion.start, 5);
        assert_eq!(completion.candidates, ["move"]);
    }

    #[test]
    fn aide_de_la_commande_en_cours() {
        let editor = Editor::new();
        let hint = help_hint(editor.commands(), "file open src").unwrap();
        assert!(hint.starts_with("file open <chemin> —"), "{hint}");
        assert_eq!(help_hint(editor.commands(), "file"), None);
    }

    #[test]
    fn prefixe_commun() {
        let c = |items: &[&str]| Completion {
            start: 0,
            candidates: items.iter().map(|s| (*s).to_owned()).collect(),
        };
        assert_eq!(c(&["save", "save-as"]).common_prefix(), "save");
        assert_eq!(c(&["été", "étage"]).common_prefix(), "ét");
        assert_eq!(c(&[]).common_prefix(), "");
    }
}
