//! Écrans : leur liste, fournie par l'interface, leur numérotation, et le placement des fenêtres.
//!
//! Les écrans sont numérotés à partir de 1, **de gauche à droite, puis de haut en bas** (d'après
//! la position de leur coin haut gauche sur le bureau) : un même agencement d'écrans donne
//! toujours les mêmes numéros, quel que soit l'ordre dans lequel l'OS les énumère.

/// Un écran.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    /// Nom donné par l'OS (peut être vide).
    pub name: String,
    /// Coin haut gauche sur le bureau, en pixels physiques.
    pub x: i32,
    /// Coin haut gauche sur le bureau, en pixels physiques.
    pub y: i32,
    /// Largeur, en pixels physiques.
    pub width: u32,
    /// Hauteur, en pixels physiques.
    pub height: u32,
    /// Échelle (DPI) : 1, 1,5, 2…
    pub scale: f64,
    /// Écran principal de l'OS.
    pub primary: bool,
}

/// Position et taille d'une fenêtre, en pixels physiques.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Coin haut gauche.
    pub x: i32,
    /// Coin haut gauche.
    pub y: i32,
    /// Largeur intérieure.
    pub width: u32,
    /// Hauteur intérieure.
    pub height: u32,
}

/// Comment placer une fenêtre sur un écran demandé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementStrategy {
    /// À cette position et à cette taille.
    Position(Placement),
    /// En plein écran sur l'écran demandé (Wayland : une application ne peut pas se placer).
    Fullscreen,
    /// Là où le compositeur le décide (Wayland).
    Compositor,
}

/// Comment honorer, sous Wayland, la demande d'un écran précis (`[window] wayland_placement`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaylandPlacement {
    /// Plein écran sur l'écran demandé.
    #[default]
    Fullscreen,
    /// Placement laissé au compositeur.
    Compositor,
}

/// Stratégie de placement : la position calculée, sauf sous Wayland, où l'application ne peut
/// pas positionner ses fenêtres : plein écran ou compositeur, selon la configuration.
pub fn strategy(
    wayland: bool,
    setting: WaylandPlacement,
    placement: Placement,
) -> PlacementStrategy {
    match (wayland, setting) {
        (false, _) => PlacementStrategy::Position(placement),
        (true, WaylandPlacement::Fullscreen) => PlacementStrategy::Fullscreen,
        (true, WaylandPlacement::Compositor) => PlacementStrategy::Compositor,
    }
}

/// Part maximale d'un écran occupée par une fenêtre placée dessus.
const MAX_SHARE: f64 = 0.9;

impl MonitorInfo {
    /// L'écran contient-il le point (`x`, `y`) ?
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && i64::from(x) < i64::from(self.x) + i64::from(self.width)
            && i64::from(y) < i64::from(self.y) + i64::from(self.height)
    }

    /// Fenêtre de taille `width` × `height` centrée sur l'écran, réduite pour tenir dans 90 % de
    /// l'écran.
    pub fn center(&self, width: u32, height: u32) -> Placement {
        let width = width.min((f64::from(self.width) * MAX_SHARE) as u32).max(1);
        let height = height
            .min((f64::from(self.height) * MAX_SHARE) as u32)
            .max(1);
        Placement {
            x: self.x + ((self.width - width) / 2) as i32,
            y: self.y + ((self.height - height) / 2) as i32,
            width,
            height,
        }
    }

    /// Description pour `window list-monitors` : nom, résolution, échelle.
    pub fn describe(&self) -> String {
        let name = if self.name.trim().is_empty() {
            "écran sans nom"
        } else {
            self.name.trim()
        };
        let primary = if self.primary { ", principal" } else { "" };
        format!(
            "{name} — {}×{}, échelle {}{primary}",
            self.width,
            self.height,
            format_scale(self.scale)
        )
    }
}

/// Place d'une fenêtre restaurée : là où elle était si elle y est encore entièrement visible ;
/// sinon rapatriée sur un écran disponible (son écran d'avant s'il est toujours là, sinon
/// l'écran principal, sinon le premier), réduite pour y tenir, et entièrement visible.
pub fn rescue(saved: Placement, monitor: Option<&str>, monitors: &[MonitorInfo]) -> Placement {
    if monitors.is_empty() {
        return saved;
    }
    let by_name = |m: &&MonitorInfo| monitor.is_some_and(|name| m.name == name);
    // L'écran qui contient le coin haut gauche : la fenêtre y reste, poussée à l'intérieur.
    let holder = monitors
        .iter()
        .filter(|m| m.contains(saved.x, saved.y))
        .max_by_key(|m| by_name(m));
    if let Some(screen) = holder {
        return screen.fit(saved);
    }
    // Sinon, rapatriée et centrée.
    let target = monitors
        .iter()
        .find(by_name)
        .or_else(|| monitors.iter().find(|m| m.primary))
        .unwrap_or(&monitors[0]);
    target.center(saved.width, saved.height)
}

impl MonitorInfo {
    /// `placement` réduite et déplacée pour être entièrement sur l'écran.
    pub fn fit(&self, placement: Placement) -> Placement {
        let width = placement.width.clamp(1, self.width.max(1));
        let height = placement.height.clamp(1, self.height.max(1));
        let max_x = self.x + (self.width - width) as i32;
        let max_y = self.y + (self.height - height) as i32;
        Placement {
            x: placement.x.clamp(self.x, max_x),
            y: placement.y.clamp(self.y, max_y),
            width,
            height,
        }
    }
}

/// Échelle sans zéros inutiles : `2`, `1,5`, `1,25`.
fn format_scale(scale: f64) -> String {
    let text = format!("{:.2}", (scale * 100.0).round() / 100.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.replace('.', ",")
}

/// Trie les écrans dans l'ordre de leur numérotation : de gauche à droite, puis de haut en bas.
pub fn sort_monitors(monitors: &mut [MonitorInfo]) {
    monitors.sort_by(|a, b| (a.x, a.y, &a.name).cmp(&(b.x, b.y, &b.name)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, x: i32, y: i32, width: u32, height: u32) -> MonitorInfo {
        MonitorInfo {
            name: name.to_owned(),
            x,
            y,
            width,
            height,
            scale: 1.0,
            primary: false,
        }
    }

    #[test]
    fn numerotation_stable() {
        let mut a = vec![
            monitor("droite", 1920, 0, 1920, 1080),
            monitor("haut", -1920, -1080, 1920, 1080),
            monitor("centre", 0, 0, 1920, 1080),
            monitor("gauche", -1920, 0, 1920, 1080),
        ];
        let mut b = a.clone();
        b.reverse();
        sort_monitors(&mut a);
        sort_monitors(&mut b);
        let names = |m: &[MonitorInfo]| m.iter().map(|m| m.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&a), ["haut", "gauche", "centre", "droite"]);
        assert_eq!(names(&a), names(&b), "indépendant de l'ordre de l'OS");
    }

    #[test]
    fn centrer_une_fenetre() {
        let screen = monitor("4K", 1920, 0, 3840, 2160);
        assert_eq!(
            screen.center(1000, 800),
            Placement {
                x: 1920 + 1420,
                y: 680,
                width: 1000,
                height: 800
            }
        );
        let big = screen.center(5000, 5000);
        assert_eq!((big.width, big.height), (3456, 1944), "réduite à 90 %");
        assert!(screen.contains(big.x, big.y));
        assert!(!screen.contains(0, 0));
    }

    #[test]
    fn description() {
        let mut m = monitor("DELL U2720Q", 0, 0, 3840, 2160);
        m.scale = 1.5;
        m.primary = true;
        assert_eq!(
            m.describe(),
            "DELL U2720Q — 3840×2160, échelle 1,5, principal"
        );
        m.scale = 2.0;
        m.name = String::new();
        m.primary = false;
        assert_eq!(m.describe(), "écran sans nom — 3840×2160, échelle 2");
    }

    #[test]
    fn repli_wayland() {
        let placement = monitor("A", 0, 0, 1920, 1080).center(800, 600);
        assert_eq!(
            strategy(false, WaylandPlacement::Fullscreen, placement),
            PlacementStrategy::Position(placement)
        );
        assert_eq!(
            strategy(true, WaylandPlacement::Fullscreen, placement),
            PlacementStrategy::Fullscreen
        );
        assert_eq!(
            strategy(true, WaylandPlacement::Compositor, placement),
            PlacementStrategy::Compositor
        );
    }

    #[test]
    fn fenetre_rapatriee() {
        let main = MonitorInfo {
            primary: true,
            ..monitor("principal", 0, 0, 1920, 1080)
        };
        let side = monitor("côté", 1920, 0, 1280, 1024);
        let monitors = [main.clone(), side.clone()];
        let at = |x, y, width, height| Placement {
            x,
            y,
            width,
            height,
        };
        // Toujours là, entièrement visible : inchangée.
        let saved = at(2000, 100, 1000, 700);
        assert_eq!(rescue(saved, Some("côté"), &monitors), saved);
        // Dépasse de son écran : poussée à l'intérieur, réduite s'il le faut.
        assert_eq!(
            rescue(at(2800, 600, 1000, 1200), Some("côté"), &monitors),
            at(2200, 0, 1000, 1024)
        );
        // Écran disparu (coin sur un bureau qui n'existe plus) : centrée sur le principal.
        let lost = rescue(at(5000, 200, 1000, 700), Some("ancien"), &monitors);
        assert_eq!(lost, main.center(1000, 700));
        // Écran toujours branché mais déplacé : rejoint son écran par son nom.
        let moved = rescue(at(-1500, 0, 800, 600), Some("côté"), &monitors);
        assert_eq!(moved, side.center(800, 600));
        // Sans écrans connus : telle quelle.
        assert_eq!(rescue(saved, None, &[]), saved);
    }
}
