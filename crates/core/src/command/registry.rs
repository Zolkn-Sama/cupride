//! Registre des commandes.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use clap::ArgMatches;

use super::{CommandError, CommandResult};
use crate::editor::Editor;

/// Fonction qui exécute une commande, avec un accès en écriture à l'éditeur et ses arguments
/// déjà analysés.
pub type Handler = Arc<dyn Fn(&mut Editor, &ArgMatches) -> CommandResult + Send + Sync>;

/// Définition d'une commande : groupe, nom, description, arguments et exécution.
#[derive(Clone)]
pub struct CommandDef {
    group: String,
    name: String,
    description: String,
    args: clap::Command,
    handler: Handler,
}

impl fmt::Debug for CommandDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandDef")
            .field("name", &self.full_name())
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

impl CommandDef {
    /// Commande `group name`, sans argument. Ajouter les arguments avec [`CommandDef::arg`].
    pub fn new(
        group: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        handler: impl Fn(&mut Editor, &ArgMatches) -> CommandResult + Send + Sync + 'static,
    ) -> Self {
        let group = group.into();
        let name = name.into();
        let description = description.into();
        let args = clap::Command::new(name.clone())
            .bin_name(format!("{group} {name}"))
            .about(description.clone())
            .no_binary_name(true)
            // L'aide est rendue par `help_text`, en français ; `--help` est traité avant clap.
            .disable_help_flag(true);
        Self {
            group,
            name,
            description,
            args,
            handler: Arc::new(handler),
        }
    }

    /// Ajoute un argument, décrit avec `clap`.
    pub fn arg(mut self, arg: clap::Arg) -> Self {
        self.args = self.args.arg(arg);
        self
    }

    /// Groupe de la commande (`file`, `edit`…).
    pub fn group(&self) -> &str {
        &self.group
    }

    /// Nom de la commande dans son groupe (`open`, `save`…).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Nom complet, tel qu'on le tape : `file open`.
    pub fn full_name(&self) -> String {
        format!("{} {}", self.group, self.name)
    }

    /// Description en une ligne.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Définition `clap` des arguments.
    pub fn args(&self) -> &clap::Command {
        &self.args
    }

    /// Analyse les arguments (sans le nom de la commande).
    pub fn parse(&self, args: &[String]) -> Result<ArgMatches, clap::Error> {
        self.args.clone().try_get_matches_from(args)
    }

    /// Ligne d'utilisation : `file open <chemin> [--force]`.
    pub fn usage(&self) -> String {
        let mut usage = self.full_name();
        for arg in self.args.get_arguments() {
            let value = arg
                .get_value_names()
                .and_then(|names| names.first())
                .map_or_else(|| arg.get_id().to_string(), ToString::to_string);
            let part = match arg.get_long() {
                Some(long) if arg.get_action().takes_values() => format!("--{long} <{value}>"),
                Some(long) => format!("--{long}"),
                None => format!("<{value}>"),
            };
            if arg.is_required_set() {
                usage.push_str(&format!(" {part}"));
            } else {
                usage.push_str(&format!(" [{part}]"));
            }
        }
        usage
    }

    /// Aide complète de la commande, affichée par `<commande> --help`.
    pub fn help_text(&self) -> String {
        let mut help = format!(
            "{}

Utilisation : {}",
            self.description,
            self.usage()
        );
        let describe =
            |arg: &clap::Arg| arg.get_help().map(ToString::to_string).unwrap_or_default();
        let positionals: Vec<_> = self.args.get_positionals().collect();
        if !positionals.is_empty() {
            help.push_str("\n\nArguments :");
            for arg in positionals {
                help.push_str(&format!("\n  <{}>  {}", arg.get_id(), describe(arg)));
            }
        }
        let options: Vec<_> = self
            .args
            .get_arguments()
            .filter(|a| !a.is_positional())
            .collect();
        if !options.is_empty() {
            help.push_str("\n\nOptions :");
            for arg in options {
                let long = arg.get_long().unwrap_or_default();
                let short = arg
                    .get_short()
                    .map(|c| format!("-{c}, "))
                    .unwrap_or_default();
                help.push_str(&format!("\n  {short}--{long}  {}", describe(arg)));
            }
        }
        help
    }

    pub(crate) fn handler(&self) -> Handler {
        Arc::clone(&self.handler)
    }
}

/// Registre unique des commandes, indexé par nom complet (`file open`).
#[derive(Default)]
pub struct CommandRegistry {
    commands: BTreeMap<String, Arc<CommandDef>>,
}

impl fmt::Debug for CommandRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.commands.keys()).finish()
    }
}

impl CommandRegistry {
    /// Registre vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enregistre une commande. Un nom déjà enregistré est une erreur.
    pub fn register(&mut self, command: CommandDef) -> Result<(), CommandError> {
        let name = command.full_name();
        if self.commands.contains_key(&name) {
            return Err(CommandError::AlreadyRegistered { name });
        }
        self.commands.insert(name, Arc::new(command));
        Ok(())
    }

    /// Retire une commande (par exemple quand un plugin est désactivé).
    pub fn unregister(&mut self, full_name: &str) -> Option<Arc<CommandDef>> {
        self.commands.remove(full_name)
    }

    /// Commande de nom complet `full_name`.
    pub fn get(&self, full_name: &str) -> Option<Arc<CommandDef>> {
        self.commands.get(full_name).cloned()
    }

    /// Indique si `group` est un groupe de commandes.
    pub fn has_group(&self, group: &str) -> bool {
        self.commands.values().any(|c| c.group() == group)
    }

    /// Toutes les commandes, triées par nom complet.
    pub fn list(&self) -> impl Iterator<Item = &CommandDef> {
        self.commands.values().map(Arc::as_ref)
    }

    /// Nombre de commandes.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Indique si le registre est vide.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Commandes correspondant à `query` (sans tenir compte de la casse), pour la complétion :
    /// d'abord celles dont le nom commence par `query`, puis celles dont le nom le contient, puis
    /// celles dont la description le contient. La recherche floue de la palette viendra avec elle.
    pub fn search(&self, query: &str) -> Vec<&CommandDef> {
        let query = query.trim().to_lowercase();
        let mut scored: Vec<(u8, &CommandDef)> = self
            .list()
            .filter_map(|command| {
                let name = command.full_name().to_lowercase();
                let score = if name.starts_with(&query) {
                    0
                } else if name.contains(&query) {
                    1
                } else if command.description().to_lowercase().contains(&query) {
                    2
                } else {
                    return None;
                };
                Some((score, command))
            })
            .collect();
        scored.sort_by_key(|&(score, _)| score);
        scored.into_iter().map(|(_, command)| command).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandOutput;
    use clap::Arg;

    fn noop(group: &str, name: &str, description: &str) -> CommandDef {
        CommandDef::new(group, name, description, |_, _| Ok(CommandOutput::None))
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn enregistrer_et_executer() {
        let mut editor = Editor::new();
        editor
            .commands_mut()
            .register(
                CommandDef::new("test", "count", "Crée N documents", |editor, matches| {
                    let n: usize = *matches.get_one("n").expect("argument requis");
                    for _ in 0..n {
                        editor.new_document();
                    }
                    Ok(CommandOutput::message(format!("{n} documents créés")))
                })
                .arg(
                    Arg::new("n")
                        .required(true)
                        .value_parser(clap::value_parser!(usize)),
                ),
            )
            .unwrap();
        let output = editor.run("test count", &args(&["3"])).unwrap();
        assert_eq!(output, CommandOutput::message("3 documents créés"));
        assert_eq!(editor.document_count(), 3);
    }

    #[test]
    fn nom_deja_enregistre() {
        let mut registry = CommandRegistry::new();
        registry.register(noop("file", "open", "Ouvre")).unwrap();
        let error = registry
            .register(noop("file", "open", "Autre"))
            .unwrap_err();
        assert!(
            matches!(error, CommandError::AlreadyRegistered { ref name } if name == "file open")
        );
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn commande_inconnue_et_arguments_invalides() {
        let mut editor = Editor::new();
        assert!(matches!(
            editor.run("file nope", &[]),
            Err(CommandError::Unknown { .. })
        ));
        editor
            .commands_mut()
            .register(noop("a", "b", "").arg(Arg::new("x").required(true)))
            .unwrap();
        assert!(matches!(
            editor.run("a b", &[]),
            Err(CommandError::InvalidArguments { .. })
        ));
    }

    #[test]
    fn liste_triee_et_recherche() {
        let mut registry = CommandRegistry::new();
        registry
            .register(noop("file", "save", "Enregistre le document"))
            .unwrap();
        registry
            .register(noop("edit", "undo", "Annule la dernière modification"))
            .unwrap();
        registry
            .register(noop("file", "open", "Ouvre un fichier"))
            .unwrap();
        let names: Vec<String> = registry.list().map(CommandDef::full_name).collect();
        assert_eq!(names, ["edit undo", "file open", "file save"]);

        let found: Vec<String> = registry
            .search("file")
            .iter()
            .map(|c| c.full_name())
            .collect();
        assert_eq!(found, ["file open", "file save"]);
        let found: Vec<String> = registry
            .search("SAVE")
            .iter()
            .map(|c| c.full_name())
            .collect();
        assert_eq!(found, ["file save"]);
        let found: Vec<String> = registry
            .search("annule")
            .iter()
            .map(|c| c.full_name())
            .collect();
        assert_eq!(found, ["edit undo"]);
        assert!(registry.has_group("edit"));
        assert!(!registry.has_group("window"));
        assert!(registry.unregister("edit undo").is_some());
        assert!(registry.get("edit undo").is_none());
    }
}
