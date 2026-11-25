//! Langage d'un document, d'après l'extension ou le nom de son fichier, ou sa ligne `#!`.
//!
//! Le nom du langage sert à l'affichage (barre d'état), à la configuration propre au langage
//! (`[languages.<Nom>]`) et à choisir sa grammaire (`cupride-syntax`).

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

/// Langage d'un document : d'après son fichier `path` (extension ou nom), sinon d'après sa
/// première ligne `first_line` (`#!/usr/bin/env python3`).
pub fn detect(path: Option<&Path>, first_line: &str) -> Option<&'static str> {
    path.and_then(language_of).or_else(|| shebang(first_line))
}

/// Programmes des lignes `#!` connus, et leur langage.
const INTERPRETERS: &[(&[&str], &str)] = &[
    (&["python", "pypy"], "Python"),
    (&["sh", "bash", "zsh", "dash", "ksh"], "Shell"),
    (&["node", "nodejs", "deno", "bun"], "JavaScript"),
    (&["ruby"], "Ruby"),
    (&["lua", "luajit"], "Lua"),
    (&["php"], "PHP"),
    (&["rust-script", "cargo"], "Rust"),
];

/// Langage désigné par la ligne `#!` `line` : `#!/bin/bash`, `#!/usr/bin/env python3`,
/// `#!/usr/bin/env -S cargo +nightly -Zscript`.
fn shebang(line: &str) -> Option<&'static str> {
    let mut words = line.strip_prefix("#!")?.split_whitespace();
    let mut program = words.next()?.rsplit('/').next()?;
    if program == "env" {
        program = words.find(|w| !w.starts_with('-') && !w.contains('='))?;
    }
    // python3.12 → python
    let program = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    INTERPRETERS
        .iter()
        .find(|(names, _)| names.contains(&program))
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

    #[test]
    fn ligne_shebang() {
        let none = None;
        assert_eq!(detect(none, "#!/bin/bash"), Some("Shell"));
        assert_eq!(detect(none, "#!/usr/bin/env python3.12"), Some("Python"));
        assert_eq!(detect(none, "#! /usr/bin/env node"), Some("JavaScript"));
        assert_eq!(
            detect(none, "#!/usr/bin/env -S cargo +nightly -Zscript"),
            Some("Rust")
        );
        assert_eq!(detect(none, "#!/usr/bin/env -S FOO=1 ruby"), Some("Ruby"));
        assert_eq!(detect(none, "#!/opt/inconnu"), None);
        assert_eq!(detect(none, "fn main() {}"), None);
        // L'extension l'emporte sur la ligne `#!`.
        assert_eq!(
            detect(Some(Path::new("outil.rs")), "#!/bin/sh"),
            Some("Rust")
        );
        assert_eq!(detect(Some(Path::new("outil")), "#!/bin/sh"), Some("Shell"));
    }
}
