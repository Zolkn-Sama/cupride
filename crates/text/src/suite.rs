//! Suite de tests commune à toutes les implémentations de [`TextBuffer`](crate::TextBuffer).
//!
//! Elle ne dépend que du trait : chaque implémentation l'exécute avec
//! `crate::suite::buffer_suite!(MonBuffer);` dans son module de tests.

/// Générateur pseudo-aléatoire minimal et déterministe, pour des séquences reproductibles.
#[derive(Debug)]
pub(crate) struct Lcg(u64);

impl Lcg {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// Nombre dans `0..=max`.
    pub(crate) fn below(&mut self, max: usize) -> usize {
        (self.next() % (max as u64 + 1)) as usize
    }

    /// Position aléatoire sur une frontière de caractère de `text`.
    pub(crate) fn boundary(&mut self, text: &str) -> usize {
        let mut offset = self.below(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// Texte aléatoire mêlant ASCII, multi-octets, emojis et fins de ligne.
    pub(crate) fn text(&mut self, max_len: usize) -> String {
        const PIECES: &[&str] = &[
            "a", "b", "z", " ", "é", "\n", "\r\n", "日本", "👍🏽", "e\u{301}",
        ];
        let count = self.below(max_len);
        (0..count)
            .map(|_| PIECES[self.below(PIECES.len() - 1)])
            .collect()
    }
}

/// Génère la suite de tests pour le type de buffer donné.
macro_rules! buffer_suite {
    ($buffer:ty) => {
        mod buffer_suite {
            #[allow(unused_imports)]
            use super::*;
            use $crate::suite::Lcg;
            use $crate::{TextBuffer, TextError};

            type B = $buffer;

            fn buf(text: &str) -> B {
                <B as TextBuffer>::from_text(text)
            }

            /// Compare toutes les requêtes du buffer avec celles d'une `String` de référence.
            fn assert_same(buffer: &B, expected: &str) {
                assert_eq!(buffer.to_text(), expected);
                assert_eq!(buffer.len_bytes(), expected.len());
                assert_eq!(buffer.is_empty(), expected.is_empty());
                let lines: Vec<String> = expected
                    .split('\n')
                    .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
                    .collect();
                assert_eq!(buffer.len_lines(), lines.len());
                assert_eq!(buffer.lines().collect::<Vec<_>>(), lines);
                assert_eq!(buffer.chars().collect::<String>(), expected);
                let mut line_start = 0;
                // Au plus ~200 lignes vérifiées une à une : la référence est en O(n) par ligne.
                let step = (lines.len() / 200).max(1);
                for (i, line) in expected.split('\n').enumerate() {
                    if i % step != 0 && i + 1 != lines.len() {
                        line_start += line.len() + 1;
                        continue;
                    }
                    assert_eq!(
                        buffer.line_to_byte(i).unwrap(),
                        line_start,
                        "début ligne {i}"
                    );
                    assert_eq!(buffer.line(i).unwrap(), lines[i], "ligne {i}");
                    assert_eq!(buffer.byte_to_line(line_start).unwrap(), i);
                    line_start += line.len() + 1;
                }
            }

            #[test]
            fn buffer_vide() {
                let b = buf("");
                assert_same(&b, "");
                assert_eq!(b.len_lines(), 1);
                assert_eq!(b.line(0).unwrap(), "");
                assert_eq!(b.slice(0..0).unwrap(), "");
                assert_eq!(b.byte_to_line(0).unwrap(), 0);
                assert_eq!(b.line_to_byte(0).unwrap(), 0);
                assert_eq!(
                    b.line(1),
                    Err(TextError::LineOutOfBounds { line: 1, count: 1 })
                );
            }

            #[test]
            fn creation_et_relecture() {
                for text in ["a", "abc\ndef", "é😀\r\nx\n", "\n\n\n", "日本語"] {
                    assert_same(&buf(text), text);
                }
            }

            #[test]
            fn insertion_debut_milieu_fin() {
                let mut b = buf("monde");
                b.insert(0, "le ").unwrap();
                assert_same(&b, "le monde");
                b.insert(3, "vaste ").unwrap();
                assert_same(&b, "le vaste monde");
                b.insert(b.len_bytes(), " !").unwrap();
                assert_same(&b, "le vaste monde !");
                b.insert(5, "").unwrap();
                assert_same(&b, "le vaste monde !");
            }

            #[test]
            fn insertion_dans_un_buffer_vide() {
                let mut b = buf("");
                b.insert(0, "ligne 1\nligne 2").unwrap();
                assert_same(&b, "ligne 1\nligne 2");
            }

            #[test]
            fn insertion_hors_du_texte() {
                let mut b = buf("abc");
                assert_eq!(
                    b.insert(4, "x"),
                    Err(TextError::OffsetOutOfBounds { offset: 4, len: 3 })
                );
                assert_same(&b, "abc");
            }

            #[test]
            fn insertion_au_milieu_d_un_caractere() {
                let mut b = buf("é");
                assert_eq!(
                    b.insert(1, "x"),
                    Err(TextError::NotCharBoundary { offset: 1 })
                );
                assert_same(&b, "é");
            }

            #[test]
            fn suppression() {
                let mut b = buf("le vaste monde");
                b.remove(3..9).unwrap();
                assert_same(&b, "le monde");
                b.remove(0..3).unwrap();
                assert_same(&b, "monde");
                b.remove(3..5).unwrap();
                assert_same(&b, "mon");
                b.remove(1..1).unwrap();
                assert_same(&b, "mon");
                b.remove(0..3).unwrap();
                assert_same(&b, "");
            }

            #[test]
            fn suppression_de_fins_de_ligne() {
                let mut b = buf("a\r\nb\nc");
                b.remove(1..3).unwrap();
                assert_same(&b, "ab\nc");
                b.remove(2..3).unwrap();
                assert_same(&b, "abc");
            }

            #[test]
            fn plages_invalides() {
                let mut b = buf("abc");
                #[allow(clippy::reversed_empty_ranges)]
                let inversee = 2..1;
                assert_eq!(
                    b.remove(inversee.clone()),
                    Err(TextError::InvalidRange {
                        start: 2,
                        end: 1,
                        len: 3
                    })
                );
                assert_eq!(
                    b.slice(1..4),
                    Err(TextError::InvalidRange {
                        start: 1,
                        end: 4,
                        len: 3
                    })
                );
                assert_eq!(
                    b.slice(inversee),
                    Err(TextError::InvalidRange {
                        start: 2,
                        end: 1,
                        len: 3
                    })
                );
                let b2 = buf("aé");
                assert_eq!(
                    b2.slice(0..2),
                    Err(TextError::NotCharBoundary { offset: 2 })
                );
                assert_same(&b, "abc");
            }

            #[test]
            fn extraction() {
                let b = buf("un\ndeux\r\ntrois");
                assert_eq!(b.slice(0..2).unwrap(), "un");
                assert_eq!(b.slice(3..7).unwrap(), "deux");
                assert_eq!(b.slice(2..10).unwrap(), "\ndeux\r\nt");
                assert_eq!(b.slice(0..b.len_bytes()).unwrap(), "un\ndeux\r\ntrois");
            }

            #[test]
            fn lignes_et_fins_de_ligne() {
                let b = buf("a\nb\r\nc");
                assert_eq!(b.len_lines(), 3);
                assert_eq!(b.line(0).unwrap(), "a");
                assert_eq!(b.line(1).unwrap(), "b");
                assert_eq!(b.line(2).unwrap(), "c");
                assert_eq!(b.line_range(1).unwrap(), 2..5);
                assert_eq!(b.line_content_range(1).unwrap(), 2..3);

                let b = buf("a\n");
                assert_eq!(b.len_lines(), 2);
                assert_eq!(b.line(1).unwrap(), "");
                assert_eq!(b.line_range(1).unwrap(), 2..2);
            }

            #[test]
            fn conversions_ligne_position() {
                let b = buf("ab\ncd\r\n\nef");
                assert_eq!(b.byte_to_line(0).unwrap(), 0);
                assert_eq!(b.byte_to_line(2).unwrap(), 0);
                assert_eq!(b.byte_to_line(3).unwrap(), 1);
                assert_eq!(b.byte_to_line(6).unwrap(), 1);
                assert_eq!(b.byte_to_line(7).unwrap(), 2);
                assert_eq!(b.byte_to_line(8).unwrap(), 3);
                assert_eq!(b.byte_to_line(10).unwrap(), 3);
                assert_eq!(
                    b.byte_to_line(11),
                    Err(TextError::OffsetOutOfBounds {
                        offset: 11,
                        len: 10
                    })
                );
                assert_eq!(b.line_to_byte(3).unwrap(), 8);
                assert_eq!(
                    b.line_to_byte(4),
                    Err(TextError::LineOutOfBounds { line: 4, count: 4 })
                );
            }

            #[test]
            fn position_au_milieu_d_un_caractere() {
                let b = buf("é");
                assert_eq!(
                    b.byte_to_line(1),
                    Err(TextError::NotCharBoundary { offset: 1 })
                );
            }

            #[test]
            fn morceaux_complets_et_sans_caractere_coupe() {
                let text = "é😀\n".repeat(2_000);
                let b = buf(&text);
                let mut total = String::new();
                for chunk in b.chunks() {
                    // Un &str est toujours de l'UTF-8 valide : il suffit de vérifier le total.
                    total.push_str(chunk);
                }
                assert_eq!(total, text);
            }

            #[test]
            fn gros_texte() {
                let line = "Une ligne de texte assez longue, avec é et 😀.\n";
                let text = line.repeat(20_000);
                let mut b = buf(&text);
                assert_eq!(b.len_lines(), 20_001);
                assert_eq!(b.line(12_345).unwrap(), line.trim_end_matches('\n'));
                let mid = b.line_to_byte(10_000).unwrap();
                assert_eq!(mid, 10_000 * line.len());
                assert_eq!(b.byte_to_line(mid).unwrap(), 10_000);
                b.insert(mid, "insertion\n").unwrap();
                assert_eq!(b.line(10_000).unwrap(), "insertion");
                assert_eq!(b.len_lines(), 20_002);
                b.remove(mid..mid + 10).unwrap();
                assert_same(&b, &text);
                b.remove(line.len()..b.len_bytes() - line.len()).unwrap();
                assert_same(&b, &line.repeat(2));
            }

            #[test]
            fn sequence_de_modifications() {
                let mut rng = Lcg::new(42);
                let mut expected = String::new();
                let mut b = buf("");
                for _ in 0..2_000 {
                    if rng.below(2) > 0 || expected.is_empty() {
                        let offset = rng.boundary(&expected);
                        let text = rng.text(300);
                        expected.insert_str(offset, &text);
                        b.insert(offset, &text).unwrap();
                    } else {
                        let a = rng.boundary(&expected);
                        let c = rng.boundary(&expected);
                        let range = a.min(c)..a.max(c);
                        expected.replace_range(range.clone(), "");
                        b.remove(range).unwrap();
                    }
                    assert_eq!(b.len_bytes(), expected.len());
                }
                assert_same(&b, &expected);
            }
        }
    };
}

pub(crate) use buffer_suite;
