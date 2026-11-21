//! Découpage de l'espace : un rectangle partagé en lignes ou en colonnes.
//!
//! Chaque part a une taille : fixe (en pixels physiques), proportionnelle, ou le reste. Les tailles
//! fixes sont servies d'abord, puis les proportions se partagent ce qui reste ; s'il manque de la
//! place, les dernières parts sont réduites, jamais négatives.

use cupride_render::Rect;

/// Taille d'une part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    /// Taille fixe, en pixels physiques.
    Fixed(f32),
    /// Part proportionnelle de l'espace laissé par les tailles fixes : `Fraction(1.0)` et
    /// `Fraction(2.0)` donnent un tiers et deux tiers.
    Fraction(f32),
}

impl Size {
    /// Tout l'espace restant (une part proportionnelle de poids 1).
    pub const FILL: Self = Self::Fraction(1.0);
}

/// Sens du découpage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Parts empilées de haut en bas.
    Vertical,
    /// Parts côte à côte, de gauche à droite.
    Horizontal,
}

/// Découpe `rect` en parts de tailles `sizes`, dans le sens `axis`. Les bords sont arrondis au
/// pixel, sans trou ni chevauchement entre les parts.
pub fn split(rect: Rect, axis: Axis, sizes: &[Size]) -> Vec<Rect> {
    let total = match axis {
        Axis::Vertical => rect.height,
        Axis::Horizontal => rect.width,
    }
    .max(0.0);
    let fixed: f32 = sizes
        .iter()
        .map(|s| match s {
            Size::Fixed(px) => px.max(0.0),
            Size::Fraction(_) => 0.0,
        })
        .sum();
    let weights: f32 = sizes
        .iter()
        .map(|s| match s {
            Size::Fraction(w) => w.max(0.0),
            Size::Fixed(_) => 0.0,
        })
        .sum();
    let free = (total - fixed).max(0.0);
    let mut position = 0.0_f32;
    sizes
        .iter()
        .map(|size| {
            let wanted = match size {
                Size::Fixed(px) => px.max(0.0),
                Size::Fraction(w) if weights > 0.0 => free * w.max(0.0) / weights,
                Size::Fraction(_) => 0.0,
            };
            let start = position.round();
            position = (position + wanted).min(total);
            let end = position.round();
            match axis {
                Axis::Vertical => Rect::new(rect.x, rect.y + start, rect.width, end - start),
                Axis::Horizontal => Rect::new(rect.x + start, rect.y, end - start, rect.height),
            }
        })
        .collect()
}

/// Retire `amount` pixels de chaque côté de `rect`.
pub fn inset(rect: Rect, amount: f32) -> Rect {
    let x = amount.min(rect.width / 2.0);
    let y = amount.min(rect.height / 2.0);
    Rect::new(
        rect.x + x,
        rect.y + y,
        rect.width - 2.0 * x,
        rect.height - 2.0 * y,
    )
}

/// Rectangle de taille `width` × `height` centré horizontalement dans `area`, à `top` pixels de
/// son bord haut ; réduit pour tenir dans `area`.
pub fn centered_top(area: Rect, width: f32, height: f32, top: f32) -> Rect {
    let width = width.min(area.width).max(0.0);
    let height = height.min((area.height - top).max(0.0));
    Rect::new(
        (area.x + (area.width - width) / 2.0).round(),
        (area.y + top).round(),
        width.round(),
        height.round(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lignes_fixes_et_reste() {
        let area = Rect::new(0.0, 0.0, 800.0, 600.0);
        let parts = split(area, Axis::Vertical, &[Size::FILL, Size::Fixed(30.0)]);
        assert_eq!(parts[0], Rect::new(0.0, 0.0, 800.0, 570.0));
        assert_eq!(parts[1], Rect::new(0.0, 570.0, 800.0, 30.0));
    }

    #[test]
    fn colonnes_proportionnelles_sans_trou() {
        let area = Rect::new(10.0, 0.0, 100.0, 50.0);
        let parts = split(
            area,
            Axis::Horizontal,
            &[Size::Fraction(1.0), Size::Fixed(10.0), Size::Fraction(2.0)],
        );
        assert_eq!(parts[0].x, 10.0);
        assert_eq!(parts[0].right(), parts[1].x);
        assert_eq!(parts[1].width, 10.0);
        assert_eq!(parts[1].right(), parts[2].x);
        assert_eq!(parts[2].right(), 110.0);
        assert_eq!(parts[0].width, 30.0);
        assert_eq!(parts[2].width, 60.0);
    }

    #[test]
    fn place_insuffisante() {
        let area = Rect::new(0.0, 0.0, 50.0, 20.0);
        let parts = split(
            area,
            Axis::Horizontal,
            &[Size::Fixed(40.0), Size::Fixed(40.0)],
        );
        assert_eq!(parts[0].width, 40.0);
        assert_eq!(parts[1].width, 10.0, "réduite, jamais négative");
        assert_eq!(inset(area, 100.0).width, 0.0);
        let popup = centered_top(Rect::new(0.0, 0.0, 1000.0, 500.0), 600.0, 300.0, 50.0);
        assert_eq!(popup, Rect::new(200.0, 50.0, 600.0, 300.0));
    }
}
