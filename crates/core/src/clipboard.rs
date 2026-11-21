//! Presse-papiers.
//!
//! Le noyau ne parle pas au système : il passe par le trait [`Clipboard`], que l'application
//! implémente avec le presse-papiers de l'OS. Par défaut, l'éditeur utilise un
//! [`MemoryClipboard`], interne au processus (tests, REPL).
//!
//! Avec plusieurs curseurs, `edit copy` copie un fragment par sélection, séparés par des fins de
//! ligne. `edit paste` redistribue un fragment par curseur quand leurs nombres correspondent :
//! fragments de la dernière copie faite dans l'IDE, ou sinon lignes du texte collé.

use std::fmt;

/// Presse-papiers visé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardKind {
    /// Presse-papiers habituel : copier, couper, coller.
    Standard,
    /// Sélection primaire de X11 et Wayland : le texte sélectionné, collé au clic du milieu.
    Primary,
}

/// Accès à un presse-papiers.
pub trait Clipboard: fmt::Debug + Send {
    /// Texte du presse-papiers `kind`, ou `None` s'il est vide, ne contient pas de texte, ou
    /// n'existe pas sur cet OS.
    fn get(&mut self, kind: ClipboardKind) -> Option<String>;

    /// Remplace le texte du presse-papiers `kind`. Renvoie une erreur lisible en cas d'échec.
    fn set(&mut self, kind: ClipboardKind, text: String) -> Result<(), String>;

    /// La sélection primaire existe-t-elle (X11, Wayland) ?
    fn has_primary(&self) -> bool;
}

/// Presse-papiers en mémoire, propre au processus, avec une sélection primaire.
#[derive(Debug, Default, Clone)]
pub struct MemoryClipboard {
    standard: Option<String>,
    primary: Option<String>,
}

impl Clipboard for MemoryClipboard {
    fn get(&mut self, kind: ClipboardKind) -> Option<String> {
        match kind {
            ClipboardKind::Standard => self.standard.clone(),
            ClipboardKind::Primary => self.primary.clone(),
        }
    }

    fn set(&mut self, kind: ClipboardKind, text: String) -> Result<(), String> {
        match kind {
            ClipboardKind::Standard => self.standard = Some(text),
            ClipboardKind::Primary => self.primary = Some(text),
        }
        Ok(())
    }

    fn has_primary(&self) -> bool {
        true
    }
}

/// Dernière copie faite dans l'IDE : le texte envoyé au presse-papiers et ses fragments, un par
/// sélection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LastCopy {
    pub(crate) text: String,
    pub(crate) fragments: Vec<String>,
}

/// Fragments à coller, un par curseur, ou `None` pour coller tout le texte à chaque curseur.
///
/// - Le texte est celui de la dernière copie de l'IDE, avec autant de fragments que de curseurs :
///   ces fragments (ils peuvent contenir des fins de ligne).
/// - Sinon, avec plusieurs curseurs et autant de lignes que de curseurs : une ligne par curseur.
pub(crate) fn split_for_cursors(
    text: &str,
    cursors: usize,
    last_copy: Option<&LastCopy>,
) -> Option<Vec<String>> {
    if cursors < 2 {
        return None;
    }
    if let Some(copy) = last_copy
        && copy.text == text
        && copy.fragments.len() == cursors
    {
        return Some(copy.fragments.clone());
    }
    let lines: Vec<String> = text
        .strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();
    (lines.len() == cursors).then_some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repartition_par_curseur() {
        assert_eq!(split_for_cursors("a\nb", 1, None), None, "un seul curseur");
        assert_eq!(
            split_for_cursors("a\nb\nc", 3, None),
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
        assert_eq!(
            split_for_cursors("a\r\nb\r\n", 2, None),
            Some(vec!["a".into(), "b".into()]),
            "CRLF et fin de ligne finale"
        );
        assert_eq!(
            split_for_cursors("a\nb", 3, None),
            None,
            "nombres différents"
        );
        // Fragments multi-lignes de la dernière copie.
        let copy = LastCopy {
            text: "un\ndeux\ntrois".into(),
            fragments: vec!["un\ndeux".into(), "trois".into()],
        };
        assert_eq!(
            split_for_cursors("un\ndeux\ntrois", 2, Some(&copy)),
            Some(vec!["un\ndeux".into(), "trois".into()])
        );
        assert_eq!(
            split_for_cursors("autre\ntexte", 2, Some(&copy)),
            Some(vec!["autre".into(), "texte".into()]),
            "texte copié ailleurs : par lignes"
        );
    }
}
