//! Post-link helpers: `strip`, attestation sidecar, binary-size
//! print, intermediate-object cleanup.
//!
//! Extracted from `compile.rs` for issue #1105 PR 3 (directory
//! split). Pure file move — no behavior change. Each helper holds a
//! verbatim copy of an inline block from `run_with_parse_cache`.

use crate::OutputFormat;
use std::fs;
use std::path::{Path, PathBuf};

use super::{CompilationContext, ObjectCache};

/// Strip debug symbols from the final binary (reduces size
/// significantly). Skipped for: dylib output, every cross-
/// compilation target whose host `strip` can't parse foreign object
/// formats (iOS/visionOS/tvOS/watchOS/HarmonyOS/Android), and when
/// `PERRY_DEBUG_SYMBOLS=1` is set so crash backtraces stay
/// symbolicated. `--debug-symbols` (#1663) promotes itself to that env
/// var in the compile driver, so passing the flag also takes this skip
/// path on Linux/macOS.
///
/// When `ctx.needs_plugins` is true the build uses `strip -x` to
/// retain exported symbols — `dlopen`'d plugins resolve
/// `hone_host_api_*` from the main executable's symbol table.
#[allow(clippy::too_many_arguments)]
pub(super) fn strip_final_binary(
    ctx: &CompilationContext,
    exe_path: &Path,
    target: Option<&str>,
    is_dylib: bool,
    is_ios: bool,
    is_visionos: bool,
    is_tvos: bool,
    is_watchos: bool,
    is_harmonyos: bool,
) {
    if is_dylib
        || is_ios
        || is_visionos
        || is_tvos
        || is_watchos
        || is_harmonyos
        || target == Some("android")
        // Wear OS ships the same dlopen'd .so — stripping would drop the
        // no_mangle JNI/FFI symbols PerryActivity resolves at load.
        || target == Some("wearos")
        || std::env::var("PERRY_DEBUG_SYMBOLS").is_ok()
    {
        return;
    }
    if ctx.needs_plugins {
        let _ = std::process::Command::new("strip")
            .arg("-x")
            .arg(exe_path)
            .status();
    } else {
        let _ = std::process::Command::new("strip").arg(exe_path).status();
    }
}

/// #504: emit `<binary>.attest.json` AFTER strip/codesign so the
/// captured SHA-256 matches what users will actually download.
/// Best-effort — errors log and continue.
pub(super) fn emit_attestation_sidecar(
    ctx: &CompilationContext,
    exe_path: &Path,
    format: OutputFormat,
) {
    if !ctx.emit_attest {
        return;
    }
    match crate::commands::attest::build_attestation(exe_path, &ctx.project_root) {
        Ok(manifest) => match crate::commands::attest::write_attestation(exe_path, &manifest) {
            Ok(sidecar) => {
                if let OutputFormat::Text = format {
                    println!("Wrote attestation: {}", sidecar.display())
                }
            }
            Err(e) => {
                if let OutputFormat::Text = format {
                    eprintln!("warning: failed to write attestation: {}", e)
                }
            }
        },
        Err(e) => {
            if let OutputFormat::Text = format {
                eprintln!("warning: failed to build attestation: {}", e)
            }
        }
    }
}

pub(super) fn print_binary_size(format: OutputFormat, exe_path: &Path) {
    if let OutputFormat::Text = format {
        if let Ok(meta) = fs::metadata(exe_path) {
            let size_mb = meta.len() as f64 / 1_048_576.0;
            println!("Binary size: {:.1}MB", size_mb);
        }
    }
}

/// Remove intermediate `.o` files unless `--keep-intermediates` was
/// passed.
pub(super) fn cleanup_intermediates(keep_intermediates: bool, obj_paths: &[PathBuf]) {
    if keep_intermediates {
        return;
    }
    for obj_path in obj_paths {
        let _ = fs::remove_file(obj_path);
    }
}

/// Bundle the object cache's hit/miss/store counters for the
/// `CompileResult` return value. `None` when the cache was disabled
/// (`--no-cache`, `PERRY_NO_CACHE=1`, or bitcode-link mode).
pub(super) fn summarize_codegen_cache_stats(
    object_cache: &ObjectCache,
) -> Option<(usize, usize, usize, usize)> {
    if !object_cache.is_enabled() {
        return None;
    }
    Some((
        object_cache.hits(),
        object_cache.misses(),
        object_cache.stores(),
        object_cache.store_errors(),
    ))
}
