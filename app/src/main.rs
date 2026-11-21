//! Point d'entrée de CuprIDE.

mod app;
mod clipboard;
mod close;
mod input;
mod latency;
mod logging;

use anyhow::Context;
use cupride_core::Editor;
use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> anyhow::Result<()> {
    let _log_guard = logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "démarrage de CuprIDE");

    let mut editor = Editor::new();
    editor.set_clipboard(clipboard::SystemClipboard::new_or_memory());
    for message in editor.reload_config() {
        tracing::warn!("configuration : {message}");
    }

    // Provisoire : le premier argument est le fichier à ouvrir (vraie CLI : RENDER-11).
    match std::env::args().nth(1) {
        Some(path) => {
            editor
                .open(&path)
                .with_context(|| format!("ouverture de {path}"))?;
        }
        None => {
            editor.new_document();
        }
    }

    let event_loop = EventLoop::new().context("création de la boucle d'événements")?;
    // Attente passive : la boucle ne se réveille que sur un événement.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = app::App::new(editor);
    event_loop
        .run_app(&mut app)
        .context("exécution de la boucle d'événements")?;

    if let Some(message) = app.fatal_error() {
        anyhow::bail!("CuprIDE ne peut pas démarrer : {message}");
    }
    tracing::info!("arrêt de CuprIDE");
    Ok(())
}
