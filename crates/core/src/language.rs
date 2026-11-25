//! Langage d'un document, d'après l'extension ou le nom de son fichier.
//!
//! Sert à l'affichage (barre d'état) ; la coloration syntaxique (épic Syntaxe) s'appuiera sur sa
//! propre détection.

use std::path::Path;

/// Extensions et noms de fichiers connus, et leur langage.
const LANGUAGES: &[(&[&str], &str)] = &[
    (&["rs"], "Rust"),
    (&["toml", "Cargo.lock"], "TOML"),
    (&["md", "markdown"], "Markdown"),
    (&["py", "pyi"], "Python"),
    (&["js", "mjs", "cjs"], "JavaScript"),
    (&["ts", "mts", "cts"], "TypeScript"),
    (&["tsx"], "TSX"),
    (&["jsx"], "JSX"),
    (&["json", "jsonc"], "JSON"),
    (&["yaml", "yml"], "YAML"),
    (&["c", "h"], "C"),
    (&["cpp", "cc", "cxx", "hpp", "hh", "hxx"], "C++"),
    (&["cs"], "C#"),
    (&["go"], "Go"),
    (&["java"], "Java"),
    (&["kt", "kts"], "Kotlin"),
    (&["swift"], "Swift"),
    (&["rb"], "Ruby"),
    (&["php"], "PHP"),
    (&["lua"], "Lua"),
    (&["zig"], "Zig"),
    (&["sh", "bash", "zsh"], "Shell"),
    (&["html", "htm"], "HTML"),
    (&["css"], "CSS"),
    (&["scss"], "SCSS"),
    (&["sql"], "SQL"),
    (&["xml", "svg"], "XML"),
    (&["wgsl"], "WGSL"),
    (&["txt"], "Texte"),
    (&["Dockerfile"], "Dockerfile"),
    (&["Makefile", "makefile", "mk"], "Makefile"),
];

/// Langage du fichier `path`, ou `None` s'il n'est pas reconnu.
pub fn language_of(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    let extension = path.extension().and_then(|e| e.to_str());
    LANGUAGES
        .iter()
        .find(|(keys, _)| keys.contains(&name) || extension.is_some_and(|ext| keys.contains(&ext)))
        .map(|(_, language)| *language)
}

/// Début d'un commentaire de ligne dans le langage `language` (en attendant les paquets de
/// langage, qui le fourniront).
pub fn line_comment(language: &str) -> Option<&'static str> {
    Some(match language {
        "Rust" | "JavaScript" | "TypeScript" | "TSX" | "JSX" | "JSON" | "C" | "C++" | "C#"
        | "Go" | "Java" | "Kotlin" | "Swift" | "PHP" | "Zig" | "SCSS" | "WGSL" => "//",
        "Python" | "TOML" | "YAML" | "Ruby" | "Shell" | "Dockerfile" | "Makefile" => "#",
        "Lua" | "SQL" => "--",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langages() {
        assert_eq!(language_of(Path::new("src/main.rs")), Some("Rust"));
        assert_eq!(language_of(Path::new("Cargo.toml")), Some("TOML"));
        assert_eq!(language_of(Path::new("Cargo.lock")), Some("TOML"));
        assert_eq!(language_of(Path::new("app/Dockerfile")), Some("Dockerfile"));
        assert_eq!(language_of(Path::new("notes.inconnu")), None);
        assert_eq!(language_of(Path::new("")), None);
    }
}
