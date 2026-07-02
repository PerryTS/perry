use super::*;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::commands::stdlib_features::{compute_required_features, features_to_cargo_arg};
use crate::OutputFormat;

use super::super::library_search::{find_harmonyos_sdk, harmonyos_cross_env};
use super::super::{find_perry_workspace_root, rust_target_triple, CompilationContext};

/// Resolve well-known wrapper archives without rebuilding runtime/stdlib.
///
/// Used when automatic runtime/stdlib specialization is disabled. The
/// no-auto path still needs wrapper archives for FFI symbols that are not
/// defined by the full prebuilt stdlib, such as the `perry-ext-http` server
/// entry points recorded by the codegen FFI registry. Prefer already-built
/// archives, but when the Perry workspace source is available, build a missing
/// wrapper once in the caller's cargo target dir so fresh dev checkouts still
/// link no-auto parity cases correctly.
pub(crate) fn resolve_no_auto_optimized_libs(
    ctx: &CompilationContext,
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> OptimizedLibs {
    if matches!(format, OutputFormat::Text) && verbose > 0 {
        eprintln!("  auto-optimize: skipped; using prebuilt target/release/libperry_*.a");
    }
    let well_known_libs = if std::env::var_os("PERRY_DISABLE_WELL_KNOWN").is_none() {
        resolve_prebuilt_ext_libs(&well_known_iteration_set(ctx), target, format, verbose)
    } else {
        Vec::new()
    };
    OptimizedLibs {
        prefer_well_known_before_stdlib: !well_known_libs.is_empty(),
        well_known_libs,
        ..OptimizedLibs::empty()
    }
}

/// #2532 / #3954 — resolve the `perry-ext-*` staticlibs a program needs
/// while runtime/stdlib auto-specialization is disabled.
///
/// The in-tree path strips the matching perry-stdlib feature and rebuilds
/// stdlib so the ext lib and stdlib don't both define the same `_js_*`
/// symbols. Out-of-tree we can't rebuild — the link uses the prebuilt full
/// `libperry_stdlib.a`, so the no-auto/fallback linker path places wrappers
/// before stdlib. That lets wrapper factories and their duplicate client-side
/// follow-up symbols come from the same archive while still letting the full
/// stdlib satisfy unrelated bundled modules.
///
/// Each well-known lib is first located through `find_library`, which honours
/// the `PERRY_LIB_DIR` / `PERRY_RUNTIME_DIR` overrides and the exe-dir /
/// Homebrew `../lib` probes. If that fails in an in-tree dev checkout, build
/// the missing wrapper crate once and link the resulting archive.
pub(crate) fn resolve_prebuilt_ext_libs(
    iteration_set: &std::collections::BTreeSet<String>,
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    // Dedup by lib basename — http / https / http2 all map to
    // `perry_ext_http`, so without this the same `.a` would be added
    // (and warned about) three times.
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for module in iteration_set {
        let Some(binding) = super::super::well_known::lookup_well_known(module) else {
            continue;
        };
        if !seen.insert(binding.lib.clone()) {
            continue;
        }
        let filename = super::super::well_known::ext_staticlib_filename(
            &binding.lib,
            rust_target_triple(target),
        );
        match super::super::library_search::find_library(&filename, target) {
            Some(path) => {
                if matches!(format, OutputFormat::Text) {
                    println!(
                        "  well-known (no-auto): routing `{}` → {} ({})",
                        module,
                        path.display(),
                        binding.tracking.as_deref().unwrap_or("no tracking issue")
                    );
                }
                libs.push(path);
            }
            None => {
                if let Some(workspace_root) = find_perry_workspace_root() {
                    if let Some(path) = build_missing_prebuilt_ext_lib(
                        &workspace_root,
                        binding,
                        &filename,
                        target,
                        format,
                        verbose,
                    ) {
                        libs.push(path);
                        continue;
                    }
                }
                if matches!(format, OutputFormat::Text) && verbose > 0 {
                    eprintln!(
                        "  well-known (no-auto): `{}` not found for `{}` — install \
                         Perry's bundled ext libs next to the perry binary, set \
                         PERRY_LIB_DIR, or build `{}`; the link will fail with \
                         unresolved `js_*` symbols.",
                        filename, module, binding.krate
                    );
                }
            }
        }
    }
    libs
}

fn cargo_target_dir_for_workspace(workspace_root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(raw) if !raw.is_empty() => {
            let path = PathBuf::from(raw);
            if path.is_absolute() {
                path
            } else {
                workspace_root.join(path)
            }
        }
        _ => workspace_root.join("target"),
    }
}

fn built_staticlib_path(workspace_root: &Path, filename: &str, target: Option<&str>) -> PathBuf {
    let mut release_dir = cargo_target_dir_for_workspace(workspace_root);
    if let Some(triple) = rust_target_triple(target) {
        release_dir = release_dir.join(triple);
    }
    release_dir.join("release").join(filename)
}

pub(crate) fn build_missing_prebuilt_ext_lib(
    workspace_root: &Path,
    binding: &super::super::well_known::WellKnownBinding,
    filename: &str,
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> Option<PathBuf> {
    let crate_dir = workspace_root.join("crates").join(&binding.krate);
    if !crate_dir.is_dir() {
        if matches!(format, OutputFormat::Text) && verbose > 0 {
            eprintln!(
                "  well-known (no-auto): skipping `{}` — crate source not found at {}",
                binding.krate,
                crate_dir.display()
            );
        }
        return None;
    }

    if matches!(format, OutputFormat::Text) {
        println!(
            "  well-known (no-auto): building missing `{}` from `{}`",
            filename, binding.krate
        );
    }

    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd
        .current_dir(workspace_root)
        .arg("build")
        .arg("--release")
        .arg("-p")
        .arg(&binding.krate);
    if let Some(triple) = rust_target_triple(target) {
        cargo_cmd.arg("--target").arg(triple);
    }

    let status = match cargo_cmd.status() {
        Ok(status) => status,
        Err(err) => {
            if matches!(format, OutputFormat::Text) && verbose > 0 {
                eprintln!(
                    "  well-known (no-auto): failed to spawn cargo for `{}` ({})",
                    binding.krate, err
                );
            }
            return None;
        }
    };
    if !status.success() {
        if matches!(format, OutputFormat::Text) && verbose > 0 {
            eprintln!(
                "  well-known (no-auto): cargo build for `{}` failed ({})",
                binding.krate, status
            );
        }
        return None;
    }

    let path = built_staticlib_path(workspace_root, filename, target);
    if path.exists() {
        if matches!(format, OutputFormat::Text) {
            println!(
                "  well-known (no-auto): routing `{}` → {}",
                binding.package,
                path.display()
            );
        }
        return Some(path);
    }

    if matches!(format, OutputFormat::Text) && verbose > 0 {
        eprintln!(
            "  well-known (no-auto): cargo finished but `{}` was not produced at {}",
            filename,
            path.display()
        );
    }
    None
}
