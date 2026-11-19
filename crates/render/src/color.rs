//! Couleurs, en sRGB.

/// Couleur sRGB avec opacité, composantes de 0 à 255.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Color {
    /// Rouge.
    pub r: u8,
    /// Vert.
    pub g: u8,
    /// Bleu.
    pub b: u8,
    /// Opacité (255 : opaque).
    pub a: u8,
}

impl Color {
    /// Couleur opaque.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Couleur avec opacité.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Couleur depuis une notation hexadécimale : `#rrggbb` ou `#rrggbbaa`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// Composantes linéaires (0 à 1), opacité prémultipliée non appliquée : pour le GPU.
    pub fn to_linear(self) -> [f32; 4] {
        fn channel(c: u8) -> f32 {
            let c = f32::from(c) / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        [
            channel(self.r),
            channel(self.g),
            channel(self.b),
            f32::from(self.a) / 255.0,
        ]
    }

    /// Couleur `wgpu`, linéaire, pour effacer une image.
    pub fn to_wgpu(self) -> wgpu::Color {
        let [r, g, b, a] = self.to_linear();
        wgpu::Color {
            r: f64::from(r),
            g: f64::from(g),
            b: f64::from(b),
            a: f64::from(a),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notation_hexadecimale() {
        assert_eq!(Color::from_hex("#ff8040"), Some(Color::rgb(255, 128, 64)));
        assert_eq!(
            Color::from_hex("#ff804080"),
            Some(Color::rgba(255, 128, 64, 128))
        );
        assert_eq!(Color::from_hex("ff8040"), None);
        assert_eq!(Color::from_hex("#ff80"), None);
        assert_eq!(Color::from_hex("#gg0000"), None);
    }

    #[test]
    fn conversion_lineaire() {
        assert_eq!(Color::rgb(0, 0, 0).to_linear(), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(Color::rgb(255, 255, 255).to_linear(), [1.0, 1.0, 1.0, 1.0]);
        let mid = Color::rgb(128, 128, 128).to_linear()[0];
        assert!((mid - 0.2158).abs() < 0.001, "{mid}");
    }
}
