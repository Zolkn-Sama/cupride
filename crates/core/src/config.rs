//! Configuration de l'utilisateur, en TOML.
//!
//! Le fichier `config.toml` est lu dans le dossier de configuration de l'OS :
//!
//! | OS | Emplacement |
//! |---|---|
//! | Linux | `~/.config/cupride/config.toml` |
//! | macOS | `~/Library/Application Support/cupride/config.toml` |
//! | Windows | `%APPDATA%\cupride\config\config.toml` |
//!
//! Un fichier absent donne les valeurs par défaut. Un fichier invalide donne aussi les valeurs par
//! défaut, avec une erreur indiquant la ligne et la colonne : l'IDE démarre toujours.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use directories::ProjectDirs;
use serde::Deserialize;

use crate::document::LineEnding;

/// Contenu proposé par `config open` quand le fichier n'existe pas encore : toutes les options,
/// commentées, avec leur valeur par défaut.
pub const DEFAULT_CONFIG_TEMPLATE: &str = r#"# Configuration de CuprIDE.
# Les lignes commentées donnent les valeurs par défaut.

[editor]
# Fin de ligne des nouveaux documents : "auto" (LF), "lf" ou "crlf".
# Un fichier existant garde toujours ses fins de ligne.
# line_ending = "auto"

# Pause, en millisecondes, au-delà de laquelle une nouvelle frappe commence un nouveau bloc
# d'annulation. 0 : chaque frappe est annulée séparément.
# undo_group_delay_ms = 1000

# Sélection primaire (Linux, X11 et Wayland) : le texte sélectionné est copié dans la sélection
# primaire, et le clic du milieu le colle. Sans effet sous Windows et macOS.
# primary_selection = false

[font]
# Famille de police : "monospace" (police à chasse fixe du système) ou un nom de famille
# installée, par exemple "JetBrains Mono" ou "Fira Code" (ligatures).
# family = "monospace"

# Taille en points (pixels logiques, multipliés par l'échelle de l'écran).
# size = 14.0

# Hauteur de ligne, en multiple de la taille.
# line_height = 1.4

[view]
# Clignotement du curseur. Il s'arrête de lui-même après quelques secondes sans saisie, pour que
# l'IDE ne consomme aucun CPU au repos.
# cursor_blink = true

# Demi-période du clignotement, en millisecondes.
# cursor_blink_ms = 530

# Surligne la ligne du curseur principal.
# highlight_current_line = true

# Numéros de ligne : "absolute", "relative" (distance au curseur, numéro réel sur sa ligne)
# ou "off".
# line_numbers = "absolute"

[status_bar]
# Éléments de la barre d'état, à gauche et à droite : "file" (nom du fichier), "modified"
# (modifications non enregistrées), "keys" (séquence de touches en cours), "selections" (nombre
# de curseurs), "position" (ligne et colonne), "line_ending", "encoding", "language".
# left = ["file", "modified", "keys"]
# right = ["selections", "position", "line_ending", "encoding", "language"]

[keys]
# Raccourcis : touches = "ligne de commande". Ils s'ajoutent aux raccourcis par défaut et les
# remplacent pour les mêmes touches. Une chaîne vide retire un raccourci par défaut.
# "mod" vaut Ctrl sous Windows et Linux, Cmd sous macOS.
# "mod+shift+s" = "file save-as"
# "ctrl+k ctrl+c" = "edit insert '// '"
# "mod+k mod+w" = ""
"#;

/// Fin de ligne des nouveaux documents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEndingSetting {
    /// Choix automatique : LF.
    #[default]
    Auto,
    /// Toujours `\n`.
    Lf,
    /// Toujours `\r\n`.
    Crlf,
}

impl LineEndingSetting {
    /// Fin de ligne à utiliser pour un nouveau document.
    pub fn for_new_document(self) -> LineEnding {
        match self {
            Self::Auto | Self::Lf => LineEnding::Lf,
            Self::Crlf => LineEnding::Crlf,
        }
    }
}

/// Section `[editor]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    /// Fin de ligne des nouveaux documents.
    pub line_ending: LineEndingSetting,
    /// Délai de regroupement des frappes dans l'historique, en millisecondes.
    pub undo_group_delay_ms: u64,
    /// Sélection primaire de X11 et Wayland.
    pub primary_selection: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            line_ending: LineEndingSetting::Auto,
            undo_group_delay_ms: 1_000,
            primary_selection: false,
        }
    }
}

impl EditorConfig {
    /// Délai de regroupement des frappes.
    pub fn undo_group_delay(&self) -> Duration {
        Duration::from_millis(self.undo_group_delay_ms)
    }
}

/// Section `[font]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    /// Famille de police : `monospace` ou le nom d'une famille installée.
    pub family: String,
    /// Taille en pixels logiques.
    pub size: f32,
    /// Hauteur de ligne, en multiple de la taille.
    pub line_height: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_owned(),
            size: 14.0,
            line_height: 1.4,
        }
    }
}

/// Numéros de ligne dans la gouttière.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineNumbers {
    /// Numéro de chaque ligne.
    #[default]
    Absolute,
    /// Distance à la ligne du curseur ; numéro réel sur la ligne du curseur.
    Relative,
    /// Pas de numéros (la place des marqueurs reste réservée).
    Off,
}

/// Section `[view]` : affichage de l'éditeur.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ViewConfig {
    /// Clignotement du curseur.
    pub cursor_blink: bool,
    /// Demi-période du clignotement, en millisecondes.
    pub cursor_blink_ms: u64,
    /// Surlignage de la ligne du curseur principal.
    pub highlight_current_line: bool,
    /// Numéros de ligne.
    pub line_numbers: LineNumbers,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            cursor_blink: true,
            cursor_blink_ms: 530,
            highlight_current_line: true,
            line_numbers: LineNumbers::Absolute,
        }
    }
}

impl ViewConfig {
    /// Demi-période du clignotement, bornée à des valeurs raisonnables (50 ms à 5 s).
    pub fn cursor_blink_interval(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_ms.clamp(50, 5_000))
    }
}

/// Section `[status_bar]` : éléments affichés dans la barre d'état.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    /// Éléments à gauche.
    pub left: Vec<String>,
    /// Éléments à droite.
    pub right: Vec<String>,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        let names = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect();
        Self {
            left: names(&["file", "modified", "keys"]),
            right: names(&[
                "selections",
                "position",
                "line_ending",
                "encoding",
                "language",
            ]),
        }
    }
}

/// Configuration complète.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Section `[editor]`.
    pub editor: EditorConfig,
    /// Section `[font]`.
    pub font: FontConfig,
    /// Section `[view]`.
    pub view: ViewConfig,
    /// Section `[status_bar]`.
    pub status_bar: StatusBarConfig,
    /// Section `[keys]` : touches → ligne de commande (vide pour retirer un raccourci).
    pub keys: BTreeMap<String, String>,
}

/// Clés connues, pour signaler les fautes de frappe.
const KNOWN_KEYS: [(&str, &[&str]); 5] = [
    (
        "editor",
        &["line_ending", "undo_group_delay_ms", "primary_selection"],
    ),
    ("font", &["family", "size", "line_height"]),
    (
        "view",
        &[
            "cursor_blink",
            "cursor_blink_ms",
            "highlight_current_line",
            "line_numbers",
        ],
    ),
    ("status_bar", &["left", "right"]),
    ("keys", &[]),
];

/// Problème dans le fichier de configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// Fichier concerné.
    pub path: PathBuf,
    /// Ligne (à partir de 1), si connue.
    pub line: Option<usize>,
    /// Colonne (à partir de 1, en caractères), si connue.
    pub column: Option<usize>,
    /// Explication.
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())?;
        if let (Some(line), Some(column)) = (self.line, self.column) {
            write!(f, ":{line}:{column}")?;
        }
        write!(f, " : {}", self.message)
    }
}

impl std::error::Error for ConfigError {}

/// Résultat du chargement : la configuration à utiliser, toujours présente, et les problèmes à
/// signaler à l'utilisateur.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    /// Configuration à utiliser.
    pub config: Config,
    /// Erreur qui a fait revenir aux valeurs par défaut.
    pub error: Option<ConfigError>,
    /// Avertissements (clés inconnues ignorées).
    pub warnings: Vec<String>,
}

/// Emplacement du fichier de configuration de l'utilisateur, s'il peut être déterminé.
pub fn default_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "cupride").map(|dirs| dirs.config_dir().join("config.toml"))
}

/// Charge la configuration depuis `path`. Ne renvoie jamais d'erreur bloquante : voir [`Loaded`].
pub fn load(path: &Path) -> Loaded {
    match fs::read_to_string(path) {
        Ok(text) => parse(path, &text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Loaded::default(),
        Err(error) => Loaded {
            error: Some(ConfigError {
                path: path.to_owned(),
                line: None,
                column: None,
                message: format!("lecture impossible ({error}), valeurs par défaut utilisées"),
            }),
            ..Loaded::default()
        },
    }
}

/// Analyse le contenu d'un fichier de configuration.
pub fn parse(path: &Path, text: &str) -> Loaded {
    match toml::from_str::<Config>(text) {
        Ok(config) => Loaded {
            config,
            error: None,
            warnings: unknown_keys(text),
        },
        Err(error) => {
            let (line, column) = match error.span() {
                Some(span) => {
                    let (line, column) = line_column(text, span.start);
                    (Some(line), Some(column))
                }
                None => (None, None),
            };
            Loaded {
                config: Config::default(),
                error: Some(ConfigError {
                    path: path.to_owned(),
                    line,
                    column,
                    message: format!(
                        "{}, valeurs par défaut utilisées",
                        error.message().trim_end_matches('\n')
                    ),
                }),
                warnings: Vec::new(),
            }
        }
    }
}

/// Ligne et colonne (à partir de 1, colonne en caractères) de la position `offset`.
fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[line_start..].chars().count() + 1)
}

/// Clés inconnues, ignorées : sans doute des fautes de frappe.
fn unknown_keys(text: &str) -> Vec<String> {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    for (section, value) in &table {
        let Some((_, known)) = KNOWN_KEYS.iter().find(|(name, _)| name == section) else {
            warnings.push(format!("section inconnue « [{section}] » ignorée"));
            continue;
        };
        if section == "keys" {
            continue;
        }
        if let Some(entries) = value.as_table() {
            for key in entries.keys().filter(|k| !known.contains(&k.as_str())) {
                warnings.push(format!("option inconnue « {section}.{key} » ignorée"));
            }
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(text: &str) -> Loaded {
        parse(Path::new("config.toml"), text)
    }

    #[test]
    fn fichier_absent_valeurs_par_defaut() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(&dir.path().join("config.toml"));
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.error.is_none());
        assert_eq!(
            loaded.config.editor.undo_group_delay(),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn modele_par_defaut_valide() {
        let loaded = parse_str(DEFAULT_CONFIG_TEMPLATE);
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.config, Config::default());
        // Décommenté, le modèle donne exactement les valeurs par défaut.
        let uncommented: String = DEFAULT_CONFIG_TEMPLATE
            .lines()
            .map(|l| match l.strip_prefix("# ") {
                Some(rest)
                    if rest.split_once(" = ").is_some_and(|(key, _)| {
                        key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    }) =>
                {
                    rest
                }
                _ => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse_str(&uncommented).config, Config::default());
    }

    #[test]
    fn valeurs_lues() {
        let loaded = parse_str(
            "[editor]\nline_ending = \"crlf\"\nundo_group_delay_ms = 250\n\n[keys]\n\"mod+shift+s\" = \"file save-as\"\n",
        );
        assert!(loaded.error.is_none());
        assert_eq!(loaded.config.editor.line_ending, LineEndingSetting::Crlf);
        assert_eq!(loaded.config.editor.undo_group_delay_ms, 250);
        assert_eq!(loaded.config.keys["mod+shift+s"], "file save-as");
    }

    #[test]
    fn erreur_de_syntaxe_avec_ligne_et_colonne() {
        let loaded = parse_str("[editor]\nline_ending = \"lf\"\nundo_group_delay_ms = = 3\n");
        let error = loaded.error.unwrap();
        assert_eq!((error.line, error.column), (Some(3), Some(23)));
        assert!(
            error.to_string().starts_with("config.toml:3:23 : "),
            "{error}"
        );
        assert!(error.message.ends_with("valeurs par défaut utilisées"));
        assert_eq!(loaded.config, Config::default());
    }

    #[test]
    fn erreur_de_type_avec_position() {
        let loaded = parse_str("[editor]\nline_ending = \"mac\"\n");
        let error = loaded.error.unwrap();
        assert_eq!(error.line, Some(2));
        assert!(error.message.contains("mac"), "{}", error.message);
    }

    #[test]
    fn cles_inconnues_signalees() {
        let loaded = parse_str("[editor]\ntab_widht = 2\n\n[theme]\nname = \"rubis\"\n");
        assert!(loaded.error.is_none());
        assert_eq!(
            loaded.warnings,
            [
                "option inconnue « editor.tab_widht » ignorée",
                "section inconnue « [theme] » ignorée"
            ]
        );
    }

    #[test]
    fn position_en_caracteres() {
        assert_eq!(line_column("é = x", 5), (1, 5));
        assert_eq!(line_column("a\nbc", 3), (2, 2));
    }
}
