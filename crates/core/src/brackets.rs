//! Parenthèses, crochets et accolades : paire autour du curseur, et paires fermées
//! automatiquement.

use cupride_text::TextBuffer;

use crate::document::Document;

/// Distance maximale parcourue dans le texte pour trouver une parenthèse correspondante, quand
/// l'arbre syntaxique ne la donne pas.
pub const MAX_BRACKET_SCAN: usize = 64 * 1024;

/// Paires ouvrant et fermant toutes seules dans le langage `language` (`None` : texte brut).
pub fn pairs(language: Option<&str>) -> &'static [(char, char)] {
    const TEXT: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];
    // Rust : l'apostrophe sert aux durées de vie (`'a`) et aux caractères.
    const RUST: &[(char, char)] = TEXT;
    const MARKDOWN: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('`', '`')];
    const CODE: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];
    match language {
        None | Some("Texte") => TEXT,
        Some("Rust") => RUST,
        Some("Markdown") => MARKDOWN,
        Some(_) => CODE,
    }
}

/// Caractère qui commence à `offset`.
pub(crate) fn char_at(buffer: &impl TextBuffer, offset: usize) -> Option<char> {
    let end = (offset + 4).min(buffer.len_bytes());
    let text = buffer.slice(offset..end).ok()?;
    text.chars().next()
}

/// Caractère qui finit à `offset`, et sa position.
pub(crate) fn char_before(buffer: &impl TextBuffer, offset: usize) -> Option<(usize, char)> {
    let start = offset.saturating_sub(4);
    (start..offset).find_map(|from| {
        let text = buffer.slice(from..offset).ok()?;
        let mut chars = text.chars();
        let c = chars.next()?;
        chars.next().is_none().then_some((from, c))
    })
}

/// Parenthèse à côté du curseur `head` (juste après, sinon juste avant) et sa correspondante :
/// leurs positions. D'après l'arbre syntaxique quand le caractère en est une parenthèse, sinon
/// (texte brut, chaîne, commentaire) en comptant les parenthèses du texte, sur au plus
/// [`MAX_BRACKET_SCAN`] octets.
pub fn bracket_pair(document: &Document, head: usize) -> Option<(usize, usize)> {
    let buffer = document.buffer();
    let after = char_at(buffer, head).map(|c| (head, c));
    let before = char_before(buffer, head);
    [after, before].into_iter().flatten().find_map(|(at, c)| {
        cupride_syntax::partner(c)?;
        let tree = document
            .syntax()
            .filter(|s| s.is_fresh())
            .and_then(|s| s.matching_bracket(at));
        let partner = match tree {
            Some(found) => found,
            None => scan(buffer, at, c),
        };
        partner.map(|p| (at, p))
    })
}

/// Parenthèse correspondante de `c` (à `at`), en comptant les parenthèses du texte.
fn scan(buffer: &impl TextBuffer, at: usize, c: char) -> Option<usize> {
    let partner = cupride_syntax::partner(c)?;
    let opening = matches!(c, '(' | '[' | '{');
    let mut depth = 0usize;
    if opening {
        let start = at + c.len_utf8();
        let end = (start + MAX_BRACKET_SCAN).min(buffer.len_bytes());
        let text = buffer.slice(start..end).ok()?;
        for (index, ch) in text.char_indices() {
            if ch == c {
                depth += 1;
            } else if ch == partner {
                if depth == 0 {
                    return Some(start + index);
                }
                depth -= 1;
            }
        }
    } else {
        let start = at.saturating_sub(MAX_BRACKET_SCAN);
        // Début sur une frontière de caractère.
        let start = (start..at).find(|&s| buffer.slice(s..at).is_ok())?;
        let text = buffer.slice(start..at).ok()?;
        for (index, ch) in text.char_indices().rev() {
            if ch == c {
                depth += 1;
            } else if ch == partner {
                if depth == 0 {
                    return Some(start + index);
                }
                depth -= 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(text: &str, head: usize) -> Option<(usize, usize)> {
        bracket_pair(&Document::from_text(text), head)
    }

    #[test]
    fn paire_dans_le_texte() {
        let text = "f(a, (b), [c]) {}";
        assert_eq!(pair(text, 1), Some((1, 13)), "curseur avant « ( »");
        assert_eq!(pair(text, 14), Some((13, 1)), "curseur après « ) »");
        assert_eq!(pair(text, 5), Some((5, 7)));
        assert_eq!(pair(text, 10), Some((10, 12)), "crochets");
        assert_eq!(pair(text, 3), None, "pas de parenthèse à côté");
        assert_eq!(pair("(((", 0), None, "pas de partenaire");
        assert_eq!(
            pair("é(é)", 2),
            Some((2, 5)),
            "caractères sur plusieurs octets"
        );
    }

    #[test]
    fn paire_d_apres_l_arbre() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        // La parenthèse de la chaîne ne ferme pas l'appel.
        std::fs::write(&path, "g(\")\", 1)").unwrap();
        let mut document = Document::open(&path).unwrap();
        document.start_syntax();
        assert_eq!(bracket_pair(&document, 1), Some((1, 8)));
        // Dans la chaîne : comptée comme du texte.
        assert_eq!(bracket_pair(&document, 3), Some((3, 1)));
    }

    #[test]
    fn paires_par_langage() {
        assert!(pairs(Some("C")).contains(&('\'', '\'')));
        assert!(!pairs(Some("Rust")).contains(&('\'', '\'')));
        assert!(pairs(Some("Markdown")).contains(&('`', '`')));
        assert!(!pairs(None).contains(&('\'', '\'')), "apostrophes du texte");
    }
}
