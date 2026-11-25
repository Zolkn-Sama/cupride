//! Point d'entrée de CuprIDE.

mod app;
mod cli;
mod clipboard;
mod close;
mod file_index;
mod input;
mod latency;
mod logging;
mod redraw;
mod remote;
mod watcher;

use anyhow::Context;
use cupride_core::Editor;
use cupride_core::notifications::Level;
use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> anyhow::Result<()> {
    // La ligne de commande d'abord : --help et --version n'ouvrent ni journal ni fenêtre.
    let builtins = cupride_core::command::builtin_registry();
    let launch = match cli::parse(std::env::args_os().skip(1), |g| builtins.has_group(g)) {
        cli::Cli::Run(launch) => launch,
        cli::Cli::Print(text) => {
            print!("{text}");
            return Ok(());
        }
        cli::Cli::Error(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    // Instance unique : si CuprIDE tourne déjà, il reçoit la ligne de commande, et ce processus
    // affiche sa réponse puis se termine. Sinon, ce processus devient l'instance principale. Une
    // instance indépendante (--new-instance) ne cherche pas l'instance et n'écoute pas.
    let server = if launch.new_instance {
        None
    } else {
        match single_instance() {
            Instance::Forwarded(code) => std::process::exit(code),
            Instance::Primary(server) => server,
        }
    };
    // Dossier de l'instance indépendante : celui demandé, sinon un dossier temporaire vierge,
    // supprimé à la sortie.
    let mut temporary = None;
    let instance_dir = if launch.new_instance {
        match &launch.config_dir {
            Some(dir) => Some(std::path::absolute(dir).unwrap_or_else(|_| dir.clone())),
            None => {
                let dir = tempfile::Builder::new()
                    .prefix("cupride-instance-")
                    .tempdir()
                    .context("dossier temporaire de l'instance indépendante")?;
                let path = dir.path().to_path_buf();
                temporary = Some(dir);
                Some(path)
            }
        }
    } else {
        None
    };

    let _log_guard = logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "démarrage de CuprIDE");

    let mut editor = Editor::new();
    if let Some(dir) = &instance_dir {
        tracing::info!(dossier = %dir.display(), "instance indépendante");
        editor.set_config_path(Some(dir.join("config.toml")));
        editor.set_data_dir(Some(dir.join("data")));
    }
    editor.set_clipboard(clipboard::SystemClipboard::new_or_memory());
    // Supprimer depuis l'explorateur met à la corbeille de l'OS.
    editor.set_trash(Box::new(|path| {
        trash::delete(path).map_err(std::io::Error::other)
    }));
    for message in editor.reload_config() {
        tracing::warn!("configuration : {message}");
        editor.notify(Level::Warning, format!("configuration : {message}"));
    }

    // Un fichier ou un dossier illisible n'empêche pas de démarrer : il est signalé, comme un
    // fichier de la session supprimé entre-temps.
    let sessions = (launch.restore && editor.config().session.restore)
        .then(|| editor.sessions_dir())
        .flatten();
    let opened = cli::open(&mut editor, &launch.paths, sessions.as_deref());
    let mut problems = opened.problems;
    if let Some(window) = launch.window
        && !editor.focus_window(cupride_core::layout::WindowId::new(window))
    {
        problems.push(format!("fenêtre {window} introuvable"));
    }
    // Commande demandée au lancement (cupride panel focus left) : exécutée une fois la session
    // restaurée ; son message va dans le terminal.
    if let Some(command) = &launch.command {
        let cwd = std::env::current_dir().unwrap_or_default();
        match editor.run_in(&cwd, command) {
            Ok(cupride_core::command::CommandOutput::Message(text)) => println!("{text}"),
            Ok(cupride_core::command::CommandOutput::None) => {}
            Err(error) => {
                eprintln!("{error}");
                problems.push(error.to_string());
            }
        }
    }
    for problem in problems {
        tracing::warn!("{problem}");
        editor.notify(Level::Warning, problem);
    }

    let event_loop = EventLoop::<file_index::AppEvent>::with_user_event()
        .build()
        .context("création de la boucle d'événements")?;
    // Attente passive : la boucle ne se réveille que sur un événement.
    event_loop.set_control_flow(ControlFlow::Wait);
    let remote = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    match server {
        Some(server) => {
            tracing::info!(socket = %server.endpoint().describe(), "instance principale");
            let queue = std::sync::Arc::clone(&remote);
            let proxy = event_loop.create_proxy();
            // Une demande attend dans la file ; la boucle d'événements est réveillée.
            let spawned = server.spawn(move |incoming| {
                if let Ok(mut pending) = queue.lock() {
                    pending.push(incoming);
                }
                let _ = proxy.send_event(file_index::AppEvent::Remote);
            });
            if let Err(error) = spawned {
                tracing::warn!("instance unique indisponible : {error}");
            }
        }
        None if launch.new_instance => {}
        None => tracing::warn!("instance unique indisponible : cette instance reste seule"),
    }
    let mut app = app::App::new(editor, event_loop.create_proxy(), remote);
    if launch.wait {
        // Sans instance existante, ce processus est l'éditeur : il s'arrête quand les fichiers
        // attendus sont fermés, comme un éditeur de terminal (pour Git).
        app.exit_when_closed(opened.documents);
    }
    event_loop
        .run_app(&mut app)
        .context("exécution de la boucle d'événements")?;

    if let Some(message) = app.fatal_error() {
        anyhow::bail!("CuprIDE ne peut pas démarrer : {message}");
    }
    // Le dossier temporaire d'une instance indépendante est supprimé maintenant.
    drop(temporary);
    tracing::info!("arrêt de CuprIDE");
    Ok(())
}

/// Résultat de la recherche d'une instance.
enum Instance {
    /// Une instance a reçu la ligne de commande : sortir avec ce code.
    Forwarded(i32),
    /// Ce processus est l'instance principale (sans socket si l'IPC est indisponible).
    Primary(Option<cupride_ipc::Server>),
}

/// Transmet la ligne de commande à l'instance en marche, ou devient l'instance principale.
/// L'IPC indisponible (dossier non sûr…) n'empêche pas de démarrer : l'instance reste seule.
fn single_instance() -> Instance {
    let endpoint = match cupride_ipc::Endpoint::for_user() {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("{error}");
            return Instance::Primary(None);
        }
    };
    match cupride_ipc::connect(&endpoint) {
        Ok(Some(client)) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let args = std::env::args_os()
                .skip(1)
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let request = cupride_ipc::Request::new(cwd, args);
            let sent = client.send(&request, |reply| match reply {
                cupride_ipc::Reply::Output { text } => print!("{text}"),
                cupride_ipc::Reply::Error { text } => eprintln!("{text}"),
                cupride_ipc::Reply::Exit { .. } => {}
            });
            match sent {
                Ok(code) => Instance::Forwarded(code),
                Err(error) => {
                    eprintln!("{error}");
                    Instance::Forwarded(1)
                }
            }
        }
        Ok(None) => match cupride_ipc::Server::bind(&endpoint) {
            Ok(server) => Instance::Primary(Some(server)),
            Err(error) => {
                eprintln!("{error}");
                Instance::Primary(None)
            }
        },
        Err(error) => {
            eprintln!("{error}");
            Instance::Primary(None)
        }
    }
}
