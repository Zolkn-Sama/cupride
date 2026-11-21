//! Latence de frappe : du moment où l'événement clavier arrive au moment où l'image qui le montre
//! est présentée.
//!
//! Chaque mesure est journalisée au niveau `debug` (`CUPRIDE_LOG=cupride=debug`) ; un résumé
//! (nombre, médiane, 99e centile, maximum) l'est au niveau `info` à la fermeture.

use std::time::Duration;

/// Mesures de latence de la session.
#[derive(Debug, Default)]
pub(crate) struct LatencyStats {
    samples: Vec<Duration>,
}

/// Résumé des mesures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) count: usize,
    pub(crate) median: Duration,
    pub(crate) p99: Duration,
    pub(crate) max: Duration,
}

impl LatencyStats {
    /// Ajoute une mesure.
    pub(crate) fn record(&mut self, latency: Duration) {
        tracing::debug!(latence_us = latency.as_micros() as u64, "latence de frappe");
        self.samples.push(latency);
    }

    /// Résumé, ou `None` sans mesure.
    pub(crate) fn summary(&self) -> Option<Summary> {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let last = sorted.len().checked_sub(1)?;
        let at = |q: f64| sorted[((last as f64) * q).round() as usize];
        Some(Summary {
            count: sorted.len(),
            median: at(0.5),
            p99: at(0.99),
            max: sorted[last],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume() {
        let mut stats = LatencyStats::default();
        assert_eq!(stats.summary(), None);
        for ms in [5, 1, 3, 2, 4] {
            stats.record(Duration::from_millis(ms));
        }
        let summary = stats.summary().unwrap();
        assert_eq!(summary.count, 5);
        assert_eq!(summary.median, Duration::from_millis(3));
        assert_eq!(summary.max, Duration::from_millis(5));
        assert_eq!(summary.p99, Duration::from_millis(5));
    }
}
