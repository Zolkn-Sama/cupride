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

# Indentation : "auto" (détectée dans chaque fichier ; espaces pour un fichier sans
# indentation), "spaces" ou "tabs". Largeur d'un niveau, et d'une tabulation affichée.
# indent_style = "auto"
# indent_width = 4

# Paires fermées automatiquement : taper « ( » insère « () », taper « ) » devant un « ) » passe
# par-dessus, retour arrière entre les deux les efface, et une sélection tapée « ( » est
# entourée.
# auto_close = true

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

# Thème : "padparadscha", "clair", "sombre", ou un thème du dossier themes/ de la configuration
# (son nom de fichier, sans .toml).
# theme = "padparadscha"

# Retour à la ligne visuel des lignes plus larges que la zone (le fichier n'est pas modifié).
# wrap = false

# Espaces et tabulations visibles (points et flèches discrets), fins de ligne visibles.
# whitespace = false
# line_endings = false

# Règles verticales aux colonnes indiquées, par exemple [80, 100].
# rulers = []

[notifications]
# Durée d'affichage des informations et avertissements, en millisecondes. 0 : jusqu'à un clic.
# timeout_ms = 4000

# Durée d'affichage des erreurs, en millisecondes. 0 : jusqu'à un clic.
# error_timeout_ms = 8000

[status_bar]
# Éléments de la barre d'état, à gauche et à droite : "file" (nom du fichier), "modified"
# (modifications non enregistrées), "keys" (séquence de touches en cours), "selections" (nombre
# de curseurs), "position" (ligne et colonne), "indent" (tabulations ou espaces), "line_ending",
# "encoding", "language".
# left = ["file", "modified", "keys"]
# right = ["selections", "position", "indent", "line_ending", "encoding", "language"]

[tabs]
# Onglet d'aperçu : un fichier ouvert depuis l'ouverture rapide ou l'explorateur remplace l'onglet
# d'aperçu de la zone au lieu d'en ajouter un ; l'onglet est gardé dès qu'on modifie le document
# (ou avec « tab keep »).
# preview = false

[session]
# Au lancement, l'espace de travail du projet (zones, onglets, fichiers, curseurs, défilement)
# est restauré tel qu'il était à la fermeture. « cupride --no-restore » démarre sans le restaurer.
# restore = true

[explorer]
# Fichiers ignorés (.gitignore, .ignore) : "dim" (affichés en gris), "hide" (masqués) ou
# "show" (affichés comme les autres). Le dossier .git n'est jamais affiché.
# ignored = "dim"

[window]
# Fermer la dernière fenêtre quitte CuprIDE. Avec false, CuprIDE reste lancé sans fenêtre : la
# prochaine commande « cupride » en ouvre une, dans la même instance.
# quit_on_last_close = true

# Sous Wayland, une application ne peut pas placer ses fenêtres. Quand un écran est demandé
# (window new --monitor, window move) : "fullscreen" (plein écran sur cet écran) ou
# "compositor" (le compositeur choisit). Une notification le signale. Sans effet ailleurs.
# wayland_placement = "fullscreen"

# [languages.Makefile]
# Réglages par langage (nom affiché dans la barre d'état), prioritaires sur [editor].
# indent_style = "tabs"
# indent_width = 8

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
    /// Tabulations, espaces, ou détection.
    pub indent_style: crate::indent::IndentStyle,
    /// Largeur d'un niveau d'indentation.
    pub indent_width: usize,
    /// Paires (parenthèses, guillemets…) fermées automatiquement.
    pub auto_close: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            line_ending: LineEndingSetting::Auto,
            undo_group_delay_ms: 1_000,
            primary_selection: false,
            indent_style: crate::indent::IndentStyle::Auto,
            indent_width: 4,
            auto_close: true,
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
    /// Identifiant du thème.
    pub theme: String,
    /// Retour à la ligne visuel des lignes trop longues.
    pub wrap: bool,
    /// Espaces et tabulations visibles.
    pub whitespace: bool,
    /// Fins de ligne visibles.
    pub line_endings: bool,
    /// Règles verticales, aux colonnes indiquées.
    pub rulers: Vec<usize>,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            cursor_blink: true,
            cursor_blink_ms: 530,
            highlight_current_line: true,
            line_numbers: LineNumbers::Absolute,
            theme: "padparadscha".to_owned(),
            wrap: false,
            whitespace: false,
            line_endings: false,
            rulers: Vec::new(),
        }
    }
}

impl ViewConfig {
    /// Demi-période du clignotement, bornée à des valeurs raisonnables (50 ms à 5 s).
    pub fn cursor_blink_interval(&self) -> Duration {
        Duration::from_millis(self.cursor_blink_ms.clamp(50, 5_000))
    }
}

/// Section `[notifications]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct NotificationsConfig {
    /// Durée d'affichage des informations et avertissements, en millisecondes (0 : jusqu'à un
    /// clic).
    pub timeout_ms: u64,
    /// Durée d'affichage des erreurs, en millisecondes (0 : jusqu'à un clic).
    pub error_timeout_ms: u64,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 4_000,
            error_timeout_ms: 8_000,
        }
    }
}

impl NotificationsConfig {
    /// Durée d'affichage d'une notification de gravité `level`, ou `None` pour la garder jusqu'à
    /// un clic.
    pub fn timeout(&self, level: crate::notifications::Level) -> Option<Duration> {
        let ms = match level {
            crate::notifications::Level::Error => self.error_timeout_ms,
            _ => self.timeout_ms,
        };
        (ms > 0).then(|| Duration::from_millis(ms))
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
                "indent",
                "line_ending",
                "encoding",
                "language",
            ]),
        }
    }
}

/// Section `[tabs]` : onglets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TabsConfig {
    /// Onglet d'aperçu, remplacé par le prochain fichier ouvert en aperçu.
    pub preview: bool,
}

/// Section `[session]` : espace de travail entre deux lancements.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Restaurer la session du projet au lancement.
    pub restore: bool,
}

impl Config {
    /// Retour à la ligne et règles pour le langage `language`.
    pub fn display_for(&self, language: Option<&str>) -> (bool, Vec<usize>) {
        let specific = language.and_then(|l| self.languages.get(l));
        (
            specific.and_then(|l| l.wrap).unwrap_or(self.view.wrap),
            specific
                .and_then(|l| l.rulers.clone())
                .unwrap_or_else(|| self.view.rulers.clone()),
        )
    }

    /// Fermeture automatique des paires pour le langage `language`.
    pub fn auto_close_for(&self, language: Option<&str>) -> bool {
        language
            .and_then(|l| self.languages.get(l))
            .and_then(|l| l.auto_close)
            .unwrap_or(self.editor.auto_close)
    }

    /// Indentation réglée pour le langage `language` : celle de `[languages.<Nom>]`, sinon celle
    /// de `[editor]`.
    pub fn indent_for(&self, language: Option<&str>) -> (crate::indent::IndentStyle, usize) {
        let specific = language.and_then(|l| self.languages.get(l));
        (
            specific
                .and_then(|l| l.indent_style)
                .unwrap_or(self.editor.indent_style),
            specific
                .and_then(|l| l.indent_width)
                .unwrap_or(self.editor.indent_width),
        )
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { restore: true }
    }
}

/// Affichage des fichiers ignorés dans l'explorateur.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IgnoredFiles {
    /// Affichés en gris.
    #[default]
    Dim,
    /// Masqués.
    Hide,
    /// Affichés comme les autres.
    Show,
}

/// Section `[explorer]` : explorateur de fichiers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ExplorerConfig {
    /// Fichiers ignorés par `.gitignore` et `.ignore`.
    pub ignored: IgnoredFiles,
}

/// Section `[window]` : fenêtres.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    /// Fermer la dernière fenêtre quitte l'application.
    pub quit_on_last_close: bool,
    /// Placement d'une fenêtre sur un écran demandé, sous Wayland.
    pub wayland_placement: crate::monitors::WaylandPlacement,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            quit_on_last_close: true,
            wayland_placement: crate::monitors::WaylandPlacement::default(),
        }
    }
}

/// Section `[languages.<Nom>]` : réglages propres à un langage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct LanguageConfig {
    /// Indentation, à la place de `[editor] indent_style`.
    pub indent_style: Option<crate::indent::IndentStyle>,
    /// Largeur, à la place de `[editor] indent_width`.
    pub indent_width: Option<usize>,
    /// Début d'un commentaire de ligne (`//`, `#`…), pour `edit toggle-comment`.
    pub comment: Option<String>,
    /// Retour à la ligne visuel, à la place de `[view] wrap`.
    pub wrap: Option<bool>,
    /// Règles verticales, à la place de `[view] rulers`.
    pub rulers: Option<Vec<usize>>,
    /// Paires fermées automatiquement, à la place de `[editor] auto_close`.
    pub auto_close: Option<bool>,
}

/// Clés connues d'une section `[languages.<Nom>]`.
const LANGUAGE_KEYS: [&str; 6] = [
    "indent_style",
    "indent_width",
    "comment",
    "wrap",
    "rulers",
    "auto_close",
];

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
    /// Section `[notifications]`.
    pub notifications: NotificationsConfig,
    /// Section `[status_bar]`.
    pub status_bar: StatusBarConfig,
    /// Section `[tabs]`.
    pub tabs: TabsConfig,
    /// Section `[session]`.
    pub session: SessionConfig,
    /// Section `[explorer]`.
    pub explorer: ExplorerConfig,
    /// Section `[window]`.
    pub window: WindowConfig,
    /// Sections `[languages.<Nom>]`, par nom de langage.
    pub languages: BTreeMap<String, LanguageConfig>,
    /// Section `[keys]` : touches → ligne de commande (vide pour retirer un raccourci).
    pub keys: BTreeMap<String, String>,
}

/// Clés connues, pour signaler les fautes de frappe.
const KNOWN_KEYS: [(&str, &[&str]); 11] = [
    (
        "editor",
        &[
            "line_ending",
            "undo_group_delay_ms",
            "primary_selection",
            "indent_style",
            "indent_width",
            "auto_close",
        ],
    ),
    ("languages", &[]),
    ("font", &["family", "size", "line_height"]),
    (
        "view",
        &[
            "cursor_blink",
            "cursor_blink_ms",
            "highlight_current_line",
            "line_numbers",
            "theme",
            "wrap",
            "whitespace",
            "line_endings",
            "rulers",
        ],
    ),
    ("notifications", &["timeout_ms", "error_timeout_ms"]),
    ("status_bar", &["left", "right"]),
    ("tabs", &["preview"]),
    ("session", &["restore"]),
    ("explorer", &["ignored"]),
    ("window", &["quit_on_last_close", "wayland_placement"]),
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
        if section == "languages" {
            for (language, entries) in value.as_table().into_iter().flatten() {
                let keys = entries.as_table().into_iter().flat_map(|t| t.keys());
                for key in keys.filter(|k| !LANGUAGE_KEYS.contains(&k.as_str())) {
                    warnings.push(format!(
                        "option inconnue « languages.{language}.{key} » ignorée"
                    ));
                }
            }
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

    #[test]
    fn reglages_par_langage() {
        let loaded = parse_str(
            "[editor]\nindent_width = 2\n\n[languages.Makefile]\nindent_style = \"tabs\"\nindent_widht = 8\n",
        );
        assert!(loaded.error.is_none(), "{:?}", loaded.error);
        assert_eq!(
            loaded.warnings,
            ["option inconnue « languages.Makefile.indent_widht » ignorée"]
        );
        let config = loaded.config;
        assert_eq!(
            config.indent_for(Some("Makefile")),
            (crate::indent::IndentStyle::Tabs, 2)
        );
        assert_eq!(
            config.indent_for(Some("Rust")),
            (crate::indent::IndentStyle::Auto, 2)
        );
    }
}
