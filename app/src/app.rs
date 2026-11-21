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
use cupride_core::text::{Selections, TextBuffer};
use cupride_core::{DocumentId, Editor};
use cupride_render::Frame;
use cupride_render::{Acquire, Gpu, Rect, Renderer, TextSystem, WindowSurface};
use cupride_ui::{
    ClickCounter, ClickModifiers, CommandLine, CursorBlink, EditorView, Focus, MouseSelection,
    Pointer, Preedit, Theme, ViewSettings, WHEEL_LINES,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::ModifiersState;
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{CursorIcon, Window, WindowId};

mod overlays;

use overlays::Element;

use crate::close::{CloseDecision, CloseGuard};
use crate::input;
use crate::latency::LatencyStats;

/// Taille initiale de la fenêtre, en pixels logiques.
const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1_100.0, 720.0);

/// Fenêtre ouverte et ses ressources de rendu.
struct WindowState {
    // Ordre des champs : la surface doit être libérée avant la fenêtre.
    surface: WindowSurface,
    renderer: Renderer,
    view: EditorView,
    window: Arc<Window>,
}

/// État de l'application.
pub(crate) struct App {
    editor: Editor,
    text: TextSystem,
    theme: Theme,
    gpu: Option<Gpu>,
    window: Option<WindowState>,
    close_guard: CloseGuard,
    blink: CursorBlink,
    /// La fenêtre a-t-elle le focus clavier ? Sans focus, les curseurs sont masqués et ne
    /// clignotent pas.
    focused: bool,
    /// Modificateurs enfoncés.
    modifiers: ModifiersState,
    /// Document et position du curseur principal au dernier dessin : quand ils changent, la vue
    /// suit le curseur.
    last_cursor: Option<(DocumentId, usize)>,
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
    /// Forme actuelle du pointeur.
    pointer: Pointer,
    /// Dernier texte envoyé à la sélection primaire, pour ne pas le renvoyer à chaque image.
    last_primary: Option<String>,
    /// Élément qui reçoit le clavier : l'éditeur, ou une couche ouverte par-dessus.
    focus: Focus<Element>,
    /// Ligne de commande, quand elle est ouverte.
    command_line: Option<CommandLine>,
    /// Message affiché dans le titre (avertissement de fermeture), s'il y en a un.
    notice: Option<String>,
    /// Erreur qui a empêché de démarrer.
    fatal: Option<String>,
}

impl App {
    pub(crate) fn new(editor: Editor) -> Self {
        let text = TextSystem::new();
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
            pointer: Pointer::Default,
            last_primary: None,
            focus: Focus::new(Element::Editor),
            command_line: None,
            text,
            theme: Theme::default(),
            gpu: None,
            window: None,
            close_guard: CloseGuard::default(),
            notice: None,
            fatal: None,
        }
    }

    /// Erreur qui a empêché de démarrer, à signaler par le code de sortie.
    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }

    /// Message affiché dans le titre jusqu'à la prochaine commande.
    pub(crate) fn set_notice(&mut self, notice: String) {
        self.notice = Some(notice);
        self.refresh_title();
    }

    /// Titre de la fenêtre : document actif, état modifié, projet, et avertissement éventuel.
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
        if let Some(notice) = &self.notice {
            title = format!("{title} — {notice}");
        }
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
            Resolution::Pending => {
                let pending: Vec<String> = self
                    .resolver
                    .pending()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                self.notice = Some(format!("{} …", pending.join(" ")));
                self.refresh_title();
            }
            Resolution::Unbound => {
                if self.notice.take().is_some() {
                    self.refresh_title();
                }
                tracing::debug!(touches = %combo, "aucun raccourci");
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
                if let Some(state) = &mut self.window {
                    state.view.set_preedit(preedit);
                }
                self.blink.reset(Instant::now());
                self.request_redraw();
            }
            Ime::Commit(text) => {
                self.composing = false;
                if let Some(state) = &mut self.window {
                    state.view.set_preedit(None);
                }
                if !self.overlay_commit(&text) {
                    self.insert(&text, Instant::now());
                }
            }
            Ime::Enabled => {}
            Ime::Disabled => {
                self.composing = false;
                if let Some(state) = &mut self.window {
                    state.view.set_preedit(None);
                }
                self.request_redraw();
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

    /// Après une commande : message éventuel dans le titre, réglages rechargés, curseur rallumé
    /// et nouvelle image.
    fn after_command(&mut self, result: cupride_core::command::CommandResult, start: Instant) {
        self.notice = match result {
            Ok(CommandOutput::None) => None,
            Ok(CommandOutput::Message(message)) => Some(message),
            Err(error) => {
                tracing::warn!("{error}");
                Some(error.to_string())
            }
        };
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
        let config = self.editor.config();
        self.blink.configure(
            config.view.cursor_blink,
            config.view.cursor_blink_interval(),
        );
        if let Some(state) = &mut self.window {
            state.view.set_settings(ViewSettings::from_config(config));
            // Zoom de l'interface (view zoom-in…) : appliqué à chaud, comme un changement d'écran.
            state
                .view
                .set_scale(state.window.scale_factor() as f32 * self.editor.zoom());
        }
    }

    /// Bouton gauche enfoncé : curseur, mot ou ligne selon le nombre de clics ; Maj étend la
    /// sélection, Alt ajoute un curseur.
    fn mouse_pressed(&mut self) {
        let (x, y) = self.mouse_position;
        if let Some(line) = &self.command_line
            && line.area().contains(x, y)
        {
            return;
        }
        // Un clic dans l'éditeur ferme les couches et lui rend le focus.
        self.close_overlays();
        let (Some(state), Some(document)) = (&self.window, self.editor.active_document_mut())
        else {
            return;
        };
        let (x, y) = self.mouse_position;
        let now = Instant::now();
        let clicks = self
            .clicks
            .click(now, x, y, state.window.scale_factor() as f32);
        let offset = state.view.offset_at(x, y, document.buffer());
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
        if !self.editor.config().editor.primary_selection {
            return;
        }
        let (Some(state), Some(document)) = (&self.window, self.editor.active_document_mut())
        else {
            return;
        };
        let (x, y) = self.mouse_position;
        let offset = state.view.offset_at(x, y, document.buffer());
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
        let pointer = state.view.pointer_at(x, y);
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
        let Some(document) = self.editor.active_document_mut() else {
            return;
        };
        let offset = state.view.offset_at(x, y, document.buffer());
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
        let (dx, dy, smooth) = match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                let step = WHEEL_LINES * state.view.line_height();
                (-x * step, -y * step, true)
            }
            MouseScrollDelta::PixelDelta(p) => (-p.x as f32, -p.y as f32, false),
        };
        let (dx, dy) = if self.modifiers.shift_key() && dx == 0.0 {
            (dy, 0.0)
        } else {
            (dx, dy)
        };
        state.view.scroll_by(dx, dy, smooth, Instant::now());
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
                self.notice = Some(message);
                self.refresh_title();
                // Le réveil à l'expiration de l'avertissement est programmé par `about_to_wait`.
            }
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
        let view = EditorView::new(settings, window.scale_factor() as f32 * self.editor.zoom());
        self.gpu = Some(gpu);
        self.window = Some(WindowState {
            surface,
            renderer,
            view,
            window,
        });
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
        state.view.set_bounds(window_rect);
        self.editor.set_page_lines(state.view.page_lines());
        let now = Instant::now();
        let blink_on = self.focused && self.blink.visible(now);
        let editor_focused = self.focus.is_focused(Element::Editor);
        let cursors_visible = blink_on && editor_focused;
        let animating = state.view.tick(now);
        let mut scene = Frame::new(self.theme.background);
        if let Some(document) = self.editor.active_document() {
            let cursor = self
                .editor
                .active_id()
                .map(|id| (id, document.selections().primary().head));
            if cursor != self.last_cursor {
                self.last_cursor = cursor;
                state.view.follow_cursor();
            }
            state.view.prepare(&mut self.text, document);
            state
                .view
                .draw(scene.base(), &self.theme, document, cursors_visible);
        }
        let style = state.view.style();
        let mut ime_area = None;
        if let Some(line) = &mut self.command_line {
            line.prepare(&mut self.text, &style, window_rect, 0.0);
            ime_area = Some(line.cursor_rect(&style));
        }
        if let Some(line) = &self.command_line {
            line.draw(scene.push_layer(), &style, &self.theme, blink_on);
        }
        if let Err(error) = state
            .renderer
            .render(gpu, &mut self.text, &frame.view, size, &scene)
        {
            tracing::error!("{error}");
        }
        if let Some(cursor) = ime_area.or_else(|| {
            self.editor
                .active_document()
                .and_then(|document| state.view.primary_cursor_rect(document))
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

impl ApplicationHandler for App {
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
                        .view
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
            if self.close_guard.expired(now) {
                self.notice = None;
                self.refresh_title();
            }
            // Réveil du clignotement : le curseur change d'état.
            if self.focused {
                self.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Prochain réveil : expiration de l'avertissement de fermeture, ou changement d'état du
        // curseur. Sans l'un ni l'autre, la boucle dort jusqu'au prochain événement.
        let now = Instant::now();
        let blink = self.focused.then(|| self.blink.next_change(now)).flatten();
        let wake = match (self.close_guard.deadline(), blink) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
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
        // Les ressources de la fenêtre sont libérées avant le GPU et la sortie de la boucle.
        self.window = None;
        self.gpu = None;
    }
}
