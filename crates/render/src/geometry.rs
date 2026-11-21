//! Géométrie en pixels physiques.

/// Rectangle en pixels physiques : coin haut gauche et taille.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    /// Bord gauche.
    pub x: f32,
    /// Bord haut.
    pub y: f32,
    /// Largeur.
    pub width: f32,
    /// Hauteur.
    pub height: f32,
}

impl Rect {
    /// Rectangle de coin haut gauche (`x`, `y`) et de taille `width` × `height`.
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

    /// Indique si le point (`x`, `y`) est dans le rectangle.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Intersection avec `other` (vide si les rectangles ne se touchent pas).
    pub fn intersect(&self, other: &Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Self::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_et_contenance() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), Rect::new(5.0, 5.0, 5.0, 5.0));
        assert_eq!(a.intersect(&Rect::new(20.0, 0.0, 1.0, 1.0)).width, 0.0);
        assert!(a.contains(0.0, 9.9));
        assert!(!a.contains(10.0, 5.0));
    }
}
