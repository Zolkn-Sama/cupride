//! Champ de saisie d'une ligne : ligne de commande, palette, ouverture rapide.
//!
//! Déplacements par graphème et par mot, sélection avec Maj, suppression, et presse-papiers
//! (demandé à l'appelant, qui a accès au presse-papiers du système).

use cupride_core::keymap::{Key, KeyCombo, NamedKey, Platform};
use cupride_core::text::movement;
use cupride_core::text::{StringBuffer, TextBuffer};

/// Résultat d'une touche passée au champ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    /// Touche traitée : le texte ou le curseur ont pu changer.
    Handled,
    /// Entrée : valider.
    Submit,
    /// Échap : fermer.
    Cancel,
    /// Flèche haut.
    Up,
    /// Flèche bas.
    Down,
    /// Tabulation : compléter.
    Complete,
    /// Copier ce texte dans le presse-papiers.
    Copy(String),
    /// Coller : l'appelant lit le presse-papiers et appelle [`TextInput::insert`].
    Paste,
    /// Touche sans effet ici : à passer au keymap.
    Ignored,
}

/// Texte d'une ligne, curseur et sélection (positions en octets).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    text: String,
    cursor: usize,
    anchor: usize,
}

impl TextInput {
    /// Champ contenant `text`, curseur à la fin.
    pub fn with_text(text: &str) -> Self {
        let mut input = Self::default();
        input.set_text(text);
        input
    }

    /// Texte saisi.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Position du curseur.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Plage sélectionnée (vide sans sélection).
    pub fn selection(&self) -> std::ops::Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    /// Remplace tout le texte ; curseur à la fin.
    pub fn set_text(&mut self, text: &str) {
        // Une ligne : les sauts de ligne collés deviennent des espaces.
        self.text = text.replace(['\r', '\n'], " ");
        self.cursor = self.text.len();
        self.anchor = self.cursor;
    }

    /// Sélectionne tout le texte : la prochaine frappe le remplace.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// Insère `text` au curseur, à la place de la sélection.
    pub fn insert(&mut self, text: &str) {
        let text = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
        let range = self.selection();
        self.text.replace_range(range.clone(), &text);
        self.cursor = range.start + text.len();
        self.anchor = self.cursor;
    }

    /// Remplace la plage `range` par `text` ; le curseur se place après.
    pub fn replace(&mut self, range: std::ops::Range<usize>, text: &str) {
        let end = range.end.min(self.text.len());
        let start = range.start.min(end);
        self.text.replace_range(start..end, text);
        self.cursor = start + text.len();
        self.anchor = self.cursor;
    }

    fn buffer(&self) -> StringBuffer {
        StringBuffer::from_text(&self.text)
    }

    fn move_to(&mut self, position: usize, extend: bool) {
        self.cursor = position.min(self.text.len());
        if !extend {
            self.anchor = self.cursor;
        }
    }

    /// Supprime la sélection, ou la plage `range` si rien n'est sélectionné.
    fn delete(&mut self, range: std::ops::Range<usize>) {
        let range = if self.cursor == self.anchor {
            range
        } else {
            self.selection()
        };
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = range.start;
    }

    /// Traite une touche. `platform` choisit les modificateurs : mot par mot avec Alt sous macOS,
    /// Ctrl ailleurs ; Cmd ou Ctrl pour le presse-papiers.
    pub fn key(&mut self, combo: KeyCombo, platform: Platform) -> InputEvent {
        let m = combo.modifiers;
        let primary = match platform {
            Platform::MacOs => m.meta,
            Platform::Other => m.ctrl,
        };
        let word = match platform {
            Platform::MacOs => m.alt,
            Platform::Other => m.ctrl,
        };
        let extend = m.shift;
        let buffer = self.buffer();
        let cursor = self.cursor;
        let word_left = || movement::prev_word(&buffer, cursor).unwrap_or(0);
        let word_right = || movement::next_word(&buffer, cursor).unwrap_or(cursor);
        match combo.key {
            Key::Named(NamedKey::Enter) => return InputEvent::Submit,
            Key::Named(NamedKey::Escape) => return InputEvent::Cancel,
            Key::Named(NamedKey::Up) => return InputEvent::Up,
            Key::Named(NamedKey::Down) => return InputEvent::Down,
            Key::Named(NamedKey::Tab) => return InputEvent::Complete,
            Key::Named(NamedKey::Left) => {
                let target = if word {
                    word_left()
                } else if primary {
                    0
                } else if !extend && self.cursor != self.anchor {
                    self.selection().start
                } else {
                    movement::prev_grapheme(&buffer, cursor).unwrap_or(0)
                };
                self.move_to(target, extend);
            }
            Key::Named(NamedKey::Right) => {
                let target = if word {
                    word_right()
                } else if primary {
                    self.text.len()
                } else if !extend && self.cursor != self.anchor {
                    self.selection().end
                } else {
                    movement::next_grapheme(&buffer, cursor).unwrap_or(cursor)
                };
                self.move_to(target, extend);
            }
            Key::Named(NamedKey::Home) => self.move_to(0, extend),
            Key::Named(NamedKey::End) => self.move_to(self.text.len(), extend),
            Key::Named(NamedKey::Backspace) => {
                let start = if word {
                    word_left()
                } else if primary {
                    0
                } else {
                    movement::prev_grapheme(&buffer, cursor).unwrap_or(0)
                };
                self.delete(start..cursor);
            }
            Key::Named(NamedKey::Delete) => {
                let end = if word {
                    word_right()
                } else {
                    movement::next_grapheme(&buffer, cursor).unwrap_or(cursor)
                };
                self.delete(cursor..end);
            }
            Key::Char('a') if primary => {
                self.anchor = 0;
                self.cursor = self.text.len();
            }
            Key::Char('c' | 'x') if primary => {
                let selected = self.text[self.selection()].to_owned();
                if combo.key == Key::Char('x') {
                    self.delete(cursor..cursor);
                }
                return if selected.is_empty() {
                    InputEvent::Handled
                } else {
                    InputEvent::Copy(selected)
                };
            }
            Key::Char('v') if primary => return InputEvent::Paste,
            _ => return InputEvent::Ignored,
        }
        InputEvent::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(input: &mut TextInput, keys: &str) -> InputEvent {
        let mut last = InputEvent::Ignored;
        for key in keys.split_whitespace() {
            last = input.key(
                KeyCombo::parse(key, Platform::Other).unwrap(),
                Platform::Other,
            );
        }
        last
    }

    #[test]
    fn edition() {
        let mut input = TextInput::default();
        input.insert("file open");
        assert_eq!(input.cursor(), 9);
        press(&mut input, "ctrl+left");
        assert_eq!(input.cursor(), 5, "début du mot");
        input.insert("x ");
        assert_eq!(input.text(), "file x open");
        press(
            &mut input,
            "home shift+right shift+right shift+right shift+right",
        );
        assert_eq!(input.selection(), 0..4);
        input.insert("edit");
        assert_eq!(input.text(), "edit x open");
        press(&mut input, "end ctrl+backspace");
        assert_eq!(input.text(), "edit x ");
        press(&mut input, "backspace backspace");
        assert_eq!(input.text(), "edit ");
        press(&mut input, "home delete");
        assert_eq!(input.text(), "dit ");
    }

    #[test]
    fn graphemes_et_lignes() {
        let mut input = TextInput::with_text("a👨‍👩‍👧b");
        press(&mut input, "left backspace");
        assert_eq!(input.text(), "ab", "un emoji composé s'efface d'un coup");
        input.insert("x\ny");
        assert_eq!(input.text(), "ax yb", "une seule ligne");
    }

    #[test]
    fn touches_speciales_et_presse_papiers() {
        let mut input = TextInput::with_text("bonjour");
        assert_eq!(press(&mut input, "enter"), InputEvent::Submit);
        assert_eq!(press(&mut input, "escape"), InputEvent::Cancel);
        assert_eq!(press(&mut input, "tab"), InputEvent::Complete);
        assert_eq!(press(&mut input, "up"), InputEvent::Up);
        assert_eq!(
            press(&mut input, "ctrl+a ctrl+x"),
            InputEvent::Copy("bonjour".into())
        );
        assert_eq!(input.text(), "", "couper vide le champ");
        assert_eq!(press(&mut input, "ctrl+v"), InputEvent::Paste);
        assert_eq!(press(&mut input, "f5"), InputEvent::Ignored);
        // macOS : Alt pour les mots, Cmd pour le presse-papiers.
        let mut input = TextInput::with_text("un deux");
        let alt_left = KeyCombo::parse("alt+left", Platform::MacOs).unwrap();
        input.key(alt_left, Platform::MacOs);
        assert_eq!(input.cursor(), 3);
        let cmd_v = KeyCombo::parse("cmd+v", Platform::MacOs).unwrap();
        assert_eq!(input.key(cmd_v, Platform::MacOs), InputEvent::Paste);
    }
}
