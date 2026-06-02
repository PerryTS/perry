//! `perry native list` — show the bindings shipped with this Perry build.

use anyhow::Result;
use clap::Args;

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct ListArgs {}

pub fn run(_args: ListArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    let entries: Vec<_> = crate::commands::compile::well_known::iter_well_known().collect();

    match format {
        OutputFormat::Json => {
            let v: Vec<serde_json::Value> = entries
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "package": b.package,
                        "crate": b.krate,
                        "lib": b.lib,
                        "tracking": b.tracking,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        _ => {
            println!("{} bindings ship with this Perry build:", entries.len());
            println!();
            for b in &entries {
                let tracking = b.tracking.as_deref().unwrap_or("-");
                println!("  {:<28}  → {:<32}  ({})", b.package, b.krate, tracking);
            }
            println!();
            println!("Resolution order (see docs/src/native-libraries/manifest-v1.md):");
            println!("  1. node_modules/<name>/ with perry.nativeLibrary  → use it");
            println!("  2. node_modules/<name>/ without manifest          → V8 fallback");
            println!("  3. well-known table (above)                       → bundled crate");
            println!("  4. nothing matches                                → resolution error");
        }
    }

    Ok(())
}
