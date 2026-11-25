//! Indentation d'un document : tabulations ou espaces, et largeur.
//!
//! Le réglage vient de la configuration, globalement (`[editor] indent_style`, `indent_width`)
//! et par langage (`[languages.Rust]`) ; avec `auto` (par défaut), l'indentation d'un fichier
//! existant est détectée d'après son contenu, et la configuration ne sert que pour un fichier
//! vide ou sans indentation.

use serde::Deserialize;

/// Lignes examinées pour détecter l'indentation.
const DETECT_LINES: usize = 2_000;

/// Indentation d'un document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    /// Tabulations (sinon espaces).
    pub tabs: bool,
    /// Largeur d'un niveau, en colonnes (aussi la largeur affichée d'une tabulation).
    pub width: usize,
}

impl Default for Indent {
    fn default() -> Self {
        Self {
            tabs: false,
            width: 4,
        }
    }
}

impl Indent {
    /// Un niveau d'indentation : une tabulation, ou `width` espaces.
    pub fn unit(&self) -> String {
        if self.tabs {
            "\t".to_owned()
        } else {
            " ".repeat(self.width)
        }
    }

    /// Description pour la barre d'état : « Espaces : 4 », « Tabulations : 4 ».
    pub fn describe(&self) -> String {
        let kind = if self.tabs { "Tabulations" } else { "Espaces" };
        format!("{kind} : {}", self.width)
    }
}

/// Réglage de l'indentation dans la configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndentStyle {
    /// Détectée d'après le fichier ; espaces pour un fichier sans indentation.
    #[default]
    Auto,
    /// Espaces.
    Spaces,
    /// Tabulations.
    Tabs,
}

/// Indentation détectée dans un texte : tabulations ou espaces, et largeur si elle se déduit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    /// Tabulations en majorité.
    pub tabs: bool,
    /// Largeur la plus fréquente entre deux niveaux (espaces).
    pub width: Option<usize>,
}

/// Indentation d'un texte, d'après ses premières lignes : tabulations si plus de lignes en
/// commencent par une que par des espaces ; largeur : l'écart le plus fréquent entre deux lignes
/// dont l'indentation augmente. `None` sans aucune ligne indentée.
pub fn detect(text: &str) -> Option<Detected> {
    let (mut tabs, mut spaces) = (0_usize, 0_usize);
    let mut deltas = [0_usize; 9];
    let mut previous = 0;
    for line in text.lines().take(DETECT_LINES) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('\t') {
            tabs += 1;
            previous = 0;
            continue;
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent > 0 {
            spaces += 1;
        }
        if indent > previous && indent - previous <= 8 {
            deltas[indent - previous] += 1;
        }
        previous = indent;
    }
    if tabs == 0 && spaces == 0 {
        return None;
    }
    let width = (2..=8)
        .filter(|&w| deltas[w] > 0)
        .max_by_key(|&w| (deltas[w], std::cmp::Reverse(w)));
    Some(Detected {
        tabs: tabs > spaces,
        width: if tabs > spaces { None } else { width },
    })
}

/// Indentation d'un document : le réglage (`style`, `width`), ou, avec `auto`, ce qui est
/// détecté dans `text`, complété par la largeur réglée.
pub fn resolve(style: IndentStyle, width: usize, text: &str) -> Indent {
    let width = width.clamp(1, 16);
    match style {
        IndentStyle::Spaces => Indent { tabs: false, width },
        IndentStyle::Tabs => Indent { tabs: true, width },
        IndentStyle::Auto => match detect(text) {
            Some(found) => Indent {
                tabs: found.tabs,
                width: found.width.unwrap_or(width),
            },
            None => Indent { tabs: false, width },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection() {
        let two = "a {\n  b {\n    c\n  }\n}\n";
        assert_eq!(
            detect(two),
            Some(Detected {
                tabs: false,
                width: Some(2)
            })
        );
        let four = "fn x() {\n    if y {\n        z\n    }\n}\n";
        assert_eq!(detect(four).unwrap().width, Some(4));
        let tabs = "a:\n\tb\n\tc\n\t\td\n";
        assert_eq!(
            detect(tabs),
            Some(Detected {
                tabs: true,
                width: None
            })
        );
        assert_eq!(detect("plat\nsans indentation\n"), None);
        // Une ligne de continuation isolée ne l'emporte pas sur l'indentation habituelle.
        let mixed = "a\n    b\n    c\n        d\n    e\n f\n";
        assert_eq!(detect(mixed).unwrap().width, Some(4));
    }

    #[test]
    fn resolution() {
        let tabs = "a\n\tb\n";
        assert_eq!(
            resolve(IndentStyle::Auto, 4, tabs),
            Indent {
                tabs: true,
                width: 4
            }
        );
        assert_eq!(
            resolve(IndentStyle::Spaces, 2, tabs),
            Indent {
                tabs: false,
                width: 2
            }
        );
        assert_eq!(
            resolve(IndentStyle::Auto, 3, ""),
            Indent {
                tabs: false,
                width: 3
            }
        );
        assert_eq!(
            resolve(IndentStyle::Auto, 8, "a\n  b\n"),
            Indent {
                tabs: false,
                width: 2
            }
        );
        assert_eq!(resolve(IndentStyle::Tabs, 0, "").width, 1, "bornée");
        assert_eq!(
            Indent {
                tabs: false,
                width: 2
            }
            .unit(),
            "  "
        );
        assert_eq!(Indent::default().describe(), "Espaces : 4");
    }
}
