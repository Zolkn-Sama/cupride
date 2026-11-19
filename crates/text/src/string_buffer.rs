//! Implémentation de référence du buffer, sur une simple `String`.
//!
//! Lente sur les gros textes (chaque modification décale tout ce qui suit), mais évidente à
//! vérifier : elle sert de référence pour tester la [`Rope`](crate::Rope).

use std::ops::Range;

use crate::buffer::{TextBuffer, check_offset, check_range};
use crate::error::{Result, TextError};

/// Buffer de texte stocké dans une seule `String`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StringBuffer {
    text: String,
}

impl StringBuffer {
    /// Crée un buffer vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Texte complet du buffer.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl TextBuffer for StringBuffer {
    fn from_text(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }

    fn len_bytes(&self) -> usize {
        self.text.len()
    }

    fn len_lines(&self) -> usize {
        self.text.bytes().filter(|&b| b == b'\n').count() + 1
    }

    fn insert(&mut self, offset: usize, text: &str) -> Result<()> {
        check_offset(&self.text, offset)?;
        self.text.insert_str(offset, text);
        Ok(())
    }

    fn remove(&mut self, range: Range<usize>) -> Result<()> {
        check_range(&self.text, &range)?;
        self.text.replace_range(range, "");
        Ok(())
    }

    fn slice(&self, range: Range<usize>) -> Result<String> {
        check_range(&self.text, &range)?;
        Ok(self.text[range].to_owned())
    }

    fn byte_to_line(&self, offset: usize) -> Result<usize> {
        check_offset(&self.text, offset)?;
        Ok(self.text.as_bytes()[..offset]
            .iter()
            .filter(|&&b| b == b'\n')
            .count())
    }

    fn line_to_byte(&self, line: usize) -> Result<usize> {
        if line == 0 {
            return Ok(0);
        }
        self.text
            .match_indices('\n')
            .nth(line - 1)
            .map(|(pos, _)| pos + 1)
            .ok_or(TextError::LineOutOfBounds {
                line,
                count: self.len_lines(),
            })
    }

    fn chunks(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        Box::new(std::iter::once(self.text.as_str()))
    }

    fn to_text(&self) -> String {
        self.text.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::StringBuffer;

    crate::suite::buffer_suite!(StringBuffer);
}
