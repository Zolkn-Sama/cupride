//! Fermeture de la fenêtre avec confirmation quand des documents sont modifiés.
//!
//! Sans boîte de dialogue native, la confirmation se fait en deux temps : une première demande de
//! fermeture prévient (dans le titre de la fenêtre) ; une seconde demande dans les secondes qui
//! suivent ferme sans enregistrer.

use std::time::{Duration, Instant};

/// Délai pendant lequel une seconde demande de fermeture confirme la première.
pub(crate) const CONFIRM_WINDOW: Duration = Duration::from_secs(5);

/// Décision à la suite d'une demande de fermeture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CloseDecision {
    /// Fermer.
    Close,
    /// Prévenir l'utilisateur avec ce message, et attendre une confirmation.
    Warn(String),
}

/// Suit les demandes de fermeture successives.
#[derive(Debug, Default)]
pub(crate) struct CloseGuard {
    warned_at: Option<Instant>,
}

impl CloseGuard {
    /// Traite une demande de fermeture à l'instant `now`, avec la liste des documents modifiés.
    pub(crate) fn request(&mut self, modified: &[String], now: Instant) -> CloseDecision {
        if modified.is_empty() {
            return CloseDecision::Close;
        }
        if let Some(at) = self.warned_at
            && now.saturating_duration_since(at) <= CONFIRM_WINDOW
        {
            return CloseDecision::Close;
        }
        self.warned_at = Some(now);
        let names = match modified {
            [one] => format!("« {one} » a des modifications non enregistrées"),
            many => format!(
                "{} documents ont des modifications non enregistrées",
                many.len()
            ),
        };
        CloseDecision::Warn(format!(
            "{names} : fermer à nouveau pour quitter sans enregistrer"
        ))
    }

    /// Indique si un avertissement est en cours, pour le retirer quand il a expiré.
    pub(crate) fn expired(&mut self, now: Instant) -> bool {
        match self.warned_at {
            Some(at) if now.saturating_duration_since(at) > CONFIRM_WINDOW => {
                self.warned_at = None;
                true
            }
            _ => false,
        }
    }

    /// Instant où l'avertissement en cours expire.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.warned_at.map(|at| at + CONFIRM_WINDOW)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sans_modification_fermeture_directe() {
        let mut guard = CloseGuard::default();
        assert_eq!(guard.request(&[], Instant::now()), CloseDecision::Close);
    }

    #[test]
    fn confirmation_par_une_seconde_demande() {
        let t0 = Instant::now();
        let mut guard = CloseGuard::default();
        let modified = vec!["main.rs".to_owned()];
        assert_eq!(
            guard.request(&modified, t0),
            CloseDecision::Warn(
                "« main.rs » a des modifications non enregistrées : fermer à nouveau pour quitter sans enregistrer".into()
            )
        );
        assert_eq!(
            guard.request(&modified, t0 + Duration::from_secs(2)),
            CloseDecision::Close
        );
    }

    #[test]
    fn avertissement_expire() {
        let t0 = Instant::now();
        let mut guard = CloseGuard::default();
        let modified = vec!["a".to_owned(), "b".to_owned()];
        assert!(
            matches!(guard.request(&modified, t0), CloseDecision::Warn(m) if m.starts_with("2 documents"))
        );
        assert_eq!(guard.deadline(), Some(t0 + CONFIRM_WINDOW));
        assert!(!guard.expired(t0 + Duration::from_secs(1)));
        assert!(guard.expired(t0 + Duration::from_secs(6)));
        assert!(matches!(
            guard.request(&modified, t0 + Duration::from_secs(6)),
            CloseDecision::Warn(_)
        ));
    }
}
