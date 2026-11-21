//! Application : boucle d'événements `winit` et fenêtre de l'éditeur.
//!
//! La boucle attend les événements (`ControlFlow::Wait`) : sans saisie, sans redimensionnement et
//! sans minuterie, le processus dort et ne consomme pas de CPU. Rien n'est redessiné tant que rien
//! n'a changé. Les seules minuteries sont le clignotement du curseur, qui s'arrête de lui-même
//! quelques secondes après la dernière saisie, et l'expiration d'un avertissement.

use std::sync::Arc;
use std::time::Instant;

use cupride_core::Editor;
use cupride_render::Frame;
use cupride_render::{Acquire, Gpu, Rect, Renderer, TextSystem, WindowSurface};
use cupride_ui::{CursorBlink, EditorView, Theme, ViewSettings};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::close::{CloseDecision, CloseGuard};

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

    /// Titre de la fenêtre : document actif, état modifié, et avertissement éventuel.
    fn title(&self) -> String {
        let mut title = match self.editor.active_document() {
            Some(doc) => {
                let modified = if doc.is_modified() { " •" } else { "" };
                format!("{}{modified} — CuprIDE", doc.display_name())
            }
            None => "CuprIDE".to_owned(),
        };
        if let Some(notice) = &self.notice {
            title = format!("{title} — {notice}");
        }
        title
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
        let (gpu, surface) = Gpu::for_window(Arc::clone(&window)).map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let surface =
            WindowSurface::new(&gpu, surface, Arc::clone(&window), size.width, size.height)
                .map_err(|e| e.to_string())?;
        tracing::info!(gpu = %gpu.describe(), format = ?surface.format(), "rendu prêt");
        let renderer = Renderer::new(&gpu, surface.format());
        let settings = ViewSettings::from_config(self.editor.config());
        let view = EditorView::new(settings, window.scale_factor() as f32);
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
        state
            .view
            .set_bounds(Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32));
        let cursors_visible = self.focused && self.blink.visible(Instant::now());
        let scene = match self.editor.active_document() {
            Some(document) => {
                state.view.prepare(&mut self.text, document);
                state.view.frame(&self.theme, document, cursors_visible)
            }
            None => Frame::new(self.theme.background),
        };
        if let Err(error) = state
            .renderer
            .render(gpu, &mut self.text, &frame.view, size, &scene)
        {
            tracing::error!("{error}");
        }
        state.window.pre_present_notify();
        state.surface.present(gpu, frame);
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
                    state.view.set_scale(scale_factor as f32);
                    state.window.request_redraw();
                }
            }
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
        // Les ressources de la fenêtre sont libérées avant le GPU et la sortie de la boucle.
        self.window = None;
        self.gpu = None;
    }
}
