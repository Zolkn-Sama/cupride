//! Déplacements du curseur et plages de suppression, respectueux d'Unicode.
//!
//! Le curseur avance par **graphème** : un caractère tel que l'utilisateur le voit, qui peut
//! regrouper plusieurs points de code (emoji composé, lettre suivie d'un accent combinant,
//! drapeau…). Un `\r\n` est aussi un seul graphème.
//!
//! Toutes les fonctions prennent et renvoient des positions en octets ; une position hors du texte
//! ou au milieu d'un caractère est une erreur. Aux extrémités du texte, le déplacement s'arrête.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::buffer::TextBuffer;
use crate::error::Result;

/// Ligne contenant `offset` : numéro, position de début et texte complet (fin de ligne comprise).
fn line_at<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<(usize, usize, String)> {
    let line = buffer.byte_to_line(offset)?;
    let range = buffer.line_range(line)?;
    let start = range.start;
    Ok((line, start, buffer.slice(range)?))
}

/// Texte complet (fin de ligne comprise) et position de début de la ligne `line`.
fn line_text<B: TextBuffer + ?Sized>(buffer: &B, line: usize) -> Result<(usize, String)> {
    let range = buffer.line_range(line)?;
    let start = range.start;
    Ok((start, buffer.slice(range)?))
}

/// Un segment de mot (au sens d'Unicode, UAX #29) contenant une lettre, un chiffre ou `_`.
fn is_word(segment: &str) -> bool {
    segment.chars().any(|c| c.is_alphanumeric() || c == '_')
}

/// Position après le graphème qui suit `offset`.
pub fn next_grapheme<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<usize> {
    let (_, start, text) = line_at(buffer, offset)?;
    let local = offset - start;
    // Un saut de ligne termine toujours un graphème : la recherche reste dans la ligne.
    let next = text
        .grapheme_indices(true)
        .map(|(i, g)| i + g.len())
        .find(|&end| end > local)
        .unwrap_or(local);
    Ok(start + next)
}

/// Position du début du graphème qui précède `offset`.
pub fn prev_grapheme<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<usize> {
    let (line, start, text) = line_at(buffer, offset)?;
    let local = offset - start;
    if local == 0 {
        if line == 0 {
            return Ok(0);
        }
        // Début de ligne : le graphème précédent est la fin de ligne de la ligne précédente.
        let (prev_start, prev_text) = line_text(buffer, line - 1)?;
        let last = prev_text
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i);
        return Ok(prev_start + last);
    }
    let prev = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .take_while(|&i| i < local)
        .last()
        .unwrap_or(0);
    Ok(start + prev)
}

/// Position de la fin du mot suivant (ou courant, si `offset` est au milieu d'un mot). Les
/// espaces, la ponctuation et les fins de ligne sont sautés.
pub fn next_word<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<usize> {
    let (mut line, mut start, mut text) = line_at(buffer, offset)?;
    let mut local = offset - start;
    loop {
        let found = text
            .split_word_bound_indices()
            .find(|&(i, seg)| is_word(seg) && i + seg.len() > local)
            .map(|(i, seg)| i + seg.len());
        if let Some(end) = found {
            return Ok(start + end);
        }
        if line + 1 >= buffer.len_lines() {
            return Ok(buffer.len_bytes());
        }
        line += 1;
        (start, text) = line_text(buffer, line)?;
        local = 0;
    }
}

/// Position du début du mot précédent (ou courant, si `offset` est au milieu d'un mot).
pub fn prev_word<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<usize> {
    let (mut line, mut start, mut text) = line_at(buffer, offset)?;
    let mut local = offset - start;
    loop {
        let found = text
            .split_word_bound_indices()
            .rev()
            .find(|&(i, seg)| is_word(seg) && i < local)
            .map(|(i, _)| i);
        if let Some(begin) = found {
            return Ok(start + begin);
        }
        if line == 0 {
            return Ok(0);
        }
        line -= 1;
        (start, text) = line_text(buffer, line)?;
        local = text.len();
    }
}

/// Début de la ligne contenant `offset`.
pub fn line_start<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<usize> {
    buffer.line_to_byte(buffer.byte_to_line(offset)?)
}

/// Fin du contenu de la ligne contenant `offset`, avant sa fin de ligne.
pub fn line_end<B: TextBuffer + ?Sized>(buffer: &B, offset: usize) -> Result<usize> {
    Ok(buffer.line_content_range(buffer.byte_to_line(offset)?)?.end)
}

/// Début du texte.
pub fn document_start<B: TextBuffer + ?Sized>(_buffer: &B) -> usize {
    0
}

/// Fin du texte.
pub fn document_end<B: TextBuffer + ?Sized>(buffer: &B) -> usize {
    buffer.len_bytes()
}

/// Plage supprimée par « retour arrière » depuis `offset` : le graphème précédent.
pub fn delete_backward_range<B: TextBuffer + ?Sized>(
    buffer: &B,
    offset: usize,
) -> Result<Range<usize>> {
    Ok(prev_grapheme(buffer, offset)?..offset)
}

/// Plage supprimée par « suppr » depuis `offset` : le graphème suivant.
pub fn delete_forward_range<B: TextBuffer + ?Sized>(
    buffer: &B,
    offset: usize,
) -> Result<Range<usize>> {
    Ok(offset..next_grapheme(buffer, offset)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rope, StringBuffer, TextError};

    fn both(text: &str, check: impl Fn(&dyn TextBuffer)) {
        check(&StringBuffer::from_text(text));
        check(&Rope::from_text(text));
        // Rope à plusieurs feuilles : le même texte, précédé de beaucoup de lignes.
        let prefix = "remplissage\n".repeat(50);
        let rope = Rope::from_text(&format!("{prefix}{text}"));
        let shifted = ShiftedBuffer {
            inner: rope,
            shift: prefix.len(),
            lines: 50,
        };
        check(&shifted);
    }

    /// Vue d'un buffer décalé : permet de rejouer les mêmes positions sur une rope plus grande.
    struct ShiftedBuffer {
        inner: Rope,
        shift: usize,
        lines: usize,
    }

    impl TextBuffer for ShiftedBuffer {
        fn from_text(_: &str) -> Self {
            unreachable!()
        }
        fn len_bytes(&self) -> usize {
            self.inner.len_bytes() - self.shift
        }
        fn len_lines(&self) -> usize {
            self.inner.len_lines() - self.lines
        }
        fn insert(&mut self, offset: usize, text: &str) -> Result<()> {
            self.inner.insert(offset + self.shift, text)
        }
        fn remove(&mut self, range: Range<usize>) -> Result<()> {
            self.inner
                .remove(range.start + self.shift..range.end + self.shift)
        }
        fn slice(&self, range: Range<usize>) -> Result<String> {
            self.inner
                .slice(range.start + self.shift..range.end + self.shift)
        }
        fn byte_to_line(&self, offset: usize) -> Result<usize> {
            Ok(self.inner.byte_to_line(offset + self.shift)? - self.lines)
        }
        fn line_to_byte(&self, line: usize) -> Result<usize> {
            Ok(self.inner.line_to_byte(line + self.lines)? - self.shift)
        }
        fn chunks(&self) -> Box<dyn Iterator<Item = &str> + '_> {
            unreachable!()
        }
    }

    /// Positions successives atteintes en avançant (ou reculant) jusqu'au bout.
    fn walk(
        b: &dyn TextBuffer,
        start: usize,
        step: fn(&dyn TextBuffer, usize) -> Result<usize>,
    ) -> Vec<usize> {
        let mut positions = vec![start];
        loop {
            let last = *positions.last().unwrap();
            let next = step(b, last).unwrap();
            if next == last {
                return positions;
            }
            positions.push(next);
        }
    }

    fn next_g(b: &dyn TextBuffer, o: usize) -> Result<usize> {
        next_grapheme(b, o)
    }
    fn prev_g(b: &dyn TextBuffer, o: usize) -> Result<usize> {
        prev_grapheme(b, o)
    }
    fn next_w(b: &dyn TextBuffer, o: usize) -> Result<usize> {
        next_word(b, o)
    }
    fn prev_w(b: &dyn TextBuffer, o: usize) -> Result<usize> {
        prev_word(b, o)
    }

    #[test]
    fn graphemes_composes() {
        // e + accent combinant | emoji + modificateur | famille (ZWJ) | drapeau | CJK | tab | x
        let text = "e\u{301}👍🏽👨‍👩‍👧🇫🇷日\tx";
        let expected: Vec<usize> = std::iter::once(0)
            .chain(text.grapheme_indices(true).map(|(i, g)| i + g.len()))
            .collect();
        assert_eq!(expected.len(), 8);
        both(text, |b| {
            assert_eq!(walk(b, 0, next_g), expected);
            let mut back = expected.clone();
            back.reverse();
            assert_eq!(walk(b, text.len(), prev_g), back);
        });
    }

    #[test]
    fn crlf_est_un_seul_grapheme() {
        both("a\r\nb\nc", |b| {
            assert_eq!(walk(b, 0, next_g), vec![0, 1, 3, 4, 5, 6]);
            assert_eq!(walk(b, 6, prev_g), vec![6, 5, 4, 3, 1, 0]);
            assert_eq!(delete_backward_range(b, 3).unwrap(), 1..3);
            assert_eq!(delete_forward_range(b, 1).unwrap(), 1..3);
        });
    }

    #[test]
    fn suppression_par_grapheme() {
        let text = "a👨‍👩‍👧e\u{301}";
        both(text, |b| {
            let family_end = 1 + "👨‍👩‍👧".len();
            assert_eq!(delete_backward_range(b, family_end).unwrap(), 1..family_end);
            assert_eq!(delete_forward_range(b, 1).unwrap(), 1..family_end);
            assert_eq!(
                delete_backward_range(b, text.len()).unwrap(),
                family_end..text.len()
            );
            assert_eq!(delete_backward_range(b, 0).unwrap(), 0..0);
            assert_eq!(
                delete_forward_range(b, text.len()).unwrap(),
                text.len()..text.len()
            );
        });
    }

    #[test]
    fn mots() {
        let text = "let x_1 = foo(bar);\n  // été\n";
        both(text, |b| {
            // Fins de : let, x_1, foo, bar, été, puis la fin du texte.
            assert_eq!(walk(b, 0, next_w), vec![0, 3, 7, 13, 17, 30, 31]);
            // Débuts de : été, bar, foo, x_1, let.
            assert_eq!(walk(b, text.len(), prev_w), vec![31, 25, 14, 10, 4, 0]);
            // Au milieu d'un mot : fin ou début de ce mot.
            assert_eq!(next_word(b, 11).unwrap(), 13);
            assert_eq!(prev_word(b, 11).unwrap(), 10);
        });
    }

    #[test]
    fn mots_avec_cjk_et_emojis() {
        let text = "日本語 😀 naïve";
        both(text, |b| {
            let forward = walk(b, 0, next_w);
            assert_eq!(*forward.last().unwrap(), text.len());
            assert!(forward.windows(2).all(|w| w[0] < w[1]));
            for &pos in &forward {
                assert!(text.is_char_boundary(pos));
            }
            assert_eq!(
                prev_word(b, text.len()).unwrap(),
                text.len() - "naïve".len()
            );
        });
    }

    #[test]
    fn lignes_et_document() {
        let text = "un\r\ndeux\ntrois";
        both(text, |b| {
            assert_eq!(line_start(b, 6).unwrap(), 4);
            assert_eq!(line_end(b, 6).unwrap(), 8);
            assert_eq!(line_end(b, 0).unwrap(), 2);
            assert_eq!(line_start(b, 14).unwrap(), 9);
            assert_eq!(line_end(b, 9).unwrap(), 14);
            assert_eq!(document_start(b), 0);
            assert_eq!(document_end(b), 14);
        });
    }

    #[test]
    fn tabulations() {
        both("\t\tx", |b| {
            assert_eq!(walk(b, 0, next_g), vec![0, 1, 2, 3]);
            assert_eq!(next_word(b, 0).unwrap(), 3);
        });
    }

    #[test]
    fn extremites_et_erreurs() {
        both("", |b| {
            assert_eq!(next_grapheme(b, 0).unwrap(), 0);
            assert_eq!(prev_grapheme(b, 0).unwrap(), 0);
            assert_eq!(next_word(b, 0).unwrap(), 0);
            assert_eq!(prev_word(b, 0).unwrap(), 0);
        });
        // Sans décalage : les positions des erreurs sont celles du buffer lui-même.
        for b in [
            &StringBuffer::from_text("é") as &dyn TextBuffer,
            &Rope::from_text("é"),
        ] {
            assert_eq!(
                next_grapheme(b, 1),
                Err(TextError::NotCharBoundary { offset: 1 })
            );
            assert_eq!(
                prev_grapheme(b, 3),
                Err(TextError::OffsetOutOfBounds { offset: 3, len: 2 })
            );
        }
    }
}
