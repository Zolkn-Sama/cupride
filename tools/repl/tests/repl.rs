//! Scénarios complets : le REPL est lancé comme un vrai programme, piloté par son entrée standard.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

/// Lance le REPL sans configuration utilisateur, envoie `script` et renvoie sa sortie.
fn repl(args: &[&str], script: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cupride-repl"))
        .arg("--no-config")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lancement du REPL");
    child
        .stdin
        .take()
        .expect("entrée standard")
        .write_all(script.as_bytes())
        .expect("écriture du script");
    let output = child.wait_with_output().expect("fin du REPL");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).expect("sortie UTF-8")
}

#[test]
fn ouvrir_modifier_et_enregistrer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("salut.txt");
    fs::write(&path, "bonjour\nau revoir\n").unwrap();
    let out = repl(
        &[path.to_str().unwrap()],
        "edit move line-end\nedit insert ' le monde'\nedit move down\nedit newline\nedit insert 'à bientôt'\nfile save\nquit\n",
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "bonjour le monde\nau revoir\nà bientôt\n"
    );
    assert!(out.contains("1 │ bonjour le monde|"), "{out}");
    assert!(out.contains("« salut.txt » enregistré"), "{out}");
}

#[test]
fn nouveau_fichier_par_commande() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nouveau.rs");
    let script = format!(
        "file open '{}'\nedit insert 'fn main() {{}}'\nedit undo\nedit redo\nfile save\nquit\n",
        path.display()
    );
    let out = repl(&[], &script);
    assert_eq!(fs::read_to_string(&path).unwrap(), "fn main() {}");
    assert!(
        out.contains("── nouveau.rs * — ligne 1, colonne 13"),
        "{out}"
    );
}

#[test]
fn quitter_protege_les_modifications() {
    let out = repl(&[], "file list\nquit\n");
    assert!(out.contains("aucun document ouvert"), "{out}");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    let script = format!(
        "file open '{}'\nedit insert x\nquit\nquit!\n",
        path.display()
    );
    let out = repl(&[], &script);
    assert!(
        out.contains("modifications non enregistrées : a.txt — « quit! » pour quitter quand même"),
        "{out}"
    );
    assert!(!path.exists());
}

#[test]
fn erreurs_et_aide() {
    let out = repl(
        &[],
        "file opne x\nedit insert x\nhelp file open\nhelp\nquit\n",
    );
    assert!(
        out.contains(
            "erreur : commande inconnue : « file opne » (vouliez-vous dire « file open » ?)"
        ),
        "{out}"
    );
    assert!(out.contains("erreur : aucun document ouvert"), "{out}");
    assert!(out.contains("Utilisation : file open <chemin>"), "{out}");
    assert!(
        out.contains("  save-as          Enregistre le document actif sous"),
        "{out}"
    );
}
