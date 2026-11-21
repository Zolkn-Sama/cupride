//! Focus clavier et survol de la souris.
//!
//! Le focus est une pile : ouvrir une couche (ligne de commande, palette) lui donne le focus, la
//! fermer le rend à l'élément d'avant. Le survol est l'élément sous la souris.

/// Focus et survol d'éléments identifiés par `T`.
#[derive(Debug, Clone)]
pub struct Focus<T> {
    stack: Vec<T>,
    hovered: Option<T>,
}

impl<T: Copy + Eq> Focus<T> {
    /// Focus sur `root`, l'élément de base (l'éditeur), qui ne se ferme jamais.
    pub fn new(root: T) -> Self {
        Self {
            stack: vec![root],
            hovered: None,
        }
    }

    /// Élément qui reçoit le clavier.
    pub fn current(&self) -> T {
        self.stack[self.stack.len() - 1]
    }

    /// `target` a-t-il le focus ?
    pub fn is_focused(&self, target: T) -> bool {
        self.current() == target
    }

    /// Donne le focus à `target`, par-dessus l'élément actuel. S'il était déjà dans la pile, il
    /// remonte en haut.
    pub fn push(&mut self, target: T) {
        if self.stack[0] == target {
            self.stack.truncate(1);
            return;
        }
        self.stack.retain(|t| *t != target);
        self.stack.push(target);
    }

    /// Retire `target` de la pile (couche fermée) ; le focus revient à l'élément d'en dessous.
    /// L'élément de base ne peut pas être retiré.
    pub fn remove(&mut self, target: T) {
        if self.stack.len() > 1 && self.stack[0] != target {
            self.stack.retain(|t| *t != target);
        }
        if self.hovered == Some(target) {
            self.hovered = None;
        }
    }

    /// Élément sous la souris.
    pub fn hovered(&self) -> Option<T> {
        self.hovered
    }

    /// Change l'élément sous la souris ; renvoie vrai s'il a changé (à redessiner).
    pub fn hover(&mut self, target: Option<T>) -> bool {
        let changed = self.hovered != target;
        self.hovered = target;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Element {
        Editor,
        CommandLine,
        Palette,
    }

    #[test]
    fn pile_de_focus() {
        let mut focus = Focus::new(Element::Editor);
        assert!(focus.is_focused(Element::Editor));
        focus.push(Element::CommandLine);
        focus.push(Element::Palette);
        assert_eq!(focus.current(), Element::Palette);
        focus.remove(Element::Palette);
        assert_eq!(focus.current(), Element::CommandLine);
        focus.push(Element::Palette);
        focus.push(Element::CommandLine);
        assert_eq!(focus.current(), Element::CommandLine, "remonte en haut");
        focus.remove(Element::CommandLine);
        focus.remove(Element::Palette);
        focus.remove(Element::Editor);
        assert_eq!(focus.current(), Element::Editor, "la base reste");
        focus.push(Element::Palette);
        focus.push(Element::Editor);
        assert_eq!(
            focus.current(),
            Element::Editor,
            "revenir à la base ferme le reste"
        );
    }

    #[test]
    fn survol() {
        let mut focus = Focus::new(Element::Editor);
        assert!(focus.hover(Some(Element::Palette)));
        assert!(!focus.hover(Some(Element::Palette)));
        focus.remove(Element::Palette);
        assert_eq!(focus.hovered(), None);
    }
}
