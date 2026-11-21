//! Recherche floue, avec `nucleo` (le moteur de Helix) : « fop » trouve « file open ».
//!
//! Le score favorise les lettres consécutives et celles en début de mot ; les ex æquo gardent
//! l'ordre de priorité donné par l'appelant (commandes récentes d'abord, par exemple).

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Élément trouvé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    /// Rang de l'élément dans la liste cherchée.
    pub index: usize,
    /// Score (plus grand : meilleur).
    pub score: u32,
}

/// Moteur de recherche floue, réutilisé d'une recherche à l'autre (pas d'allocation par frappe).
pub struct Fuzzy {
    matcher: Matcher,
    buffer: Vec<char>,
}

impl std::fmt::Debug for Fuzzy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fuzzy").finish_non_exhaustive()
    }
}

impl Fuzzy {
    /// Moteur pour des noms (commandes), ou pour des chemins (`paths` : bonus après les `/`).
    pub fn new(paths: bool) -> Self {
        let config = if paths {
            Config::DEFAULT.match_paths()
        } else {
            Config::DEFAULT
        };
        Self {
            matcher: Matcher::new(config),
            buffer: Vec::new(),
        }
    }

    fn pattern(query: &str) -> Pattern {
        Pattern::parse(query, CaseMatching::Smart, Normalization::Smart)
    }

    /// Éléments de `items` qui correspondent à `query`, du meilleur au moins bon. À score égal,
    /// l'élément de plus petite `priority` passe devant, puis l'ordre de `items`. Une recherche
    /// vide garde tous les éléments, triés par priorité.
    pub fn search<'a>(
        &mut self,
        query: &str,
        items: impl IntoIterator<Item = &'a str>,
        priority: impl Fn(usize) -> usize,
    ) -> Vec<Found> {
        let pattern = Self::pattern(query);
        let mut found: Vec<Found> = items
            .into_iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let haystack = Utf32Str::new(item, &mut self.buffer);
                let score = pattern.score(haystack, &mut self.matcher)?;
                Some(Found { index, score })
            })
            .collect();
        found.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| priority(a.index).cmp(&priority(b.index)))
                .then(a.index.cmp(&b.index))
        });
        found
    }

    /// Positions (en octets) des caractères de `item` qui correspondent à `query`, pour les mettre
    /// en valeur.
    pub fn positions(&mut self, query: &str, item: &str) -> Vec<usize> {
        let pattern = Self::pattern(query);
        let mut indices = Vec::new();
        let haystack = Utf32Str::new(item, &mut self.buffer);
        if pattern
            .indices(haystack, &mut self.matcher, &mut indices)
            .is_none()
        {
            return Vec::new();
        }
        indices.sort_unstable();
        indices.dedup();
        let chars: Vec<usize> = item.char_indices().map(|(i, _)| i).collect();
        indices
            .into_iter()
            .filter_map(|i| chars.get(i as usize).copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recherche_floue() {
        let mut fuzzy = Fuzzy::new(false);
        let items = ["edit undo", "file open", "file reload", "view zoom-in"];
        let found = fuzzy.search("fop", items, |_| 0);
        assert_eq!(found[0].index, 1, "« fop » : file open");
        let found = fuzzy.search("zoom", items, |_| 0);
        assert_eq!(found.len(), 1);
        assert!(fuzzy.search("xyz", items, |_| 0).is_empty());
        // Recherche vide : tout, par priorité.
        let all = fuzzy.search("", items, |i| if i == 2 { 0 } else { 1 });
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].index, 2, "récent en tête");
    }

    #[test]
    fn positions_en_octets() {
        let mut fuzzy = Fuzzy::new(false);
        assert_eq!(fuzzy.positions("fo", "file open"), [0, 5]);
        assert_eq!(
            fuzzy.positions("ét", "été"),
            [0, 2],
            "après « é » (2 octets)"
        );
        assert!(fuzzy.positions("z", "abc").is_empty());
    }

    #[test]
    fn chemins() {
        let mut fuzzy = Fuzzy::new(true);
        let items = ["src/app.rs", "crates/ui/src/editor_view.rs", "docs/perf.md"];
        let found = fuzzy.search("ev", items, |_| 0);
        assert_eq!(found[0].index, 1, "editor_view");
    }
}
