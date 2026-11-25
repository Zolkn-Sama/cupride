//! Recherche dans un document : motif, options, occurrences, et la recherche en cours.
//!
//! Le motif est du texte, ou une expression régulière (`regex`) ; options : respect de la casse,
//! mot entier. Les occurrences se calculent sur un texte figé (`&str`) : l'interface les cherche
//! dans un thread à part sur une copie du document, pour ne jamais bloquer, et les donne au noyau
//! ([`Editor::set_search_results`]). Sans interface, les commandes les calculent elles-mêmes.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use regex::{Regex, RegexBuilder};
use thiserror::Error;

use crate::command::CommandError;
use crate::editor::{DocumentId, Editor};
use crate::text::{Edit, EditKind, Selection, Selections, TextBuffer};

/// Nombre maximal d'occurrences retenues : au-delà, la recherche s'arrête (et le signale).
pub const MAX_MATCHES: usize = 100_000;

/// Ce qu'on cherche.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// Motif : texte, ou expression régulière avec `regex`.
    pub pattern: String,
    /// Respecter la casse.
    pub case_sensitive: bool,
    /// Mot entier seulement.
    pub whole_word: bool,
    /// Le motif est une expression régulière.
    pub regex: bool,
}

/// Motif invalide.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SearchError {
    /// Motif vide.
    #[error("motif de recherche vide")]
    Empty,
    /// Expression régulière invalide.
    #[error("expression régulière invalide : {0}")]
    Regex(String),
}

impl SearchQuery {
    /// Motif `pattern`, sans option.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            ..Self::default()
        }
    }

    /// Expression régulière équivalente.
    pub fn compile(&self) -> Result<Regex, SearchError> {
        if self.pattern.is_empty() {
            return Err(SearchError::Empty);
        }
        let pattern = if self.regex {
            self.pattern.clone()
        } else {
            regex::escape(&self.pattern)
        };
        let pattern = if self.whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .multi_line(true)
            .size_limit(50 * (1 << 20))
            .build()
            .map_err(|e| SearchError::Regex(first_line(&e.to_string())))
    }

    /// Description courte des options : `Aa`, `mot`, `.*`.
    pub fn describe_options(&self) -> String {
        let mut options = Vec::new();
        if self.case_sensitive {
            options.push("casse");
        }
        if self.whole_word {
            options.push("mot entier");
        }
        if self.regex {
            options.push("regex");
        }
        options.join(", ")
    }
}

/// Dernière ligne utile d'un message d'erreur de `regex` (qui en fait plusieurs).
fn first_line(message: &str) -> String {
    message
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(message)
        .trim()
        .trim_start_matches("error: ")
        .to_owned()
}

/// Occurrences de `regex` dans `text`, dans l'ordre, sans occurrence vide, au plus
/// [`MAX_MATCHES`]. S'arrête dès que `cancel` passe à vrai (une recherche plus récente l'a
/// remplacée) : renvoie alors `None`.
pub fn find_all(text: &str, regex: &Regex, cancel: &AtomicBool) -> Option<Vec<Range<usize>>> {
    let mut matches = Vec::new();
    for (index, found) in regex.find_iter(text).enumerate() {
        if index % 4096 == 0 && cancel.load(Ordering::Relaxed) {
            return None;
        }
        if found.is_empty() {
            continue;
        }
        matches.push(found.range());
        if matches.len() >= MAX_MATCHES {
            break;
        }
    }
    Some(matches)
}

/// Recherche en cours : motif, et occurrences dans un document à une révision donnée.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    query: Option<SearchQuery>,
    results: Option<(DocumentId, u64, Vec<Range<usize>>)>,
}

impl SearchState {
    /// Motif en cours.
    pub fn query(&self) -> Option<&SearchQuery> {
        self.query.as_ref()
    }
}

impl Editor {
    /// Recherche en cours.
    pub fn search(&self) -> &SearchState {
        &self.search
    }

    /// Change le motif : les occurrences connues sont oubliées. `None` arrête la recherche.
    pub fn set_search_query(&mut self, query: Option<SearchQuery>) {
        if self.search.query != query {
            self.search.query = query;
            self.search.results = None;
        }
    }

    /// Donne les occurrences du motif dans `document` à la révision `revision` (calculées par
    /// l'interface, dans un thread à part).
    pub fn set_search_results(
        &mut self,
        document: DocumentId,
        revision: u64,
        matches: Vec<Range<usize>>,
    ) {
        self.search.results = Some((document, revision, matches));
    }

    /// Occurrences connues dans `document`, si elles sont à jour (même révision).
    pub fn search_matches(&self, document: DocumentId) -> Option<&[Range<usize>]> {
        let (id, revision, matches) = self.search.results.as_ref()?;
        let current = self.document(document)?.revision();
        (*id == document && *revision == current).then_some(matches.as_slice())
    }

    /// Occurrences dans le document actif ; calculées maintenant si elles ne sont pas à jour.
    fn active_matches(&mut self) -> Result<(DocumentId, Vec<Range<usize>>), CommandError> {
        let query = self
            .search
            .query
            .clone()
            .ok_or_else(|| CommandError::Failed("aucune recherche en cours".into()))?;
        let id = self.active_id().ok_or(CommandError::NoActiveDocument)?;
        if let Some(matches) = self.search_matches(id) {
            return Ok((id, matches.to_vec()));
        }
        let regex = query
            .compile()
            .map_err(|e| CommandError::Failed(e.to_string()))?;
        let document = self.document(id).ok_or(CommandError::NoActiveDocument)?;
        let revision = document.revision();
        let text = document.buffer().to_text();
        let matches = find_all(&text, &regex, &AtomicBool::new(false)).unwrap_or_default();
        self.set_search_results(id, revision, matches.clone());
        Ok((id, matches))
    }

    /// Sélectionne l'occurrence suivante (ou précédente) à partir de `from` (par défaut, la
    /// sélection principale), en revenant au début après la dernière. Renvoie la position de
    /// l'occurrence (à partir de 1) et leur nombre.
    pub fn select_match(
        &mut self,
        forward: bool,
        from: Option<usize>,
    ) -> Result<(usize, usize), CommandError> {
        let (id, matches) = self.active_matches()?;
        let query = self.search.query.clone().unwrap_or_default();
        if matches.is_empty() {
            return Err(CommandError::Failed(format!(
                "« {} » introuvable",
                query.pattern
            )));
        }
        let document = self
            .document_mut(id)
            .ok_or(CommandError::NoActiveDocument)?;
        let primary = document.selections().primary();
        let index = if forward {
            let from = from.unwrap_or(primary.end());
            matches.iter().position(|m| m.start >= from).unwrap_or(0)
        } else {
            let from = from.unwrap_or(primary.start());
            matches
                .iter()
                .rposition(|m| m.end <= from)
                .unwrap_or(matches.len() - 1)
        };
        let found = &matches[index];
        document.set_selections(Selections::single(Selection::new(found.start, found.end)));
        Ok((index + 1, matches.len()))
    }
}

/// Texte qui remplace une occurrence : en mode expression régulière, `$1`, `${nom}` et `$0`
/// reprennent les groupes de capture (`$$` pour un `$`) ; sinon, le texte tel quel.
fn replacement(query: &SearchQuery, captures: &regex::Captures<'_>, with: &str) -> String {
    if query.regex {
        let mut text = String::new();
        captures.expand(with, &mut text);
        text
    } else {
        with.to_owned()
    }
}

impl Editor {
    /// Remplace l'occurrence sélectionnée par `with` (groupes de capture en mode expression
    /// régulière), puis sélectionne la suivante. Si la sélection n'est pas une occurrence, la
    /// suivante est seulement sélectionnée. Renvoie la position de l'occurrence sélectionnée et
    /// leur nombre, ou `None` s'il n'en reste aucune.
    pub fn replace_match(&mut self, with: &str) -> Result<Option<(usize, usize)>, CommandError> {
        let query = self
            .search
            .query
            .clone()
            .ok_or_else(|| CommandError::Failed("aucune recherche en cours".into()))?;
        let regex = query
            .compile()
            .map_err(|e| CommandError::Failed(e.to_string()))?;
        let (id, document) = crate::command::builtin_active(self)?;
        let selection = document.selections().primary();
        let text = document.buffer().to_text();
        let found = regex
            .captures_at(&text, selection.start())
            .filter(|c| c.get(0).is_some_and(|m| m.range() == selection.range()));
        let Some(captures) = found else {
            return match self.select_match(true, Some(selection.start())) {
                Ok(position) => Ok(Some(position)),
                Err(_) => Ok(None),
            };
        };
        let new_text = replacement(&query, &captures, with);
        let range = selection.range();
        let document = self
            .document_mut(id)
            .ok_or(CommandError::NoActiveDocument)?;
        document.transact(EditKind::Other, |buffer, tx| {
            let edit = Edit::replace(buffer, range.clone(), new_text.clone())?;
            tx.apply(buffer, edit)?;
            tx.set_selections_after(Selections::cursor(range.start + new_text.len()));
            Ok(())
        })?;
        self.flush_events();
        match self.select_match(true, None) {
            Ok(position) => Ok(Some(position)),
            Err(_) => Ok(None),
        }
    }

    /// Remplace toutes les occurrences par `with`, en **une seule** modification : un seul
    /// « annuler » les rétablit toutes. Renvoie le nombre de remplacements.
    pub fn replace_all(&mut self, with: &str) -> Result<usize, CommandError> {
        let query = self
            .search
            .query
            .clone()
            .ok_or_else(|| CommandError::Failed("aucune recherche en cours".into()))?;
        let regex = query
            .compile()
            .map_err(|e| CommandError::Failed(e.to_string()))?;
        let (id, document) = crate::command::builtin_active(self)?;
        let text = document.buffer().to_text();
        let replacements: Vec<(Range<usize>, String)> = regex
            .captures_iter(&text)
            .filter_map(|c| {
                let whole = c.get(0)?;
                (!whole.is_empty()).then(|| (whole.range(), replacement(&query, &c, with)))
            })
            .collect();
        if replacements.is_empty() {
            return Err(CommandError::Failed(format!(
                "« {} » introuvable",
                query.pattern
            )));
        }
        let count = replacements.len();
        let document = self
            .document_mut(id)
            .ok_or(CommandError::NoActiveDocument)?;
        document.transact(EditKind::Other, |buffer, tx| {
            // De la fin vers le début : les positions des occurrences restantes ne bougent pas.
            for (range, new_text) in replacements.iter().rev() {
                let edit = Edit::replace(buffer, range.clone(), new_text.clone())?;
                tx.apply(buffer, edit)?;
            }
            Ok(())
        })?;
        self.flush_events();
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(text: &str, query: &SearchQuery) -> Vec<Range<usize>> {
        find_all(text, &query.compile().unwrap(), &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn options() {
        let text = "Chat chat chaton CHAT";
        assert_eq!(
            all(text, &SearchQuery::new("chat")).len(),
            4,
            "casse ignorée"
        );
        let case = SearchQuery {
            case_sensitive: true,
            ..SearchQuery::new("chat")
        };
        assert_eq!(all(text, &case), vec![5..9, 10..14]);
        let word = SearchQuery {
            whole_word: true,
            ..SearchQuery::new("chat")
        };
        assert_eq!(all(text, &word), vec![0..4, 5..9, 17..21]);
        let regex = SearchQuery {
            regex: true,
            ..SearchQuery::new(r"ch\w+n")
        };
        assert_eq!(all(text, &regex), vec![10..16]);
        // Sans regex, les caractères spéciaux sont du texte.
        assert_eq!(all("a.b axb", &SearchQuery::new("a.b")), vec![0..3]);
        // Unicode : positions en octets.
        assert_eq!(all("été, ÉTÉ", &SearchQuery::new("été")), vec![0..5, 7..12]);
    }

    #[test]
    fn motifs_invalides() {
        assert_eq!(
            SearchQuery::new("").compile().unwrap_err(),
            SearchError::Empty
        );
        let bad = SearchQuery {
            regex: true,
            ..SearchQuery::new("(")
        };
        assert!(matches!(bad.compile(), Err(SearchError::Regex(_))));
        // Occurrences vides ignorées.
        let empty = SearchQuery {
            regex: true,
            ..SearchQuery::new("x*")
        };
        assert_eq!(all("axxb", &empty), vec![1..3]);
    }

    #[test]
    fn annulation() {
        let cancel = AtomicBool::new(true);
        let regex = SearchQuery::new("a").compile().unwrap();
        assert_eq!(find_all("aaaa", &regex, &cancel), None);
    }

    #[test]
    fn suivante_precedente() {
        let mut editor = Editor::new();
        editor.insert_document(crate::Document::from_text("un deux un trois un"));
        editor.set_search_query(Some(SearchQuery::new("un")));
        assert_eq!(editor.select_match(true, None).unwrap(), (1, 3));
        assert_eq!(editor_selection(&editor), 0..2);
        editor.select_match(true, None).unwrap();
        assert_eq!(editor_selection(&editor), 8..10);
        editor.select_match(true, None).unwrap();
        assert_eq!(
            editor.select_match(true, None).unwrap(),
            (1, 3),
            "revient au début"
        );
        assert_eq!(
            editor.select_match(false, None).unwrap(),
            (3, 3),
            "puis à la fin"
        );
        editor.set_search_query(Some(SearchQuery::new("zut")));
        let error = editor.select_match(true, None).unwrap_err().to_string();
        assert_eq!(error, "« zut » introuvable");
    }

    fn editor_selection(editor: &Editor) -> Range<usize> {
        editor
            .active_document()
            .unwrap()
            .selections()
            .primary()
            .range()
    }

    #[test]
    fn resultats_perimes_ignores() {
        let mut editor = Editor::new();
        let id = editor.insert_document(crate::Document::from_text("abc abc"));
        editor.set_search_query(Some(SearchQuery::new("abc")));
        let found: Vec<Range<usize>> = std::iter::once(0..3).collect();
        editor.set_search_results(id, 0, found.clone());
        assert_eq!(editor.search_matches(id), Some(found.as_slice()));
        editor.execute("edit insert 'x'").unwrap();
        assert_eq!(editor.search_matches(id), None, "le texte a changé");
    }

    #[test]
    fn remplacer() {
        let mut editor = Editor::new();
        editor.insert_document(crate::Document::from_text("chat, chat et chaton"));
        editor.set_search_query(Some(SearchQuery {
            whole_word: true,
            ..SearchQuery::new("chat")
        }));
        // La sélection n'est pas une occurrence : la première est seulement sélectionnée.
        assert_eq!(editor.replace_match("chien").unwrap(), Some((1, 2)));
        assert_eq!(text(&editor), "chat, chat et chaton");
        assert_eq!(editor.replace_match("chien").unwrap(), Some((1, 1)));
        assert_eq!(text(&editor), "chien, chat et chaton");
        assert_eq!(editor.replace_match("chien").unwrap(), None, "plus aucune");
        assert_eq!(text(&editor), "chien, chien et chaton");
    }

    #[test]
    fn tout_remplacer_en_une_transaction() {
        let mut editor = Editor::new();
        editor.insert_document(crate::Document::from_text("a=1; b=22; c=333;"));
        editor.set_search_query(Some(SearchQuery {
            regex: true,
            ..SearchQuery::new(r"(\w)=(\d+)")
        }));
        assert_eq!(
            editor.replace_all("$2:${1}").unwrap(),
            3,
            "groupes de capture"
        );
        assert_eq!(text(&editor), "1:a; 22:b; 333:c;");
        editor.execute("edit undo").unwrap();
        assert_eq!(text(&editor), "a=1; b=22; c=333;", "un seul annuler");
        // Sans expression régulière, le remplacement est littéral.
        editor.set_search_query(Some(SearchQuery::new("b=22")));
        editor.replace_all("$1").unwrap();
        assert_eq!(text(&editor), "a=1; $1; c=333;");
        editor.set_search_query(Some(SearchQuery::new("zut")));
        assert!(editor.replace_all("x").is_err());
    }

    fn text(editor: &Editor) -> String {
        editor.active_document().unwrap().buffer().to_text()
    }
}
