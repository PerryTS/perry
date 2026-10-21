//! Test/support utility for core modules carrying wit-bindgen metadata.
use anyhow::{bail, Result};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        bail!("usage: componentize <core.wasm> <component.wasm>");
    }
    let module = std::fs::read(&args[0])?;
    let component = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)?
        .encode()?;
    std::fs::write(&args[1], component)?;
    Ok(())
}
