//! Private acceptance generator; does not enable async public CLI output.
use anyhow::{ensure, Result};
use std::path::Path;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 3,
        "usage: async_guest <wit> <world> <output-dir>"
    );
    let world = perry_wit::parse_file(Path::new(&args[0]), Some(&args[1]))?;
    let files = perry_wit::experimental_async_guest(Path::new(&args[0]), Some(&args[1]))?;
    std::fs::create_dir_all(&args[2])?;
    for (name, contents) in files {
        std::fs::write(Path::new(&args[2]).join(name), contents)?;
    }
    std::fs::write(
        Path::new(&args[2]).join("guest.ts"),
        perry_wit::typescript_guest(&world),
    )?;
    Ok(())
}
