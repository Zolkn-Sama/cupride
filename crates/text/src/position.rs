//! Conversions entre positions en octets et positions ligne / colonne.
//!
//! En interne, une position est un offset en octets UTF-8. Trois unités de colonne sont proposées :
//!
//! | Unité | Usage | Fonctions |
//! |---|---|---|
//! | octets | interne, exacte et réversible | [`offset_to_point`], [`point_to_offset`] |
//! | graphèmes | affichage : un graphème est un caractère tel que l'utilisateur le voit | [`offset_to_grapheme_point`], [`grapheme_point_to_offset`] |
//! | unités UTF-16 | protocole LSP (encodage par défaut) | [`offset_to_utf16_point`], [`utf16_point_to_offset`] |
//!
//! La ligne est trouvée en O(log n) avec une [`Rope`](crate::Rope) ; la colonne est calculée dans
//! la ligne, en temps proportionnel à sa longueur.
//!
//! Fins de ligne : `\n` et `\r\n` comptent pour un seul saut de ligne. En graphèmes et en UTF-16,
//! une position située dans la fin de ligne est ramenée à la fin du contenu de la ligne.

use unicode_segmentation::UnicodeSegmentation;

use crate::buffer::TextBuffer;
use crate::error::{Result, TextError};

/// Position ligne / colonne, à partir de 0. L'unité de la colonne dépend de la conversion utilisée.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    /// Ligne, à partir de 0.
    pub line: usize,
    /// Colonne, à partir de 0.
    pub column: usize,
}

impl Point {
    /// Crée une position.
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// Position en octets → ligne et colonne en octets.
pub fn offset_to_point<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<Point> {
    let line = buffer.byte_to_line(offset)?;
    let start = buffer.line_to_byte(line)?;
    Ok(Point::new(line, offset - start))
}

/// Ligne et colonne en octets → position en octets.
///
/// La colonne peut aller jusqu'au `\n` de la ligne (ou jusqu'à la fin du texte pour la dernière
/// ligne) et doit tomber sur une frontière de caractère.
pub fn point_to_offset<B: TextBuffer + ?Sized>(buffer: &B, point: Point) -> Result<usize> {
    let range = buffer.line_range(point.line)?;
    let is_last = point.line + 1 == buffer.len_lines();
    let max = if is_last {
        range.len()
    } else {
        range.len() - 1
    };
    let invalid = TextError::InvalidColumn {
        line: point.line,
        column: point.column,
    };
    if point.column > max {
        return Err(invalid);
    }
    let offset = range.start + point.column;
    // Frontière de caractère : vérifiée en extrayant le début de la ligne.
    buffer.slice(range.start..offset).map_err(|_| invalid)?;
    Ok(offset)
}

/// Contenu de la ligne contenant `offset`, et position locale ramenée dans ce contenu.
fn line_and_local<B: TextBuffer + ?Sized>(
    buffer: &B,
    offset: usize,
) -> Result<(usize, String, usize)> {
    let line = buffer.byte_to_line(offset)?;
    let start = buffer.line_to_byte(line)?;
    let content = buffer.line(line)?;
    let local = (offset - start).min(content.len());
    Ok((line, content, local))
}

/// Position en octets → ligne et colonne en graphèmes.
///
/// Une position située au milieu d'un graphème compte ce graphème comme déjà passé.
pub fn offset_to_grapheme_point<B: TextBuffer + ?Sized>(
    buffer: &B,
    offset: usize,
) -> Result<Point> {
    let (line, content, local) = line_and_local(buffer, offset)?;
    Ok(Point::new(line, content[..local].graphemes(true).count()))
}

/// Ligne et colonne en graphèmes → position en octets. La colonne peut aller jusqu'au nombre de
/// graphèmes de la ligne (fin du contenu).
pub fn grapheme_point_to_offset<B: TextBuffer + ?Sized>(buffer: &B, point: Point) -> Result<usize> {
    let start = buffer.line_to_byte(point.line)?;
    let content = buffer.line(point.line)?;
    let local = content
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain(std::iter::once(content.len()))
        .nth(point.column)
        .ok_or(TextError::InvalidColumn {
            line: point.line,
            column: point.column,
        })?;
    Ok(start + local)
}

/// Position en octets → ligne et colonne en unités UTF-16 (LSP).
pub fn offset_to_utf16_point<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<Point> {
    let (line, content, local) = line_and_local(buffer, offset)?;
    Ok(Point::new(line, content[..local].encode_utf16().count()))
}

/// Ligne et colonne en unités UTF-16 (LSP) → position en octets.
///
/// Une colonne au-delà de la ligne, ou au milieu d'une paire de substitution UTF-16, est une
/// erreur.
pub fn utf16_point_to_offset<B: TextBuffer + ?Sized>(buffer: &B, point: Point) -> Result<usize> {
    let start = buffer.line_to_byte(point.line)?;
    let content = buffer.line(point.line)?;
    let mut units = 0;
    for (index, ch) in content.char_indices() {
        if units == point.column {
            return Ok(start + index);
        }
        units += ch.len_utf16();
        if units > point.column {
            break;
        }
    }
    if units == point.column {
        return Ok(start + content.len());
    }
    Err(TextError::InvalidColumn {
        line: point.line,
        column: point.column,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rope, StringBuffer};

    /// Exécute le test sur les deux implémentations.
    fn both(text: &str, check: impl Fn(&dyn TextBuffer)) {
        check(&StringBuffer::from_text(text));
        check(&Rope::from_text(text));
    }

    #[test]
    fn octets_aller_retour_sur_toutes_les_positions() {
        let text = "ab\r\né😀\n\nfin";
        both(text, |b| {
            for offset in (0..=text.len()).filter(|&o| text.is_char_boundary(o)) {
                let point = offset_to_point(b, offset).unwrap();
                assert_eq!(point_to_offset(b, point).unwrap(), offset, "{point:?}");
            }
        });
    }

    #[test]
    fn octets_valeurs() {
        both("ab\r\ncd\nef", |b| {
            assert_eq!(offset_to_point(b, 0).unwrap(), Point::new(0, 0));
            assert_eq!(offset_to_point(b, 2).unwrap(), Point::new(0, 2));
            assert_eq!(offset_to_point(b, 4).unwrap(), Point::new(1, 0));
            assert_eq!(offset_to_point(b, 9).unwrap(), Point::new(2, 2));
            assert_eq!(point_to_offset(b, Point::new(1, 2)).unwrap(), 6);
            // Colonne 3 de la ligne 0 : entre \r et \n, encore dans la ligne.
            assert_eq!(point_to_offset(b, Point::new(0, 3)).unwrap(), 3);
            assert_eq!(
                point_to_offset(b, Point::new(0, 4)),
                Err(TextError::InvalidColumn { line: 0, column: 4 })
            );
            assert_eq!(
                point_to_offset(b, Point::new(2, 3)),
                Err(TextError::InvalidColumn { line: 2, column: 3 })
            );
            assert_eq!(
                point_to_offset(b, Point::new(3, 0)),
                Err(TextError::LineOutOfBounds { line: 3, count: 3 })
            );
        });
        both("é", |b| {
            assert_eq!(
                point_to_offset(b, Point::new(0, 1)),
                Err(TextError::InvalidColumn { line: 0, column: 1 })
            );
        });
    }

    #[test]
    fn crlf_compte_pour_un_saut_de_ligne() {
        both("a\r\nb\r\nc", |b| {
            assert_eq!(b.len_lines(), 3);
            assert_eq!(offset_to_point(b, 3).unwrap(), Point::new(1, 0));
            assert_eq!(offset_to_grapheme_point(b, 6).unwrap(), Point::new(2, 0));
            // Dans la fin de ligne : ramené à la fin du contenu.
            assert_eq!(offset_to_grapheme_point(b, 2).unwrap(), Point::new(0, 1));
            assert_eq!(offset_to_utf16_point(b, 2).unwrap(), Point::new(0, 1));
        });
    }

    #[test]
    fn colonnes_en_graphemes() {
        // e + accent combinant, emoji avec modificateur, drapeau, famille (ZWJ), CJK, tabulation.
        let line = "e\u{301}👍🏽🇫🇷👨‍👩‍👧日\tz";
        both(line, |b| {
            let boundaries: Vec<usize> = line
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([line.len()])
                .collect();
            assert_eq!(boundaries.len(), 8);
            for (column, &offset) in boundaries.iter().enumerate() {
                assert_eq!(
                    offset_to_grapheme_point(b, offset).unwrap(),
                    Point::new(0, column)
                );
                assert_eq!(
                    grapheme_point_to_offset(b, Point::new(0, column)).unwrap(),
                    offset
                );
            }
            assert_eq!(
                grapheme_point_to_offset(b, Point::new(0, 8)),
                Err(TextError::InvalidColumn { line: 0, column: 8 })
            );
        });
    }

    #[test]
    fn colonnes_utf16() {
        // a (1 unité), é (1), 😀 (2, paire de substitution), 日 (1).
        let text = "x\naé😀日";
        both(text, |b| {
            let cases = [(2, 0), (3, 1), (5, 2), (9, 4), (12, 5)];
            for (offset, column) in cases {
                assert_eq!(
                    offset_to_utf16_point(b, offset).unwrap(),
                    Point::new(1, column)
                );
                assert_eq!(
                    utf16_point_to_offset(b, Point::new(1, column)).unwrap(),
                    offset
                );
            }
            // Milieu de la paire de substitution de 😀.
            assert_eq!(
                utf16_point_to_offset(b, Point::new(1, 3)),
                Err(TextError::InvalidColumn { line: 1, column: 3 })
            );
            assert_eq!(
                utf16_point_to_offset(b, Point::new(1, 6)),
                Err(TextError::InvalidColumn { line: 1, column: 6 })
            );
        });
    }

    #[test]
    fn grand_texte_avec_la_rope() {
        let line = "ligne é😀\r\n";
        let rope = Rope::from_text(&line.repeat(50_000));
        // « ligne é » : 8 octets, 7 graphèmes, 7 unités UTF-16.
        let offset = 31_415 * line.len() + 8;
        let expected = Point::new(31_415, 8);
        assert_eq!(offset_to_point(&rope, offset).unwrap(), expected);
        assert_eq!(point_to_offset(&rope, expected).unwrap(), offset);
        let expected = Point::new(31_415, 7);
        assert_eq!(offset_to_grapheme_point(&rope, offset).unwrap(), expected);
        assert_eq!(offset_to_utf16_point(&rope, offset).unwrap(), expected);
    }
}
