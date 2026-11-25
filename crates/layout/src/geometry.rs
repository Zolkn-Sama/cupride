//! Géométrie du layout : rectangles, axes et directions.
//!
//! Le layout calcule lui-même la place de chaque zone, pour que la navigation, le redimensionnement
//! et le glisser-déposer se testent sans interface. Les unités sont celles de l'appelant (pixels
//! physiques dans l'application).

use serde::{Deserialize, Serialize};

/// Rectangle : coin haut gauche et dimensions.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bounds {
    /// Bord gauche.
    pub x: f32,
    /// Bord haut.
    pub y: f32,
    /// Largeur.
    pub width: f32,
    /// Hauteur.
    pub height: f32,
}

impl Bounds {
    /// Rectangle de coin (`x`, `y`) et de taille `width` × `height`.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Bord droit.
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Bord bas.
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// Le point (`x`, `y`) est-il dans le rectangle ?
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Début du rectangle le long de `axis`.
    pub fn start(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.x,
            Axis::Vertical => self.y,
        }
    }

    /// Longueur du rectangle le long de `axis`.
    pub fn length(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.width,
            Axis::Vertical => self.height,
        }
    }

    /// Fin du rectangle le long de `axis`.
    pub fn end(&self, axis: Axis) -> f32 {
        self.start(axis) + self.length(axis)
    }

    /// Même rectangle, de `start` à `start + length` le long de `axis`.
    pub(crate) fn with_span(self, axis: Axis, start: f32, length: f32) -> Self {
        match axis {
            Axis::Horizontal => Self::new(start, self.y, length, self.height),
            Axis::Vertical => Self::new(self.x, start, self.width, length),
        }
    }

    /// Centre du rectangle.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// Sens d'un split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Enfants côte à côte, de gauche à droite.
    Horizontal,
    /// Enfants empilés, de haut en bas.
    Vertical,
}

impl Axis {
    /// L'autre axe.
    pub fn cross(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

/// Direction, pour diviser, déplacer ou naviguer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Vers la gauche.
    Left,
    /// Vers la droite.
    Right,
    /// Vers le haut.
    Up,
    /// Vers le bas.
    Down,
}

impl Direction {
    /// Toutes les directions.
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Up, Self::Down];

    /// Axe le long duquel on se déplace.
    pub fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Up | Self::Down => Axis::Vertical,
        }
    }

    /// La direction va-t-elle vers la fin de l'axe (droite, bas) ?
    pub fn is_forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }

    /// Nom, tel qu'on le tape dans une commande.
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    /// Direction à partir de son nom (`left`, `right`, `up`, `down`).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name)
    }
}

/// Découpe la longueur `total` en parts de poids `weights`, séparées par `gap`. Les bords sont
/// arrondis à l'unité, sans trou ni chevauchement : renvoie le début et la longueur de chaque
/// part, depuis 0.
pub(crate) fn spans(total: f32, gap: f32, weights: &[f32]) -> Vec<(f32, f32)> {
    let count = weights.len();
    if count == 0 {
        return Vec::new();
    }
    let gaps = gap.max(0.0) * (count - 1) as f32;
    let free = (total - gaps).max(0.0);
    let sum: f32 = weights.iter().sum();
    let mut cursor = 0.0_f32;
    let mut spans = Vec::with_capacity(count);
    for (index, weight) in weights.iter().enumerate() {
        let wanted = if sum > 0.0 {
            free * weight / sum
        } else {
            free / count as f32
        };
        let start = cursor.round();
        let end = if index + 1 == count {
            total
        } else {
            (cursor + wanted).round()
        };
        spans.push((start, (end - start).max(0.0)));
        cursor += wanted + gap.max(0.0);
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_sans_trou_ni_chevauchement() {
        let parts = spans(101.0, 1.0, &[0.5, 0.5]);
        assert_eq!(parts[0], (0.0, 50.0));
        assert_eq!(parts[1], (51.0, 50.0));
        let parts = spans(100.0, 0.0, &[1.0, 1.0, 1.0]);
        let mut end = 0.0;
        for (start, length) in &parts {
            assert_eq!(*start, end);
            end = start + length;
        }
        assert_eq!(end, 100.0);
        assert!(
            spans(10.0, 20.0, &[0.5, 0.5])
                .iter()
                .all(|(_, l)| *l >= 0.0)
        );
    }

    #[test]
    fn directions() {
        assert_eq!(Direction::parse("up"), Some(Direction::Up));
        assert_eq!(Direction::parse("haut"), None);
        assert_eq!(Direction::Left.axis(), Axis::Horizontal);
        assert!(Direction::Down.is_forward());
        assert!(!Direction::Left.is_forward());
        assert_eq!(Axis::Horizontal.cross(), Axis::Vertical);
    }
}
