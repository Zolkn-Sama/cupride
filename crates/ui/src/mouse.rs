//! Souris : clics multiples et sélection par glisser.
//!
//! - clic : place le curseur ; glisser : sélectionne caractère par caractère ;
//! - double clic : sélectionne un mot ; glisser ensuite étend mot par mot ;
//! - triple clic : sélectionne une ligne ; glisser ensuite étend ligne par ligne ;
//! - Maj+clic : étend la sélection principale jusqu'au clic ;
//! - Alt+clic : ajoute un curseur (ou un mot, une ligne) sans toucher aux autres.

use std::ops::Range;
use std::time::{Duration, Instant};

use cupride_core::text::movement::word_range;
use cupride_core::text::{Selection, Selections, TextBuffer};

/// Délai maximal entre deux clics d'un double ou triple clic.
pub const MULTI_CLICK_DELAY: Duration = Duration::from_millis(500);

/// Distance maximale, en pixels logiques, entre deux clics d'un double ou triple clic.
const MULTI_CLICK_DISTANCE: f32 = 4.0;

/// Compte les clics successifs au même endroit : 1, 2, 3, puis de nouveau 1.
#[derive(Debug, Clone, Default)]
pub struct ClickCounter {
    last: Option<(Instant, f32, f32)>,
    count: u8,
}

impl ClickCounter {
    /// Clic à (`x`, `y`) pixels physiques, sur un écran d'échelle `scale`. Renvoie le rang du clic.
    pub fn click(&mut self, now: Instant, x: f32, y: f32, scale: f32) -> u8 {
        let near = self.last.is_some_and(|(at, lx, ly)| {
            now.saturating_duration_since(at) <= MULTI_CLICK_DELAY
                && (x - lx).hypot(y - ly) <= MULTI_CLICK_DISTANCE * scale
        });
        self.count = if near && self.count < 3 {
            self.count + 1
        } else {
            1
        };
        self.last = Some((now, x, y));
        self.count
    }
}

/// Unité de sélection, selon le nombre de clics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Char,
    Word,
    Line,
}

/// Modificateurs utiles à la souris.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClickModifiers {
    /// Étendre la sélection principale.
    pub extend: bool,
    /// Ajouter une sélection.
    pub add: bool,
}

/// Sélection à la souris en cours.
#[derive(Debug, Clone)]
pub struct MouseSelection {
    unit: Unit,
    /// Plage de départ (le mot ou la ligne du clic), gardée pendant le glisser.
    anchor: Range<usize>,
    dragging: bool,
}

impl Default for MouseSelection {
    fn default() -> Self {
        Self {
            unit: Unit::Char,
            anchor: 0..0,
            dragging: false,
        }
    }
}

/// Plage de l'unité `unit` à la position `offset`.
fn unit_range(buffer: &impl TextBuffer, offset: usize, unit: Unit) -> Range<usize> {
    match unit {
        Unit::Char => offset..offset,
        Unit::Word => word_range(buffer, offset).unwrap_or(offset..offset),
        Unit::Line => buffer
            .byte_to_line(offset)
            .and_then(|line| buffer.line_range(line))
            .unwrap_or(offset..offset),
    }
}

/// Remplace la sélection principale.
fn with_primary(selections: &Selections, selection: Selection) -> Selections {
    let mut ranges = selections.as_slice().to_vec();
    let primary = selections.primary_index();
    ranges[primary] = selection;
    Selections::from_vec(ranges, primary).unwrap_or_else(|| Selections::single(selection))
}

impl MouseSelection {
    /// Un glisser est-il en cours ?
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Bouton enfoncé à la position `offset`, pour le `clicks`-ième clic : nouvelles sélections.
    pub fn press(
        &mut self,
        buffer: &impl TextBuffer,
        selections: &Selections,
        offset: usize,
        clicks: u8,
        modifiers: ClickModifiers,
    ) -> Selections {
        self.unit = match clicks {
            0 | 1 => Unit::Char,
            2 => Unit::Word,
            _ => Unit::Line,
        };
        self.dragging = true;
        let range = unit_range(buffer, offset, self.unit);
        if modifiers.extend && !modifiers.add {
            let anchor = selections.primary().anchor;
            self.anchor = anchor..anchor;
            return with_primary(selections, Selection::new(anchor, offset));
        }
        self.anchor = range.clone();
        let selection = Selection::new(range.start, range.end);
        if modifiers.add {
            let mut selections = selections.clone();
            selections.push(selection);
            selections
        } else {
            Selections::single(selection)
        }
    }

    /// Souris déplacée en position `offset`, bouton enfoncé : la sélection principale va de la
    /// plage de départ à l'unité sous la souris.
    pub fn drag(
        &self,
        buffer: &impl TextBuffer,
        selections: &Selections,
        offset: usize,
    ) -> Option<Selections> {
        if !self.dragging {
            return None;
        }
        let range = unit_range(buffer, offset, self.unit);
        let selection = if range.start < self.anchor.start {
            Selection::new(self.anchor.end, range.start)
        } else {
            Selection::new(self.anchor.start, range.end.max(self.anchor.end))
        };
        Some(with_primary(selections, selection))
    }

    /// Bouton relâché.
    pub fn release(&mut self) {
        self.dragging = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cupride_core::text::Rope;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn rang_des_clics() {
        let t0 = Instant::now();
        let mut counter = ClickCounter::default();
        assert_eq!(counter.click(t0, 10.0, 10.0, 2.0), 1);
        assert_eq!(counter.click(t0 + 200 * MS, 12.0, 11.0, 2.0), 2);
        assert_eq!(counter.click(t0 + 400 * MS, 12.0, 11.0, 2.0), 3);
        assert_eq!(
            counter.click(t0 + 600 * MS, 12.0, 11.0, 2.0),
            1,
            "après trois : un"
        );
        assert_eq!(
            counter.click(t0 + 1_200 * MS, 12.0, 11.0, 2.0),
            1,
            "trop tard"
        );
        assert_eq!(
            counter.click(t0 + 1_300 * MS, 40.0, 11.0, 2.0),
            1,
            "trop loin"
        );
    }

    fn pairs(selections: &Selections) -> Vec<(usize, usize)> {
        selections.iter().map(|s| (s.anchor, s.head)).collect()
    }

    #[test]
    fn clic_et_glisser() {
        let buffer = Rope::from_text("un deux trois\nquatre cinq");
        let current = Selections::cursor(0);
        let mut mouse = MouseSelection::default();
        let s = mouse.press(&buffer, &current, 4, 1, ClickModifiers::default());
        assert_eq!(pairs(&s), [(4, 4)]);
        let s = mouse.drag(&buffer, &s, 10).unwrap();
        assert_eq!(pairs(&s), [(4, 10)]);
        let s = mouse.drag(&buffer, &s, 1).unwrap();
        assert_eq!(pairs(&s), [(4, 1)], "vers l'arrière");
        mouse.release();
        assert!(mouse.drag(&buffer, &s, 7).is_none());
    }

    #[test]
    fn double_et_triple_clic() {
        let buffer = Rope::from_text("un deux trois\nquatre cinq");
        let current = Selections::cursor(0);
        let mut mouse = MouseSelection::default();
        let s = mouse.press(&buffer, &current, 5, 2, ClickModifiers::default());
        assert_eq!(pairs(&s), [(3, 7)], "le mot « deux »");
        let s = mouse.drag(&buffer, &s, 10).unwrap();
        assert_eq!(pairs(&s), [(3, 13)], "étendu mot par mot");
        let s = mouse.drag(&buffer, &s, 0).unwrap();
        assert_eq!(
            pairs(&s),
            [(7, 0)],
            "vers l'arrière, en gardant le mot de départ"
        );

        let s = mouse.press(&buffer, &current, 5, 3, ClickModifiers::default());
        assert_eq!(pairs(&s), [(0, 14)], "la ligne, saut de ligne compris");
        let s = mouse.drag(&buffer, &s, 20).unwrap();
        assert_eq!(pairs(&s), [(0, 25)], "étendu ligne par ligne");
    }

    #[test]
    fn maj_et_alt() {
        let buffer = Rope::from_text("un deux trois\nquatre cinq");
        let mut mouse = MouseSelection::default();
        let current = Selections::cursor(3);
        let extend = ClickModifiers {
            extend: true,
            add: false,
        };
        let s = mouse.press(&buffer, &current, 13, 1, extend);
        assert_eq!(pairs(&s), [(3, 13)], "Maj+clic étend");

        let add = ClickModifiers {
            extend: false,
            add: true,
        };
        let s = mouse.press(&buffer, &Selections::cursor(0), 20, 1, add);
        assert_eq!(pairs(&s), [(0, 0), (20, 20)], "Alt+clic ajoute un curseur");
        assert_eq!(s.primary().head, 20);
        let s = mouse.drag(&buffer, &s, 23).unwrap();
        assert_eq!(
            pairs(&s),
            [(0, 0), (20, 23)],
            "glisser ne touche que l'ajouté"
        );
    }
}
