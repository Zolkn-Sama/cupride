//! Gouttière : marqueurs et numéros de ligne, à gauche du texte.
//!
//! De gauche à droite : une colonne réservée aux marqueurs (diagnostics, état Git, points
//! d'arrêt), puis les numéros, alignés à droite. Sans numéros (`line_numbers = "off"`), la
//! colonne des marqueurs reste là, pour que le texte ne se décale pas quand un marqueur apparaît.

use std::collections::HashMap;

use cupride_core::config::LineNumbers;
use cupride_render::Color;
use cupride_render::TextSystem;
use cupride_render::text::{Attrs, Buffer, Metrics, Shaping, Wrap};

/// Largeur de la colonne des marqueurs, en pixels logiques.
pub(crate) const MARKER_LANE: f32 = 12.0;

/// Espace entre les numéros et le texte, en pixels logiques.
pub(crate) const NUMBER_GAP: f32 = 12.0;

/// Nombre minimal de chiffres réservés aux numéros : la gouttière ne s'élargit pas avant 1 000
/// lignes.
const MIN_DIGITS: usize = 3;

/// Marqueur affiché dans la gouttière, devant une ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GutterMarker {
    /// Ligne marquée.
    pub line: usize,
    /// Couleur du marqueur.
    pub color: Color,
}

/// Texte affiché pour la ligne `line` (à partir de 0), le curseur étant sur `cursor_line`.
pub(crate) fn label(line: usize, cursor_line: usize, mode: LineNumbers) -> Option<String> {
    match mode {
        LineNumbers::Off => None,
        LineNumbers::Absolute => Some((line + 1).to_string()),
        LineNumbers::Relative if line == cursor_line => Some((line + 1).to_string()),
        LineNumbers::Relative => Some(line.abs_diff(cursor_line).to_string()),
    }
}

/// Nombre de chiffres réservés pour un document de `line_count` lignes.
pub(crate) fn digits(line_count: usize) -> usize {
    line_count.max(1).to_string().len().max(MIN_DIGITS)
}

/// Un numéro mis en forme.
pub(crate) struct Number {
    pub(crate) label: String,
    pub(crate) buffer: Buffer,
    /// Largeur du numéro, pour l'aligner à droite.
    pub(crate) width: f32,
}

/// Numéros mis en forme des lignes visibles, gardés tant que leur texte ne change pas.
#[derive(Default)]
pub(crate) struct Gutter {
    /// Largeur d'un chiffre, mesurée une fois par police et échelle.
    pub(crate) digit_width: f32,
    pub(crate) numbers: HashMap<usize, Number>,
}

impl Gutter {
    /// Oublie tout : la police ou l'échelle a changé.
    pub(crate) fn clear(&mut self) {
        self.digit_width = 0.0;
        self.numbers.clear();
    }

    /// Met en forme les numéros des lignes `visible`.
    pub(crate) fn prepare(
        &mut self,
        text: &mut TextSystem,
        metrics: Metrics,
        attrs: &Attrs<'_>,
        visible: std::ops::Range<usize>,
        cursor_line: usize,
        mode: LineNumbers,
    ) {
        if self.digit_width == 0.0 {
            self.digit_width = shape(text, metrics, attrs, "0").1;
        }
        self.numbers.retain(|line, _| visible.contains(line));
        for line in visible {
            let Some(label) = label(line, cursor_line, mode) else {
                self.numbers.remove(&line);
                continue;
            };
            if self.numbers.get(&line).is_some_and(|n| n.label == label) {
                continue;
            }
            let (buffer, width) = shape(text, metrics, attrs, &label);
            self.numbers.insert(
                line,
                Number {
                    label,
                    buffer,
                    width,
                },
            );
        }
    }
}

/// Met en forme `label` sur une ligne ; renvoie le texte et sa largeur.
fn shape(text: &mut TextSystem, metrics: Metrics, attrs: &Attrs<'_>, label: &str) -> (Buffer, f32) {
    let mut buffer = Buffer::new(text.font_system(), metrics);
    buffer.set_wrap(Wrap::None);
    buffer.set_size(None, Some(metrics.line_height));
    buffer.set_text(label, attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(text.font_system(), false);
    let width = buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0, f32::max);
    (buffer, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libelles() {
        assert_eq!(label(0, 5, LineNumbers::Absolute).as_deref(), Some("1"));
        assert_eq!(label(9, 5, LineNumbers::Relative).as_deref(), Some("4"));
        assert_eq!(label(2, 5, LineNumbers::Relative).as_deref(), Some("3"));
        assert_eq!(
            label(5, 5, LineNumbers::Relative).as_deref(),
            Some("6"),
            "ligne du curseur : numéro réel"
        );
        assert_eq!(label(5, 5, LineNumbers::Off), None);
    }

    #[test]
    fn chiffres_reserves() {
        assert_eq!(digits(1), 3);
        assert_eq!(digits(999), 3);
        assert_eq!(digits(1_000), 4);
        assert_eq!(digits(100_000), 6);
    }
}
