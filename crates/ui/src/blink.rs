//! Clignotement du curseur.
//!
//! Le curseur clignote après chaque saisie, puis s'arrête, visible, au bout de
//! [`BLINK_IDLE_STOP`] sans activité : au repos, plus aucun réveil n'est programmé et l'IDE ne
//! consomme plus de CPU.

use std::time::{Duration, Instant};

/// Durée sans saisie après laquelle le curseur cesse de clignoter.
pub const BLINK_IDLE_STOP: Duration = Duration::from_secs(10);

/// État du clignotement.
#[derive(Debug, Clone)]
pub struct CursorBlink {
    enabled: bool,
    interval: Duration,
    /// Dernière activité : le curseur est visible à cet instant, puis alterne.
    since: Instant,
}

impl CursorBlink {
    /// Clignotement de demi-période `interval`, ou curseur fixe si `enabled` est faux.
    pub fn new(enabled: bool, interval: Duration, now: Instant) -> Self {
        Self {
            enabled,
            interval: interval.max(Duration::from_millis(1)),
            since: now,
        }
    }

    /// Change les réglages.
    pub fn configure(&mut self, enabled: bool, interval: Duration) {
        self.enabled = enabled;
        self.interval = interval.max(Duration::from_millis(1));
    }

    /// Activité (frappe, clic) : curseur visible, et le clignotement reprend.
    pub fn reset(&mut self, now: Instant) {
        self.since = now;
    }

    /// Le clignotement est-il terminé (ou désactivé) ?
    fn stopped(&self, now: Instant) -> bool {
        !self.enabled || now.saturating_duration_since(self.since) >= BLINK_IDLE_STOP
    }

    /// Le curseur est-il visible à l'instant `now` ?
    pub fn visible(&self, now: Instant) -> bool {
        if self.stopped(now) {
            return true;
        }
        let elapsed = now.saturating_duration_since(self.since);
        (elapsed.as_nanos() / self.interval.as_nanos()).is_multiple_of(2)
    }

    /// Prochain changement d'état à partir de `now`, ou `None` si le curseur ne changera plus
    /// sans nouvelle activité. Quand le clignotement s'arrête sur une phase éteinte, le dernier
    /// réveil est celui qui le rallume.
    pub fn next_change(&self, now: Instant) -> Option<Instant> {
        if self.stopped(now) {
            return None;
        }
        let elapsed = now.saturating_duration_since(self.since);
        let phases = elapsed.as_nanos() / self.interval.as_nanos() + 1;
        let next = self.since + self.interval * u32::try_from(phases).unwrap_or(u32::MAX);
        Some(next.min(self.since + BLINK_IDLE_STOP))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn alterne_puis_s_arrete_visible() {
        let t0 = Instant::now();
        let blink = CursorBlink::new(true, 500 * MS, t0);
        assert!(blink.visible(t0));
        assert!(!blink.visible(t0 + 600 * MS));
        assert!(blink.visible(t0 + 1_100 * MS));
        assert_eq!(blink.next_change(t0 + 100 * MS), Some(t0 + 500 * MS));
        assert_eq!(blink.next_change(t0 + 500 * MS), Some(t0 + 1_000 * MS));
        // Juste avant l'arrêt, le prochain réveil est l'arrêt lui-même, curseur rallumé.
        assert_eq!(
            blink.next_change(t0 + BLINK_IDLE_STOP - MS),
            Some(t0 + BLINK_IDLE_STOP)
        );
        assert!(blink.visible(t0 + BLINK_IDLE_STOP + 250 * MS));
        assert_eq!(blink.next_change(t0 + BLINK_IDLE_STOP), None);
    }

    #[test]
    fn activite_relance_le_clignotement() {
        let t0 = Instant::now();
        let mut blink = CursorBlink::new(true, 500 * MS, t0);
        let later = t0 + Duration::from_secs(60);
        assert_eq!(blink.next_change(later), None);
        blink.reset(later);
        assert_eq!(blink.next_change(later), Some(later + 500 * MS));
    }

    #[test]
    fn desactive_toujours_visible_sans_reveil() {
        let t0 = Instant::now();
        let blink = CursorBlink::new(false, 500 * MS, t0);
        assert!(blink.visible(t0 + 600 * MS));
        assert_eq!(blink.next_change(t0), None);
    }
}
