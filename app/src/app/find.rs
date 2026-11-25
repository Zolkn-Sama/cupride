//! Barre de recherche : recherche incrémentale dans le document actif, en arrière-plan.

use std::sync::Arc;
use std::time::Instant;

use cupride_core::clipboard::ClipboardKind;
use cupride_core::command::CommandOutput;
use cupride_core::keymap::{Key, KeyCombo, Platform};
use cupride_core::text::TextBuffer;
use cupride_ui::{FindBar, FindOption, InputEvent};

use super::App;
use super::overlays::Element;

impl App {
    /// Ouvre la barre de recherche, préremplie avec `pattern` (sinon le motif en cours).
    pub(super) fn open_find(&mut self, pattern: &str) {
        if self.command_line.is_some() {
            self.close_command_line();
        }
        if self.picker.is_some() {
            self.close_picker();
        }
        let mut query = self.editor.search().query().cloned().unwrap_or_default();
        if !pattern.is_empty() {
            query.pattern = pattern.to_owned();
        }
        self.find = Some(FindBar::new(&query));
        self.focus.push(Element::Find);
        // Point de départ de la recherche incrémentale : le début de la sélection.
        self.search_origin = self
            .editor
            .active_document()
            .map(|d| d.selections().primary().start());
        self.apply_find_query(true);
    }

    /// Ferme la barre ; le motif reste (F3 cherche encore), les occurrences ne sont plus
    /// surlignées.
    pub(super) fn close_find(&mut self) {
        self.find = None;
        self.search.stop();
        self.focus.remove(Element::Find);
        self.refresh_highlights();
        self.request_redraw();
    }

    /// Le motif ou les options ont changé : nouvelle recherche ; avec `jump`, la première
    /// occurrence après le point de départ sera sélectionnée.
    fn apply_find_query(&mut self, jump: bool) {
        let Some(bar) = &mut self.find else {
            return;
        };
        let query = bar.query();
        if query.pattern.is_empty() {
            bar.set_status(None, false);
            self.editor.set_search_query(None);
            self.search.stop();
        } else if let Err(error) = query.compile() {
            bar.set_status(Some(error.to_string()), true);
            self.editor.set_search_query(None);
            self.search.stop();
        } else {
            self.editor.set_search_query(Some(query));
            self.search_jump = jump;
            self.search.stop();
            self.start_search();
        }
        self.refresh_highlights();
        self.request_redraw();
    }

    /// Lance la recherche en arrière-plan si les occurrences du document actif ne sont pas à
    /// jour (motif changé, texte modifié, autre document).
    pub(super) fn start_search(&mut self) {
        if self.find.is_none() {
            return;
        }
        let Some(query) = self.editor.search().query().cloned() else {
            return;
        };
        let Some(id) = self.editor.active_id() else {
            return;
        };
        let Some(document) = self.editor.document(id) else {
            return;
        };
        let revision = document.revision();
        if self.editor.search_matches(id).is_some() || self.search.covers(id, revision) {
            return;
        }
        let Ok(regex) = query.compile() else {
            return;
        };
        let text: Arc<str> = document.buffer().to_text().into();
        let proxy = self.proxy.clone();
        let done = move || {
            let _ = proxy.send_event(crate::file_index::AppEvent::SearchDone);
        };
        self.search.start(done, id, revision, text, regex);
    }

    /// Résultats d'une recherche arrivés du thread.
    pub(super) fn search_done(&mut self) {
        let Some(found) = self.search.take() else {
            return;
        };
        let current = self
            .editor
            .document(found.document)
            .is_some_and(|d| d.revision() == found.revision);
        if !current {
            return;
        }
        let count = found.matches.len();
        self.editor
            .set_search_results(found.document, found.revision, found.matches);
        let status = if count == 0 {
            Some("aucun résultat".to_owned())
        } else if std::mem::take(&mut self.search_jump) {
            let origin = self.search_origin;
            match self.editor.select_match(true, origin) {
                Ok((index, count)) => Some(format!("{index}/{count}")),
                Err(error) => Some(error.to_string()),
            }
        } else {
            Some(format!(
                "{count} résultat{}",
                if count > 1 { "s" } else { "" }
            ))
        };
        let limited = count >= cupride_core::search::MAX_MATCHES;
        if let Some(bar) = &mut self.find {
            let status = status.map(|s| if limited { format!("{s} (limite)") } else { s });
            bar.set_status(status, false);
        }
        self.refresh_highlights();
        self.after_any_command(Instant::now());
    }

    /// Surligne les occurrences dans les vues du document actif, tant que la barre est ouverte ;
    /// ailleurs, rien.
    pub(super) fn refresh_highlights(&mut self) {
        let active = self.editor.active_id();
        let matches = self
            .find
            .as_ref()
            .and(active)
            .and_then(|id| Some((id, self.editor.search_matches(id)?)));
        // Les occurrences ne sont copiées qu'une fois par résultat : les vues partagent l'Arc.
        let matches = match (&self.highlights, matches) {
            (Some((id, revision, cached)), Some((doc, found)))
                if *id == doc
                    && Some(*revision) == self.editor.document(doc).map(|d| d.revision())
                    && cached.as_slice() == found =>
            {
                Some((doc, Arc::clone(cached)))
            }
            (_, Some((doc, found))) => {
                let shared = Arc::new(found.to_vec());
                let revision = self.editor.document(doc).map_or(0, |d| d.revision());
                self.highlights = Some((doc, revision, Arc::clone(&shared)));
                Some((doc, shared))
            }
            (_, None) => {
                self.highlights = None;
                None
            }
        };
        let empty: Arc<Vec<std::ops::Range<usize>>> = Arc::default();
        for state in self.windows.values_mut() {
            let panels: Vec<_> = self
                .editor
                .workspace()
                .window(state.views.window().unwrap_or(self.current))
                .map(|w| w.tree().panels())
                .unwrap_or_default();
            for panel in panels {
                let shown = self.editor.panel_document(panel);
                if let Some(view) = state.views.editor_view_mut(panel) {
                    let highlights = match &matches {
                        Some((doc, found)) if shown == Some(*doc) => Arc::clone(found),
                        _ => Arc::clone(&empty),
                    };
                    view.set_highlights(highlights);
                }
            }
        }
    }

    /// Touche pour la barre de recherche. Entrée : suivante ; Maj+Entrée : précédente ;
    /// Échap : fermer ; Alt+C, Alt+W, Alt+R (aussi avec Cmd ou Ctrl) : options.
    pub(super) fn find_key(&mut self, combo: Option<KeyCombo>, text: Option<&str>) -> bool {
        if self.find.is_none() {
            return false;
        }
        if let Some(combo) = combo
            && combo.modifiers.alt
            && let Key::Char(c) = combo.key
        {
            let option = match c.to_ascii_lowercase() {
                'c' => Some(FindOption::Case),
                'w' => Some(FindOption::Word),
                'r' => Some(FindOption::Regex),
                _ => None,
            };
            if let Some(option) = option {
                if let Some(bar) = &mut self.find {
                    bar.toggle(option);
                }
                self.apply_find_query(true);
                return true;
            }
        }
        if let Some(text) = text {
            if let Some(bar) = &mut self.find {
                bar.input.insert(text);
            }
            self.apply_find_query(true);
            return true;
        }
        let Some(combo) = combo else {
            return true;
        };
        let Some(bar) = &mut self.find else {
            return false;
        };
        let before = bar.input.text().to_owned();
        match bar.input.key(combo, Platform::current()) {
            InputEvent::Submit => {
                let command = if combo.modifiers.shift {
                    "search previous"
                } else {
                    "search next"
                };
                self.step_match(command);
            }
            InputEvent::Cancel => self.close_find(),
            InputEvent::Up => self.step_match("search previous"),
            InputEvent::Down => self.step_match("search next"),
            InputEvent::Copy(text) => {
                if let Err(error) = self
                    .editor
                    .clipboard_mut()
                    .set(ClipboardKind::Standard, text)
                {
                    tracing::warn!("presse-papiers : {error}");
                }
            }
            InputEvent::Paste => {
                if let Some(text) = self.editor.clipboard_mut().get(ClipboardKind::Standard)
                    && let Some(bar) = &mut self.find
                {
                    bar.input.insert(&text);
                }
                self.apply_find_query(true);
            }
            InputEvent::Handled | InputEvent::Complete => {
                if self.find.as_ref().is_some_and(|b| b.input.text() != before) {
                    self.apply_find_query(true);
                } else {
                    self.request_redraw();
                }
            }
            InputEvent::Ignored => return false,
        }
        true
    }

    /// Occurrence suivante ou précédente, et sa position dans la barre.
    fn step_match(&mut self, command: &str) {
        let result = self.editor.execute(command);
        if let Some(bar) = &mut self.find {
            match &result {
                Ok(CommandOutput::Message(position)) => {
                    bar.set_status(Some(position.clone()), false)
                }
                Ok(CommandOutput::None) => {}
                Err(error) => bar.set_status(Some(error.to_string()), true),
            }
        }
        // Le point de départ suit : une nouvelle frappe repart de l'occurrence choisie.
        self.search_origin = self
            .editor
            .active_document()
            .map(|d| d.selections().primary().start());
        self.after_any_command(Instant::now());
    }

    /// Clic sur la barre : une option bascule. Renvoie vrai si le clic était sur la barre.
    pub(super) fn find_clicked(&mut self, x: f32, y: f32) -> bool {
        let Some(bar) = &mut self.find else {
            return false;
        };
        if !bar.area().contains(x, y) {
            return false;
        }
        if let Some(option) = bar.option_at(x, y) {
            bar.toggle(option);
            self.apply_find_query(true);
        }
        self.focus.push(Element::Find);
        true
    }
}
