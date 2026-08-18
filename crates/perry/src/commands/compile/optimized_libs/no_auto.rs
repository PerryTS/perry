use super::*;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::OutputFormat;

use super::super::library_search::{android_cross_env, find_harmonyos_sdk, harmonyos_cross_env};
use super::super::{
    find_perry_workspace_root, is_android_target, is_windows_target, rust_target_triple,
    CompilationContext,
};

/// Resolve well-known wrapper archives without rebuilding runtime/stdlib.
///
/// Used when automatic runtime/stdlib specialization is disabled. The
/// no-auto path still needs wrapper archives for FFI symbols that are not
/// defined by the full prebuilt stdlib, such as the `perry-ext-http` server
/// entry points recorded by the codegen FFI registry. Prefer already-built
/// archives, but when the Perry workspace source is available, build a missing
/// wrapper once in the caller's cargo target dir so fresh dev checkouts still
/// link no-auto parity cases correctly.
///
/// When the program references `WebAssembly.*` (or `--enable-wasm-runtime` was
/// passed, which folds into `ctx.needs_wasm_runtime`), the prebuilt
/// `libperry_runtime.a` is insufficient: `wasm-host` is deliberately kept out
/// of perry-runtime's `default` feature set so non-wasm programs don't pay for
/// wasmi. The no-auto path can't enable a cargo feature on an already-built
/// archive, so it does a targeted rebuild of just `perry-runtime-static` with
/// default features + `perry-runtime/wasm-host` into a dedicated target dir.
/// This is the same on-demand build pattern `build_missing_prebuilt_ext_lib`
/// uses for CPU-only ext wrappers. `perry-wasm-host` has no tokio dep, so
/// there is no #507 shared-tokio concern with the prebuilt stdlib.
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
    // Issue #76 — the prebuilt runtime is built WITHOUT `wasm-host` (kept
    // out of `default` to avoid wasmi bloat on non-wasm programs). When the
    // program uses `WebAssembly.*`, rebuild just the runtime with the
    // feature on so `js_webassembly_*` symbols are defined. The prebuilt
    // stdlib is unaffected (wasm-host only adds a module to perry-runtime).
    let runtime = if ctx.needs_wasm_runtime {
        build_wasm_host_runtime(target, format, verbose)
    } else {
        None
    };
    OptimizedLibs {
        runtime,
        prefer_well_known_before_stdlib: !well_known_libs.is_empty(),
        well_known_libs,
        ..OptimizedLibs::empty()
    }
}

/// Build `perry-runtime-static` with default features + `perry-runtime/wasm-host`
/// into a dedicated target dir so the prebuilt `libperry_runtime.a` is not
/// clobbered. Returns the path to the rebuilt archive, or `None` when there's
/// no workspace source or the build fails (the caller falls back to the
/// prebuilt runtime, which will fail to link with `_js_webassembly_*`
/// undefined — the error message points the user at the cause).
fn build_wasm_host_runtime(
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> Option<PathBuf> {
    let workspace_root = find_perry_workspace_root()?;
    let crate_dir = workspace_root.join("crates").join("perry-runtime-static");
    if !crate_dir.is_dir() {
        if matches!(format, OutputFormat::Text) && verbose > 0 {
            eprintln!(
                "  wasm-host (no-auto): skipping runtime rebuild — crate source not found at {}",
                crate_dir.display()
            );
        }
        return None;
    }

    if matches!(format, OutputFormat::Text) {
        println!("  wasm-host (no-auto): rebuilding perry-runtime-static with wasm-host feature");
    }

    // Use a dedicated target dir so the prebuilt libperry_runtime.a in
    // target/release is not overwritten. Cargo's incremental cache makes
    // repeat builds a no-op.
    let wasm_host_target_dir = workspace_root
        .join("target")
        .join("perry-wasm-host-runtime");

    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd
        .current_dir(&workspace_root)
        .env("CARGO_TARGET_DIR", &wasm_host_target_dir)
        .arg("build")
        .arg("--release")
        .arg("-p")
        .arg("perry-runtime-static")
        .arg("--features")
        .arg("perry-runtime/wasm-host");
    if let Some(triple) = rust_target_triple(target) {
        cargo_cmd.arg("--target").arg(triple);
    }
    // Cross-compile envs — mirror `build_missing_prebuilt_ext_lib` so a
    // `--target harmonyos` / Android rebuild of the runtime (which has C
    // deps via libmimalloc-sys) can succeed.
    if matches!(target, Some("harmonyos") | Some("harmonyos-simulator")) {
        match find_harmonyos_sdk() {
            Some(sdk) => {
                for (k, v) in harmonyos_cross_env(&sdk, target) {
                    cargo_cmd.env(k, v);
                }
            }
            None => {
                if matches!(format, OutputFormat::Text) && verbose > 0 {
                    eprintln!(
                        "  wasm-host (no-auto): skipping runtime rebuild — OHOS SDK not found (set OHOS_SDK_HOME)"
                    );
                }
                return None;
            }
        }
    }
    if is_android_target(target) {
        if let Some(ndk) = std::env::var_os("ANDROID_NDK_HOME") {
            for (k, v) in android_cross_env(std::path::Path::new(&ndk), target) {
                cargo_cmd.env(k, v);
            }
        }
    }

    match cargo_cmd.status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            if matches!(format, OutputFormat::Text) {
                eprintln!(
                    "  wasm-host (no-auto): cargo build for perry-runtime-static failed ({status})"
                );
            }
            return None;
        }
        Err(err) => {
            if matches!(format, OutputFormat::Text) {
                eprintln!("  wasm-host (no-auto): failed to spawn cargo ({err})");
            }
            return None;
        }
    }

    let lib_name = if is_windows_target(target) {
        "perry_runtime.lib"
    } else {
        "libperry_runtime.a"
    };
    let mut release_dir = wasm_host_target_dir;
    if let Some(triple) = rust_target_triple(target) {
        release_dir = release_dir.join(triple);
    }
    let built = release_dir.join("release").join(lib_name);
    if built.exists() {
        return Some(built);
    }

    if matches!(format, OutputFormat::Text) && verbose > 0 {
        eprintln!(
            "  wasm-host (no-auto): cargo finished but {lib_name} was not produced at {}",
            built.display()
        );
    }
    None
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
                // #7629 — a tokio-using wrapper cannot be repaired from here.
                // Building it alone gives it its own tokio compilation (cargo
                // unifies features per invocation); building it *with*
                // perry-stdlib-static would fix tokio but silently overwrite
                // the prebuilt stdlib with this invocation's feature set,
                // dropping the `external-*-pump` features the no-auto flow
                // depends on — trading an abort for a hang.
                //
                // So warn, build anyway, and let the link-time check in
                // `compile/shared_tokio.rs` decide: it compares the tokio
                // compilation ids in the actual archives, which is evidence
                // rather than a prediction. Refusing here instead would also
                // fail the cases where the two invocations happen to unify to
                // the same tokio, and those link and run correctly.
                if binding_needs_shared_tokio(module.strip_prefix("node:").unwrap_or(module)) {
                    eprintln!(
                        "warning: `{}` needs {}, which is not on disk. \
                         PERRY_NO_AUTO_OPTIMIZE=1 forbids the specialized rebuild, so the \
                         wrapper can only be built in its OWN cargo invocation — and cargo \
                         resolves feature unification per invocation, so its bundled tokio \
                         is very likely a different compilation than the prebuilt \
                         libperry_stdlib.a's. Two tokio compilations means two \
                         `tokio::runtime::context::CONTEXT` thread-locals and the program \
                         aborts at its first socket with \"there is no reactor running\" \
                         (#507, #7629).\n  \
                         The link refuses that pair once the archives can be compared, so \
                         this build may fail after the wrapper finishes. To get it right \
                         the first time, build the wrapper in the SAME cargo invocation as \
                         the stdlib archive:\n    \
                         cargo build --release -p perry -p perry-runtime-static \
                         -p perry-stdlib-static -p {}\n  \
                         (plus the `--features perry-stdlib/external-*-pump` this module \
                         needs), or unset PERRY_NO_AUTO_OPTIMIZE and let auto-optimize \
                         build a coherent set itself.",
                        module, filename, binding.krate
                    );
                }
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
    // Cross-compile envs, mirroring `build_optimized_libs`: without the OHOS
    // SDK / Android NDK clang on the compile env, a `--target harmonyos` /
    // Android auto-build of a C-dependent CPU-only wrapper (e.g. sharp) fails
    // in build.rs before we can fall back. Apply the same target-specific env
    // the specialized invocation uses so the auto-build can actually succeed.
    if matches!(target, Some("harmonyos") | Some("harmonyos-simulator")) {
        match super::super::library_search::find_harmonyos_sdk() {
            Some(sdk) => {
                for (k, v) in super::super::library_search::harmonyos_cross_env(&sdk, target) {
                    cargo_cmd.env(k, v);
                }
            }
            None => {
                if matches!(format, OutputFormat::Text) && verbose > 0 {
                    eprintln!(
                        "  well-known (no-auto): skipping `{}` — OHOS SDK not found (set \
                         OHOS_SDK_HOME); falling back.",
                        binding.krate
                    );
                }
                return None;
            }
        }
    }
    if is_android_target(target) {
        if let Some(ndk) = std::env::var_os("ANDROID_NDK_HOME") {
            for (k, v) in
                super::super::library_search::android_cross_env(std::path::Path::new(&ndk), target)
            {
                cargo_cmd.env(k, v);
            }
        }
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
