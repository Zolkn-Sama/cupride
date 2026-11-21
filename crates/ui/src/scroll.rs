//! Position de défilement, avec animation courte pour la molette.
//!
//! Le pavé tactile envoie des déplacements au pixel, déjà fluides : ils sont appliqués tels
//! quels. La molette avance par crans de plusieurs lignes : chaque cran est animé sur
//! [`SCROLL_DURATION`], avec un départ rapide et une fin douce, pour que l'œil suive le texte.

use std::time::{Duration, Instant};

/// Durée de l'animation d'un cran de molette.
pub(crate) const SCROLL_DURATION: Duration = Duration::from_millis(120);

/// Position de défilement (x, y), en pixels physiques.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scroll {
    position: [f32; 2],
    target: [f32; 2],
    /// Animation en cours : instant de départ et position de départ.
    animation: Option<(Instant, [f32; 2])>,
}

/// Courbe d'animation : départ rapide, arrivée douce.
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

impl Scroll {
    /// Position actuelle.
    pub(crate) fn position(&self) -> [f32; 2] {
        self.position
    }

    /// Position visée (fin de l'animation en cours, ou position actuelle).
    pub(crate) fn target(&self) -> [f32; 2] {
        self.target
    }

    /// Va immédiatement à `position`, en abandonnant toute animation.
    pub(crate) fn jump_to(&mut self, position: [f32; 2]) {
        self.position = position;
        self.target = position;
        self.animation = None;
    }

    /// Anime de la position actuelle jusqu'à `target`. Un nouveau cran pendant une animation
    /// repart de la position atteinte, vers la nouvelle cible : le mouvement reste continu.
    pub(crate) fn animate_to(&mut self, target: [f32; 2], now: Instant) {
        if target == self.position {
            self.jump_to(target);
            return;
        }
        self.target = target;
        self.animation = Some((now, self.position));
    }

    /// Avance l'animation à l'instant `now`. Renvoie vrai si elle n'est pas terminée.
    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let Some((start, from)) = self.animation else {
            return false;
        };
        let elapsed = now.saturating_duration_since(start);
        if elapsed >= SCROLL_DURATION {
            self.jump_to(self.target);
            return false;
        }
        let t = ease_out(elapsed.as_secs_f32() / SCROLL_DURATION.as_secs_f32());
        self.position =
            std::array::from_fn(|axis| from[axis] + (self.target[axis] - from[axis]) * t);
        true
    }

    /// L'animation est-elle en cours ?
    pub(crate) fn is_animating(&self) -> bool {
        self.animation.is_some()
    }

    /// Ramène la position et la cible dans les limites `[0, max]`.
    pub(crate) fn clamp(&mut self, max: [f32; 2]) {
        let limit = max.map(|m| m.max(0.0));
        self.position = std::array::from_fn(|axis| self.position[axis].clamp(0.0, limit[axis]));
        self.target = std::array::from_fn(|axis| self.target[axis].clamp(0.0, limit[axis]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_jusqu_a_la_cible() {
        let t0 = Instant::now();
        let mut scroll = Scroll::default();
        scroll.animate_to([0.0, 300.0], t0);
        assert_eq!(scroll.position(), [0.0, 0.0]);
        assert!(scroll.tick(t0 + SCROLL_DURATION / 2));
        let [_, y] = scroll.position();
        assert!(y > 150.0 && y < 300.0, "départ rapide : {y}");
        assert!(!scroll.tick(t0 + SCROLL_DURATION));
        assert_eq!(scroll.position(), [0.0, 300.0]);
        assert!(!scroll.is_animating());
    }

    #[test]
    fn nouveau_cran_repart_de_la_position_atteinte() {
        let t0 = Instant::now();
        let mut scroll = Scroll::default();
        scroll.animate_to([0.0, 100.0], t0);
        let mid = t0 + SCROLL_DURATION / 2;
        scroll.tick(mid);
        let reached = scroll.position()[1];
        scroll.animate_to([0.0, 200.0], mid);
        assert_eq!(scroll.position()[1], reached, "pas de saut");
        scroll.tick(mid + SCROLL_DURATION);
        assert_eq!(scroll.position()[1], 200.0);
    }

    #[test]
    fn limites() {
        let mut scroll = Scroll::default();
        scroll.jump_to([-5.0, 900.0]);
        scroll.clamp([0.0, 500.0]);
        assert_eq!(scroll.position(), [0.0, 500.0]);
        assert_eq!(scroll.target(), [0.0, 500.0]);
    }
}
