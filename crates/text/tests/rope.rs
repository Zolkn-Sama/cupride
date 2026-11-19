//! Tests de la rope avec ses tailles réelles (feuilles de 1 à 4 Ko, 8 à 16 enfants).

use cupride_text::{Rope, TextBuffer};

/// Générateur pseudo-aléatoire minimal et déterministe.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, max: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % (max as u64 + 1)) as usize
    }

    fn boundary(&mut self, text: &str) -> usize {
        let mut offset = self.below(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }
}

#[test]
fn gros_fichier_equilibre() {
    let line = "fn main() { println!(\"é😀\"); }\r\n";
    let text = line.repeat(40_000); // ~1,3 Mo
    let rope = Rope::from_text(&text);
    rope.check_invariants().unwrap();
    assert!(rope.height() >= 2);
    assert_eq!(rope.len_lines(), 40_001);
    assert_eq!(rope.line(20_000).unwrap(), line.trim_end());
    assert_eq!(rope.to_text(), text);
}

#[test]
fn modifications_aleatoires_sur_un_gros_fichier() {
    let mut rng = Lcg(2024);
    let mut expected = "ligne de départ é😀\n".repeat(30_000);
    let mut rope = Rope::from_text(&expected);
    let pieces = ["x", "é", "😀", "\n", "\r\n", "un mot ", "日本語"];
    for step in 0..2_000 {
        if rng.below(1) == 0 {
            let offset = rng.boundary(&expected);
            let count = if step % 100 == 0 {
                5_000
            } else {
                rng.below(20)
            };
            let text: String = (0..count)
                .map(|_| pieces[rng.below(pieces.len() - 1)])
                .collect();
            expected.insert_str(offset, &text);
            rope.insert(offset, &text).unwrap();
        } else {
            let a = rng.boundary(&expected);
            let len = if step % 100 == 1 {
                200_000
            } else {
                rng.below(500)
            };
            let mut b = (a + len).min(expected.len());
            while !expected.is_char_boundary(b) {
                b -= 1;
            }
            expected.replace_range(a..b, "");
            rope.remove(a..b).unwrap();
        }
        if step % 100 == 0 {
            rope.check_invariants()
                .unwrap_or_else(|e| panic!("étape {step} : {e}"));
        }
    }
    rope.check_invariants().unwrap();
    assert_eq!(rope.to_text(), expected);
    assert_eq!(rope.len_lines(), expected.matches('\n').count() + 1);
}
