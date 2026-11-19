//! Analyse d'une ligne de commande : `file open "mon fichier.rs"`.
//!
//! La ligne est découpée en mots en respectant les guillemets (`shlex`, règles du shell POSIX) :
//! `"..."` et `'...'` regroupent un argument contenant des espaces, et `\` échappe le caractère
//! suivant hors des guillemets simples. Sous Windows, écrire les chemins entre guillemets simples
//! (`'C:\Users\moi'`) ou avec des `/`.
//!
//! Les deux premiers mots désignent la commande (groupe puis nom), le reste ses arguments, analysés
//! par `clap` avec la définition de la commande : la même pour la ligne interne et la CLI externe.

use clap::error::{ContextKind, ContextValue, ErrorKind};

use super::{CommandError, CommandRegistry};

/// Ligne de commande découpée : commande désignée et arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLine {
    /// Nom complet de la commande (`file open`).
    pub name: String,
    /// Arguments, sans le nom de la commande.
    pub args: Vec<String>,
}

/// Découpe `line` en mots, en respectant les guillemets.
pub fn split(line: &str) -> Result<Vec<String>, CommandError> {
    shlex::split(line).ok_or_else(|| {
        CommandError::Syntax("guillemet non fermé ou « \\ » en fin de ligne".to_owned())
    })
}

/// Découpe `line` et identifie la commande dans `registry`. Renvoie `None` pour une ligne vide.
pub fn parse(registry: &CommandRegistry, line: &str) -> Result<Option<ParsedLine>, CommandError> {
    let mut words = split(line)?;
    if words.is_empty() {
        return Ok(None);
    }
    if words.len() >= 2 {
        let name = format!("{} {}", words[0], words[1]);
        if registry.get(&name).is_some() {
            let args = words.split_off(2);
            return Ok(Some(ParsedLine { name, args }));
        }
    }
    // Commande introuvable : groupe seul, ou faute de frappe.
    let typed = words.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    if words.len() == 1 && registry.has_group(&words[0]) {
        let names: Vec<String> = registry
            .list()
            .filter(|c| c.group() == words[0])
            .map(|c| c.name().to_owned())
            .collect();
        return Err(CommandError::InvalidArguments {
            command: typed,
            message: format!("commande incomplète, choisir parmi : {}", names.join(", ")),
        });
    }
    Err(CommandError::Unknown {
        suggestion: suggest(registry, &typed),
        name: typed,
    })
}

/// Commande au nom le plus proche de `typed`, si elle est assez proche pour être une faute de
/// frappe.
pub fn suggest(registry: &CommandRegistry, typed: &str) -> Option<String> {
    let max_distance = (typed.chars().count() / 3).max(2);
    registry
        .list()
        .map(|c| {
            let name = c.full_name();
            (strsim::levenshtein(typed, &name), name)
        })
        .filter(|(distance, _)| *distance <= max_distance)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name)
}

/// Traduit une erreur de `clap` en message lisible, en français.
pub fn describe_clap_error(command: &str, error: &clap::Error) -> CommandError {
    let context = |kind: ContextKind| -> String {
        match error.get(kind) {
            Some(ContextValue::String(s)) => s.clone(),
            Some(ContextValue::Strings(list)) => list.join(", "),
            Some(other) => other.to_string(),
            None => String::new(),
        }
    };
    let arg = context(ContextKind::InvalidArg);
    let value = context(ContextKind::InvalidValue);
    let message = match error.kind() {
        ErrorKind::MissingRequiredArgument => format!("argument manquant : {arg}"),
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => {
            let valid = context(ContextKind::ValidValue);
            let mut message = format!("valeur invalide pour {arg} : « {value} »");
            if !valid.is_empty() {
                message.push_str(&format!(" (valeurs possibles : {valid})"));
            }
            message
        }
        ErrorKind::UnknownArgument => format!("argument inattendu : « {arg} »"),
        ErrorKind::TooManyValues | ErrorKind::WrongNumberOfValues => {
            format!("nombre de valeurs incorrect pour {arg}")
        }
        ErrorKind::ArgumentConflict => {
            format!(
                "{arg} ne peut pas être utilisé avec {}",
                context(ContextKind::PriorArg)
            )
        }
        _ => error
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("error: ")
            .to_owned(),
    };
    CommandError::InvalidArguments {
        command: command.to_owned(),
        message: format!("{message} (voir « {command} --help »)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandDef, CommandOutput};
    use crate::editor::Editor;
    use clap::{Arg, ArgAction};

    fn editor() -> Editor {
        let mut editor = Editor::without_builtins();
        let registry = editor.commands_mut();
        registry
            .register(
                CommandDef::new("file", "open", "Ouvre un fichier", |_, m| {
                    let path: &String = m.get_one("chemin").expect("requis");
                    Ok(CommandOutput::message(path.clone()))
                })
                .arg(Arg::new("chemin").required(true).help("Fichier à ouvrir")),
            )
            .unwrap();
        registry
            .register(
                CommandDef::new("file", "close", "Ferme le document", |_, m| {
                    Ok(CommandOutput::message(format!(
                        "force={}",
                        m.get_flag("force")
                    )))
                })
                .arg(
                    Arg::new("force")
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help("Ignore les modifications"),
                ),
            )
            .unwrap();
        registry
            .register(
                CommandDef::new("window", "new", "Nouvelle fenêtre", |_, m| {
                    let n: u8 = *m.get_one("monitor").unwrap_or(&0);
                    Ok(CommandOutput::message(n.to_string()))
                })
                .arg(
                    Arg::new("monitor")
                        .long("monitor")
                        .value_name("N")
                        .value_parser(clap::value_parser!(u8))
                        .help("Écran"),
                ),
            )
            .unwrap();
        editor
    }

    fn message(result: Result<CommandOutput, CommandError>) -> String {
        match result {
            Ok(CommandOutput::Message(m)) => m,
            Ok(CommandOutput::None) => String::new(),
            Err(e) => format!("ERREUR {e}"),
        }
    }

    #[test]
    fn guillemets() {
        let mut e = editor();
        assert_eq!(
            message(e.execute(r#"file open "mon fichier.rs""#)),
            "mon fichier.rs"
        );
        assert_eq!(message(e.execute("file open 'a b/c d.txt'")), "a b/c d.txt");
        assert_eq!(message(e.execute(r"file open un\ espace")), "un espace");
        assert!(matches!(
            e.execute(r#"file open "pas fermé"#),
            Err(CommandError::Syntax(_))
        ));
    }

    #[test]
    fn ligne_vide() {
        let mut e = editor();
        assert_eq!(e.execute("   ").unwrap(), CommandOutput::None);
    }

    #[test]
    fn commande_inconnue_avec_suggestion() {
        let mut e = editor();
        let error = e.execute("file opne x").unwrap_err();
        assert_eq!(
            error.to_string(),
            "commande inconnue : « file opne » (vouliez-vous dire « file open » ?)"
        );
        let error = e.execute("zzz qqq").unwrap_err();
        assert_eq!(error.to_string(), "commande inconnue : « zzz qqq »");
    }

    #[test]
    fn groupe_seul() {
        let mut e = editor();
        let error = e.execute("file").unwrap_err().to_string();
        assert_eq!(
            error,
            "file : commande incomplète, choisir parmi : close, open"
        );
    }

    #[test]
    fn erreurs_d_arguments_lisibles() {
        let mut e = editor();
        assert_eq!(
            e.execute("file open").unwrap_err().to_string(),
            "file open : argument manquant : <chemin> (voir « file open --help »)"
        );
        assert_eq!(
            e.execute("window new --monitor deux")
                .unwrap_err()
                .to_string(),
            "window new : valeur invalide pour --monitor <N> : « deux » (voir « window new --help »)"
        );
        assert_eq!(
            e.execute("file close --vite").unwrap_err().to_string(),
            "file close : argument inattendu : « --vite » (voir « file close --help »)"
        );
        assert_eq!(message(e.execute("file close --force")), "force=true");
        assert_eq!(message(e.execute("window new --monitor 2")), "2");
    }

    #[test]
    fn aide_integree() {
        let mut e = editor();
        let help = message(e.execute("file open --help"));
        assert_eq!(
            help,
            "Ouvre un fichier\n\nUtilisation : file open <chemin>\n\nArguments :\n  <chemin>  Fichier à ouvrir"
        );
        let help = message(e.execute("window new -h"));
        assert!(
            help.contains("Utilisation : window new [--monitor <N>]"),
            "{help}"
        );
        assert!(help.contains("Options :\n  --monitor  Écran"), "{help}");
    }
}
