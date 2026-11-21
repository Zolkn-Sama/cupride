//! Notifications : informations, avertissements et erreurs à montrer à l'utilisateur.
//!
//! Le noyau garde l'historique (les [`HISTORY_LIMIT`] dernières) ; l'interface affiche les
//! nouvelles quelques secondes. `notifications list` affiche l'historique.

use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, Instant};

/// Nombre de notifications gardées dans l'historique.
pub const HISTORY_LIMIT: usize = 200;

/// Gravité d'une notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Information.
    Info,
    /// Avertissement.
    Warning,
    /// Erreur.
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "avertissement",
            Self::Error => "erreur",
        })
    }
}

/// Une notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// Identifiant, croissant : une notification plus récente a un identifiant plus grand.
    pub id: u64,
    /// Gravité.
    pub level: Level,
    /// Message.
    pub message: String,
    /// Moment où elle a été émise.
    pub at: Instant,
}

/// Historique des notifications.
#[derive(Debug, Clone, Default)]
pub struct Notifications {
    history: VecDeque<Notification>,
    next_id: u64,
}

impl Notifications {
    /// Émet une notification ; renvoie son identifiant.
    pub fn push(&mut self, level: Level, message: impl Into<String>) -> u64 {
        self.next_id += 1;
        self.history.push_back(Notification {
            id: self.next_id,
            level,
            message: message.into(),
            at: Instant::now(),
        });
        if self.history.len() > HISTORY_LIMIT {
            self.history.pop_front();
        }
        self.next_id
    }

    /// Notifications émises après l'identifiant `id` (0 : toutes celles de l'historique).
    pub fn since(&self, id: u64) -> impl Iterator<Item = &Notification> {
        self.history.iter().filter(move |n| n.id > id)
    }

    /// Historique, de la plus ancienne à la plus récente.
    pub fn history(&self) -> impl DoubleEndedIterator<Item = &Notification> {
        self.history.iter()
    }

    /// Vide l'historique (les identifiants continuent de croître).
    pub fn clear(&mut self) {
        self.history.clear();
    }
}

/// Âge lisible : « à l'instant », « il y a 12 s », « il y a 3 min », « il y a 2 h ».
pub fn age(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    match seconds {
        0..=4 => "à l'instant".to_owned(),
        5..=59 => format!("il y a {seconds} s"),
        60..=3_599 => format!("il y a {} min", seconds / 60),
        _ => format!("il y a {} h", seconds / 3_600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historique_borne_et_nouvelles() {
        let mut notifications = Notifications::default();
        let first = notifications.push(Level::Info, "un");
        let second = notifications.push(Level::Error, "deux");
        assert!(second > first);
        let new: Vec<_> = notifications
            .since(first)
            .map(|n| n.message.as_str())
            .collect();
        assert_eq!(new, ["deux"]);
        for i in 0..HISTORY_LIMIT {
            notifications.push(Level::Warning, format!("{i}"));
        }
        assert_eq!(notifications.history().count(), HISTORY_LIMIT);
        assert_eq!(notifications.history().next().unwrap().message, "0");
        notifications.clear();
        assert_eq!(notifications.history().count(), 0);
        assert!(
            notifications.push(Level::Info, "x") > second,
            "identifiants croissants"
        );
    }

    #[test]
    fn ages() {
        assert_eq!(age(Duration::from_secs(2)), "à l'instant");
        assert_eq!(age(Duration::from_secs(42)), "il y a 42 s");
        assert_eq!(age(Duration::from_secs(185)), "il y a 3 min");
        assert_eq!(age(Duration::from_secs(7_300)), "il y a 2 h");
    }
}
