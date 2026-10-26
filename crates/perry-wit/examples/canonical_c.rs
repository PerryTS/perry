//! Generate the maintained C backend alone for canonical ABI acceptance probes.
//! This does not generate Perry adapters or enable Perry async guest output.
use anyhow::{ensure, Result};
use std::path::Path;
use wit_bindgen_core::Files;
use wit_parser::Resolve;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 3,
        "usage: canonical_c <wit> <world> <output-dir>"
    );
    let mut resolve = Resolve::default();
    let (package, _) = resolve.push_path(Path::new(&args[0]))?;
    let world = resolve.select_world(&[package], Some(&args[1]))?;
    let mut files = Files::default();
    wit_bindgen_c::Opts {
        no_sig_flattening: true,
        autodrop_borrows: wit_bindgen_c::Enabled::Yes,
        ..Default::default()
    }
    .build()
    .generate(&mut resolve, world, &mut files)?;
    std::fs::create_dir_all(&args[2])?;
    for (name, data) in files.iter() {
        std::fs::write(Path::new(&args[2]).join(name), data)?;
    }
    Ok(())
}
