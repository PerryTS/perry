//! Encode canonical WAT fixtures for native TypeScript acceptance tests.
use anyhow::{bail, Result};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        bail!("usage: component_wat <component.wat> <component.wasm>");
    }
    std::fs::write(&args[1], wat::parse_file(&args[0])?)?;
    Ok(())
}
