//! Recherche dans tous les fichiers du projet, et remplacement avec aperçu.
//!
//! Les fichiers sont parcourus avec `ignore` (`.gitignore`, `.ignore` et fichiers cachés
//! respectés) et lus avec `grep-searcher` (fichiers binaires sautés, ligne par ligne). La
//! recherche est synchrone : l'interface la lance dans un thread, et reçoit les résultats fichier
//! par fichier, à afficher au fil de l'eau. Le noyau garde la recherche en cours et ses résultats
//! ([`Editor::project_search`]).
//!
//! Remplacer dans le projet se fait en deux temps : un aperçu ([`Editor::preview_project_replace`],
//! chaque ligne telle qu'elle deviendrait), puis la confirmation ([`Editor::apply_project_replace`]).

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use regex::Regex;

use crate::command::CommandError;
use crate::editor::Editor;
use crate::search::{SearchError, SearchQuery};
use crate::text::{Edit, EditKind, TextBuffer};

/// Nombre maximal d'occurrences retenues dans le projet.
pub const MAX_PROJECT_MATCHES: usize = 10_000;

/// Longueur maximale (en octets) d'une ligne de résultat gardée pour l'affichage.
const MAX_LINE_LEN: usize = 400;

/// Une ligne qui contient le motif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatch {
    /// Numéro de la ligne, à partir de 0.
    pub line: usize,
    /// Texte de la ligne (sans fin de ligne, tronqué pour l'affichage).
    pub text: String,
    /// Occurrences dans ce texte, en octets.
    pub ranges: Vec<Range<usize>>,
}

/// Les lignes d'un fichier qui contiennent le motif.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    /// Fichier.
    pub path: PathBuf,
    /// Lignes, dans l'ordre.
    pub lines: Vec<LineMatch>,
}

impl FileMatches {
    /// Nombre d'occurrences dans le fichier.
    pub fn count(&self) -> usize {
        self.lines.iter().map(|l| l.ranges.len()).sum()
    }
}

/// Bilan d'une recherche terminée.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Fichiers parcourus.
    pub searched: usize,
    /// La limite d'occurrences a été atteinte : la recherche s'est arrêtée.
    pub truncated: bool,
}

/// Cherche `query` dans les fichiers de `root`. `on_file` reçoit chaque fichier qui contient le
/// motif, dès qu'il est lu ; la recherche s'arrête si `cancel` passe à vrai.
pub fn search_project(
    root: &Path,
    query: &SearchQuery,
    cancel: &AtomicBool,
    mut on_file: impl FnMut(FileMatches),
) -> Result<Summary, SearchError> {
    let regex = query.compile()?;
    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(!query.case_sensitive)
        .build(&query.regex_source()?)
        .map_err(|e| SearchError::Regex(e.to_string()))?;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\0'))
        .line_number(true)
        .build();
    let mut summary = Summary::default();
    let mut total = 0;
    for entry in ignore::WalkBuilder::new(root).require_git(false).build() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        summary.searched += 1;
        let mut lines = Vec::new();
        let result = searcher.search_path(
            &matcher,
            entry.path(),
            UTF8(|number, line| {
                if let Some(found) = line_match(&regex, number, line) {
                    total += found.ranges.len();
                    lines.push(found);
                }
                Ok(total < MAX_PROJECT_MATCHES && !cancel.load(Ordering::Relaxed))
            }),
        );
        if let Err(error) = result {
            tracing::debug!(fichier = %entry.path().display(), "illisible : {error}");
            continue;
        }
        if !lines.is_empty() {
            on_file(FileMatches {
                path: entry.into_path(),
                lines,
            });
        }
        if total >= MAX_PROJECT_MATCHES {
            summary.truncated = true;
            break;
        }
    }
    Ok(summary)
}

/// Occurrences de `regex` dans la ligne `number` (à partir de 1) ; texte tronqué pour
/// l'affichage, sur une frontière de caractère.
fn line_match(regex: &Regex, number: u64, line: &str) -> Option<LineMatch> {
    let line = line.trim_end_matches(['\n', '\r']);
    let ranges: Vec<Range<usize>> = regex
        .find_iter(line)
        .filter(|m| !m.is_empty())
        .map(|m| m.range())
        .collect();
    if ranges.is_empty() {
        return None;
    }
    let mut end = line.len().min(MAX_LINE_LEN);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    let ranges = ranges
        .into_iter()
        .filter(|r| r.start < end)
        .map(|r| r.start..r.end.min(end))
        .collect();
    Some(LineMatch {
        line: usize::try_from(number).unwrap_or(1).saturating_sub(1),
        text: line[..end].to_owned(),
        ranges,
    })
}

/// Recherche dans le projet en cours ou terminée, et ses résultats.
#[derive(Debug, Clone, Default)]
pub struct ProjectSearch {
    /// Motif.
    pub query: SearchQuery,
    /// Dossier parcouru.
    pub root: PathBuf,
    /// Fichiers qui contiennent le motif, dans l'ordre d'arrivée.
    pub files: Vec<FileMatches>,
    /// Bilan, une fois la recherche terminée.
    pub summary: Option<Summary>,
    /// Remplacement en aperçu, en attente de confirmation.
    pub replacement: Option<String>,
    /// Numéro de la recherche : des résultats d'une recherche remplacée sont ignorés.
    pub generation: u64,
}

impl ProjectSearch {
    /// Nombre d'occurrences trouvées.
    pub fn count(&self) -> usize {
        self.files.iter().map(FileMatches::count).sum()
    }

    /// Ligne telle qu'elle deviendrait avec le remplacement en aperçu.
    pub fn preview(&self, line: &LineMatch) -> Option<String> {
        let with = self.replacement.as_ref()?;
        let regex = self.query.compile().ok()?;
        let mut result = String::new();
        let mut last = 0;
        for captures in regex.captures_iter(&line.text) {
            let whole = captures.get(0)?;
            if whole.is_empty() {
                continue;
            }
            result.push_str(&line.text[last..whole.start()]);
            result.push_str(&crate::search::replacement(&self.query, &captures, with));
            last = whole.end();
        }
        result.push_str(&line.text[last..]);
        Some(result)
    }

    /// Bilan à afficher : « 12 résultats dans 3 fichiers ».
    pub fn describe(&self) -> String {
        let count = self.count();
        let files = self.files.len();
        let mut text = format!(
            "{count} résultat{} dans {files} fichier{}",
            if count > 1 { "s" } else { "" },
            if files > 1 { "s" } else { "" }
        );
        match self.summary {
            None => text.push_str(" (recherche en cours…)"),
            Some(summary) if summary.truncated => text.push_str(" (limite atteinte)"),
            Some(_) => {}
        }
        text
    }
}

impl Editor {
    /// Recherche dans le projet, si une a été lancée.
    pub fn project_search(&self) -> Option<&ProjectSearch> {
        self.project_search.as_ref()
    }

    /// Dossier où chercher : le projet, sinon le dossier courant.
    pub fn search_root(&self) -> Option<PathBuf> {
        self.project()
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
    }

    /// Commence une recherche dans le projet : les résultats précédents sont oubliés. Renvoie
    /// son numéro, à donner avec ses résultats.
    pub fn start_project_search(&mut self, query: SearchQuery, root: PathBuf) -> u64 {
        let generation = self.project_search.as_ref().map_or(1, |s| s.generation + 1);
        self.project_search = Some(ProjectSearch {
            query,
            root,
            generation,
            ..ProjectSearch::default()
        });
        generation
    }

    /// Ajoute les résultats d'un fichier à la recherche `generation`.
    pub fn add_project_results(&mut self, generation: u64, files: Vec<FileMatches>) {
        if let Some(search) = self
            .project_search
            .as_mut()
            .filter(|s| s.generation == generation)
        {
            search.files.extend(files);
        }
    }

    /// Termine la recherche `generation`.
    pub fn finish_project_search(&mut self, generation: u64, summary: Summary) {
        if let Some(search) = self
            .project_search
            .as_mut()
            .filter(|s| s.generation == generation)
        {
            search.summary = Some(summary);
        }
    }

    /// Cherche dans le projet sans thread (sans interface : REPL, tests).
    pub fn run_project_search(&mut self, query: SearchQuery) -> Result<String, CommandError> {
        let root = self
            .search_root()
            .ok_or_else(|| CommandError::Failed("aucun dossier où chercher".into()))?;
        let generation = self.start_project_search(query.clone(), root.clone());
        let mut files = Vec::new();
        let summary = search_project(&root, &query, &AtomicBool::new(false), |f| files.push(f))
            .map_err(|e| CommandError::Failed(e.to_string()))?;
        self.add_project_results(generation, files);
        self.finish_project_search(generation, summary);
        Ok(self
            .project_search
            .as_ref()
            .map(ProjectSearch::describe)
            .unwrap_or_default())
    }

    /// Met en aperçu le remplacement des résultats par `with` : chaque ligne s'affiche telle
    /// qu'elle deviendrait ; rien n'est modifié avant [`Editor::apply_project_replace`].
    pub fn preview_project_replace(&mut self, with: &str) -> Result<String, CommandError> {
        let search = self.project_search.as_mut().ok_or_else(no_search)?;
        if search.files.is_empty() {
            return Err(CommandError::Failed("aucun résultat à remplacer".into()));
        }
        search.replacement = Some(with.to_owned());
        Ok(format!(
            "aperçu : {} — confirmer avec « search project-apply »",
            search.describe()
        ))
    }

    /// Applique le remplacement en aperçu à tous les fichiers des résultats. Un fichier ouvert
    /// est modifié dans son document (une seule modification annulable, à enregistrer) ; un
    /// autre est réécrit sur le disque (enregistrement atomique). Renvoie le nombre de
    /// remplacements et de fichiers.
    pub fn apply_project_replace(&mut self) -> Result<(usize, usize), CommandError> {
        let search = self.project_search.as_ref().ok_or_else(no_search)?;
        let with = search.replacement.clone().ok_or_else(|| {
            CommandError::Failed("aucun remplacement en aperçu : « search project-replace »".into())
        })?;
        let query = search.query.clone();
        let regex = query
            .compile()
            .map_err(|e| CommandError::Failed(e.to_string()))?;
        let paths: Vec<PathBuf> = search.files.iter().map(|f| f.path.clone()).collect();
        let (mut count, mut changed) = (0, 0);
        for path in paths {
            let already_open = self.find_by_path(&path);
            let id = match already_open {
                Some(id) => id,
                None => self.load(&path)?,
            };
            let Some(document) = self.document_mut(id) else {
                continue;
            };
            let text = document.buffer().to_text();
            let replacements: Vec<(Range<usize>, String)> = regex
                .captures_iter(&text)
                .filter_map(|c| {
                    let whole = c.get(0)?;
                    (!whole.is_empty())
                        .then(|| (whole.range(), crate::search::replacement(&query, &c, &with)))
                })
                .collect();
            if !replacements.is_empty() {
                document.transact(EditKind::Other, |buffer, tx| {
                    for (range, new_text) in replacements.iter().rev() {
                        let edit = Edit::replace(buffer, range.clone(), new_text.clone())?;
                        tx.apply(buffer, edit)?;
                    }
                    Ok(())
                })?;
                count += replacements.len();
                changed += 1;
                if already_open.is_none() {
                    document.save()?;
                }
            }
            if already_open.is_none() {
                self.close(id);
            }
        }
        if let Some(search) = &mut self.project_search {
            search.replacement = None;
            search.files.clear();
        }
        self.flush_events();
        Ok((count, changed))
    }
}

fn no_search() -> CommandError {
    CommandError::Failed("aucune recherche dans le projet : « search project <motif> »".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Projet : deux fichiers qui contiennent « cible », un ignoré, un binaire.
    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();
        fs::write(root.join("src/a.rs"), "une cible\nrien\nCible et cible\n").unwrap();
        fs::write(root.join("src/b.rs"), "pas ici\n").unwrap();
        fs::write(root.join("README.md"), "la cible du projet\n").unwrap();
        fs::write(root.join("target/out.txt"), "cible ignorée\n").unwrap();
        fs::write(root.join("image.bin"), b"cible\0\0binaire").unwrap();
        (dir, root)
    }

    fn found(root: &Path, query: &SearchQuery) -> Vec<FileMatches> {
        let mut files = Vec::new();
        search_project(root, query, &AtomicBool::new(false), |f| files.push(f)).unwrap();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files
    }

    #[test]
    fn chercher_dans_le_projet() {
        let (_dir, root) = project();
        let files = found(&root, &SearchQuery::new("cible"));
        let names: Vec<_> = files
            .iter()
            .map(|f| {
                f.path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            names,
            ["README.md", "src/a.rs"],
            ".gitignore et binaires exclus"
        );
        let a = &files[1];
        assert_eq!(a.lines.len(), 2);
        assert_eq!(a.lines[0].line, 0);
        assert_eq!(a.lines[1].line, 2);
        assert_eq!(a.lines[1].text, "Cible et cible");
        assert_eq!(a.lines[1].ranges, vec![0..5, 9..14]);
        assert_eq!(a.count(), 3);
        let case = SearchQuery {
            case_sensitive: true,
            ..SearchQuery::new("Cible")
        };
        assert_eq!(
            found(&root, &case)
                .iter()
                .map(FileMatches::count)
                .sum::<usize>(),
            1
        );
    }

    #[test]
    fn annulee() {
        let (_dir, root) = project();
        let mut files = Vec::new();
        search_project(
            &root,
            &SearchQuery::new("cible"),
            &AtomicBool::new(true),
            |f| files.push(f),
        )
        .unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn remplacer_dans_le_projet_avec_apercu() {
        let (_dir, root) = project();
        let mut editor = Editor::new();
        editor.set_project(&root).unwrap();
        // Un des fichiers est ouvert : il est modifié dans son document, pas sur le disque.
        let open = editor.open(root.join("README.md")).unwrap();
        let summary = editor
            .run_project_search(SearchQuery::new("cible"))
            .unwrap();
        assert_eq!(summary, "4 résultats dans 2 fichiers");
        let message = editor.preview_project_replace("but").unwrap();
        assert!(message.contains("search project-apply"), "{message}");
        let search = editor.project_search().unwrap();
        let a = search
            .files
            .iter()
            .find(|f| f.path.ends_with("a.rs"))
            .unwrap();
        assert_eq!(search.preview(&a.lines[1]).as_deref(), Some("but et but"));
        assert_eq!(
            fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "une cible\nrien\nCible et cible\n",
            "rien n'est écrit avant la confirmation"
        );
        assert_eq!(editor.apply_project_replace().unwrap(), (4, 2));
        assert_eq!(
            fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "une but\nrien\nbut et but\n"
        );
        let readme = editor.document(open).unwrap();
        assert_eq!(readme.buffer().to_text(), "la but du projet\n");
        assert!(readme.is_modified(), "ouvert : à enregistrer");
        assert_eq!(
            fs::read_to_string(root.join("README.md")).unwrap(),
            "la cible du projet\n"
        );
        assert_eq!(
            editor.document_count(),
            1,
            "les fichiers non ouverts sont refermés"
        );
        assert!(
            editor.apply_project_replace().is_err(),
            "plus rien en aperçu"
        );
    }
}
