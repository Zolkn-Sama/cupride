//! Point d'entrée de CuprIDE.

fn main() -> anyhow::Result<()> {
    println!("CuprIDE {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
