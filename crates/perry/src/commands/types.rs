//! Types command - generate TypeScript type stubs for Perry built-in modules
//!
//! Writes `.d.ts` declarations into `.perry/types/perry/` so that tsc, tsgo,
//! and IDEs can resolve `import { ... } from "perry/ui"` etc. via the
//! `paths` mapping in `tsconfig.json`.

use anyhow::Result;
use clap::Args;
use std::fs;
use std::path::Path;

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct TypesArgs {
    /// Project directory (default: current)
    #[arg(default_value = ".")]
    pub path: std::path::PathBuf,
}

// Canonical `.d.ts` sources, embedded at compile time from `types/perry/`.
const PERRY_UI_DTS: &str = include_str!("../../../../types/perry/ui/index.d.ts");
const PERRY_THREAD_DTS: &str = include_str!("../../../../types/perry/thread/index.d.ts");
const PERRY_I18N_DTS: &str = include_str!("../../../../types/perry/i18n/index.d.ts");
const PERRY_SYSTEM_DTS: &str = include_str!("../../../../types/perry/system/index.d.ts");
const PERRY_MEDIA_DTS: &str = include_str!("../../../../types/perry/media/index.d.ts");

/// Write Perry type stubs into `<project>/.perry/types/perry/`.
/// Always overwrites — these are generated files.
pub fn write_perry_type_stubs(project_path: &Path, quiet: bool) -> Result<()> {
    let base = project_path.join(".perry").join("types").join("perry");

    let modules: &[(&str, &str)] = &[
        ("ui", PERRY_UI_DTS),
        ("thread", PERRY_THREAD_DTS),
        ("i18n", PERRY_I18N_DTS),
        ("system", PERRY_SYSTEM_DTS),
        ("media", PERRY_MEDIA_DTS),
    ];

    // Each sub-module gets index.d.ts
    for (name, dts) in modules {
        let dir = base.join(name);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("index.d.ts"), dts)?;
    }

    if !quiet {
        println!("  Created .perry/types/ type stubs (ui, thread, i18n, system, media)");
    }

    Ok(())
}

pub fn run(args: TypesArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    let project_path = args.path.canonicalize().unwrap_or(args.path.clone());

    match format {
        OutputFormat::Text => {
            println!("Writing Perry type stubs...\n");
        }
        OutputFormat::Json => {}
    }

    write_perry_type_stubs(&project_path, false)?;

    match format {
        OutputFormat::Text => {
            println!("\nDone! IDEs and tsc can now resolve perry/* imports.");
        }
        OutputFormat::Json => {
            let result = serde_json::json!({
                "success": true,
                "path": project_path.to_string_lossy(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok(())
}
