//! Application : boucle d'événements `winit` et fenêtre de l'éditeur.
//!
//! La boucle attend les événements (`ControlFlow::Wait`) : sans saisie, sans redimensionnement et
//! sans minuterie, le processus dort et ne consomme pas de CPU. Rien n'est redessiné tant que rien
//! n'a changé.

use std::sync::Arc;
use std::time::Instant;

use cupride_core::Editor;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::close::{CloseDecision, CloseGuard};

/// Taille initiale de la fenêtre, en pixels logiques.
const INITIAL_SIZE: LogicalSize<f64> = LogicalSize::new(1_100.0, 720.0);

/// État de l'application.
pub(crate) struct App {
    editor: Editor,
    window: Option<Arc<Window>>,
    close_guard: CloseGuard,
    /// Message affiché dans le titre (avertissement de fermeture), s'il y en a un.
    notice: Option<String>,
}

impl App {
    pub(crate) fn new(editor: Editor) -> Self {
        Self {
            editor,
            window: None,
            close_guard: CloseGuard::default(),
            notice: None,
        }
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

    fn refresh_title(&self) {
        if let Some(window) = &self.window {
            window.set_title(&self.title());
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
                // Réveil unique à l'expiration de l'avertissement, pour retirer le message.
                if let Some(deadline) = self.close_guard.deadline() {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                }
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(INITIAL_SIZE);
        match event_loop.create_window(attributes) {
            Ok(window) => {
                tracing::info!(id = ?window.id(), "fenêtre ouverte");
                self.window = Some(Arc::new(window));
            }
            Err(error) => {
                tracing::error!("impossible d'ouvrir une fenêtre : {error}");
                eprintln!("CuprIDE : impossible d'ouvrir une fenêtre : {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.request_close(event_loop),
            WindowEvent::Destroyed => event_loop.exit(),
            _ => {}
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        if let winit::event::StartCause::ResumeTimeReached { .. } = cause
            && self.close_guard.expired(Instant::now())
        {
            self.notice = None;
            self.refresh_title();
        }
        if self.close_guard.deadline().is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // La fenêtre est libérée avant la sortie de la boucle.
        self.window = None;
    }
}
