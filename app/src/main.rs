//! Point d'entrée de CuprIDE.

mod app;
mod cli;
mod clipboard;
mod close;
mod input;
mod latency;
mod logging;

use anyhow::Context;
use cupride_core::Editor;
use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> anyhow::Result<()> {
    // La ligne de commande d'abord : --help et --version n'ouvrent ni journal ni fenêtre.
    let paths = match cli::parse(std::env::args_os().skip(1)) {
        cli::Cli::Run(paths) => paths,
        cli::Cli::Print(text) => {
            print!("{text}");
            return Ok(());
        }
        cli::Cli::Error(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    let _log_guard = logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "démarrage de CuprIDE");

    let mut editor = Editor::new();
    editor.set_clipboard(clipboard::SystemClipboard::new_or_memory());
    for message in editor.reload_config() {
        tracing::warn!("configuration : {message}");
    }

    // Un fichier ou un dossier illisible n'empêche pas de démarrer : il est signalé.
    let problems = cli::open(&mut editor, &paths);
    for problem in &problems {
        tracing::warn!("{problem}");
    }

    let event_loop = EventLoop::new().context("création de la boucle d'événements")?;
    // Attente passive : la boucle ne se réveille que sur un événement.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = app::App::new(editor);
    if !problems.is_empty() {
        app.set_notice(problems.join(" ; "));
    }
    event_loop
        .run_app(&mut app)
        .context("exécution de la boucle d'événements")?;

    if let Some(message) = app.fatal_error() {
        anyhow::bail!("CuprIDE ne peut pas démarrer : {message}");
    }
    tracing::info!("arrêt de CuprIDE");
    Ok(())
}
