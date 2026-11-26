//! Lecture du texte d'une rope par Tree-sitter, morceau par morceau, sans la copier.

use cupride_text::{Rope, TextBuffer};

/// Morceaux d'une rope et leur position, pour retrouver vite celui qui contient une position.
pub(crate) struct Source<'a> {
    chunks: Vec<(usize, &'a str)>,
    len: usize,
}

impl<'a> Source<'a> {
    pub(crate) fn new(text: &'a Rope) -> Self {
        let mut start = 0;
        let chunks = text
            .chunks()
            .map(|chunk| {
                let entry = (start, chunk);
                start += chunk.len();
                entry
            })
            .collect();
        Self { chunks, len: start }
    }

    /// Texte à partir de la position `byte`, jusqu'à la fin de son morceau ; vide à la fin du
    /// texte.
    pub(crate) fn at(&self, byte: usize) -> &'a [u8] {
        if byte >= self.len {
            return &[];
        }
        let index = self.chunks.partition_point(|(start, _)| *start <= byte) - 1;
        let (start, chunk) = self.chunks[index];
        &chunk.as_bytes()[byte - start..]
    }

    /// Texte de la plage `range`, par morceaux.
    pub(crate) fn slices(&self, range: std::ops::Range<usize>) -> Vec<&'a [u8]> {
        let mut slices = Vec::new();
        let mut at = range.start;
        let end = range.end.min(self.len);
        while at < end {
            let rest = self.at(at);
            let take = rest.len().min(end - at);
            slices.push(&rest[..take]);
            at += take;
        }
        slices
    }

    /// Texte de la plage `range`, copié.
    pub(crate) fn text(&self, range: std::ops::Range<usize>) -> String {
        String::from_utf8_lossy(&self.slices(range).concat()).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morceaux_retrouves_par_position() {
        let text: String = (0..2000).map(|i| format!("ligne {i}\n")).collect();
        let rope = Rope::from_text(&text);
        let source = Source::new(&rope);
        assert!(source.chunks.len() > 1, "plusieurs morceaux");
        for byte in [0, 1, 4095, 4096, 9000, text.len() - 1] {
            assert_eq!(source.at(byte)[0], text.as_bytes()[byte]);
        }
        assert!(source.at(text.len()).is_empty());
        let joined: Vec<u8> = source.slices(10..9000).concat();
        assert_eq!(joined, text.as_bytes()[10..9000]);
        assert_eq!(source.text(0..7), "ligne 0");
    }
}
