//! Keymap : associe des combinaisons de touches à des lignes de commande.
//!
//! Un raccourci n'est qu'une commande pré-remplie : le keymap ne contient aucune logique. La
//! syntaxe des touches est celle des fichiers de configuration :
//!
//! - une combinaison : modificateurs puis touche, séparés par `+` : `ctrl+alt+t`, `shift+f5` ;
//! - une séquence : plusieurs combinaisons séparées par des espaces : `ctrl+k ctrl+c` ;
//! - modificateurs : `ctrl`, `alt`, `shift`, `meta` (alias `cmd`, `super`, `win`) ;
//! - `mod` : le modificateur principal de l'OS, `ctrl` sous Windows et Linux, `cmd` sous macOS ;
//! - touches spéciales : `enter`, `tab`, `backspace`, `delete`, `escape`, `space`, `left`,
//!   `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown`, `insert`, `f1` à `f24`, et
//!   `plus` pour la touche `+`.
//!
//! Les caractères imprimables tapés sans modificateur ne passent pas par le keymap : l'interface
//! les insère directement avec `edit insert`.

use std::fmt;

use thiserror::Error;

/// OS pour lequel les raccourcis sont interprétés (alias `mod`, raccourcis par défaut).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS : `mod` = `cmd`.
    MacOs,
    /// Windows et Linux : `mod` = `ctrl`.
    Other,
}

impl Platform {
    /// OS courant.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other
        }
    }
}

/// Modificateurs enfoncés.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Contrôle.
    pub ctrl: bool,
    /// Alt (Option sous macOS).
    pub alt: bool,
    /// Majuscule.
    pub shift: bool,
    /// Commande sous macOS, touche Windows ou Super ailleurs.
    pub meta: bool,
}

impl Modifiers {
    /// Aucun modificateur.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    /// Indique si aucun modificateur n'est enfoncé.
    pub fn is_empty(self) -> bool {
        self == Self::NONE
    }
}

/// Touche nommée (non imprimable, ou espace).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum NamedKey {
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Space,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
}

const NAMED_KEYS: [(NamedKey, &str, &[&str]); 15] = [
    (NamedKey::Enter, "enter", &["return"]),
    (NamedKey::Tab, "tab", &[]),
    (NamedKey::Backspace, "backspace", &[]),
    (NamedKey::Delete, "delete", &["del"]),
    (NamedKey::Escape, "escape", &["esc"]),
    (NamedKey::Space, "space", &[]),
    (NamedKey::Left, "left", &[]),
    (NamedKey::Right, "right", &[]),
    (NamedKey::Up, "up", &[]),
    (NamedKey::Down, "down", &[]),
    (NamedKey::Home, "home", &[]),
    (NamedKey::End, "end", &[]),
    (NamedKey::PageUp, "pageup", &["pgup"]),
    (NamedKey::PageDown, "pagedown", &["pgdn"]),
    (NamedKey::Insert, "insert", &["ins"]),
];

/// Une touche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Touche de caractère, en minuscule : `a`, `1`, `/`, `+`.
    Char(char),
    /// Touche nommée.
    Named(NamedKey),
    /// Touche de fonction `F1` à `F24`.
    F(u8),
}

/// Une combinaison : des modificateurs et une touche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    /// Modificateurs.
    pub modifiers: Modifiers,
    /// Touche.
    pub key: Key,
}

/// Erreur d'analyse d'un raccourci.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyParseError {
    /// Raccourci vide, ou `+` sans touche.
    #[error("raccourci vide")]
    Empty,
    /// Nom de touche inconnu.
    #[error("touche inconnue : « {0} »")]
    UnknownKey(String),
    /// Nom de modificateur inconnu.
    #[error("modificateur inconnu : « {0} »")]
    UnknownModifier(String),
    /// Modificateur répété.
    #[error("modificateur répété : « {0} »")]
    DuplicateModifier(String),
}

impl KeyCombo {
    /// Combinaison sans modificateur.
    pub fn new(key: Key) -> Self {
        Self {
            modifiers: Modifiers::NONE,
            key,
        }
    }

    /// Analyse une combinaison : `ctrl+alt+t`, `mod+s`, `shift+f5`.
    pub fn parse(text: &str, platform: Platform) -> Result<Self, KeyParseError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(KeyParseError::Empty);
        }
        // `+` seul, ou en fin (`ctrl++`), désigne la touche `+`.
        let (mods, key) = if text == "+" {
            ("", "+")
        } else if let Some(prefix) = text.strip_suffix("++") {
            (prefix, "+")
        } else {
            match text.rsplit_once('+') {
                Some((mods, key)) => (mods, key),
                None => ("", text),
            }
        };
        if key.is_empty() {
            return Err(KeyParseError::Empty);
        }
        let mut modifiers = Modifiers::NONE;
        for name in mods.split('+').filter(|m| !m.is_empty()) {
            let lower = name.to_lowercase();
            let slot = match lower.as_str() {
                "ctrl" | "control" => &mut modifiers.ctrl,
                "alt" | "option" | "opt" => &mut modifiers.alt,
                "shift" => &mut modifiers.shift,
                "meta" | "cmd" | "command" | "super" | "win" => &mut modifiers.meta,
                "mod" => match platform {
                    Platform::MacOs => &mut modifiers.meta,
                    Platform::Other => &mut modifiers.ctrl,
                },
                _ => return Err(KeyParseError::UnknownModifier(name.to_owned())),
            };
            if *slot {
                return Err(KeyParseError::DuplicateModifier(name.to_owned()));
            }
            *slot = true;
        }
        let key = parse_key(key, &mut modifiers)?;
        Ok(Self { modifiers, key })
    }
}

fn parse_key(name: &str, modifiers: &mut Modifiers) -> Result<Key, KeyParseError> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        // Une majuscule équivaut à shift + la minuscule.
        if c.is_uppercase() {
            modifiers.shift = true;
            return Ok(Key::Char(c.to_lowercase().next().unwrap_or(c)));
        }
        return Ok(Key::Char(c));
    }
    let lower = name.to_lowercase();
    if lower == "plus" {
        return Ok(Key::Char('+'));
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok())
        && (1..=24).contains(&n)
    {
        return Ok(Key::F(n));
    }
    NAMED_KEYS
        .iter()
        .find(|(_, main, aliases)| *main == lower || aliases.contains(&lower.as_str()))
        .map(|(key, _, _)| Key::Named(*key))
        .ok_or_else(|| KeyParseError::UnknownKey(name.to_owned()))
}

impl fmt::Display for KeyCombo {
    /// Forme canonique : `ctrl+alt+shift+meta+touche`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.modifiers;
        for (on, name) in [
            (m.ctrl, "ctrl"),
            (m.alt, "alt"),
            (m.shift, "shift"),
            (m.meta, "meta"),
        ] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        match self.key {
            Key::Char('+') => f.write_str("plus"),
            Key::Char(c) => write!(f, "{c}"),
            Key::F(n) => write!(f, "f{n}"),
            Key::Named(key) => {
                let name = NAMED_KEYS
                    .iter()
                    .find(|(k, _, _)| *k == key)
                    .map_or("?", |(_, name, _)| name);
                f.write_str(name)
            }
        }
    }
}

/// Séquence de combinaisons : `ctrl+k ctrl+c`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySequence(pub Vec<KeyCombo>);

impl KeySequence {
    /// Analyse une séquence : combinaisons séparées par des espaces.
    pub fn parse(text: &str, platform: Platform) -> Result<Self, KeyParseError> {
        let combos = text
            .split_whitespace()
            .map(|part| KeyCombo::parse(part, platform))
            .collect::<Result<Vec<_>, _>>()?;
        if combos.is_empty() {
            return Err(KeyParseError::Empty);
        }
        Ok(Self(combos))
    }

    /// Indique si `self` commence par `prefix` (et lui est strictement plus long).
    pub fn starts_with(&self, prefix: &[KeyCombo]) -> bool {
        self.0.len() > prefix.len() && self.0.starts_with(prefix)
    }
}

impl fmt::Display for KeySequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, combo) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{combo}")?;
        }
        Ok(())
    }
}

/// Un raccourci : une séquence de touches et la ligne de commande qu'elle exécute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// Touches.
    pub keys: KeySequence,
    /// Ligne de commande exécutée : `file save`, `edit move left --extend`.
    pub command: String,
}

/// Problème signalé au chargement d'un keymap.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeymapIssue {
    /// Raccourci illisible ; il est ignoré.
    #[error("raccourci « {keys} » ignoré : {error}")]
    Invalid {
        /// Texte du raccourci.
        keys: String,
        /// Erreur d'analyse.
        error: KeyParseError,
    },
    /// La même séquence est associée à deux commandes ; la dernière l'emporte.
    #[error("« {keys} » est associé à « {first} » puis à « {second} » : « {second} » l'emporte")]
    Duplicate {
        /// Séquence.
        keys: KeySequence,
        /// Commande remplacée.
        first: String,
        /// Commande retenue.
        second: String,
    },
    /// Une séquence en masque une autre : `ctrl+k` empêche d'atteindre `ctrl+k ctrl+c`.
    #[error(
        "« {prefix} » masque « {longer} » : la séquence la plus longue ne peut jamais être atteinte"
    )]
    PrefixConflict {
        /// Séquence courte.
        prefix: KeySequence,
        /// Séquence masquée.
        longer: KeySequence,
    },
}

/// Résultat de la recherche d'une séquence dans le keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMatch<'a> {
    /// La séquence est associée à cette ligne de commande.
    Command(&'a str),
    /// La séquence est le début d'un ou plusieurs raccourcis : attendre la touche suivante.
    Prefix,
    /// Aucun raccourci.
    None,
}

/// Ensemble des raccourcis.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Keymap {
    /// Keymap vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keymap avec les raccourcis par défaut de l'OS.
    pub fn with_defaults(platform: Platform) -> Self {
        let (keymap, issues) = Self::from_entries(default_bindings(platform), platform);
        debug_assert!(issues.is_empty(), "raccourcis par défaut : {issues:?}");
        keymap
    }

    /// Construit un keymap à partir de couples (touches, commande), dans l'ordre. Les raccourcis
    /// illisibles sont ignorés, une séquence répétée garde la dernière commande, et les conflits
    /// de préfixe sont signalés : tous ces problèmes sont renvoyés pour être affichés.
    pub fn from_entries<K, C>(
        entries: impl IntoIterator<Item = (K, C)>,
        platform: Platform,
    ) -> (Self, Vec<KeymapIssue>)
    where
        K: AsRef<str>,
        C: Into<String>,
    {
        let mut keymap = Self::new();
        let mut issues = Vec::new();
        for (keys, command) in entries {
            match KeySequence::parse(keys.as_ref(), platform) {
                Ok(sequence) => {
                    if let Some(first) = keymap.bind(sequence.clone(), command) {
                        issues.push(KeymapIssue::Duplicate {
                            second: keymap.command_for(&sequence).unwrap_or_default().to_owned(),
                            keys: sequence,
                            first,
                        });
                    }
                }
                Err(error) => issues.push(KeymapIssue::Invalid {
                    keys: keys.as_ref().to_owned(),
                    error,
                }),
            }
        }
        issues.extend(keymap.prefix_conflicts());
        (keymap, issues)
    }

    /// Associe `keys` à `command`. Renvoie la commande précédemment associée à ces touches.
    pub fn bind(&mut self, keys: KeySequence, command: impl Into<String>) -> Option<String> {
        let command = command.into();
        match self.bindings.iter_mut().find(|b| b.keys == keys) {
            Some(binding) => Some(std::mem::replace(&mut binding.command, command)),
            None => {
                self.bindings.push(Binding { keys, command });
                None
            }
        }
    }

    /// Retire le raccourci `keys`. Renvoie sa commande.
    pub fn unbind(&mut self, keys: &KeySequence) -> Option<String> {
        let index = self.bindings.iter().position(|b| &b.keys == keys)?;
        Some(self.bindings.remove(index).command)
    }

    /// Commande associée exactement à `keys`.
    pub fn command_for(&self, keys: &KeySequence) -> Option<&str> {
        self.bindings
            .iter()
            .find(|b| &b.keys == keys)
            .map(|b| b.command.as_str())
    }

    /// Cherche les touches déjà tapées : commande, début de séquence, ou rien.
    pub fn lookup(&self, pressed: &[KeyCombo]) -> KeyMatch<'_> {
        if let Some(binding) = self.bindings.iter().find(|b| b.keys.0 == pressed) {
            return KeyMatch::Command(&binding.command);
        }
        if self.bindings.iter().any(|b| b.keys.starts_with(pressed)) {
            KeyMatch::Prefix
        } else {
            KeyMatch::None
        }
    }

    /// Raccourcis, dans l'ordre de leur définition.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Raccourcis qui exécutent `command`, pour les afficher dans la palette.
    pub fn keys_for<'a>(&'a self, command: &'a str) -> impl Iterator<Item = &'a KeySequence> {
        self.bindings
            .iter()
            .filter(move |b| b.command == command)
            .map(|b| &b.keys)
    }

    /// Séquences masquées par une séquence plus courte qui en est le début.
    pub fn prefix_conflicts(&self) -> Vec<KeymapIssue> {
        let mut issues = Vec::new();
        for short in &self.bindings {
            for long in &self.bindings {
                if long.keys.starts_with(&short.keys.0) {
                    issues.push(KeymapIssue::PrefixConflict {
                        prefix: short.keys.clone(),
                        longer: long.keys.clone(),
                    });
                }
            }
        }
        issues
    }
}

/// Suivi des touches tapées, pour les séquences : retient le début d'une séquence en attendant la
/// touche suivante.
#[derive(Debug, Clone, Default)]
pub struct KeyResolver {
    pending: Vec<KeyCombo>,
}

/// Ce qu'il faut faire après une touche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Exécuter cette ligne de commande.
    Execute(String),
    /// Début de séquence : attendre la touche suivante.
    Pending,
    /// Aucun raccourci pour ces touches.
    Unbound,
}

impl KeyResolver {
    /// Aucune touche en attente.
    pub fn new() -> Self {
        Self::default()
    }

    /// Touches déjà tapées d'une séquence en cours.
    pub fn pending(&self) -> &[KeyCombo] {
        &self.pending
    }

    /// Abandonne la séquence en cours.
    pub fn reset(&mut self) {
        self.pending.clear();
    }

    /// Traite une touche.
    pub fn feed(&mut self, keymap: &Keymap, combo: KeyCombo) -> Resolution {
        self.pending.push(combo);
        match keymap.lookup(&self.pending) {
            KeyMatch::Command(command) => {
                let command = command.to_owned();
                self.pending.clear();
                Resolution::Execute(command)
            }
            KeyMatch::Prefix => Resolution::Pending,
            KeyMatch::None => {
                self.pending.clear();
                Resolution::Unbound
            }
        }
    }
}

/// Raccourcis par défaut : (touches, commande).
pub fn default_bindings(platform: Platform) -> Vec<(&'static str, &'static str)> {
    let mut bindings = vec![
        ("left", "edit move left"),
        ("right", "edit move right"),
        ("up", "edit move up"),
        ("down", "edit move down"),
        ("shift+left", "edit move left --extend"),
        ("shift+right", "edit move right --extend"),
        ("shift+up", "edit move up --extend"),
        ("shift+down", "edit move down --extend"),
        ("home", "edit move line-start"),
        ("end", "edit move line-end"),
        ("shift+home", "edit move line-start --extend"),
        ("shift+end", "edit move line-end --extend"),
        ("mod+home", "edit move doc-start"),
        ("mod+end", "edit move doc-end"),
        ("pageup", "edit move page-up"),
        ("pagedown", "edit move page-down"),
        ("shift+pageup", "edit move page-up --extend"),
        ("shift+pagedown", "edit move page-down --extend"),
        ("enter", "edit newline"),
        ("tab", "edit indent"),
        ("shift+tab", "edit outdent"),
        ("backspace", "edit delete-backward"),
        ("delete", "edit delete-forward"),
        ("escape", "edit keep-primary"),
        ("mod+a", "edit select-all"),
        ("mod+c", "edit copy"),
        ("mod+x", "edit cut"),
        ("mod+v", "edit paste"),
        ("mod+z", "edit undo"),
        ("mod+shift+z", "edit redo"),
        ("mod+;", "ui command-line"),
        ("mod+shift+p", "ui palette"),
        ("mod+p", "ui quick-open"),
        ("f1", "ui palette"),
        ("mod+s", "file save"),
        ("mod+shift+s", "ui command-line 'file save-as '"),
        ("mod+o", "ui command-line 'file open '"),
        ("mod+w", "panel close"),
        ("ctrl+tab", "tab next"),
        ("ctrl+shift+tab", "tab previous"),
        ("ctrl+pagedown", "tab next"),
        ("ctrl+pageup", "tab previous"),
        ("ctrl+shift+pagedown", "tab move-right"),
        ("ctrl+shift+pageup", "tab move-left"),
        ("mod+\\", "split right"),
        ("mod+shift+e", "explorer show"),
        ("mod+f", "ui find"),
        ("mod+d", "edit add-next-occurrence"),
        ("mod+shift+l", "edit select-all-occurrences"),
        ("mod+shift+f", "ui search-project"),
        ("f3", "search next"),
        ("shift+f3", "search previous"),
        ("ctrl+g", "ui goto"),
        ("alt+up", "edit move-lines up"),
        ("alt+down", "edit move-lines down"),
        ("alt+shift+down", "edit duplicate-lines"),
        ("mod+shift+k", "edit delete-lines"),
        ("mod+j", "edit join-lines"),
        ("mod+/", "edit toggle-comment"),
        ("mod+shift+n", "window new"),
        ("mod+shift+w", "window close"),
        ("mod+k mod+left", "panel focus left"),
        ("mod+k mod+right", "panel focus right"),
        ("mod+k mod+up", "panel focus up"),
        ("mod+k mod+down", "panel focus down"),
        ("mod+k alt+left", "panel move left"),
        ("mod+k alt+right", "panel move right"),
        ("mod+k alt+up", "panel move up"),
        ("mod+k alt+down", "panel move down"),
        ("mod+k mod+shift+right", "panel resize wider"),
        ("mod+k mod+shift+left", "panel resize narrower"),
        ("mod+k mod+shift+down", "panel resize taller"),
        ("mod+k mod+shift+up", "panel resize shorter"),
        ("mod+k mod+\\", "split down"),
        ("mod+=", "view zoom-in"),
        ("mod+shift+=", "view zoom-in"),
        ("mod+plus", "view zoom-in"),
        ("mod+-", "view zoom-out"),
        ("mod+0", "view zoom-reset"),
        ("mod+k mod+w", "file close"),
    ];
    match platform {
        Platform::MacOs => bindings.extend([
            ("alt+left", "edit move word-left"),
            ("alt+right", "edit move word-right"),
            ("alt+shift+left", "edit move word-left --extend"),
            ("alt+shift+right", "edit move word-right --extend"),
            ("cmd+left", "edit move line-start"),
            ("cmd+right", "edit move line-end"),
            ("cmd+up", "edit move doc-start"),
            ("cmd+down", "edit move doc-end"),
            ("alt+cmd+up", "edit add-cursor up"),
            ("alt+cmd+down", "edit add-cursor down"),
            ("alt+cmd+f", "ui replace"),
            ("cmd+g", "search next"),
            ("cmd+shift+g", "search previous"),
        ]),
        Platform::Other => bindings.extend([
            ("ctrl+left", "edit move word-left"),
            ("ctrl+right", "edit move word-right"),
            ("ctrl+shift+left", "edit move word-left --extend"),
            ("ctrl+shift+right", "edit move word-right --extend"),
            ("ctrl+y", "edit redo"),
            ("ctrl+insert", "edit copy"),
            ("shift+delete", "edit cut"),
            ("shift+insert", "edit paste"),
            ("ctrl+alt+up", "edit add-cursor up"),
            ("ctrl+alt+down", "edit add-cursor down"),
            ("ctrl+h", "ui replace"),
        ]),
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(text: &str) -> KeyCombo {
        KeyCombo::parse(text, Platform::Other).unwrap()
    }

    #[test]
    fn analyse_des_combinaisons() {
        let c = combo("ctrl+alt+t");
        assert!(c.modifiers.ctrl && c.modifiers.alt && !c.modifiers.shift);
        assert_eq!(c.key, Key::Char('t'));
        assert_eq!(combo("Ctrl+Shift+F5").key, Key::F(5));
        assert_eq!(combo("esc").key, Key::Named(NamedKey::Escape));
        assert_eq!(combo("pgdn").key, Key::Named(NamedKey::PageDown));
        assert_eq!(combo("ctrl+space").key, Key::Named(NamedKey::Space));
        assert_eq!(combo("ctrl++").key, Key::Char('+'));
        assert_eq!(combo("ctrl+plus"), combo("ctrl++"));
        assert_eq!(combo("ctrl+-").key, Key::Char('-'));
        assert_eq!(combo("A"), combo("shift+a"));
    }

    #[test]
    fn erreurs_d_analyse() {
        let parse = |t| KeyCombo::parse(t, Platform::Other);
        assert_eq!(parse(""), Err(KeyParseError::Empty));
        assert_eq!(parse("ctrl+"), Err(KeyParseError::Empty));
        assert_eq!(
            parse("ctrl+banane"),
            Err(KeyParseError::UnknownKey("banane".into()))
        );
        assert_eq!(
            parse("hyper+a"),
            Err(KeyParseError::UnknownModifier("hyper".into()))
        );
        assert_eq!(
            parse("ctrl+ctrl+a"),
            Err(KeyParseError::DuplicateModifier("ctrl".into()))
        );
        assert_eq!(parse("f25"), Err(KeyParseError::UnknownKey("f25".into())));
    }

    #[test]
    fn modificateur_principal_selon_l_os() {
        let mac = KeyCombo::parse("mod+s", Platform::MacOs).unwrap();
        assert!(mac.modifiers.meta && !mac.modifiers.ctrl);
        assert_eq!(mac, KeyCombo::parse("cmd+s", Platform::MacOs).unwrap());
        let other = KeyCombo::parse("mod+s", Platform::Other).unwrap();
        assert!(other.modifiers.ctrl && !other.modifiers.meta);
    }

    #[test]
    fn affichage_canonique() {
        assert_eq!(combo("shift+ctrl+K").to_string(), "ctrl+shift+k");
        assert_eq!(combo("alt+cmd+return").to_string(), "alt+meta+enter");
        assert_eq!(combo("ctrl++").to_string(), "ctrl+plus");
        let seq = KeySequence::parse("ctrl+k   ctrl+c", Platform::Other).unwrap();
        assert_eq!(seq.to_string(), "ctrl+k ctrl+c");
    }

    #[test]
    fn sequences() {
        let (keymap, issues) = Keymap::from_entries(
            [
                ("ctrl+k ctrl+c", "edit comment"),
                ("ctrl+k ctrl+u", "edit uncomment"),
                ("ctrl+s", "file save"),
            ],
            Platform::Other,
        );
        assert!(issues.is_empty(), "{issues:?}");
        let mut resolver = KeyResolver::new();
        assert_eq!(resolver.feed(&keymap, combo("ctrl+k")), Resolution::Pending);
        assert_eq!(
            resolver.feed(&keymap, combo("ctrl+c")),
            Resolution::Execute("edit comment".into())
        );
        assert_eq!(
            resolver.feed(&keymap, combo("ctrl+s")),
            Resolution::Execute("file save".into())
        );
        assert_eq!(resolver.feed(&keymap, combo("ctrl+k")), Resolution::Pending);
        assert_eq!(resolver.feed(&keymap, combo("x")), Resolution::Unbound);
        assert!(resolver.pending().is_empty());
        assert_eq!(keymap.lookup(&[combo("f1")]), KeyMatch::None);
    }

    #[test]
    fn conflits_signales() {
        let (keymap, issues) = Keymap::from_entries(
            [
                ("ctrl+k", "a"),
                ("ctrl+k ctrl+c", "b"),
                ("ctrl+s", "c"),
                ("ctrl+s", "d"),
                ("ctrl+nope", "e"),
            ],
            Platform::Other,
        );
        assert_eq!(
            keymap.command_for(&KeySequence(vec![combo("ctrl+s")])),
            Some("d")
        );
        let messages: Vec<String> = issues.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "« ctrl+s » est associé à « c » puis à « d » : « d » l'emporte",
                "raccourci « ctrl+nope » ignoré : touche inconnue : « nope »",
                "« ctrl+k » masque « ctrl+k ctrl+c » : la séquence la plus longue ne peut jamais être atteinte",
            ]
        );
    }

    #[test]
    fn raccourcis_par_defaut_sans_conflit_et_valides() {
        for platform in [Platform::MacOs, Platform::Other] {
            let (keymap, issues) = Keymap::from_entries(default_bindings(platform), platform);
            assert!(issues.is_empty(), "{platform:?} : {issues:?}");
            let mut editor = crate::Editor::new();
            for binding in keymap.bindings() {
                let name = binding
                    .command
                    .split_whitespace()
                    .take(2)
                    .collect::<Vec<_>>()
                    .join(" ");
                assert!(
                    editor.commands().get(&name).is_some(),
                    "commande inconnue : {name}"
                );
            }
            // Sans document, chaque commande échoue proprement, sans paniquer.
            for binding in keymap.bindings() {
                let _ = editor.execute(&binding.command);
            }
        }
    }

    #[test]
    fn raccourcis_d_une_commande() {
        let keymap = Keymap::with_defaults(Platform::Other);
        let keys: Vec<String> = keymap
            .keys_for("edit redo")
            .map(ToString::to_string)
            .collect();
        assert_eq!(keys, ["ctrl+shift+z", "ctrl+y"]);
    }
}
