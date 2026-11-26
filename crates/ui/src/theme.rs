//! Thèmes de couleurs.
//!
//! Un thème est un fichier TOML : `name`, une section `[ui]` pour l'interface et une section
//! `[syntax]` pour les catégories de la coloration syntaxique. Chaque couleur s'écrit `#rrggbb`
//! ou `#rrggbbaa`. Une couleur absente est reprise du thème nommé par `base` (Padparadscha par
//! défaut) : un thème peut ne changer que quelques couleurs.
//!
//! Thèmes intégrés : Padparadscha (par défaut), Clair et Sombre. Les thèmes de l'utilisateur sont
//! les fichiers `.toml` du dossier `themes/` de la configuration ; leur identifiant est le nom du
//! fichier, sans l'extension.

use std::collections::BTreeMap;
use std::path::Path;

use cupride_render::Color;
use serde::Deserialize;

/// Identifiant du thème par défaut.
pub const DEFAULT_THEME: &str = "padparadscha";

/// Thèmes intégrés : (identifiant, contenu du fichier).
const BUILTIN: [(&str, &str); 3] = [
    ("padparadscha", include_str!("../themes/padparadscha.toml")),
    ("clair", include_str!("../themes/clair.toml")),
    ("sombre", include_str!("../themes/sombre.toml")),
];

/// Couleurs des catégories syntaxiques (utilisées par la coloration syntaxique).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxColors {
    /// Commentaires.
    pub comment: Color,
    /// Mots-clés.
    pub keyword: Color,
    /// Chaînes de caractères.
    pub string: Color,
    /// Nombres.
    pub number: Color,
    /// Constantes.
    pub constant: Color,
    /// Fonctions.
    pub function: Color,
    /// Types.
    pub r#type: Color,
    /// Variables.
    pub variable: Color,
    /// Opérateurs.
    pub operator: Color,
    /// Ponctuation.
    pub punctuation: Color,
}

/// Couleurs de l'interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Nom affiché.
    pub name: String,
    /// Fond de l'éditeur.
    pub background: Color,
    /// Texte.
    pub foreground: Color,
    /// Curseurs.
    pub cursor: Color,
    /// Fond des sélections.
    pub selection: Color,
    /// Fond de la ligne du curseur principal.
    pub current_line: Color,
    /// Numéros de ligne.
    pub line_number: Color,
    /// Numéro de la ligne du curseur principal.
    pub line_number_active: Color,
    /// Fond des barres (barre d'état, ligne de commande).
    pub panel: Color,
    /// Fond des popups (palette, complétions, notifications).
    pub popup: Color,
    /// Bordures et séparations.
    pub border: Color,
    /// Couleur d'accent : invite, élément choisi, correspondances de recherche.
    pub accent: Color,
    /// Texte secondaire : descriptions, raccourcis, aide.
    pub muted: Color,
    /// Fond de l'élément choisi d'une liste.
    pub list_selection: Color,
    /// Information.
    pub info: Color,
    /// Avertissement.
    pub warning: Color,
    /// Erreur.
    pub error: Color,
    /// Coloration syntaxique.
    pub syntax: SyntaxColors,
}

impl Theme {
    /// Surlignage des occurrences de la recherche : la couleur d'accent, translucide.
    /// Fond d'une parenthèse et de sa correspondante.
    pub fn bracket_match(&self) -> Color {
        Color::rgba(self.muted.r, self.muted.g, self.muted.b, 0x48)
    }

    pub fn search_match(&self) -> Color {
        Color::rgba(self.accent.r, self.accent.g, self.accent.b, 0x55)
    }

    /// Padparadscha, le thème par défaut : un saphir rose-orangé sur fond sombre. Aussi la base
    /// des autres thèmes pour les couleurs qu'ils ne donnent pas.
    pub fn padparadscha() -> Self {
        let c = |hex| Color::from_hex(hex).expect("couleur valide");
        Self {
            name: "Padparadscha".to_owned(),
            background: c("#1e1a1d"),
            foreground: c("#f2e6e3"),
            cursor: c("#f79e86"),
            selection: c("#e87e6c48"),
            // Opaque : un blanc très transparent paraît bien plus clair, le mélange se faisant en
            // couleurs linéaires.
            current_line: c("#2a2428"),
            line_number: c("#6e6066"),
            line_number_active: c("#f79e86"),
            panel: c("#171416"),
            popup: c("#2b2429"),
            border: c("#42383e"),
            accent: c("#f79e86"),
            muted: c("#9a8a90"),
            list_selection: c("#4a3333"),
            info: c("#8cb8d8"),
            warning: c("#e8c06c"),
            error: c("#f06c6c"),
            syntax: SyntaxColors {
                comment: c("#7d6b72"),
                keyword: c("#f0889a"),
                string: c("#c9d98f"),
                number: c("#f7b98a"),
                constant: c("#f7b98a"),
                function: c("#f5c6a5"),
                r#type: c("#e8a0c8"),
                variable: c("#f2e6e3"),
                operator: c("#d8a0a8"),
                punctuation: c("#b8a6ac"),
            },
        }
    }

    /// Emplacement de la couleur `key` de la section `[ui]`.
    fn ui_slot(&mut self, key: &str) -> Option<&mut Color> {
        Some(match key {
            "background" => &mut self.background,
            "foreground" => &mut self.foreground,
            "cursor" => &mut self.cursor,
            "selection" => &mut self.selection,
            "current_line" => &mut self.current_line,
            "line_number" => &mut self.line_number,
            "line_number_active" => &mut self.line_number_active,
            "panel" => &mut self.panel,
            "popup" => &mut self.popup,
            "border" => &mut self.border,
            "accent" => &mut self.accent,
            "muted" => &mut self.muted,
            "list_selection" => &mut self.list_selection,
            "info" => &mut self.info,
            "warning" => &mut self.warning,
            "error" => &mut self.error,
            _ => return None,
        })
    }

    /// Emplacement de la couleur `key` de la section `[syntax]`.
    fn syntax_slot(&mut self, key: &str) -> Option<&mut Color> {
        let s = &mut self.syntax;
        Some(match key {
            "comment" => &mut s.comment,
            "keyword" => &mut s.keyword,
            "string" => &mut s.string,
            "number" => &mut s.number,
            "constant" => &mut s.constant,
            "function" => &mut s.function,
            "type" => &mut s.r#type,
            "variable" => &mut s.variable,
            "operator" => &mut s.operator,
            "punctuation" => &mut s.punctuation,
            _ => return None,
        })
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::padparadscha()
    }
}

/// Contenu d'un fichier de thème.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: Option<String>,
    base: Option<String>,
    #[serde(default)]
    ui: BTreeMap<String, String>,
    #[serde(default)]
    syntax: BTreeMap<String, String>,
}

/// Thème lu depuis un fichier, et les problèmes rencontrés (couleurs ignorées).
#[derive(Debug, Clone)]
pub struct ParsedTheme {
    /// Thème obtenu.
    pub theme: Theme,
    /// Avertissements : clé inconnue, couleur invalide.
    pub warnings: Vec<String>,
}

/// Thèmes disponibles, par identifiant.
#[derive(Debug, Clone)]
pub struct Themes {
    themes: BTreeMap<String, Theme>,
}

impl Default for Themes {
    /// Les thèmes intégrés.
    fn default() -> Self {
        let mut themes = Self {
            themes: BTreeMap::new(),
        };
        for (id, text) in BUILTIN {
            let parsed = themes
                .parse(text, id)
                .expect("les thèmes intégrés sont valides");
            debug_assert!(parsed.warnings.is_empty(), "{id} : {:?}", parsed.warnings);
            themes.themes.insert(id.to_owned(), parsed.theme);
        }
        themes
    }
}

impl Themes {
    /// Analyse le contenu d'un fichier de thème d'identifiant `id`. Erreur si le TOML est
    /// invalide ou si la base n'existe pas ; une couleur invalide ou une clé inconnue est
    /// seulement signalée.
    pub fn parse(&self, text: &str, id: &str) -> Result<ParsedTheme, String> {
        let file: ThemeFile = toml::from_str(text).map_err(|e| e.message().trim().to_owned())?;
        let mut theme = match file.base.as_deref() {
            Some(base) => self
                .get(base)
                .cloned()
                .ok_or_else(|| format!("thème de base inconnu : « {base} »"))?,
            None => Theme::padparadscha(),
        };
        theme.name = file.name.unwrap_or_else(|| id.to_owned());
        let mut warnings = Vec::new();
        for (section, entries) in [("ui", &file.ui), ("syntax", &file.syntax)] {
            for (key, value) in entries {
                let Some(color) = Color::from_hex(value.trim()) else {
                    warnings.push(format!(
                        "couleur invalide pour « {section}.{key} » : « {value} » (attendu : #rrggbb ou #rrggbbaa)"
                    ));
                    continue;
                };
                let slot = if section == "ui" {
                    theme.ui_slot(key)
                } else {
                    theme.syntax_slot(key)
                };
                match slot {
                    Some(slot) => *slot = color,
                    None => warnings.push(format!("couleur inconnue « {section}.{key} » ignorée")),
                }
            }
        }
        Ok(ParsedTheme { theme, warnings })
    }

    /// Ajoute les thèmes du dossier `dir` (fichiers `.toml`) ; un thème de même identifiant
    /// qu'un thème intégré le remplace. Renvoie les problèmes à signaler. Un dossier absent n'est
    /// pas une erreur.
    pub fn load_dir(&mut self, dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        paths.sort();
        let mut problems = Vec::new();
        for path in paths {
            let Some(id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_lowercase)
            else {
                continue;
            };
            let result = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| self.parse(&text, &id));
            match result {
                Ok(parsed) => {
                    problems.extend(
                        parsed
                            .warnings
                            .into_iter()
                            .map(|w| format!("thème {} : {w}", path.display())),
                    );
                    self.themes.insert(id, parsed.theme);
                }
                Err(error) => problems.push(format!("thème {} ignoré : {error}", path.display())),
            }
        }
        problems
    }

    /// Thème d'identifiant `id` (sans tenir compte de la casse).
    pub fn get(&self, id: &str) -> Option<&Theme> {
        self.themes.get(&id.to_lowercase())
    }

    /// Identifiants des thèmes disponibles, triés.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.themes.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_integres() {
        let themes = Themes::default();
        assert_eq!(
            themes.ids().collect::<Vec<_>>(),
            ["clair", "padparadscha", "sombre"]
        );
        // Le fichier de Padparadscha décrit exactement le thème par défaut.
        assert_eq!(themes.get("padparadscha"), Some(&Theme::default()));
        assert_eq!(themes.get("Clair").unwrap().name, "Clair");
        let clair = themes.get("clair").unwrap();
        assert!(clair.background.r > 0xf0, "fond clair");
        assert_ne!(clair.syntax.keyword, Theme::default().syntax.keyword);
    }

    #[test]
    fn heritage_et_problemes() {
        let themes = Themes::default();
        let parsed = themes
            .parse(
                "base = \"sombre\"\n[ui]\naccent = \"#ff0000\"\nfond = \"#000000\"\ncursor = \"rouge\"\n[syntax]\ntype = \"#00ff0080\"\n",
                "perso",
            )
            .unwrap();
        let sombre = themes.get("sombre").unwrap();
        assert_eq!(parsed.theme.name, "perso", "sans nom : l'identifiant");
        assert_eq!(parsed.theme.accent, Color::rgb(0xff, 0, 0));
        assert_eq!(
            parsed.theme.background, sombre.background,
            "hérité de la base"
        );
        assert_eq!(
            parsed.theme.cursor, sombre.cursor,
            "couleur invalide : base gardée"
        );
        assert_eq!(parsed.theme.syntax.r#type, Color::rgba(0, 0xff, 0, 0x80));
        assert_eq!(parsed.warnings.len(), 2, "{:?}", parsed.warnings);

        assert!(themes.parse("base = \"inconnu\"", "x").is_err());
        assert!(themes.parse("[ui\n", "x").is_err());
        assert!(
            themes.parse("couleurs = 1", "x").is_err(),
            "section inconnue"
        );
    }

    #[test]
    fn dossier_de_l_utilisateur() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Rubis.toml"),
            "name = \"Rubis\"\n[ui]\naccent = \"#e0115f\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("casse.toml"), "[ui\n").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "pas un thème").unwrap();
        let mut themes = Themes::default();
        let problems = themes.load_dir(dir.path());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(themes.get("rubis").unwrap().name, "Rubis");
        assert!(themes.get("casse").is_none());
        assert!(themes.load_dir(&dir.path().join("absent")).is_empty());
    }
}
