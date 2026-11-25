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
}
