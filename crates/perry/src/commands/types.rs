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
const PERRY_GC_DTS: &str = include_str!("../../../../types/perry/gc/index.d.ts");
const PERRY_I18N_DTS: &str = include_str!("../../../../types/perry/i18n/index.d.ts");
const PERRY_SYSTEM_DTS: &str = include_str!("../../../../types/perry/system/index.d.ts");
const PERRY_MEDIA_DTS: &str = include_str!("../../../../types/perry/media/index.d.ts");
const PERRY_AUDIO_DTS: &str = include_str!("../../../../types/perry/audio/index.d.ts");
const PERRY_TUI_DTS: &str = include_str!("../../../../types/perry/tui/index.d.ts");
const PERRY_WEBASSEMBLY_DTS: &str = include_str!("../../../../types/perry/webassembly/index.d.ts");
const PERRY_BUILD_DTS: &str = include_str!("../../../../types/perry/build/index.d.ts");

// Auto-generated stdlib `.d.ts` from the API manifest (#465's
// "stretch" deliverable: editor `.d.ts` shipped alongside the
// generated docs). The file at `docs/api/perry.d.ts` is rebuilt
// by `scripts/regen_api_docs.sh` and CI's `api-docs-drift` job
// fails if it's out of sync, so embedding it here is safe — the
// shipped binary always carries a current snapshot.
const PERRY_STDLIB_DTS: &str = include_str!("../../../../docs/api/perry.d.ts");

/// Write Perry type stubs into `<project>/.perry/types/perry/`.
/// Always overwrites — these are generated files.
pub fn write_perry_type_stubs(project_path: &Path, quiet: bool) -> Result<()> {
    let base = project_path.join(".perry").join("types").join("perry");

    let modules: &[(&str, &str)] = &[
        ("ui", PERRY_UI_DTS),
        ("thread", PERRY_THREAD_DTS),
        ("gc", PERRY_GC_DTS),
        ("i18n", PERRY_I18N_DTS),
        ("system", PERRY_SYSTEM_DTS),
        ("media", PERRY_MEDIA_DTS),
        ("audio", PERRY_AUDIO_DTS),
        ("tui", PERRY_TUI_DTS),
        // Issue #76 — WebAssembly host runtime ambient declarations.
        // The file declares `WebAssembly` on the global scope, so it
        // doesn't need to be imported. Lives under `perry/webassembly`
        // alongside the other built-in modules for discoverability.
        ("webassembly", PERRY_WEBASSEMBLY_DTS),
        // Issue #76 — `perry/build` compile-time intrinsics
        // (`embedWasm`). Imported via `import { embedWasm } from "perry/build"`.
        ("build", PERRY_BUILD_DTS),
    ];

    // Each sub-module gets index.d.ts
    for (name, dts) in modules {
        let dir = base.join(name);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("index.d.ts"), dts)?;
    }

    // The stdlib `.d.ts` covers the Node-compat surface (crypto,
    // fs, http, dotenv, mysql2, …) — every module the user imports
    // by bare name. One file with `declare module "<name>"` blocks;
    // tsc / VSCode picks them all up via the tsconfig `include`
    // emitted by `perry init` (`.perry/types/**/*.d.ts`).
    let stdlib_dir = project_path.join(".perry").join("types").join("stdlib");
    fs::create_dir_all(&stdlib_dir)?;
    fs::write(stdlib_dir.join("index.d.ts"), PERRY_STDLIB_DTS)?;

    if !quiet {
        println!(
            "  Created .perry/types/ type stubs (ui, thread, i18n, system, media, audio, tui, webassembly, build, stdlib)"
        );
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
