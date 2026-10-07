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
/// archive, so it does a targeted rebuild with
/// `perry-runtime/wasm-host` into a dedicated target dir.
/// This is the same on-demand build pattern `build_missing_prebuilt_ext_lib`
/// uses for CPU-only ext wrappers. On Windows the rebuilt runtime and stdlib
/// must come from one Cargo graph: mixing a feature-augmented standalone
/// runtime with the prebuilt stdlib splits process-global registries such as
/// Buffer ownership across two runtime copies.
pub(crate) fn resolve_no_auto_optimized_libs(
    ctx: &CompilationContext,
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> OptimizedLibs {
    if matches!(format, OutputFormat::Text) && verbose > 0 {
        eprintln!("  auto-optimize: skipped; using prebuilt target/release/libperry_*.a");
    }
    // PERRY_DISABLE_WELL_KNOWN=1 keeps only the wrappers that have no
    // perry-stdlib copy to revert to (`net`, `ws`, `zlib`).
    let routed = routed_modules(ctx);
    let mut well_known_libs = resolve_prebuilt_ext_libs(&routed, target, format, verbose);
    // #10458: native addons need every runtime-bearing archive rebuilt
    // together with the host feature.
    if !ctx.native_addons.is_empty() {
        return resolve_native_addon_libs(
            ctx,
            &routed,
            well_known_libs,
            find_perry_workspace_root(),
            target,
            format,
            verbose,
        );
    }
    // Issue #76 — the prebuilt runtime is built WITHOUT `wasm-host` (kept
    // out of `default` to avoid wasmi bloat on non-wasm programs). When the
    // program uses `WebAssembly.*`, rebuild the runtime with the feature on so
    // `js_webassembly_*` symbols are defined. Windows also rebuilds stdlib in
    // that Cargo invocation so its bundled runtime shares the same global
    // registries as the wasm-enabled runtime.
    let stream_features = stream_dispatch_features(&routed);
    let (mut runtime, stdlib) = if ctx.needs_wasm_runtime && stream_features.is_empty() {
        match build_optional_runtime(ctx, target, format, verbose) {
            Some((runtime, stdlib)) => (Some(runtime), stdlib),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    // A wrapper's direct constructors and its dynamic methods / iteration
    // must see one provider. The prebuilt full stdlib uses bundled zlib,
    // whereas the routed constructor comes from perry-ext-zlib. Reuse the
    // coherent build for every required stream adapter, including zlib,
    // instead of restricting this decision to HTTP client imports.
    let stdlib = stdlib.or_else(|| {
        if stream_features.is_empty() {
            return None;
        }
        let ext_crates = linked_ext_crates(&routed, target);
        let runtime_features = if ctx.needs_wasm_runtime {
            vec!["perry-runtime/wasm-host"]
        } else {
            Vec::new()
        };
        let built = build_coherent_stdlib(
            find_perry_workspace_root()?,
            &ext_crates,
            target,
            format,
            verbose,
            &stream_features,
            &runtime_features,
        )?;
        replace_rebuilt_wrappers(&mut well_known_libs, built.ext_libs);
        runtime = Some(built.runtime);
        Some(built.stdlib)
    });
    OptimizedLibs {
        runtime,
        stdlib,
        prefer_well_known_before_stdlib: !well_known_libs.is_empty(),
        well_known_libs,
        ..OptimizedLibs::empty()
    }
}

/// #10458: every archive containing runtime code must share the host
/// feature. Otherwise the stdlib-first link can select its dlopen stub, and
/// separately rebuilt wrappers can retain different global registries.
///
/// `well_known_libs` (the prebuilt wrappers found on disk) and
/// `workspace_root` are inputs rather than environment reads so tests can
/// drive this against a fake workspace without mutating process-global env
/// that concurrently running tests read.
pub(super) fn resolve_native_addon_libs(
    ctx: &CompilationContext,
    routed: &std::collections::BTreeSet<String>,
    mut well_known_libs: Vec<PathBuf>,
    workspace_root: Option<PathBuf>,
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> OptimizedLibs {
    let stdlib_features = stream_dispatch_features(routed);
    let mut features = vec!["perry-runtime/node-api-host"];
    if ctx.needs_wasm_runtime {
        features.push("perry-runtime/wasm-host");
    }
    let ext_crates = linked_ext_crates(routed, target);
    let built = workspace_root.and_then(|root| {
        build_coherent_stdlib(
            root,
            &ext_crates,
            target,
            format,
            verbose,
            &stdlib_features,
            &features,
        )
    });
    let (runtime, stdlib) = if let Some(built) = built {
        replace_rebuilt_wrappers(&mut well_known_libs, built.ext_libs);
        (Some(built.runtime), Some(built.stdlib))
    } else {
        (None, None)
    };
    OptimizedLibs {
        runtime,
        stdlib,
        prefer_well_known_before_stdlib: !well_known_libs.is_empty(),
        well_known_libs,
        ..OptimizedLibs::empty()
    }
}

fn replace_rebuilt_wrappers(prebuilt: &mut Vec<PathBuf>, rebuilt: Vec<PathBuf>) {
    let names: std::collections::BTreeSet<_> = rebuilt
        .iter()
        .filter_map(|p| p.file_name().map(|name| name.to_owned()))
        .collect();
    prebuilt.retain(|p| p.file_name().is_none_or(|name| !names.contains(name)));
    prebuilt.extend(rebuilt);
}

/// Workspace crate and archive filename of every well-known wrapper the
/// program links (its [`routed_modules`]), deduplicated by archive
/// (http / https / http2 share one).
pub(super) fn linked_ext_crates(
    routed: &std::collections::BTreeSet<String>,
    target: Option<&str>,
) -> Vec<(String, String)> {
    let mut seen = std::collections::BTreeSet::new();
    let mut crates = Vec::new();
    for module in routed {
        let Some(binding) = super::super::well_known::lookup_well_known(module) else {
            continue;
        };
        if seen.insert(binding.lib.clone()) {
            crates.push((
                binding.krate.clone(),
                super::super::well_known::ext_staticlib_filename(
                    &binding.lib,
                    rust_target_triple(target),
                ),
            ));
        }
    }
    crates
}

/// Archives produced by a coherent no-auto rebuild, all from ONE cargo
/// invocation and therefore one perry-ffi and one perry-runtime.
pub(super) struct CoherentLibraryBuild {
    pub(super) runtime: PathBuf,
    pub(super) stdlib: PathBuf,
    /// Every rebuilt wrapper the program links.
    pub(super) ext_libs: Vec<PathBuf>,
}

fn stream_dispatch_features(routed: &std::collections::BTreeSet<String>) -> Vec<&'static str> {
    routed
        .iter()
        .flat_map(|module| {
            crate::commands::stdlib_features::routed_stream_dispatch_features(module)
                .iter()
                .copied()
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Build the runtime, stdlib and every linked wrapper from one Cargo graph.
/// Optional runtime features must reach the runtime bundled into ALL archives.
pub(super) fn build_coherent_stdlib(
    workspace_root: PathBuf,
    ext_crates: &[(String, String)],
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
    stdlib_features: &[&str],
    runtime_features: &[&str],
) -> Option<CoherentLibraryBuild> {
    let workspace_root = cargo_target_dir_path(workspace_root);
    let stdlib_crate_dir = workspace_root.join("crates").join("perry-stdlib-static");
    if !stdlib_crate_dir.is_dir() {
        if matches!(format, OutputFormat::Text) && verbose > 0 {
            eprintln!(
                "  no-auto libraries: skipping rebuild — crate source not found at {}",
                stdlib_crate_dir.display()
            );
        }
        return None;
    }

    if matches!(format, OutputFormat::Text) {
        println!(
            "  no-auto libraries: rebuilding runtime + stdlib + every linked wrapper together"
        );
    }

    // Dedicated target dir so the prebuilt libperry_stdlib.a in
    // target/release is not overwritten. Cargo's incremental cache makes
    // repeat builds a no-op.
    let relative_target_dir = PathBuf::from("target").join(if runtime_features.is_empty() {
        "perry-no-auto-stream-dispatch"
    } else {
        "perry-optional-runtime"
    });
    let pump_target_dir = cargo_target_dir_path(workspace_root.join(&relative_target_dir));
    let cargo_target_dir = if cfg!(windows) {
        relative_target_dir
    } else {
        pump_target_dir.clone()
    };

    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd
        .current_dir(&workspace_root)
        .env("CARGO_TARGET_DIR", &cargo_target_dir)
        .arg("build")
        .arg("--release")
        // The stdlib's dependency disables runtime defaults. Selecting the
        // runtime package explicitly preserves the full no-auto engine set
        // in the runtime code bundled into libperry_stdlib.a (#11174).
        .arg("-p")
        .arg("perry-runtime")
        // The runtime ARCHIVE comes from this graph too: the prebuilt
        // libperry_runtime.a beside a stdlib whose bundled runtime was unified
        // here split the allocator (#11240).
        .arg("-p")
        .arg("perry-runtime-static")
        .arg("-p")
        .arg("perry-stdlib-static");
    let mut features: Vec<String> = runtime_features
        .iter()
        .copied()
        .map(str::to_owned)
        .collect();
    features.extend(
        stdlib_features
            .iter()
            .map(|feature| format!("perry-stdlib/{feature}")),
    );
    if !features.is_empty() {
        cargo_cmd.arg("--features").arg(features.join(","));
    }
    for (krate, _) in ext_crates {
        if workspace_root.join("crates").join(krate).is_dir() {
            cargo_cmd.arg("-p").arg(krate);
        }
    }
    if let Some(triple) = rust_target_triple(target) {
        cargo_cmd.arg("--target").arg(triple);
    }
    if is_android_target(target) {
        if let Some(ndk) = std::env::var_os("ANDROID_NDK_HOME") {
            for (k, v) in android_cross_env(std::path::Path::new(&ndk), target) {
                cargo_cmd.env(k, v);
            }
        }
    }
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
                        "  no-auto libraries: skipping rebuild — OHOS SDK not found (set OHOS_SDK_HOME)"
                    );
                }
                return None;
            }
        }
    }

    match super::super::tool_output::run_internal_tool(&mut cargo_cmd, verbose) {
        Ok(status) if status.success() => {}
        Ok(status) => {
            if matches!(format, OutputFormat::Text) {
                eprintln!("  no-auto libraries: coherent library build failed ({status})");
            }
            return None;
        }
        Err(err) => {
            if matches!(format, OutputFormat::Text) {
                eprintln!("  no-auto libraries: failed to spawn cargo ({err})");
            }
            return None;
        }
    }

    let (runtime_name, stdlib_name) = if is_windows_target(target) {
        ("perry_runtime.lib", "perry_stdlib.lib")
    } else {
        ("libperry_runtime.a", "libperry_stdlib.a")
    };
    let mut release_dir = pump_target_dir;
    if let Some(triple) = rust_target_triple(target) {
        release_dir = release_dir.join(triple);
    }
    let release_dir = release_dir.join("release");
    let runtime = release_dir.join(runtime_name);
    let stdlib = release_dir.join(stdlib_name);
    let mut ext_libs = Vec::new();
    for (krate, filename) in ext_crates {
        if workspace_root.join("crates").join(krate).is_dir() {
            ext_libs.push(release_dir.join(filename));
        }
    }
    for path in std::iter::once(&runtime)
        .chain(std::iter::once(&stdlib))
        .chain(ext_libs.iter())
    {
        if !path.exists() {
            if matches!(format, OutputFormat::Text) && verbose > 0 {
                eprintln!(
                    "  no-auto libraries: cargo finished but {} was not produced",
                    path.display()
                );
            }
            return None;
        }
    }
    Some(CoherentLibraryBuild {
        runtime,
        stdlib,
        ext_libs,
    })
}

/// Build `perry-runtime-static` with default features + `perry-runtime/wasm-host`
/// into a dedicated target dir so the prebuilt `libperry_runtime.a` is not
/// clobbered. Windows also builds `perry-stdlib-static` in the same graph and
/// returns it as the authoritative archive. Returns `(runtime, stdlib)` or
/// `None` when there's no workspace source or the build fails.
fn build_optional_runtime(
    ctx: &CompilationContext,
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> Option<(PathBuf, Option<PathBuf>)> {
    // Canonical Windows paths can carry a `\\?\` prefix. Cargo forwards an
    // absolute verbatim `CARGO_TARGET_DIR` to cc-rs, where MSVC interprets the
    // generated `\\?\...\mimalloc-static.cc` argument as `\mimalloc-static.cc`.
    // Match the auto-optimize path: normalize the workspace and use a relative
    // target-dir env value on Windows.
    let workspace_root = cargo_target_dir_path(find_perry_workspace_root()?);
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
        println!("  wasm-host (no-auto): rebuilding runtime with wasm-host feature");
    }

    // Use a dedicated target dir so the prebuilt libperry_runtime.a in
    // target/release is not overwritten. Cargo's incremental cache makes
    // repeat builds a no-op.
    let relative_target_dir = PathBuf::from("target").join("perry-optional-runtime");
    let wasm_host_target_dir = cargo_target_dir_path(workspace_root.join(&relative_target_dir));
    let cargo_target_dir = if cfg!(windows) {
        relative_target_dir
    } else {
        wasm_host_target_dir.clone()
    };

    let mut cargo_cmd = Command::new("cargo");
    let mut runtime_features = Vec::new();
    if ctx.needs_wasm_runtime {
        runtime_features.push("perry-runtime/wasm-host");
    }
    if !ctx.native_addons.is_empty() {
        runtime_features.push("perry-runtime/node-api-host");
    }
    cargo_cmd
        .current_dir(&workspace_root)
        .env("CARGO_TARGET_DIR", &cargo_target_dir)
        .arg("build")
        .arg("--release")
        .arg("-p")
        .arg("perry-runtime-static")
        .arg("--features")
        .arg(runtime_features.join(","));
    if is_windows_target(target) {
        cargo_cmd.arg("-p").arg("perry-stdlib-static");
    }
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

    match super::super::tool_output::run_internal_tool(&mut cargo_cmd, verbose) {
        Ok(status) if status.success() => {}
        Ok(status) => {
            if matches!(format, OutputFormat::Text) {
                eprintln!(
                    "  wasm-host (no-auto): cargo build for wasm-enabled archives failed ({status})"
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
    let release_dir = release_dir.join("release");
    let runtime = release_dir.join(lib_name);
    if !runtime.exists() {
        if matches!(format, OutputFormat::Text) && verbose > 0 {
            eprintln!(
                "  wasm-host (no-auto): cargo finished but {lib_name} was not produced at {}",
                runtime.display()
            );
        }
        return None;
    }
    let stdlib = if is_windows_target(target) {
        let path = release_dir.join("perry_stdlib.lib");
        if !path.exists() {
            if matches!(format, OutputFormat::Text) && verbose > 0 {
                eprintln!(
                    "  wasm-host (no-auto): cargo finished but perry_stdlib.lib was not produced at {}",
                    path.display()
                );
            }
            return None;
        }
        Some(path)
    } else {
        None
    };
    Some((runtime, stdlib))
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
                // A missing wrapper is built on its own. Until the final
                // tokio lane a wrapper that bundled tokio got a warning here
                // (#7629): built alone it carried a different tokio
                // compilation than the prebuilt stdlib. No wrapper bundles
                // tokio any more, so that hazard — and the warning — is gone.
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

    let status = match super::super::tool_output::run_internal_tool(&mut cargo_cmd, verbose) {
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

#[cfg(all(test, unix))]
#[path = "no_auto_addon_tests.rs"]
mod addon_tests;
