//! Application : boucle d'événements `winit` et fenêtre de l'éditeur.
//!
//! La boucle attend les événements (`ControlFlow::Wait`) : sans saisie, sans redimensionnement et
//! sans minuterie, le processus dort et ne consomme pas de CPU. Rien n'est redessiné tant que rien
//! n'a changé. Les seules minuteries sont le clignotement du curseur, qui s'arrête de lui-même
//! quelques secondes après la dernière saisie, et l'expiration d'un avertissement.

use std::sync::Arc;
use std::time::Instant;

use cupride_core::clipboard::ClipboardKind;
use cupride_core::command::CommandOutput;
use cupride_core::keymap::{KeyResolver, Resolution};
use cupride_core::layout::{Axis, Divider, DropTarget, PanelId};
use cupride_core::notifications::Level;
use cupride_core::text::{Selections, TextBuffer};
use cupride_core::{DocumentId, Editor};
use cupride_render::Frame;
use cupride_render::{Acquire, Gpu, Rect, Renderer, TextSystem, WindowSurface};
use cupride_ui::{
    ClickCounter, ClickModifiers, CommandHistory, CommandLine, CursorBlink, EditorView, Focus,
    MouseSelection, PanelViews, Picker, Pointer, Preedit, StatusBar, StatusContext, StatusElements,
    Theme, Themes, Toasts, ViewRegistry, ViewSettings, WHEEL_LINES,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{CursorIcon, Window, WindowId};

mod overlays;
mod pickers;

use overlays::{CompletionCycle, Element};
use pickers::PickerKind;

use crate::close::{CloseDecision, CloseGuard};
use crate::file_index::{AppEvent, FileIndex};
use crate::input;
use crate::latency::LatencyStats;

/// Taille initiale de la fenêtre, en pixels logiques.
const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1_100.0, 720.0);

/// Fenêtre ouverte et ses ressources de rendu.
struct WindowState {
    // Ordre des champs : la surface doit être libérée avant la fenêtre.
    surface: WindowSurface,
    renderer: Renderer,
    /// Vues des panneaux, et place de chaque zone.
    views: PanelViews,
    window: Arc<Window>,
}

impl WindowState {
    /// Vue d'éditeur du panneau `panel`.
    fn editor_view(&self, panel: Option<PanelId>) -> Option<&EditorView> {
        self.views.editor_view(panel?)
    }

    /// Vue d'éditeur du panneau `panel`, modifiable.
    fn editor_view_mut(&mut self, panel: Option<PanelId>) -> Option<&mut EditorView> {
        self.views.editor_view_mut(panel?)
    }
}

/// Onglet saisi à la souris : il devient un glisser-déposer au-delà de quelques pixels.
struct TabDrag {
    panel: PanelId,
    origin: (f32, f32),
    dragging: bool,
    target: Option<DropTarget>,
}

/// État de l'application.
pub(crate) struct App {
    editor: Editor,
    text: TextSystem,
    /// Thèmes disponibles, thème affiché et son identifiant.
    themes: Themes,
    theme: Theme,
    theme_id: String,
    gpu: Option<Gpu>,
    window: Option<WindowState>,
    close_guard: CloseGuard,
    blink: CursorBlink,
    /// La fenêtre a-t-elle le focus clavier ? Sans focus, les curseurs sont masqués et ne
    /// clignotent pas.
    focused: bool,
    /// Modificateurs enfoncés.
    modifiers: ModifiersState,
    /// Panneau actif, son document et la position du curseur principal au dernier dessin : quand
    /// ils changent, la vue suit le curseur.
    last_cursor: Option<(PanelId, DocumentId, usize)>,
    /// Séquence de touches en cours (`ctrl+k …`).
    resolver: KeyResolver,
    /// Une méthode de saisie compose du texte : les touches lui appartiennent.
    composing: bool,
    /// Instant de la plus ancienne saisie pas encore affichée, pour mesurer la latence.
    pending_input: Option<Instant>,
    latency: LatencyStats,
    /// Position de la souris dans la fenêtre, en pixels physiques.
    mouse_position: (f32, f32),
    clicks: ClickCounter,
    mouse: MouseSelection,
    /// Séparation entre deux zones, saisie à la souris.
    divider_drag: Option<Divider>,
    /// Onglet saisi à la souris, pour le déplacer.
    tab_drag: Option<TabDrag>,
    /// Forme actuelle du pointeur.
    pointer: Pointer,
    /// Dernier texte envoyé à la sélection primaire, pour ne pas le renvoyer à chaque image.
    last_primary: Option<String>,
    /// Élément qui reçoit le clavier : l'éditeur, ou une couche ouverte par-dessus.
    focus: Focus<Element>,
    /// Ligne de commande, quand elle est ouverte.
    command_line: Option<CommandLine>,
    /// Historique des commandes tapées, et fichier où il est gardé entre les sessions.
    history: CommandHistory,
    history_path: Option<std::path::PathBuf>,
    /// Complétion en cours dans la ligne de commande.
    completion: Option<CompletionCycle>,
    /// Sélecteur ouvert (palette de commandes…).
    picker: Option<(PickerKind, Picker)>,
    /// Fichiers du projet, indexés en arrière-plan pour l'ouverture rapide.
    file_index: FileIndex,
    /// Pour réveiller la boucle d'événements depuis les autres threads.
    proxy: EventLoopProxy<AppEvent>,
    status_bar: StatusBar,
    status_elements: StatusElements,
    /// Configuration de la barre d'état déjà vérifiée (noms inconnus signalés une fois).
    checked_status_config: Option<cupride_core::config::StatusBarConfig>,
    /// Notifications affichées.
    toasts: Toasts,
    /// Erreur qui a empêché de démarrer.
    fatal: Option<String>,
}

impl App {
    pub(crate) fn new(mut editor: Editor, proxy: EventLoopProxy<AppEvent>) -> Self {
        let text = TextSystem::new();
        // Thèmes intégrés, puis ceux du dossier themes/ de la configuration.
        let mut themes = Themes::default();
        if let Some(dir) = editor
            .config_path()
            .and_then(|p| p.parent())
            .map(|d| d.join("themes"))
        {
            for problem in themes.load_dir(&dir) {
                tracing::warn!("{problem}");
                editor.notify(Level::Warning, problem);
            }
        }
        editor.set_themes(themes.ids().map(ToOwned::to_owned).collect());
        let family = &editor.config().font.family;
        if !family.eq_ignore_ascii_case("monospace") && !text.has_family(family) {
            tracing::warn!(
                "police « {family} » introuvable : police à chasse fixe du système utilisée"
            );
        }
        let view = &editor.config().view;
        let blink = CursorBlink::new(
            view.cursor_blink,
            view.cursor_blink_interval(),
            Instant::now(),
        );
        let history_path = directories::ProjectDirs::from("", "", "cupride")
            .map(|dirs| dirs.data_dir().join("command-history"));
        let history = history_path
            .as_deref()
            .map(|path| {
                CommandHistory::load(path).unwrap_or_else(|error| {
                    tracing::warn!("historique des commandes illisible : {error}");
                    CommandHistory::default()
                })
            })
            .unwrap_or_default();
        Self {
            editor,
            blink,
            focused: true,
            modifiers: ModifiersState::empty(),
            last_cursor: None,
            resolver: KeyResolver::new(),
            composing: false,
            pending_input: None,
            latency: LatencyStats::default(),
            mouse_position: (0.0, 0.0),
            clicks: ClickCounter::default(),
            mouse: MouseSelection::default(),
            divider_drag: None,
            tab_drag: None,
            pointer: Pointer::Default,
            last_primary: None,
            focus: Focus::new(Element::Editor),
            command_line: None,
            history,
            history_path,
            completion: None,
            picker: None,
            file_index: FileIndex::default(),
            proxy,
            status_bar: StatusBar::default(),
            status_elements: StatusElements::default(),
            checked_status_config: None,
            text,
            themes,
            theme: Theme::default(),
            theme_id: cupride_ui::DEFAULT_THEME.to_owned(),
            gpu: None,
            window: None,
            close_guard: CloseGuard::default(),
            toasts: Toasts::default(),
            fatal: None,
        }
    }

    /// Erreur qui a empêché de démarrer, à signaler par le code de sortie.
    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }

    /// Titre de la fenêtre : document actif, état modifié et projet.
    fn title(&self) -> String {
        let mut title = match self.editor.active_document() {
            Some(doc) => {
                let modified = if doc.is_modified() { " •" } else { "" };
                format!("{}{modified} — ", doc.display_name())
            }
            None => String::new(),
        };
        if let Some(project) = self.editor.project() {
            title.push_str(&crate::cli::project_name(project));
            title.push_str(" — ");
        }
        title.push_str("CuprIDE");
        title
    }

    /// Touche enfoncée : texte à insérer, ou raccourci à exécuter.
    fn key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed || self.composing {
            return;
        }
        let start = Instant::now();
        // Une couche ouverte (ligne de commande…) reçoit d'abord les touches.
        if self.focus.current() != Element::Editor && self.resolver.pending().is_empty() {
            let text = input::text_to_insert(event.text.as_deref(), self.modifiers);
            let combo = input::combo(
                &event.logical_key,
                &event.key_without_modifiers(),
                self.modifiers,
            );
            if self.overlay_key(combo, text) {
                return;
            }
        }
        // Hors séquence en cours, le texte imprimable est inséré directement.
        if self.resolver.pending().is_empty()
            && let Some(text) = input::text_to_insert(event.text.as_deref(), self.modifiers)
        {
            let text = text.to_owned();
            self.insert(&text, start);
            return;
        }
        let Some(combo) = input::combo(
            &event.logical_key,
            &event.key_without_modifiers(),
            self.modifiers,
        ) else {
            return;
        };
        match self.resolver.feed(self.editor.keymap(), combo) {
            Resolution::Execute(command) => self.execute(&command, start),
            // La séquence en cours s'affiche dans la barre d'état.
            Resolution::Pending => self.request_redraw(),
            Resolution::Unbound => {
                tracing::debug!(touches = %combo, "aucun raccourci");
                self.request_redraw();
            }
        }
    }

    /// Événement de méthode de saisie : composition en cours, ou texte validé.
    fn ime(&mut self, ime: Ime) {
        match ime {
            // La composition n'est affichée que dans l'éditeur.
            Ime::Preedit(..) if self.focus.current() != Element::Editor => {}
            Ime::Preedit(text, cursor) => {
                self.composing = !text.is_empty();
                let preedit = self.composing.then(|| Preedit {
                    text,
                    cursor: cursor.map(|(start, _)| start),
                });
                let panel = self.editor.active_panel();
                if let Some(view) = self.window.as_mut().and_then(|s| s.editor_view_mut(panel)) {
                    view.set_preedit(preedit);
                }
                self.blink.reset(Instant::now());
                self.request_redraw();
            }
            Ime::Commit(text) => {
                self.composing = false;
                self.clear_preedit();
                if !self.overlay_commit(&text) {
                    self.insert(&text, Instant::now());
                }
            }
            Ime::Enabled => {}
            Ime::Disabled => {
                self.composing = false;
                self.clear_preedit();
                self.request_redraw();
            }
        }
    }

    /// Retire le texte en composition de toutes les vues.
    fn clear_preedit(&mut self) {
        let Some(state) = &mut self.window else {
            return;
        };
        let panels: Vec<PanelId> = self.editor.workspace().tree().panels();
        for panel in panels {
            if let Some(view) = state.views.editor_view_mut(panel) {
                view.set_preedit(None);
            }
        }
    }

    /// Insère du texte à chaque curseur.
    fn insert(&mut self, text: &str, start: Instant) {
        let result = self.editor.run("edit insert", &[text.to_owned()]);
        self.after_command(result, start);
    }

    /// Exécute une ligne de commande (raccourci).
    fn execute(&mut self, line: &str, start: Instant) {
        tracing::debug!(commande = line, "raccourci");
        let result = self.editor.execute(line);
        self.after_command(result, start);
    }

    /// Après une commande (raccourci, frappe) : message ou erreur en notification, puis la
    /// suite commune.
    fn after_command(&mut self, result: cupride_core::command::CommandResult, start: Instant) {
        match result {
            Ok(CommandOutput::None) => {}
            Ok(CommandOutput::Message(message)) => {
                self.editor.notify(Level::Info, message);
            }
            Err(error) => {
                tracing::warn!("{error}");
                self.editor.notify(Level::Error, error.to_string());
            }
        }
        self.after_any_command(start);
    }

    /// Après toute commande (raccourci, ligne de commande) : demandes à l'interface, réglages
    /// rechargés, curseur rallumé et nouvelle image.
    fn after_any_command(&mut self, start: Instant) {
        self.process_ui_requests();
        self.sync_config();
        self.update_primary_selection();
        self.blink.reset(Instant::now());
        self.pending_input.get_or_insert(start);
        self.refresh_title();
        self.request_redraw();
    }

    /// Applique les réglages d'affichage de la configuration (après `config reload`).
    fn sync_config(&mut self) {
        let status = &self.editor.config().status_bar;
        if self.checked_status_config.as_ref() != Some(status) {
            let unknown: Vec<String> = self
                .status_elements
                .unknown(&status.left)
                .into_iter()
                .chain(self.status_elements.unknown(&status.right))
                .map(|name| format!("barre d'état : élément inconnu « {name} » ignoré"))
                .collect();
            self.checked_status_config = Some(status.clone());
            for message in unknown {
                tracing::warn!("{message}");
                self.editor.notify(Level::Warning, message);
            }
        }
        if self.editor.theme() != self.theme_id {
            self.apply_theme();
        }
        let config = self.editor.config();
        self.blink.configure(
            config.view.cursor_blink,
            config.view.cursor_blink_interval(),
        );
        if let Some(state) = &mut self.window {
            state.views.set_settings(ViewSettings::from_config(config));
            // Zoom de l'interface (view zoom-in…) : appliqué à chaud, comme un changement d'écran.
            state
                .views
                .set_scale(state.window.scale_factor() as f32 * self.editor.zoom());
        }
    }

    /// Affiche le thème choisi dans l'éditeur ; un thème inconnu (configuration) est signalé et
    /// remplacé par le thème par défaut.
    fn apply_theme(&mut self) {
        let id = self.editor.theme().to_owned();
        match self.themes.get(&id) {
            Some(theme) => self.theme = theme.clone(),
            None => {
                let message = format!(
                    "thème inconnu : « {id} », thème par défaut utilisé ; voir « theme list »"
                );
                tracing::warn!("{message}");
                self.editor.notify(Level::Warning, message);
                self.theme = Theme::default();
            }
        }
        self.theme_id = id;
        self.request_redraw();
    }

    /// Bouton gauche enfoncé : curseur, mot ou ligne selon le nombre de clics ; Maj étend la
    /// sélection, Alt ajoute un curseur.
    fn mouse_pressed(&mut self) {
        let (x, y) = self.mouse_position;
        // Un clic sur une notification la retire.
        if self.toasts.dismiss_at(x, y) {
            self.request_redraw();
            return;
        }
        if let Some(line) = &self.command_line
            && line.area().contains(x, y)
        {
            return;
        }
        if let Some((_, picker)) = &self.picker
            && picker.area().contains(x, y)
        {
            let item = picker.item_at(x, y);
            if item.is_some() {
                self.choose(item);
            }
            return;
        }
        // Un clic dans l'éditeur ferme les couches et lui rend le focus.
        self.close_overlays();
        if let Some(divider) = self.window.as_ref().and_then(|s| s.views.divider_at(x, y)) {
            self.divider_drag = Some(divider.clone());
            return;
        }
        if self.tab_pressed(x, y) {
            return;
        }
        // Un clic dans une autre zone lui donne le focus ; le clic y place ensuite le curseur.
        if let Some(panel) = self.window.as_ref().and_then(|s| s.views.panel_at(x, y))
            && self.editor.active_panel() != Some(panel)
        {
            self.editor.focus_panel(panel);
            self.after_any_command(Instant::now());
        }
        let panel = self.editor.active_panel();
        let (Some(state), Some(document)) = (&self.window, self.editor.active_document_mut())
        else {
            return;
        };
        let Some(view) = state.editor_view(panel) else {
            return;
        };
        let (x, y) = self.mouse_position;
        let now = Instant::now();
        let clicks = self
            .clicks
            .click(now, x, y, state.window.scale_factor() as f32);
        let offset = view.offset_at(x, y, document.buffer());
        let modifiers = ClickModifiers {
            extend: self.modifiers.shift_key(),
            add: self.modifiers.alt_key(),
        };
        let selections = self.mouse.press(
            document.buffer(),
            document.selections(),
            offset,
            clicks,
            modifiers,
        );
        document.set_selections(selections);
        self.blink.reset(now);
        self.request_redraw();
    }

    /// Clic dans une barre d'onglets : l'onglet cliqué est affiché et prend le focus ; un double
    /// clic garde un onglet d'aperçu. Renvoie vrai si le clic était dans une barre d'onglets.
    fn tab_pressed(&mut self, x: f32, y: f32) -> bool {
        let Some(state) = &self.window else {
            return false;
        };
        let Some(zone) = state.views.tab_bar_at(x, y) else {
            return false;
        };
        let panel = state.views.tab_at(x, y).or(zone.active);
        let clicks = self
            .clicks
            .click(Instant::now(), x, y, state.window.scale_factor() as f32);
        if let Some(panel) = panel {
            self.editor.focus_panel(panel);
            if clicks >= 2 {
                let _ = self
                    .editor
                    .workspace_mut()
                    .tree_mut()
                    .set_preview(panel, false);
            }
            self.tab_drag = Some(TabDrag {
                panel,
                origin: (x, y),
                dragging: false,
                target: None,
            });
        }
        self.after_any_command(Instant::now());
        true
    }

    /// Bouton relâché pendant le glisser d'un onglet : le panneau est déplacé vers la cible.
    fn drop_tab(&mut self) {
        let Some(drag) = self.tab_drag.take() else {
            return;
        };
        if let Some(target) = drag.target.filter(|_| drag.dragging) {
            let result = self
                .editor
                .move_panel(drag.panel, target.anchor, target.placement)
                .map(|()| CommandOutput::None)
                .map_err(Into::into);
            self.after_command(result, Instant::now());
        }
        self.request_redraw();
    }

    /// Clic du milieu sur un onglet : le ferme, comme `panel close`.
    fn close_tab_at(&mut self, x: f32, y: f32) -> bool {
        let Some(panel) = self.window.as_ref().and_then(|s| s.views.tab_at(x, y)) else {
            return false;
        };
        let result = self
            .editor
            .close_panel(panel, false)
            .map(|()| CommandOutput::None);
        self.after_command(result, Instant::now());
        true
    }

    /// Sélection primaire (X11, Wayland, si activée) : le texte sélectionné y est copié.
    fn update_primary_selection(&mut self) {
        if !self.editor.config().editor.primary_selection
            || !self.editor.clipboard_mut().has_primary()
        {
            return;
        }
        let Some(document) = self.editor.active_document() else {
            return;
        };
        let selection = document.selections().primary();
        if selection.is_empty() {
            return;
        }
        let Ok(text) = document.buffer().slice(selection.range()) else {
            return;
        };
        if self.last_primary.as_ref() == Some(&text) {
            return;
        }
        if let Err(error) = self
            .editor
            .clipboard_mut()
            .set(ClipboardKind::Primary, text.clone())
        {
            tracing::warn!("sélection primaire : {error}");
        }
        self.last_primary = Some(text);
    }

    /// Clic du milieu (sélection primaire activée) : place le curseur sous la souris et y colle
    /// la sélection primaire.
    fn paste_primary(&mut self) {
        let (x, y) = self.mouse_position;
        if self.close_tab_at(x, y) {
            return;
        }
        if !self.editor.config().editor.primary_selection {
            return;
        }
        let panel = self.editor.active_panel();
        let (Some(state), Some(document)) = (&self.window, self.editor.active_document_mut())
        else {
            return;
        };
        let Some(view) = state.editor_view(panel) else {
            return;
        };
        let (x, y) = self.mouse_position;
        let offset = view.offset_at(x, y, document.buffer());
        document.set_selections(Selections::cursor(offset));
        self.execute("edit paste --primary", Instant::now());
    }

    /// Souris déplacée : forme du pointeur, et sélection si le bouton est enfoncé. Hors de la
    /// vue, la sélection continue sur les lignes au-delà, que la vue fait défiler pour suivre.
    fn mouse_moved(&mut self, x: f32, y: f32) {
        self.mouse_position = (x, y);
        let Some(state) = &self.window else {
            return;
        };
        if let Some(divider) = &self.divider_drag {
            if state.views.drag_divider(&mut self.editor, divider, x, y) {
                state.window.request_redraw();
            }
            return;
        }
        if let Some(drag) = &mut self.tab_drag {
            let threshold = state.views.style().px(6.0);
            let (ox, oy) = drag.origin;
            drag.dragging |= (x - ox).hypot(y - oy) > threshold;
            if drag.dragging {
                let target = state.views.drop_target(&self.editor, drag.panel, x, y);
                if target != drag.target {
                    drag.target = target;
                    state.window.request_redraw();
                }
            }
            return;
        }
        let pointer = match state.views.divider_at(x, y).map(|d| d.axis) {
            // Les zones d'un split horizontal sont côte à côte : la séparation est verticale.
            Some(Axis::Horizontal) => Pointer::ResizeHorizontal,
            Some(Axis::Vertical) => Pointer::ResizeVertical,
            None => state
                .views
                .panel_at(x, y)
                .and_then(|panel| state.views.view(panel))
                .map_or(Pointer::Default, |view| view.pointer_at(x, y)),
        };
        if pointer != self.pointer {
            self.pointer = pointer;
            state.window.set_cursor(match pointer {
                Pointer::Default => CursorIcon::Default,
                Pointer::Text => CursorIcon::Text,
                Pointer::ResizeHorizontal => CursorIcon::ColResize,
                Pointer::ResizeVertical => CursorIcon::RowResize,
            });
        }
        if !self.mouse.is_dragging() {
            return;
        }
        let panel = self.editor.active_panel();
        let (Some(view), Some(document)) =
            (state.editor_view(panel), self.editor.active_document_mut())
        else {
            return;
        };
        let offset = view.offset_at(x, y, document.buffer());
        if let Some(selections) = self
            .mouse
            .drag(document.buffer(), document.selections(), offset)
            && &selections != document.selections()
        {
            document.set_selections(selections);
            self.blink.reset(Instant::now());
            state.window.request_redraw();
        }
    }

    /// Défilement à la molette (crans, animés) ou au pavé tactile (pixels, immédiats). Avec Maj,
    /// la molette fait défiler horizontalement.
    fn scroll(&mut self, delta: MouseScrollDelta) {
        let Some(state) = &mut self.window else {
            return;
        };
        // La molette fait défiler le panneau sous la souris.
        let (x, y) = self.mouse_position;
        let Some(panel) = state.views.panel_at(x, y).or(self.editor.active_panel()) else {
            return;
        };
        let (dx, dy, smooth) = match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                let step = WHEEL_LINES * state.views.style().line_height;
                (-x * step, -y * step, true)
            }
            MouseScrollDelta::PixelDelta(p) => (-p.x as f32, -p.y as f32, false),
        };
        let (dx, dy) = if self.modifiers.shift_key() && dx == 0.0 {
            (dy, 0.0)
        } else {
            (dx, dy)
        };
        if let Some(view) = state.views.view_mut(panel) {
            view.scroll_by(dx, dy, smooth, Instant::now());
        }
        state.window.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(state) = &self.window {
            state.window.request_redraw();
        }
    }

    fn refresh_title(&self) {
        if let Some(state) = &self.window {
            state.window.set_title(&self.title());
        }
    }

    fn modified_documents(&self) -> Vec<String> {
        self.editor
            .documents()
            .filter(|(_, doc)| doc.is_modified())
            .map(|(_, doc)| doc.display_name())
            .collect()
    }

    fn request_close(&mut self, event_loop: &ActiveEventLoop) {
        match self
            .close_guard
            .request(&self.modified_documents(), Instant::now())
        {
            CloseDecision::Close => {
                tracing::info!("fermeture de la fenêtre");
                event_loop.exit();
            }
            CloseDecision::Warn(message) => {
                tracing::warn!("{message}");
                self.editor.notify(Level::Warning, message);
                self.request_redraw();
            }
        }
    }

    /// Enregistre l'espace de travail (layout, fichiers, curseurs, défilement) pour le prochain
    /// lancement. Un échec est journalisé, sans empêcher de quitter.
    fn save_session(&mut self) {
        if self.fatal.is_some() {
            return;
        }
        if let Some(state) = &self.window {
            for (panel, scroll) in state.views.scroll_positions() {
                self.editor.set_panel_scroll(panel, scroll);
            }
        }
        let Some(dir) = cupride_core::session::sessions_dir() else {
            tracing::warn!("dossier de données introuvable : session non enregistrée");
            return;
        };
        let path = dir.join(cupride_core::session::session_file_name(
            self.editor.project(),
        ));
        match cupride_core::session::save(&self.editor.capture_session(), &path) {
            Ok(()) => tracing::info!(session = %path.display(), "session enregistrée"),
            Err(error) => tracing::warn!("{error}"),
        }
    }

    /// Erreur qui empêche de démarrer : message clair dans les journaux, et fin de la boucle ;
    /// `main` l'affiche et sort avec un code d'erreur.
    fn fail(&mut self, event_loop: &ActiveEventLoop, message: String) {
        tracing::error!("{message}");
        self.fatal = Some(message);
        event_loop.exit();
    }

    /// Ouvre la fenêtre et initialise le GPU.
    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let attributes = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(INITIAL_SIZE);
        let window = event_loop
            .create_window(attributes)
            .map(Arc::new)
            .map_err(|error| format!("impossible d'ouvrir une fenêtre : {error}"))?;
        tracing::info!(id = ?window.id(), "fenêtre ouverte");
        // Lancée depuis un terminal, l'application passe au premier plan avec le focus clavier.
        window.focus_window();
        // Méthodes de saisie (IME) et touches mortes composées par le système.
        window.set_ime_allowed(true);
        let (gpu, surface) = Gpu::for_window(Arc::clone(&window)).map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let surface =
            WindowSurface::new(&gpu, surface, Arc::clone(&window), size.width, size.height)
                .map_err(|e| e.to_string())?;
        tracing::info!(gpu = %gpu.describe(), format = ?surface.format(), "rendu prêt");
        let renderer = Renderer::new(&gpu, surface.format());
        let settings = ViewSettings::from_config(self.editor.config());
        let views = PanelViews::new(
            ViewRegistry::default(),
            settings,
            window.scale_factor() as f32 * self.editor.zoom(),
        );
        self.gpu = Some(gpu);
        self.window = Some(WindowState {
            surface,
            renderer,
            views,
            window,
        });
        // Thème et barre d'état de la configuration, dès la première image.
        self.sync_config();
        // Fichiers du projet indexés en arrière-plan, prêts pour l'ouverture rapide.
        self.refresh_file_index();
        Ok(())
    }

    /// Dessine la fenêtre.
    fn redraw(&mut self) {
        let (Some(gpu), Some(state)) = (&self.gpu, &mut self.window) else {
            return;
        };
        let frame = match state.surface.acquire(gpu) {
            Acquire::Frame(frame) => frame,
            Acquire::Retry => {
                state.window.request_redraw();
                return;
            }
            // Fenêtre masquée : le prochain dessin viendra quand elle redeviendra visible.
            Acquire::Hidden => return,
        };
        let size = state.surface.size();
        let window_rect = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
        let style = state.views.style();
        let status_height = StatusBar::height(&style);
        let parts = cupride_ui::layout::split(
            window_rect,
            cupride_ui::layout::Axis::Vertical,
            &[
                cupride_ui::layout::Size::FILL,
                cupride_ui::layout::Size::Fixed(status_height),
            ],
        );
        state.views.layout(&mut self.text, &self.editor, parts[0]);
        self.editor.set_layout_metrics(state.views.metrics());
        let pending_keys: Vec<String> = self
            .resolver
            .pending()
            .iter()
            .map(ToString::to_string)
            .collect();
        let pending_keys = pending_keys.join(" ");
        let status_config = &self.editor.config().status_bar;
        self.status_bar.prepare(
            &mut self.text,
            &style,
            window_rect,
            &self.status_elements,
            &StatusContext {
                editor: &self.editor,
                pending_keys: &pending_keys,
            },
            (&status_config.left, &status_config.right),
        );
        let panel = self.editor.active_panel();
        if let Some(view) = state.editor_view(panel) {
            self.editor.set_page_lines(view.page_lines());
        }
        let now = Instant::now();
        let blink_on = self.focused && self.blink.visible(now);
        let editor_focused = self.focus.is_focused(Element::Editor);
        let animating = state.views.tick(now);
        let mut scene = Frame::new(self.theme.background);
        // La vue du panneau actif suit son curseur quand il bouge, ou quand le panneau change.
        let cursor = panel
            .zip(self.editor.active_document())
            .and_then(|(panel, document)| {
                let id = self.editor.panel_document(panel)?;
                Some((panel, id, document.selections().primary().head))
            });
        if cursor != self.last_cursor {
            self.last_cursor = cursor;
            if let Some(view) = state.editor_view_mut(panel) {
                view.follow_cursor();
            }
        }
        state.views.prepare(
            &mut self.text,
            &self.editor,
            &self.theme,
            self.focused && editor_focused,
            blink_on,
        );
        // Défilement de chaque vue, pour la session et les layouts nommés.
        for (panel, scroll) in state.views.scroll_positions() {
            self.editor.set_panel_scroll(panel, scroll);
        }
        state.views.draw(
            scene.base(),
            &self.editor,
            &self.theme,
            self.focused && editor_focused,
            blink_on,
        );
        if let Some(target) = self.tab_drag.as_ref().and_then(|d| d.target.as_ref()) {
            state
                .views
                .draw_drop_preview(scene.push_layer(), target, &self.theme);
        }
        self.status_bar.draw(scene.base(), &style, &self.theme);
        let mut ime_area = None;
        if let Some(line) = &mut self.command_line {
            line.prepare(&mut self.text, &style, window_rect, status_height);
            ime_area = Some(line.cursor_rect(&style));
        }
        if let Some(line) = &self.command_line {
            line.draw(scene.push_layer(), &style, &self.theme, blink_on);
        }
        if let Some((_, picker)) = &mut self.picker {
            picker.prepare(&mut self.text, &style, &self.theme, parts[0]);
            ime_area = Some(picker.cursor_rect(&style));
        }
        if let Some((_, picker)) = &self.picker {
            picker.draw(scene.push_layer(), &style, &self.theme, blink_on);
        }
        // Notifications, au-dessus de tout, dans la zone de l'éditeur.
        self.toasts.sync(
            self.editor.notifications(),
            &self.editor.config().notifications,
            now,
        );
        self.toasts.expire(now);
        if !self.toasts.is_empty() {
            let above = self.command_line.as_ref().map_or(parts[0], |line| {
                Rect::new(0.0, 0.0, window_rect.width, line.area().y)
            });
            self.toasts.prepare(&mut self.text, &style, above);
            self.toasts.draw(scene.push_layer(), &style, &self.theme);
        }
        if let Err(error) = state
            .renderer
            .render(gpu, &mut self.text, &frame.view, size, &scene)
        {
            tracing::error!("{error}");
        }
        if let Some(cursor) = ime_area.or_else(|| {
            let document = self.editor.active_document()?;
            state.editor_view(panel)?.primary_cursor_rect(document)
        }) {
            // La fenêtre de l'IME s'ouvre sous le curseur.
            state.window.set_ime_cursor_area(
                PhysicalPosition::new(cursor.x, cursor.y),
                PhysicalSize::new(cursor.width, cursor.height),
            );
        }
        state.window.pre_present_notify();
        state.surface.present(gpu, frame);
        if let Some(start) = self.pending_input.take() {
            self.latency.record(start.elapsed());
        }
        // Défilement animé : une image par rafraîchissement de l'écran (présentation synchronisée)
        // jusqu'à la fin de l'animation, puis plus rien.
        if animating {
            state.window.request_redraw();
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::FilesIndexed => self.files_indexed(),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(message) = self.open_window(event_loop) {
            self.fail(event_loop, message);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        tracing::trace!(?event, "événement de fenêtre");
        match event {
            WindowEvent::CloseRequested => self.request_close(event_loop),
            WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(state)) = (&self.gpu, &mut self.window) {
                    state.surface.resize(gpu, size.width, size.height);
                }
                // Dessin immédiat : pendant un redimensionnement à la souris, l'image suit la
                // fenêtre au lieu d'être étirée.
                self.redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // Fenêtre déplacée vers un écran d'une autre densité : le texte est remis en
                // forme à la nouvelle échelle. `Resized` suit avec la nouvelle taille physique.
                if let Some(state) = &mut self.window {
                    state
                        .views
                        .set_scale(scale_factor as f32 * self.editor.zoom());
                    state.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => self.key(&event),
            WindowEvent::Ime(ime) => self.ime(ime),
            WindowEvent::MouseWheel { delta, .. } => self.scroll(delta),
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_moved(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.mouse_pressed(),
                ElementState::Released => {
                    self.divider_drag = None;
                    self.drop_tab();
                    self.mouse.release();
                    self.update_primary_selection();
                }
            },
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Middle,
                ..
            } => self.paste_primary(),
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                self.blink.reset(Instant::now());
                self.request_redraw();
            }
            WindowEvent::RedrawRequested | WindowEvent::Occluded(false) => self.redraw(),
            _ => {}
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            let now = Instant::now();
            self.close_guard.expired(now);
            if self.toasts.expire(now) {
                self.request_redraw();
            }
            // Réveil du clignotement : le curseur change d'état.
            if self.focused {
                self.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Prochain réveil : fin du délai de confirmation de fermeture, changement d'état du
        // curseur ou expiration d'une notification. Sans rien de tout cela, la boucle dort
        // jusqu'au prochain événement.
        let now = Instant::now();
        let blink = self.focused.then(|| self.blink.next_change(now)).flatten();
        let wake = [
            self.close_guard.deadline(),
            blink,
            self.toasts.next_expiry(),
        ]
        .into_iter()
        .flatten()
        .min();
        event_loop.set_control_flow(match wake {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        });
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(summary) = self.latency.summary() {
            tracing::info!(
                frappes = summary.count,
                mediane_us = summary.median.as_micros() as u64,
                p99_us = summary.p99.as_micros() as u64,
                max_us = summary.max.as_micros() as u64,
                "latence de frappe"
            );
        }
        self.save_session();
        // Les ressources de la fenêtre sont libérées avant le GPU et la sortie de la boucle.
        self.window = None;
        self.gpu = None;
    }
}
