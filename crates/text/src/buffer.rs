//! API commune des buffers de texte.

use std::ops::Range;

use crate::error::Result;

/// Buffer de texte UTF-8 modifiable.
///
/// Conventions communes à toutes les implémentations :
///
/// - Les positions (*offsets*) sont en **octets UTF-8**, de `0` à `len_bytes()` inclus, et doivent
///   tomber sur une frontière de caractère.
/// - Une ligne se termine par `\n` ; un `\r\n` compte pour **un seul** saut de ligne. Un texte de
///   `n` sauts de ligne a donc `n + 1` lignes : un texte vide a une ligne, et un texte terminé par
///   `\n` a une dernière ligne vide.
/// - Les lignes sont numérotées à partir de 0.
pub trait TextBuffer {
    /// Crée un buffer contenant `text`.
    fn from_text(text: &str) -> Self
    where
        Self: Sized;

    /// Longueur du texte, en octets.
    fn len_bytes(&self) -> usize;

    /// Indique si le texte est vide.
    fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    /// Nombre de lignes : nombre de sauts de ligne plus un.
    fn len_lines(&self) -> usize;

    /// Insère `text` à la position `offset`.
    fn insert(&mut self, offset: usize, text: &str) -> Result<()>;

    /// Supprime le texte de la plage `range`.
    fn remove(&mut self, range: Range<usize>) -> Result<()>;

    /// Copie le texte de la plage `range`.
    fn slice(&self, range: Range<usize>) -> Result<String>;

    /// Ligne contenant la position `offset`. Une position juste après un `\n` appartient à la
    /// ligne suivante.
    fn byte_to_line(&self, offset: usize) -> Result<usize>;

    /// Position du premier octet de la ligne `line`.
    fn line_to_byte(&self, line: usize) -> Result<usize>;

    /// Morceaux successifs du texte, dans l'ordre. Leur découpage dépend de l'implémentation ;
    /// aucun morceau ne coupe un caractère.
    fn chunks(&self) -> Box<dyn Iterator<Item = &str> + '_>;

    /// Caractères du texte, dans l'ordre.
    fn chars(&self) -> Box<dyn Iterator<Item = char> + '_> {
        Box::new(self.chunks().flat_map(str::chars))
    }

    /// Lignes du texte, sans leur fin de ligne (`\n` ou `\r\n`).
    fn lines(&self) -> Lines<'_> {
        Lines {
            chunks: self.chunks(),
            current: String::new(),
            done: false,
        }
    }

    /// Plage de la ligne `line`, fin de ligne comprise.
    fn line_range(&self, line: usize) -> Result<Range<usize>> {
        let start = self.line_to_byte(line)?;
        let end = if line + 1 < self.len_lines() {
            self.line_to_byte(line + 1)?
        } else {
            self.len_bytes()
        };
        Ok(start..end)
    }

    /// Plage du contenu de la ligne `line`, sans sa fin de ligne.
    fn line_content_range(&self, line: usize) -> Result<Range<usize>> {
        let range = self.line_range(line)?;
        let text = self.slice(range.clone())?;
        let content_len = strip_line_ending(&text).len();
        Ok(range.start..range.start + content_len)
    }

    /// Contenu de la ligne `line`, sans sa fin de ligne.
    fn line(&self, line: usize) -> Result<String> {
        let mut text = self.slice(self.line_range(line)?)?;
        let content_len = strip_line_ending(&text).len();
        text.truncate(content_len);
        Ok(text)
    }

    /// Copie de tout le texte.
    fn to_text(&self) -> String {
        self.chunks().collect()
    }
}

/// Retire la fin de ligne (`\n` ou `\r\n`) à la fin de `text`, s'il y en a une.
pub(crate) fn strip_line_ending(text: &str) -> &str {
    match text.strip_suffix('\n') {
        Some(rest) => rest.strip_suffix('\r').unwrap_or(rest),
        None => text,
    }
}

/// Itérateur sur les lignes d'un buffer, sans leur fin de ligne. Voir [`TextBuffer::lines`].
pub struct Lines<'a> {
    chunks: Box<dyn Iterator<Item = &'a str> + 'a>,
    current: String,
    done: bool,
}

impl std::fmt::Debug for Lines<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lines")
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl Iterator for Lines<'_> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        if self.done {
            return None;
        }
        loop {
            if let Some(pos) = self.current.find('\n') {
                let rest = self.current.split_off(pos + 1);
                let mut line = std::mem::replace(&mut self.current, rest);
                let content_len = strip_line_ending(&line).len();
                line.truncate(content_len);
                return Some(line);
            }
            match self.chunks.next() {
                Some(chunk) => self.current.push_str(chunk),
                None => {
                    // Dernière ligne : toujours présente, même vide.
                    self.done = true;
                    return Some(std::mem::take(&mut self.current));
                }
            }
        }
    }
}

/// Vérifie qu'une position est dans le texte et sur une frontière de caractère.
pub(crate) fn check_offset(text: &str, offset: usize) -> Result<()> {
    use crate::error::TextError;
    if offset > text.len() {
        return Err(TextError::OffsetOutOfBounds {
            offset,
            len: text.len(),
        });
    }
    if !text.is_char_boundary(offset) {
        return Err(TextError::NotCharBoundary { offset });
    }
    Ok(())
}

/// Vérifie qu'une plage est ordonnée, dans le texte et sur des frontières de caractère.
pub(crate) fn check_range(text: &str, range: &Range<usize>) -> Result<()> {
    use crate::error::TextError;
    if range.start > range.end || range.end > text.len() {
        return Err(TextError::InvalidRange {
            start: range.start,
            end: range.end,
            len: text.len(),
        });
    }
    for offset in [range.start, range.end] {
        if !text.is_char_boundary(offset) {
            return Err(TextError::NotCharBoundary { offset });
        }
    }
    Ok(())
}
