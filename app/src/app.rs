//! Application : boucle d'événements `winit` et fenêtres de l'éditeur.
//!
//! Une seule boucle d'événements et un seul device GPU pour toutes les fenêtres ; chaque fenêtre a
//! sa surface, ses vues et sa barre d'état. Le noyau tient le modèle des fenêtres (chacune avec son
//! layout) : après chaque lot d'événements, les fenêtres du système sont ouvertes ou fermées pour
//! lui correspondre.
//!
//! La boucle attend les événements (`ControlFlow::Wait`) : sans saisie, sans redimensionnement et
//! sans minuterie, le processus dort et ne consomme pas de CPU. Rien n'est redessiné tant que rien
//! n'a changé. Les seules minuteries sont le clignotement du curseur, qui s'arrête de lui-même
//! quelques secondes après la dernière saisie, et l'expiration d'un avertissement.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cupride_core::clipboard::ClipboardKind;
use cupride_core::command::CommandOutput;
use cupride_core::keymap::{KeyResolver, Resolution};
use cupride_core::layout::{
    Axis, Divider, DropTarget, PanelId, WindowGeometry, WindowId as LayoutWindowId,
};
use cupride_core::monitors::{MonitorInfo, Placement, PlacementStrategy};
use cupride_core::notifications::Level;
use cupride_core::text::{Selections, TextBuffer};
use cupride_core::{DocumentId, Editor};
use cupride_render::Frame;
use cupride_render::{Acquire, Gpu, Rect, Renderer, TextSystem, WindowSurface};
use cupride_ui::{
    ClickCounter, ClickModifiers, CommandHistory, CommandLine, CursorBlink, EditorView, FindBar,
    Focus, MouseSelection, PanelViews, Picker, Pointer, Preedit, StatusBar, StatusContext,
    StatusElements, Theme, Themes, Toasts, ViewAction, ViewRegistry, ViewSettings, WHEEL_LINES,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::monitor::MonitorHandle;
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

mod find;
mod overlays;
mod pickers;

use overlays::{CompletionCycle, Element};
use pickers::PickerKind;

use crate::close::{CloseDecision, CloseGuard};
use crate::file_index::{AppEvent, FileIndex};
use crate::input;
use crate::latency::LatencyStats;
use crate::project_worker::ProjectWorker;
use crate::redraw::Stamp;
use crate::search_worker::SearchWorker;
use crate::watcher::DiskWatcher;

/// Taille initiale de la fenêtre, en pixels logiques.
const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1_100.0, 720.0);

/// Décalage d'une nouvelle fenêtre par rapport à la fenêtre courante, en pixels logiques.
const CASCADE: f64 = 32.0;

/// Fenêtre ouverte et ses ressources de rendu.
struct WindowState {
    // Ordre des champs : la surface doit être libérée avant la fenêtre.
    surface: WindowSurface,
    renderer: Renderer,
    /// Vues des panneaux de cette fenêtre, et place de chaque zone.
    views: PanelViews,
    status_bar: StatusBar,
    /// Ce que la fenêtre affichait à son dernier dessin.
    stamp: Option<Stamp>,
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

/// Occurrences de la recherche partagées par les vues : (document, révision, occurrences).
type SharedMatches = (DocumentId, u64, Arc<Vec<std::ops::Range<usize>>>);

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
    /// Fenêtres ouvertes, par identifiant du noyau.
    windows: BTreeMap<LayoutWindowId, WindowState>,
    /// Identifiant du noyau de chaque fenêtre du système.
    os_windows: HashMap<WindowId, LayoutWindowId>,
    /// Fenêtre courante : celle qui a le focus, où s'affichent les couches (ligne de commande,
    /// palette, notifications).
    current: LayoutWindowId,
    /// La boucle a démarré (la première fenêtre a pu être ouverte).
    started: bool,
    /// Génération de l'apparence (thème, police, zoom) : quand elle change, toutes les fenêtres
    /// sont redessinées.
    look: u64,
    /// Réglages d'affichage et zoom déjà appliqués.
    applied_look: Option<(ViewSettings, f32)>,
    /// Écrans, dans l'ordre de leur numérotation, et dernière énumération.
    monitors: Vec<(MonitorInfo, MonitorHandle)>,
    monitors_checked: Option<Instant>,
    /// Écran demandé pour une fenêtre pas encore ouverte.
    pending_placement: HashMap<LayoutWindowId, usize>,
    /// Session Wayland : les fenêtres ne peuvent pas être positionnées.
    wayland: bool,
    /// Demandes reçues d'autres `cupride` (instance unique), en attente.
    remote: Arc<Mutex<Vec<cupride_ipc::Incoming>>>,
    /// Terminaux qui attendent la fermeture de documents (`cupride --wait`).
    waiting: Vec<(Vec<DocumentId>, cupride_ipc::Responder)>,
    /// Lancé avec `--wait` sans instance existante : quitter quand ces documents sont fermés.
    exit_when_closed: Vec<DocumentId>,
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
    /// Barre de recherche, quand elle est ouverte.
    find: Option<FindBar>,
    /// Recherches en arrière-plan.
    search: SearchWorker,
    /// Point de départ de la recherche incrémentale, et sélection de la première occurrence
    /// trouvée à la prochaine réponse.
    search_origin: Option<usize>,
    search_jump: bool,
    /// Dernier texte de remplacement saisi.
    last_replacement: String,
    /// Recherche dans le projet, en arrière-plan.
    project_search: ProjectWorker,
    /// Occurrences surlignées, partagées par les vues : (document, révision, occurrences).
    highlights: Option<SharedMatches>,
    /// Fichiers du projet, indexés en arrière-plan pour l'ouverture rapide.
    file_index: FileIndex,
    /// Dossiers surveillés sur le disque (explorateur).
    disk: DiskWatcher,
    /// Pour réveiller la boucle d'événements depuis les autres threads.
    proxy: EventLoopProxy<AppEvent>,
    status_elements: StatusElements,
    /// Configuration de la barre d'état déjà vérifiée (noms inconnus signalés une fois).
    checked_status_config: Option<cupride_core::config::StatusBarConfig>,
    /// Notifications affichées.
    toasts: Toasts,
    /// Erreur qui a empêché de démarrer.
    fatal: Option<String>,
}

impl App {
    pub(crate) fn new(
        mut editor: Editor,
        proxy: EventLoopProxy<AppEvent>,
        remote: Arc<Mutex<Vec<cupride_ipc::Incoming>>>,
    ) -> Self {
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
        let history_path = editor.data_dir().map(|dir| dir.join("command-history"));
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
            find: None,
            search: SearchWorker::default(),
            search_origin: None,
            search_jump: false,
            last_replacement: String::new(),
            project_search: ProjectWorker::default(),
            highlights: None,
            file_index: FileIndex::default(),
            disk: DiskWatcher::new(proxy.clone()),
            proxy,
            status_elements: StatusElements::default(),
            checked_status_config: None,
            text,
            themes,
            theme: Theme::default(),
            theme_id: cupride_ui::DEFAULT_THEME.to_owned(),
            gpu: None,
            windows: BTreeMap::new(),
            os_windows: HashMap::new(),
            current: LayoutWindowId::new(1),
            started: false,
            look: 0,
            applied_look: None,
            monitors: Vec::new(),
            monitors_checked: None,
            pending_placement: HashMap::new(),
            wayland: false,
            remote,
            waiting: Vec::new(),
            exit_when_closed: Vec::new(),
            close_guard: CloseGuard::default(),
            toasts: Toasts::default(),
            fatal: None,
        }
    }

    /// Erreur qui a empêché de démarrer, à signaler par le code de sortie.
    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }

    /// Titre de la fenêtre `window` : son document actif, état modifié et projet.
    fn title(&self, window: LayoutWindowId) -> String {
        let document = self
            .editor
            .workspace()
            .window(window)
            .and_then(|w| w.active())
            .and_then(|p| self.editor.panel_document(p))
            .and_then(|d| self.editor.document(d));
        let mut title = match document {
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
        // Un panneau qui prend le clavier (explorateur) reçoit les touches ; celles qu'il ne
        // traite pas vont aux raccourcis, jamais au document.
        if self.focus.current() == Element::Editor
            && self.resolver.pending().is_empty()
            && let Some(panel) = self.keyboard_panel()
        {
            let combo = input::combo(
                &event.logical_key,
                &event.key_without_modifiers(),
                self.modifiers,
            );
            let action = match (combo, self.windows.get_mut(&self.current)) {
                (Some(combo), Some(state)) => state
                    .views
                    .view_mut(panel)
                    .map_or(ViewAction::Ignored, |view| view.key(&combo)),
                _ => ViewAction::Ignored,
            };
            if action != ViewAction::Ignored {
                self.view_action(action, start);
                return;
            }
            if let Some(combo) = combo {
                match self.resolver.feed(self.editor.keymap(), combo) {
                    Resolution::Execute(command) => self.execute(&command, start),
                    Resolution::Pending | Resolution::Unbound => self.request_redraw(),
                }
            }
            return;
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

    /// Panneau actif, s'il prend le clavier lui-même (explorateur).
    fn keyboard_panel(&self) -> Option<PanelId> {
        let panel = self.editor.active_panel()?;
        let view = self.windows.get(&self.current)?.views.view(panel)?;
        view.takes_keyboard().then_some(panel)
    }

    /// Exécute ce qu'une vue demande après une touche ou un clic.
    fn view_action(&mut self, action: ViewAction, start: Instant) {
        match action {
            ViewAction::Ignored => {}
            ViewAction::Handled => {
                self.blink.reset(Instant::now());
                self.request_redraw();
            }
            ViewAction::Run(line) => self.execute(&line, start),
            ViewAction::CommandLine(text) => self.open_command_line(&text),
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
                if let Some(view) = self
                    .windows
                    .get_mut(&self.current)
                    .and_then(|s| s.editor_view_mut(panel))
                {
                    view.set_preedit(preedit);
                }
                self.blink.reset(Instant::now());
                self.request_redraw();
            }
            Ime::Commit(text) => {
                self.composing = false;
                self.clear_preedit();
                if !self.overlay_commit(&text) && self.keyboard_panel().is_none() {
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
        let Some(state) = self.windows.get_mut(&self.current) else {
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
        // Barre de recherche ouverte : le texte ou le document actif ont pu changer.
        if self.find.is_some() {
            self.start_search();
            self.refresh_highlights();
        }
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
        let look = (
            ViewSettings::from_config(self.editor.config()),
            self.editor.zoom(),
        );
        if self.applied_look.as_ref() != Some(&look) {
            self.applied_look = Some(look);
            self.look += 1;
            self.redraw_all();
        }
        let config = self.editor.config();
        self.blink.configure(
            config.view.cursor_blink,
            config.view.cursor_blink_interval(),
        );
        for state in self.windows.values_mut() {
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
        self.look += 1;
        self.redraw_all();
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
        if self.find_clicked(x, y) {
            self.request_redraw();
            return;
        }
        // Un clic dans l'éditeur ferme les couches et lui rend le focus.
        self.close_overlays();
        if let Some(divider) = self
            .windows
            .get(&self.current)
            .and_then(|s| s.views.divider_at(x, y))
        {
            self.divider_drag = Some(divider.clone());
            return;
        }
        if self.tab_pressed(x, y) {
            return;
        }
        // Un clic dans une autre zone lui donne le focus ; le clic y place ensuite le curseur.
        if let Some(panel) = self
            .windows
            .get(&self.current)
            .and_then(|s| s.views.panel_at(x, y))
            && self.editor.active_panel() != Some(panel)
        {
            self.editor.focus_panel(panel);
            self.after_any_command(Instant::now());
        }
        // Un panneau qui gère sa souris (explorateur) reçoit le clic.
        if let Some(panel) = self.keyboard_panel() {
            let now = Instant::now();
            let action = match self.windows.get_mut(&self.current) {
                Some(state) => {
                    let clicks = self
                        .clicks
                        .click(now, x, y, state.window.scale_factor() as f32);
                    state
                        .views
                        .view_mut(panel)
                        .map_or(ViewAction::Ignored, |view| view.click(x, y, clicks))
                }
                None => ViewAction::Ignored,
            };
            self.view_action(action, now);
            return;
        }
        let panel = self.editor.active_panel();
        let (Some(state), Some(document)) = (
            self.windows.get(&self.current),
            self.editor.active_document_mut(),
        ) else {
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
        let Some(state) = self.windows.get(&self.current) else {
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
        let Some(panel) = self
            .windows
            .get(&self.current)
            .and_then(|s| s.views.tab_at(x, y))
        else {
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
        let (Some(state), Some(document)) = (
            self.windows.get(&self.current),
            self.editor.active_document_mut(),
        ) else {
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
        let Some(state) = self.windows.get(&self.current) else {
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
    fn scroll(&mut self, window: LayoutWindowId, delta: MouseScrollDelta) {
        let active = self
            .editor
            .workspace()
            .window(window)
            .and_then(|w| w.active());
        let Some(state) = self.windows.get_mut(&window) else {
            return;
        };
        // La molette fait défiler le panneau sous la souris.
        let (x, y) = self.mouse_position;
        let Some(panel) = state.views.panel_at(x, y).or(active) else {
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

    /// Redessine la fenêtre courante, et les autres fenêtres seulement si ce qu'elles affichent
    /// a changé.
    fn request_redraw(&self) {
        for (id, state) in &self.windows {
            if *id == self.current || state.stamp != Stamp::of(&self.editor, *id, self.look) {
                state.window.request_redraw();
            }
        }
    }

    /// Redessine toutes les fenêtres (focus, apparence).
    fn redraw_all(&self) {
        for state in self.windows.values() {
            state.window.request_redraw();
        }
    }

    fn refresh_title(&self) {
        for (id, state) in &self.windows {
            state.window.set_title(&self.title(*id));
        }
    }

    fn modified_documents(&self) -> Vec<String> {
        self.editor
            .documents()
            .filter(|(_, doc)| doc.is_modified())
            .map(|(_, doc)| doc.display_name())
            .collect()
    }

    /// Bouton de fermeture d'une fenêtre. La dernière fenêtre quitte l'application (option
    /// `[window] quit_on_last_close`) ; une autre est fermée avec ses documents qui ne sont
    /// affichés nulle part ailleurs. Des modifications non enregistrées demandent de confirmer en
    /// fermant une seconde fois.
    fn request_close(&mut self, event_loop: &ActiveEventLoop, window: LayoutWindowId) {
        let last = self.editor.workspace().windows().len() <= 1;
        let quit = last && self.editor.config().window.quit_on_last_close;
        let modified: Vec<String> = if quit {
            self.modified_documents()
        } else {
            self.editor
                .documents_only_in(window)
                .into_iter()
                .filter_map(|id| self.editor.document(id))
                .filter(|d| d.is_modified())
                .map(|d| d.display_name())
                .collect()
        };
        match self.close_guard.request(&modified, Instant::now()) {
            CloseDecision::Close if quit => {
                tracing::info!("fermeture de la dernière fenêtre");
                event_loop.exit();
            }
            CloseDecision::Close => {
                tracing::info!(fenetre = %window, "fermeture d'une fenêtre");
                if let Err(error) = self.editor.close_window(window, true) {
                    tracing::warn!("{error}");
                }
                self.sync_windows(event_loop);
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
        for (id, state) in &self.windows {
            for (panel, scroll) in state.views.scroll_positions() {
                self.editor.set_panel_scroll(panel, scroll);
            }
            // Écran, position, taille et état de chaque fenêtre.
            let os = &state.window;
            let position = os.outer_position().unwrap_or_default();
            let size = os.inner_size();
            let geometry = WindowGeometry {
                monitor: os.current_monitor().and_then(|m| m.name()),
                x: position.x,
                y: position.y,
                width: size.width,
                height: size.height,
                maximized: os.is_maximized(),
                fullscreen: os.fullscreen().is_some(),
            };
            self.editor.workspace_mut().set_geometry(*id, geometry);
        }
        let Some(dir) = self.editor.sessions_dir() else {
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

    /// Ouvre la fenêtre du système de la fenêtre `id` du noyau ; la première initialise le GPU.
    fn open_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: LayoutWindowId,
    ) -> Result<(), String> {
        let mut attributes = Window::default_attributes()
            .with_title(self.title(id))
            .with_inner_size(INITIAL_SIZE);
        // Sous Wayland, la position est ignorée : l'écran demandé est traité après l'ouverture.
        let pending = self.pending_placement.remove(&id);
        let saved = self
            .editor
            .workspace()
            .window(id)
            .and_then(|w| w.geometry())
            .cloned()
            .filter(|_| pending.is_none());
        if let Some(saved) = &saved {
            // Fenêtre d'une session : à sa place, ou rapatriée sur un écran disponible.
            let placement = cupride_core::monitors::rescue(
                Placement {
                    x: saved.x,
                    y: saved.y,
                    width: saved.width,
                    height: saved.height,
                },
                saved.monitor.as_deref(),
                self.editor.monitors(),
            );
            attributes = attributes
                .with_position(PhysicalPosition::new(placement.x, placement.y))
                .with_inner_size(PhysicalSize::new(placement.width, placement.height))
                .with_maximized(saved.maximized);
        } else if let Some(placement) = pending
            .filter(|_| !self.wayland)
            .and_then(|monitor| self.placement_on(monitor, None))
        {
            // Écran demandé (window new --monitor N) : centrée dessus.
            attributes = attributes
                .with_position(PhysicalPosition::new(placement.x, placement.y))
                .with_inner_size(PhysicalSize::new(placement.width, placement.height));
        } else if let Some(position) = self
            .windows
            .get(&self.current)
            .and_then(|s| {
                s.window
                    .outer_position()
                    .ok()
                    .map(|p| (p, s.window.scale_factor()))
            })
            .map(|(p, scale)| {
                let step = (CASCADE * scale).round() as i32;
                PhysicalPosition::new(p.x + step, p.y + step)
            })
        {
            attributes = attributes.with_position(position);
        }
        let window = event_loop
            .create_window(attributes)
            .map(Arc::new)
            .map_err(|error| format!("impossible d'ouvrir une fenêtre : {error}"))?;
        tracing::info!(fenetre = %id, os = ?window.id(), "fenêtre ouverte");
        // Méthodes de saisie (IME) et touches mortes composées par le système.
        window.set_ime_allowed(true);
        let surface = match &self.gpu {
            // Un seul device GPU : les fenêtres suivantes n'ont besoin que d'une surface.
            Some(gpu) => gpu
                .instance
                .create_surface(Arc::clone(&window))
                .map_err(|e| format!("pas de surface pour la fenêtre : {e}"))?,
            None => {
                let (gpu, surface) =
                    Gpu::for_window(Arc::clone(&window)).map_err(|e| e.to_string())?;
                tracing::info!(gpu = %gpu.describe(), "GPU prêt");
                self.gpu = Some(gpu);
                surface
            }
        };
        let gpu = self.gpu.as_ref().expect("GPU initialisé");
        let size = window.inner_size();
        let surface =
            WindowSurface::new(gpu, surface, Arc::clone(&window), size.width, size.height)
                .map_err(|e| e.to_string())?;
        let renderer = Renderer::new(gpu, surface.format());
        let settings = ViewSettings::from_config(self.editor.config());
        let views = PanelViews::new(
            ViewRegistry::default(),
            settings,
            window.scale_factor() as f32 * self.editor.zoom(),
        )
        .for_window(id);
        if self.editor.active_window() == Some(id) {
            // Lancée depuis un terminal, l'application passe au premier plan avec le focus.
            window.focus_window();
            self.current = id;
        }
        self.os_windows.insert(window.id(), id);
        self.windows.insert(
            id,
            WindowState {
                surface,
                renderer,
                views,
                status_bar: StatusBar::default(),
                stamp: None,
                window,
            },
        );
        if let Some(monitor) = pending.filter(|_| self.wayland) {
            self.place_window(id, monitor);
        }
        if saved.is_some_and(|s| s.fullscreen)
            && let Some(state) = self.windows.get(&id)
        {
            let monitor = state.window.current_monitor();
            state
                .window
                .set_fullscreen(Some(Fullscreen::Borderless(monitor)));
        }
        Ok(())
    }

    /// Ouvre et ferme les fenêtres du système pour qu'elles correspondent à celles du noyau.
    /// Sans plus aucune fenêtre, l'application quitte (sauf `[window] quit_on_last_close =
    /// false`).
    fn sync_windows(&mut self, event_loop: &ActiveEventLoop) {
        let wanted: Vec<LayoutWindowId> = self
            .editor
            .workspace()
            .windows()
            .iter()
            .map(|w| w.id())
            .collect();
        let gone: Vec<LayoutWindowId> = self
            .windows
            .keys()
            .filter(|id| !wanted.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(state) = self.windows.remove(&id) {
                self.os_windows.remove(&state.window.id());
                tracing::info!(fenetre = %id, "fenêtre fermée");
            }
        }
        let mut opened = false;
        for id in wanted {
            if self.windows.contains_key(&id) {
                continue;
            }
            if let Err(message) = self.open_window(event_loop, id) {
                if self.gpu.is_none() {
                    self.fail(event_loop, message);
                    return;
                }
                tracing::warn!("{message}");
                self.editor.notify(Level::Error, message);
                let _ = self.editor.close_window(id, true);
            }
            opened = true;
        }
        if opened && !self.started {
            self.started = true;
            // Thème et barre d'état de la configuration, dès la première image.
            self.sync_config();
            // Fichiers du projet indexés en arrière-plan, prêts pour l'ouverture rapide.
            self.refresh_file_index();
        }
        if opened {
            self.sync_config();
        }
        if let Some(active) = self.editor.active_window()
            && active != self.current
            && let Some(state) = self.windows.get(&active)
        {
            // Une fenêtre qui reçoit le focus par une commande passe au premier plan.
            state.window.focus_window();
            self.current = active;
        }
        if self.started && self.windows.is_empty() && self.editor.config().window.quit_on_last_close
        {
            tracing::info!("plus aucune fenêtre");
            event_loop.exit();
        }
        if opened {
            self.refresh_title();
        }
    }

    /// Énumère les écrans (au plus une fois par seconde, ou tout de suite avec `force`) et les
    /// donne au noyau, qui les numérote.
    fn refresh_monitors(&mut self, event_loop: &ActiveEventLoop, force: bool) {
        let now = Instant::now();
        if !force
            && self
                .monitors_checked
                .is_some_and(|t| now.duration_since(t).as_secs() < 1)
        {
            return;
        }
        self.monitors_checked = Some(now);
        let primary = event_loop.primary_monitor();
        let mut monitors: Vec<(MonitorInfo, MonitorHandle)> = event_loop
            .available_monitors()
            .map(|handle| {
                let position = handle.position();
                let size = handle.size();
                let info = MonitorInfo {
                    name: handle.name().unwrap_or_default(),
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                    scale: handle.scale_factor(),
                    primary: primary.as_ref() == Some(&handle),
                };
                (info, handle)
            })
            .collect();
        let mut infos: Vec<MonitorInfo> = monitors.iter().map(|(i, _)| i.clone()).collect();
        cupride_core::monitors::sort_monitors(&mut infos);
        // Même ordre que la numérotation du noyau.
        monitors.sort_by_key(|(info, _)| infos.iter().position(|i| i == info));
        if infos.as_slice() != self.editor.monitors() {
            tracing::debug!(ecrans = infos.len(), "écrans énumérés");
            self.editor.set_monitors(infos);
        }
        self.monitors = monitors;
    }

    /// Place d'une fenêtre centrée sur l'écran `monitor` (numéro à partir de 1), en gardant la
    /// taille `size` si elle tient.
    fn placement_on(&self, monitor: usize, size: Option<PhysicalSize<u32>>) -> Option<Placement> {
        let (info, _) = self.monitors.get(monitor.checked_sub(1)?)?;
        let size = size.unwrap_or_else(|| INITIAL_SIZE.to_physical(info.scale));
        Some(info.center(size.width, size.height))
    }

    /// Place la fenêtre `window` sur l'écran `monitor` : centrée, réduite pour y tenir. Une
    /// fenêtre pas encore ouverte le sera directement dessus.
    fn place_window(&mut self, window: LayoutWindowId, monitor: usize) {
        let Some(state) = self.windows.get(&window) else {
            self.pending_placement.insert(window, monitor);
            return;
        };
        let size = state.window.inner_size();
        let Some(placement) = self.placement_on(monitor, Some(size)) else {
            let message = format!("écran {monitor} introuvable");
            tracing::warn!("{message}");
            self.editor.notify(Level::Warning, message);
            return;
        };
        let os = &state.window;
        let setting = self.editor.config().window.wayland_placement;
        match cupride_core::monitors::strategy(self.wayland, setting, placement) {
            PlacementStrategy::Position(placement) => {
                os.set_fullscreen(None);
                os.set_maximized(false);
                os.set_outer_position(PhysicalPosition::new(placement.x, placement.y));
                if (placement.width, placement.height) != (size.width, size.height) {
                    let _ =
                        os.request_inner_size(PhysicalSize::new(placement.width, placement.height));
                }
                tracing::info!(fenetre = %window, ecran = monitor, "fenêtre placée");
            }
            PlacementStrategy::Fullscreen => {
                let handle = self.monitors.get(monitor - 1).map(|(_, h)| h.clone());
                os.set_fullscreen(Some(Fullscreen::Borderless(handle)));
                let message = format!(
                    "Wayland : les fenêtres ne peuvent pas être positionnées ; la fenêtre {window} \
                     passe en plein écran sur l'écran {monitor} ([window] wayland_placement)"
                );
                tracing::info!("{message}");
                self.editor.notify(Level::Info, message);
            }
            PlacementStrategy::Compositor => {
                let message = format!(
                    "Wayland : les fenêtres ne peuvent pas être positionnées ; la fenêtre {window} \
                     reste où le compositeur l'a placée, pas sur l'écran {monitor} \
                     ([window] wayland_placement)"
                );
                tracing::info!("{message}");
                self.editor.notify(Level::Info, message);
            }
        }
    }

    /// Exécute les lignes de commande reçues d'autres `cupride`, répond à chaque terminal, et
    /// amène la fenêtre active au premier plan.
    fn remote_requests(&mut self, event_loop: &ActiveEventLoop) {
        let pending = self
            .remote
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default();
        if pending.is_empty() {
            return;
        }
        for incoming in pending {
            tracing::info!(arguments = ?incoming.request.args, "demande d'un terminal");
            let answer = crate::remote::handle(&mut self.editor, &incoming.request);
            let mut responder = incoming.responder;
            if !answer.wait.is_empty() {
                // --wait : les messages tout de suite, la fin à la fermeture des documents.
                for reply in &answer.replies {
                    let _ = responder.reply(reply);
                }
                self.waiting.push((answer.wait, responder));
                continue;
            }
            if let Err(error) = responder.finish(&answer.replies, answer.code) {
                tracing::debug!("terminal parti avant la réponse : {error}");
            }
        }
        self.after_any_command(Instant::now());
        self.sync_windows(event_loop);
        if let Some(state) = self.windows.get(&self.current) {
            state.window.focus_window();
        }
    }

    /// Quitter quand ces documents sont fermés (lancé avec `--wait` sans instance existante).
    pub(crate) fn exit_when_closed(&mut self, documents: Vec<DocumentId>) {
        self.exit_when_closed = documents;
    }

    /// Répond aux terminaux dont les documents attendus sont tous fermés ; quitte si c'est
    /// l'attente de ce processus.
    fn release_waiting(&mut self, event_loop: &ActiveEventLoop) {
        let editor = &self.editor;
        let closed =
            |documents: &[DocumentId]| documents.iter().all(|d| editor.document(*d).is_none());
        let (done, still): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition(|(documents, _)| closed(documents));
        self.waiting = still;
        for (_, responder) in done {
            let _ = responder.finish(&[], 0);
        }
        if !self.exit_when_closed.is_empty() && closed(&self.exit_when_closed) {
            tracing::info!("fichiers attendus fermés (--wait) : fin");
            event_loop.exit();
        }
    }

    /// Une fenêtre reçoit le clavier ou la souris : elle devient la fenêtre courante, et la
    /// fenêtre active du noyau.
    fn enter_window(&mut self, window: LayoutWindowId) {
        if window == self.current && self.editor.active_window() == Some(window) {
            return;
        }
        // Les couches restent dans la fenêtre où elles ont été ouvertes : elles se ferment.
        if window != self.current {
            self.close_overlays();
            self.tab_drag = None;
            self.divider_drag = None;
        }
        self.current = window;
        self.editor.focus_window(window);
        self.after_any_command(Instant::now());
    }

    /// Dessine la fenêtre `id`. Les couches (ligne de commande, palette, notifications) ne sont
    /// dessinées que dans la fenêtre courante.
    fn redraw(&mut self, id: LayoutWindowId) {
        let current = id == self.current;
        let (Some(gpu), Some(state)) = (&self.gpu, self.windows.get_mut(&id)) else {
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
        if current {
            self.editor.set_layout_metrics(state.views.metrics());
        }
        // Panneau actif de cette fenêtre : sa barre d'état le décrit.
        let panel = self.editor.workspace().window(id).and_then(|w| w.active());
        let pending_keys: Vec<String> = self
            .resolver
            .pending()
            .iter()
            .map(ToString::to_string)
            .collect();
        let pending_keys = pending_keys.join(" ");
        let status_config = &self.editor.config().status_bar;
        state.status_bar.prepare(
            &mut self.text,
            &style,
            window_rect,
            &self.status_elements,
            &StatusContext {
                editor: &self.editor,
                pending_keys: &pending_keys,
                panel,
            },
            (&status_config.left, &status_config.right),
        );
        if current && let Some(view) = state.editor_view(panel) {
            self.editor.set_page_lines(view.page_lines());
        }
        let now = Instant::now();
        let blink_on = current && self.focused && self.blink.visible(now);
        let editor_focused = current && self.focus.is_focused(Element::Editor);
        let animating = state.views.tick(now);
        let mut scene = Frame::new(self.theme.background);
        // La vue du panneau actif suit son curseur quand il bouge, ou quand le panneau change.
        let cursor = panel
            .zip(self.editor.active_document())
            .filter(|_| current)
            .and_then(|(panel, document)| {
                let id = self.editor.panel_document(panel)?;
                Some((panel, id, document.selections().primary().head))
            });
        if current && cursor != self.last_cursor {
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
        if let Some(target) = self
            .tab_drag
            .as_ref()
            .filter(|_| current)
            .and_then(|d| d.target.as_ref())
        {
            state
                .views
                .draw_drop_preview(scene.push_layer(), target, &self.theme);
        }
        state.status_bar.draw(scene.base(), &style, &self.theme);
        let mut ime_area = None;
        if let Some(line) = self.command_line.as_mut().filter(|_| current) {
            line.prepare(&mut self.text, &style, window_rect, status_height);
            ime_area = Some(line.cursor_rect(&style));
        }
        if let Some(line) = self.command_line.as_ref().filter(|_| current) {
            line.draw(scene.push_layer(), &style, &self.theme, blink_on);
        }
        if let Some((_, picker)) = self.picker.as_mut().filter(|_| current) {
            picker.prepare(&mut self.text, &style, &self.theme, parts[0]);
            ime_area = Some(picker.cursor_rect(&style));
        }
        if let Some(bar) = self.find.as_mut().filter(|_| current) {
            // En haut à droite de la zone du panneau actif.
            let zone = panel
                .and_then(|p| state.views.zones().iter().find(|z| z.active == Some(p)))
                .map_or(parts[0], |z| z.content);
            bar.prepare(&mut self.text, &style, zone);
            if self.focus.is_focused(Element::Find) {
                ime_area = Some(bar.cursor_rect(&style));
            }
        }
        if let Some(bar) = self.find.as_ref().filter(|_| current) {
            let focused = self.focus.is_focused(Element::Find);
            bar.draw(scene.push_layer(), &style, &self.theme, blink_on && focused);
        }
        if let Some((_, picker)) = self.picker.as_ref().filter(|_| current) {
            picker.draw(scene.push_layer(), &style, &self.theme, blink_on);
        }
        // Notifications, au-dessus de tout, dans la zone de l'éditeur.
        self.toasts.sync(
            self.editor.notifications(),
            &self.editor.config().notifications,
            now,
        );
        self.toasts.expire(now);
        if current && !self.toasts.is_empty() {
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
        if let Some(cursor) = ime_area.filter(|_| current).or_else(|| {
            let document = self.editor.active_document().filter(|_| current)?;
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
        state.stamp = Stamp::of(&self.editor, id, self.look);
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

/// La session graphique est-elle Wayland (et non X11) ? Toujours faux hors Linux et BSD.
fn is_wayland(event_loop: &ActiveEventLoop) -> bool {
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    {
        use winit::platform::wayland::ActiveEventLoopExtWayland;
        event_loop.is_wayland()
    }
    #[cfg(not(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    )))]
    {
        let _ = event_loop;
        false
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::FilesIndexed => self.files_indexed(),
            AppEvent::Remote => self.remote_requests(event_loop),
            AppEvent::SearchDone => self.search_done(),
            AppEvent::ProjectResults => self.project_results(),
            AppEvent::FilesChanged => {
                let changed = self.disk.take_changed();
                if changed.is_empty() {
                    return;
                }
                for state in self.windows.values_mut() {
                    for view in state.views.views_mut() {
                        view.files_changed(&changed);
                    }
                    state.window.request_redraw();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        self.wayland = is_wayland(event_loop);
        if self.wayland {
            tracing::info!("session Wayland : placement des fenêtres laissé au compositeur");
        }
        self.refresh_monitors(event_loop, true);
        self.sync_windows(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, os_id: WindowId, event: WindowEvent) {
        tracing::trace!(?event, "événement de fenêtre");
        let Some(&id) = self.os_windows.get(&os_id) else {
            return;
        };
        // Clavier et clics : la fenêtre visée devient la fenêtre courante.
        let input = matches!(
            event,
            WindowEvent::KeyboardInput { .. }
                | WindowEvent::Ime(_)
                | WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                }
        );
        if input {
            self.enter_window(id);
        }
        match event {
            WindowEvent::CloseRequested => self.request_close(event_loop, id),
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(state)) = (&self.gpu, self.windows.get_mut(&id)) {
                    state.surface.resize(gpu, size.width, size.height);
                }
                // Dessin immédiat : pendant un redimensionnement à la souris, l'image suit la
                // fenêtre au lieu d'être étirée.
                self.redraw(id);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // Fenêtre déplacée vers un écran d'une autre densité : le texte est remis en
                // forme à la nouvelle échelle. `Resized` suit avec la nouvelle taille physique.
                if let Some(state) = self.windows.get_mut(&id) {
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
            WindowEvent::MouseWheel { delta, .. } => self.scroll(id, delta),
            // La souris au-dessus d'une autre fenêtre que la courante n'y change rien, tant
            // qu'on n'y clique pas.
            WindowEvent::CursorMoved { position, .. } if id == self.current => {
                self.mouse_moved(position.x as f32, position.y as f32);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_position = (position.x as f32, position.y as f32);
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
            WindowEvent::Focused(true) => {
                self.focused = true;
                self.enter_window(id);
                self.blink.reset(Instant::now());
                self.redraw_all();
            }
            WindowEvent::Focused(false) => {
                if id == self.current {
                    self.focused = false;
                }
                self.redraw_all();
            }
            WindowEvent::RedrawRequested | WindowEvent::Occluded(false) => self.redraw(id),
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
        // Terminaux qui attendaient des documents fermés pendant ce lot d'événements.
        self.release_waiting(event_loop);
        // Écrans branchés ou débranchés, puis fenêtres ouvertes ou fermées par les commandes du
        // lot d'événements.
        if self.started {
            self.refresh_monitors(event_loop, false);
            self.sync_windows(event_loop);
        }
        // Dossiers affichés par les explorateurs de toutes les fenêtres : surveillés.
        let watched: BTreeSet<std::path::PathBuf> = self
            .windows
            .values()
            .flat_map(|s| s.views.watched_dirs())
            .collect();
        self.disk.sync(watched);
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
        // Les terminaux qui attendaient (--wait) sont libérés : l'instance s'arrête.
        for (_, responder) in std::mem::take(&mut self.waiting) {
            let _ = responder.finish(&[], 0);
        }
        // Les ressources des fenêtres sont libérées avant le GPU et la sortie de la boucle.
        self.windows.clear();
        self.os_windows.clear();
        self.gpu = None;
    }
}
