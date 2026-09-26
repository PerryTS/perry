//! Post-link helpers: `strip`, attestation sidecar, binary-size
//! print, intermediate-object cleanup.
//!
//! Extracted from `compile.rs` for issue #1105 PR 3 (directory
//! split). Pure file move — no behavior change. Each helper holds a
//! verbatim copy of an inline block from `run_with_parse_cache`.

use crate::OutputFormat;
use std::fs;
use std::path::{Path, PathBuf};

use super::{is_android_target, CompilationContext, ObjectCache};

/// Strip debug symbols from the final binary (reduces size
/// significantly). Skipped for: dylib output, every cross-
/// compilation target whose host `strip` can't parse foreign object
/// formats (iOS/visionOS/tvOS/watchOS/HarmonyOS/Android), and when
/// `PERRY_DEBUG_SYMBOLS=1` is set so crash backtraces stay
/// symbolicated. `--debug-symbols` (#1663) promotes itself to that env
/// var in the compile driver, so passing the flag also takes this skip
/// path on Linux/macOS.
///
/// Plugin hosts and macOS Node-API hosts use `strip -x` to retain the exported
/// symbols their dynamically loaded libraries resolve from the executable.
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
    emit_macos_dsym(exe_path, target);
    if is_dylib
        || is_ios
        || is_visionos
        || is_tvos
        || is_watchos
        || is_harmonyos
        // Android and Wear OS ship a dlopen'd .so — stripping would drop the
        // no_mangle JNI/FFI symbols PerryActivity resolves at load.
        || is_android_target(target)
        || std::env::var("PERRY_DEBUG_SYMBOLS").is_ok()
        // #7803 tooling: keep the symbol table WITHOUT asking for DWARF.
        //
        // `PERRY_DEBUG_SYMBOLS` does both — every consumer reads it with
        // `is_some()`, so there is no value that skips the strip and leaves
        // `-g` off. That coupling is a problem for an intermittent bug: the
        // symbolized build of the #7803 corpus passed 7 seeds that the plain
        // build fails at 44%, so asking for symbols changed the subject. This
        // knob skips ONLY the strip, leaving codegen byte-identical to the
        // build that reproduces, which is what makes the backtrace it yields
        // evidence about the same program.
        || std::env::var("PERRY_KEEP_SYMBOLS").is_ok()
    {
        return;
    }
    // Apple strip removes executable exports unless asked to preserve them.
    // ELF strip already retains dynamic exports; keeping all ELF globals
    // would add an unrelated static symbol table to the host-size budget.
    if ctx.needs_plugins || (cfg!(target_os = "macos") && !ctx.native_addons.is_empty()) {
        let _ = std::process::Command::new("strip")
            .arg("-x")
            .arg(exe_path)
            .status();
    } else {
        let _ = std::process::Command::new("strip").arg(exe_path).status();
    }
}

/// #9856: under `--debug-symbols` on a macOS host building a macOS binary,
/// gather the DWARF into `<exe>.dSYM`.
///
/// Apple's linker does not copy DWARF into the executable; it records a debug
/// map pointing at the input `.o` files, and Perry deletes those right after
/// linking (`cleanup_intermediates`). Without a dSYM, lldb — and so Xcode and
/// VS Code's CodeLLDB — finds no line table and cannot bind a breakpoint.
/// Runs before the strip decision and the cleanup, while the objects exist.
fn emit_macos_dsym(exe_path: &Path, target: Option<&str>) {
    if !cfg!(target_os = "macos") || std::env::var_os("PERRY_DEBUG_SYMBOLS").is_none() {
        return;
    }
    if target.is_some_and(|t| !(t.contains("macos") || t.contains("darwin"))) {
        return;
    }
    match std::process::Command::new("dsymutil")
        .arg(exe_path)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("warning: dsymutil exited with {status}; no .dSYM emitted"),
        Err(e) => eprintln!("warning: could not run dsymutil ({e}); no .dSYM emitted"),
    }
}

/// Emit the optional profile before an app bundle is signed.
pub(super) fn emit_sandbox_sidecar(
    ctx: &CompilationContext,
    exe_path: &Path,
    format: OutputFormat,
) {
    // #506 — emit `<binary>.sandbox` next to the binary when
    // `--emit-sandbox` (or the equivalent env / package.json
    // knob) is set. macOS only for the MVP; other platforms
    // log a once-per-build note that the kernel-sandbox MVP
    // is macOS-only and the matching seccomp / AppContainer /
    // ... support lands as #506 follow-up.
    if ctx.emit_sandbox {
        #[cfg(target_os = "macos")]
        {
            match super::super::sandbox_profile::emit_macos_sandbox_profile(ctx, exe_path) {
                Ok(path) => match format {
                    OutputFormat::Text => {
                        println!("Wrote sandbox profile: {}", path.display())
                    }
                    OutputFormat::Json => {}
                },
                Err(e) => match format {
                    OutputFormat::Text => {
                        eprintln!("warning: failed to emit sandbox profile: {}", e);
                    }
                    OutputFormat::Json => {}
                },
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            // The profile is written next to the binary only on macOS.
            let _ = exe_path;
            if let OutputFormat::Text = format {
                eprintln!(
                    "note: `--emit-sandbox` is macOS-only in this MVP; Linux seccomp + Windows AppContainer support tracked under #506."
                );
            }
        }
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
