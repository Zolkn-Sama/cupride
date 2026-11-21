//! Historique des commandes tapées, parcouru avec les flèches haut et bas, et gardé entre les
//! sessions dans un fichier texte (une commande par ligne, la plus récente à la fin).

use std::io;
use std::path::Path;

/// Nombre maximal de commandes gardées.
pub const HISTORY_LIMIT: usize = 500;

/// Historique et position de parcours.
#[derive(Debug, Clone, Default)]
pub struct CommandHistory {
    entries: Vec<String>,
    /// Entrée affichée pendant le parcours (`None` : la saisie en cours).
    position: Option<usize>,
    /// Saisie en cours, retrouvée en redescendant après la dernière entrée.
    draft: String,
}

impl CommandHistory {
    /// Historique lu depuis `path` ; vide si le fichier n'existe pas.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let mut history = Self::default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            history.push(line);
        }
        Ok(history)
    }

    /// Écrit l'historique dans `path`, en créant son dossier au besoin.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = self.entries.join("\n");
        text.push('\n');
        std::fs::write(path, text)
    }

    /// Commandes, de la plus ancienne à la plus récente.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Ajoute une commande exécutée ; un doublon remonte en dernière position. Le parcours
    /// recommence depuis la fin.
    pub fn push(&mut self, command: &str) {
        let command = command.trim();
        self.position = None;
        self.draft.clear();
        if command.is_empty() {
            return;
        }
        self.entries.retain(|e| e != command);
        self.entries.push(command.to_owned());
        let excess = self.entries.len().saturating_sub(HISTORY_LIMIT);
        self.entries.drain(..excess);
    }

    /// Flèche haut : commande précédente. `current` est la saisie en cours, gardée pour y
    /// revenir. `None` au début de l'historique.
    pub fn previous(&mut self, current: &str) -> Option<&str> {
        let next = match self.position {
            None => {
                self.draft = current.to_owned();
                self.entries.len().checked_sub(1)?
            }
            Some(0) => return None,
            Some(i) => i - 1,
        };
        self.position = Some(next);
        Some(&self.entries[next])
    }

    /// Flèche bas : commande suivante, puis la saisie d'avant le parcours. `None` si le parcours
    /// n'a pas commencé.
    pub fn following(&mut self) -> Option<&str> {
        let position = self.position?;
        if position + 1 < self.entries.len() {
            self.position = Some(position + 1);
            Some(&self.entries[position + 1])
        } else {
            self.position = None;
            Some(&self.draft)
        }
    }

    /// Abandonne le parcours (ligne fermée).
    pub fn reset(&mut self) {
        self.position = None;
        self.draft.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parcours() {
        let mut history = CommandHistory::default();
        history.push("file open a.rs");
        history.push("edit undo");
        history.push("file open a.rs");
        assert_eq!(
            history.entries(),
            ["edit undo", "file open a.rs"],
            "doublon remonté"
        );

        assert_eq!(history.following(), None, "pas de parcours en cours");
        assert_eq!(history.previous("brouillon"), Some("file open a.rs"));
        assert_eq!(history.previous(""), Some("edit undo"));
        assert_eq!(history.previous(""), None, "début de l'historique");
        assert_eq!(history.following(), Some("file open a.rs"));
        assert_eq!(
            history.following(),
            Some("brouillon"),
            "retour à la saisie en cours"
        );
        assert_eq!(history.following(), None);
    }

    #[test]
    fn persistance_et_limite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("donnees").join("historique");
        assert!(CommandHistory::load(&path).unwrap().entries().is_empty());
        let mut history = CommandHistory::default();
        for i in 0..HISTORY_LIMIT + 10 {
            history.push(&format!("commande {i}"));
        }
        assert_eq!(history.entries().len(), HISTORY_LIMIT);
        history.save(&path).unwrap();
        let loaded = CommandHistory::load(&path).unwrap();
        assert_eq!(loaded.entries(), history.entries());
        assert_eq!(
            loaded.entries()[0],
            "commande 10",
            "les plus anciennes oubliées"
        );
    }
}
