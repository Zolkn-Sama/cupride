//! Point d'entrée de CuprIDE.

mod logging;

fn main() -> anyhow::Result<()> {
    let _log_guard = logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "démarrage de CuprIDE");

    println!("CuprIDE {}", env!("CARGO_PKG_VERSION"));

    tracing::info!("arrêt de CuprIDE");
    Ok(())
}
