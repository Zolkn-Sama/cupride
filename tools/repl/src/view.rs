//! Affichage du document actif dans la console : quelques lignes autour du curseur principal.

use cupride_core::Document;
use cupride_core::text::TextBuffer;
use cupride_core::text::position::offset_to_grapheme_point;

/// Nombre de lignes affichées de part et d'autre du curseur principal.
pub(crate) const CONTEXT_LINES: usize = 2;

/// Rendu du document : une ligne d'état, puis les lignes autour du curseur principal, numérotées
/// à partir de 1, avec un `|` à la position de chaque curseur.
pub(crate) fn render(document: &Document) -> String {
    let buffer = document.buffer();
    let primary = document.selections().primary();
    let point = offset_to_grapheme_point(buffer, primary.head).unwrap_or_default();
    let modified = if document.is_modified() { " *" } else { "" };
    let count = document.selections().len();
    let selections = if count > 1 {
        format!(", {count} curseurs")
    } else {
        String::new()
    };
    let mut out = format!(
        "── {}{modified} — ligne {}, colonne {}{selections}\n",
        document.display_name(),
        point.line + 1,
        point.column + 1,
    );

    let first = point.line.saturating_sub(CONTEXT_LINES);
    let last = (point.line + CONTEXT_LINES).min(buffer.len_lines() - 1);
    let width = (last + 1).to_string().len();
    for line in first..=last {
        let (Ok(range), Ok(text)) = (buffer.line_content_range(line), buffer.line(line)) else {
            continue;
        };
        // Curseurs de cette ligne, de droite à gauche pour insérer sans décaler les suivants.
        let mut heads: Vec<usize> = document
            .selections()
            .iter()
            .map(|s| s.head)
            .filter(|&h| h >= range.start && h <= range.end)
            .map(|h| h - range.start)
            .collect();
        heads.sort_unstable();
        heads.dedup();
        let mut shown = text;
        for &local in heads.iter().rev() {
            shown.insert(local, '|');
        }
        out.push_str(&format!("{:>width$} │ {shown}\n", line + 1));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cupride_core::text::{Selection, Selections};

    #[test]
    fn lignes_autour_du_curseur() {
        let mut doc = Document::from_text("un\ndeux\ntrois\nquatre\ncinq\nsix\nsept");
        doc.set_selections(Selections::cursor(10)); // « tr|ois »
        assert_eq!(
            render(&doc),
            "── sans titre — ligne 3, colonne 3\n1 │ un\n2 │ deux\n3 │ tr|ois\n4 │ quatre\n5 │ cinq\n"
        );
    }

    #[test]
    fn debut_et_multi_curseur() {
        let mut doc = Document::from_text("ab\r\ncd");
        let mut selections = Selections::cursor(4);
        selections.push(Selection::cursor(1));
        doc.set_selections(selections);
        assert_eq!(
            render(&doc),
            "── sans titre — ligne 1, colonne 2, 2 curseurs\n1 │ a|b\n2 │ |cd\n"
        );
    }

    #[test]
    fn fin_de_ligne_et_texte_vide() {
        let doc = Document::new();
        assert_eq!(render(&doc), "── sans titre — ligne 1, colonne 1\n1 │ |\n");
    }
}
