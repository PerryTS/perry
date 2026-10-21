use anyhow::{bail, Context, Result};
use std::path::Path;

fn main() -> Result<()> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let guest_c = args.first().is_some_and(|arg| arg == "--guest-c");
    if guest_c {
        args.remove(0);
    }
    if !(2..=3).contains(&args.len()) {
        bail!("usage: perry-wit [--guest-c] <wit-file-or-directory> <output-directory> [world]");
    }
    let world = perry_wit::parse_file(Path::new(&args[0]), args.get(2).map(String::as_str))?;
    let output = Path::new(&args[1]);
    // Resolve and validate before writing any output files.
    let c = if guest_c {
        Some(perry_wit::canonical_guest(
            Path::new(&args[0]),
            args.get(2).map(String::as_str),
        )?)
    } else {
        None
    };
    std::fs::create_dir_all(output).context("creating bindings output directory")?;
    std::fs::write(output.join("bindings.ts"), perry_wit::typescript(&world))?;
    std::fs::write(output.join("guest.ts"), perry_wit::typescript_guest(&world))?;
    std::fs::write(
        output.join("world.json"),
        serde_json::to_vec_pretty(&world)?,
    )?;
    if let Some(files) = c {
        for (name, bytes) in files {
            std::fs::write(output.join(name), bytes)?;
        }
    }
    Ok(())
}
