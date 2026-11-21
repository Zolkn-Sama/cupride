//! Clavier : conversion des touches `winit` vers les combinaisons du keymap, et texte à insérer.
//!
//! Une touche suit l'un de ces chemins :
//!
//! - texte imprimable tapé sans Ctrl ni Cmd (lettres, chiffres, ponctuation, accents composés par
//!   une touche morte, AltGr sous Windows) : inséré directement avec `edit insert` ;
//! - toute autre touche (flèches, Entrée, Ctrl+…, Cmd+…) : passée au keymap, qui la traduit en
//!   ligne de commande ;
//! - pendant une séquence (`ctrl+k ctrl+w`), toutes les touches vont au keymap.
//!
//! Les méthodes de saisie (chinois, japonais, coréen…) passent par les événements `Ime` de
//! `winit` : le texte en cours de composition est affiché au curseur, puis inséré une fois validé.

use cupride_core::keymap::{Key, KeyCombo, Modifiers, NamedKey};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey as WinitNamed};

/// Modificateurs du keymap à partir de ceux de `winit`.
pub(crate) fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        ctrl: state.control_key(),
        alt: state.alt_key(),
        shift: state.shift_key(),
        meta: state.super_key(),
    }
}

/// Combinaison du keymap pour une touche. `logical` est la touche produite (avec les
/// modificateurs), `base` la même touche sans les modificateurs, d'après la disposition du
/// clavier : `ctrl+shift+z` donne `z`, pas `Z`. Les modificateurs seuls ne donnent rien.
pub(crate) fn combo(
    logical: &WinitKey,
    base: &WinitKey,
    state: ModifiersState,
) -> Option<KeyCombo> {
    let key = match logical {
        WinitKey::Named(named) => named_key(*named)?,
        // Touche morte (accent en attente) : rien tant que la composition n'est pas finie.
        WinitKey::Dead(_) => return None,
        _ => match base {
            WinitKey::Character(text) => char_key(text)?,
            WinitKey::Named(named) => named_key(*named)?,
            _ => return None,
        },
    };
    Some(KeyCombo {
        modifiers: modifiers(state),
        key,
    })
}

/// Touche caractère, en minuscule ; l'espace est une touche nommée.
fn char_key(text: &str) -> Option<Key> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if c == ' ' {
        return Some(Key::Named(NamedKey::Space));
    }
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(lower), None) => Some(Key::Char(lower)),
        _ => Some(Key::Char(c)),
    }
}

/// Touche nommée du keymap.
fn named_key(named: WinitNamed) -> Option<Key> {
    use WinitNamed as N;
    let key = match named {
        N::Enter => NamedKey::Enter,
        N::Tab => NamedKey::Tab,
        N::Backspace => NamedKey::Backspace,
        N::Delete => NamedKey::Delete,
        N::Escape => NamedKey::Escape,
        N::Space => NamedKey::Space,
        N::ArrowLeft => NamedKey::Left,
        N::ArrowRight => NamedKey::Right,
        N::ArrowUp => NamedKey::Up,
        N::ArrowDown => NamedKey::Down,
        N::Home => NamedKey::Home,
        N::End => NamedKey::End,
        N::PageUp => NamedKey::PageUp,
        N::PageDown => NamedKey::PageDown,
        N::Insert => NamedKey::Insert,
        other => return function_key(other).map(Key::F),
    };
    Some(Key::Named(key))
}

/// Numéro d'une touche de fonction `F1` à `F24`.
fn function_key(named: WinitNamed) -> Option<u8> {
    use WinitNamed as N;
    const KEYS: [WinitNamed; 24] = [
        N::F1,
        N::F2,
        N::F3,
        N::F4,
        N::F5,
        N::F6,
        N::F7,
        N::F8,
        N::F9,
        N::F10,
        N::F11,
        N::F12,
        N::F13,
        N::F14,
        N::F15,
        N::F16,
        N::F17,
        N::F18,
        N::F19,
        N::F20,
        N::F21,
        N::F22,
        N::F23,
        N::F24,
    ];
    KEYS.iter()
        .position(|k| *k == named)
        .and_then(|i| u8::try_from(i + 1).ok())
}

/// Texte à insérer directement pour une touche, ou `None` si elle doit passer par le keymap.
///
/// Ctrl ou Cmd enfoncé : raccourci, sauf Ctrl+Alt, qui est AltGr sous Windows et produit des
/// caractères (`€`, `@`…). Les caractères de contrôle (Entrée, Retour arrière, Échap) ne sont
/// jamais insérés tels quels : ils passent par le keymap.
pub(crate) fn text_to_insert(text: Option<&str>, state: ModifiersState) -> Option<&str> {
    let text = text?;
    let alt_gr = state.control_key() && state.alt_key();
    if state.super_key() || (state.control_key() && !alt_gr) {
        return None;
    }
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cupride_core::keymap::Platform;

    fn ch(c: &str) -> WinitKey {
        WinitKey::Character(c.into())
    }

    fn parsed(text: &str) -> KeyCombo {
        KeyCombo::parse(text, Platform::Other).unwrap()
    }

    #[test]
    fn combinaisons() {
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(
            combo(&ch("Z"), &ch("z"), ctrl_shift),
            Some(parsed("ctrl+shift+z"))
        );
        assert_eq!(
            combo(
                &WinitKey::Named(WinitNamed::ArrowLeft),
                &WinitKey::Named(WinitNamed::ArrowLeft),
                ModifiersState::SHIFT
            ),
            Some(parsed("shift+left"))
        );
        assert_eq!(
            combo(
                &WinitKey::Named(WinitNamed::F5),
                &WinitKey::Named(WinitNamed::F5),
                ModifiersState::empty()
            ),
            Some(parsed("f5"))
        );
        assert_eq!(
            combo(&ch(" "), &ch(" "), ModifiersState::CONTROL),
            Some(parsed("ctrl+space"))
        );
        // Option+e sous macOS : touche morte, rien à faire avant la lettre suivante.
        assert_eq!(
            combo(&WinitKey::Dead(Some('´')), &ch("e"), ModifiersState::ALT),
            None
        );
        // Modificateur seul.
        assert_eq!(
            combo(
                &WinitKey::Named(WinitNamed::Shift),
                &WinitKey::Named(WinitNamed::Shift),
                ModifiersState::SHIFT
            ),
            None
        );
        let cmd_k = combo(&ch("k"), &ch("k"), ModifiersState::SUPER).unwrap();
        assert_eq!(cmd_k, KeyCombo::parse("cmd+k", Platform::MacOs).unwrap());
    }

    #[test]
    fn texte_insere_directement() {
        let none = ModifiersState::empty();
        assert_eq!(text_to_insert(Some("a"), none), Some("a"));
        assert_eq!(text_to_insert(Some("É"), ModifiersState::SHIFT), Some("É"));
        assert_eq!(
            text_to_insert(Some("é"), none),
            Some("é"),
            "touche morte composée"
        );
        assert_eq!(
            text_to_insert(Some("€"), ModifiersState::CONTROL | ModifiersState::ALT),
            Some("€"),
            "AltGr sous Windows"
        );
        assert_eq!(
            text_to_insert(Some("@"), ModifiersState::ALT),
            Some("@"),
            "Option sous macOS"
        );
        assert_eq!(text_to_insert(Some("s"), ModifiersState::CONTROL), None);
        assert_eq!(text_to_insert(Some("s"), ModifiersState::SUPER), None);
        assert_eq!(text_to_insert(Some("\r"), none), None, "Entrée : keymap");
        assert_eq!(
            text_to_insert(Some("\u{8}"), none),
            None,
            "retour arrière : keymap"
        );
        assert_eq!(
            text_to_insert(Some("\t"), none),
            None,
            "tabulation : keymap"
        );
        assert_eq!(text_to_insert(None, none), None);
    }
}
