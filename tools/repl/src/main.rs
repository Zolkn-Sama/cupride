//! REPL console : pilote le noyau de CuprIDE sans interface graphique.

fn main() -> anyhow::Result<()> {
    println!("cupride-repl {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
