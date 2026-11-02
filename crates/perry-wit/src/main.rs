use anyhow::{bail, ensure, Context, Result};
use std::path::Path;

const USAGE: &str = "usage: perry-wit [--guest-c [--target wasip3|wasip2]] <wit-file-or-directory> <output-directory> [world]\n       perry-wit compose <producer.wasm> <facade.wasm> <plan.json> <output.wasm>";

fn main() -> Result<()> {
    let raw: Vec<_> = std::env::args().skip(1).collect();
    if raw.len() == 1 && matches!(raw[0].as_str(), "--help" | "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    if raw.first().is_some_and(|arg| arg == "compose") {
        ensure!(raw.len() == 5, "{USAGE}");
        let producer = std::fs::read(&raw[1]).context("reading producer component")?;
        let facade = std::fs::read(&raw[2]).context("reading facade component")?;
        let plan = std::fs::read(&raw[3]).context("reading producer/facade plan")?;
        let bytes = perry_wit::compose_guest_p3(&producer, &facade, &plan)?;
        std::fs::write(&raw[4], bytes).context("writing composed P3 guest")?;
        return Ok(());
    }
    let mut guest_c = false;
    let mut target = None;
    let mut args = Vec::new();
    let mut input = raw.into_iter();
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "--guest-c" => guest_c = true,
            "--target" => {
                ensure!(target.is_none(), "--target must be specified once");
                let value = input.next().context("--target requires wasip3 or wasip2")?;
                ensure!(
                    matches!(value.as_str(), "wasip3" | "wasip2" | "wasi"),
                    "unsupported guest target: {value}"
                );
                target = Some(value);
            }
            _ if arg.starts_with('-') => bail!("unknown option: {arg}\n{USAGE}"),
            _ => args.push(arg),
        }
    }
    ensure!(
        target.is_none() || guest_c,
        "--target selects C guest generation; use it with --guest-c"
    );
    ensure!((2..=3).contains(&args.len()), "{USAGE}");
    let world = perry_wit::parse_file(Path::new(&args[0]), args.get(2).map(String::as_str))?;
    let output = Path::new(&args[1]);
    // Resolve and validate before writing any output files.
    let c = if guest_c {
        let generate = if target.as_deref() == Some("wasip3") {
            perry_wit::canonical_guest_p3
        } else {
            perry_wit::canonical_guest
        };
        Some(generate(
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
