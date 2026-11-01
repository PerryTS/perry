//! Private qualification tool; public async generation remains guarded.
use anyhow::{ensure, Result};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 4,
        "usage: actor_compose <producer.wasm> <facade.wasm> <plan.json> <output.wasm>"
    );
    let bytes = perry_wit::compose_experimental_guest(
        &std::fs::read(&args[0])?,
        &std::fs::read(&args[1])?,
        &std::fs::read(&args[2])?,
    )?;
    std::fs::write(&args[3], bytes)?;
    Ok(())
}
