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
    let iteration_set = well_known_iteration_set(ctx);
    // PERRY_DISABLE_WELL_KNOWN=1 keeps only the wrappers that have no
    // perry-stdlib copy to revert to (`net`, `ws`).
    let mut well_known_libs =
        resolve_prebuilt_ext_libs(&retain_routed(iteration_set.clone()), target, format, verbose);
    // #10458: native addons need every runtime-bearing archive rebuilt
    // together with the host feature.
    if !ctx.native_addons.is_empty() {
        return resolve_native_addon_libs(
            ctx,
            &iteration_set,
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
    let (mut runtime, stdlib) = if ctx.needs_wasm_runtime {
        match build_optional_runtime(ctx, target, format, verbose) {
            Some((runtime, stdlib)) => (Some(runtime), stdlib),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    // #10466 — the prebuilt `libperry_stdlib.a` is built with the default
    // `full` feature set, which deliberately excludes
    // `external-http-client-pump` (adding it to `full` would force every
    // no-auto program, HTTP client or not, to link `libperry_ext_http.a` —
    // see the Cargo.toml comment on `full`, #5983/#8587). Without that
    // feature, perry-stdlib's dynamic-dispatch fallbacks for the
    // `node:http`/`node:https` CLIENT surface (`res.pipe()`, `req.setHeader()`,
    // `req.setTimeout()`, …) don't exist in the linked archive at all — they
    // read `undefined` with no compile-time warning. When the program
    // imports `http`/`https`, rebuild perry-stdlib-static with that feature
    // added on top of `full`, the same on-demand-rebuild shape
    // `build_optional_runtime` uses for `wasm-host` — AND, in the SAME cargo
    // invocation, `perry-ext-http` itself: two archives built in separate
    // cargo invocations can bundle different tokio compilations even from an
    // identical Cargo.lock (`runtime_compat.rs`'s link-time guard exists
    // exactly for this), so a stdlib-only rebuild would leave the fresh
    // stdlib archive unlinkable against whatever `libperry_ext_http.a`
    // `resolve_prebuilt_ext_libs` found on disk. A prior wasm
    // rebuild above already producing a stdlib archive (Windows) takes
    // precedence; this only fills the common case where `stdlib` is `None`.
    let stdlib = stdlib.or_else(|| {
        let imports_http_client = iteration_set.iter().any(|m| {
            matches!(
                m.strip_prefix("node:").unwrap_or(m.as_str()),
                "http" | "https"
            )
        });
        if !imports_http_client {
            return None;
        }
        // Follow-up to #11225 / #11240: EVERY archive that bundles perry-ffi
        // or perry-runtime must come out of this one invocation, not just
        // the stdlib and ext-http. A prebuilt `libperry_ext_net.a` beside a
        // rebuilt stdlib carried its own perry-ffi (a different feature
        // unification, so a different crate hash) — two handle registries
        // minting the same ids, and ext-net itself linked twice — and the
        // prebuilt `libperry_runtime.a` beside a stdlib whose bundled runtime
        // came from this graph split the allocator (`mi_free` SIGSEGV on the
        // first regex match, #11240).
        let ext_crates = linked_ext_crates(&iteration_set, target);
        let built = build_http_client_pump_stdlib(&ext_crates, target, format, verbose)?;
        // Replace every wrapper `resolve_prebuilt_ext_libs` found with the one
        // just built in the same invocation.
        replace_rebuilt_wrappers(&mut well_known_libs, built.ext_libs);
        if runtime.is_none() {
            runtime = Some(built.runtime);
        }
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
    iteration_set: &std::collections::BTreeSet<String>,
    mut well_known_libs: Vec<PathBuf>,
    workspace_root: Option<PathBuf>,
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> OptimizedLibs {
    let http_pump = iteration_set.iter().any(|m| {
        matches!(
            m.strip_prefix("node:").unwrap_or(m.as_str()),
            "http" | "https"
        )
    });
    let mut features = vec!["perry-runtime/node-api-host"];
    if ctx.needs_wasm_runtime {
        features.push("perry-runtime/wasm-host");
    }
    let ext_crates = linked_ext_crates(iteration_set, target);
    let built = workspace_root.and_then(|root| {
        build_coherent_stdlib(
            root,
            &ext_crates,
            target,
            format,
            verbose,
            http_pump,
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
/// program links, deduplicated by archive (http / https / http2 share one).
pub(super) fn linked_ext_crates(
    iteration_set: &std::collections::BTreeSet<String>,
    target: Option<&str>,
) -> Vec<(String, String)> {
    let mut seen = std::collections::BTreeSet::new();
    let mut crates = Vec::new();
    for module in &retain_routed(iteration_set.clone()) {
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

/// #10466 — on-demand rebuild of `perry-stdlib-static` (default `full`
/// features plus `external-http-client-pump`) into a dedicated target dir,
/// so the no-auto path's client-side `node:http`/`node:https` dynamic
/// dispatch (`res.pipe()`/`req.setHeader()`/`req.setTimeout()`/…) has
/// somewhere to link against without forcing every other no-auto program to
/// carry `libperry_ext_http.a`. `perry-ext-http` is rebuilt **in the same
/// cargo invocation** — two archives from separate invocations can bundle
/// different tokio compilations even off an identical `Cargo.lock`
/// (`runtime_compat.rs`'s link-time guard exists exactly for this pair), so
/// a stdlib-only rebuild would leave the fresh stdlib unlinkable against
/// whatever `libperry_ext_http.a` `resolve_prebuilt_ext_libs` found on disk.
/// Mirrors `build_optional_runtime`'s `wasm-host` rebuild; returns `None` on
/// any failure (no source on disk, no cargo, build error) so the caller
/// falls back to the prebuilt full stdlib (same #10466 gap, not a new
/// failure mode). Returns `(stdlib_archive, ext_http_archive)`.
pub(super) fn build_http_client_pump_stdlib(
    ext_crates: &[(String, String)],
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
) -> Option<CoherentLibraryBuild> {
    build_coherent_stdlib(
        find_perry_workspace_root()?,
        ext_crates,
        target,
        format,
        verbose,
        true,
        &[],
    )
}

/// Build the runtime, stdlib and every linked wrapper from one Cargo graph.
/// Optional runtime features must reach the runtime bundled into ALL archives.
fn build_coherent_stdlib(
    workspace_root: PathBuf,
    ext_crates: &[(String, String)],
    target: Option<&str>,
    format: OutputFormat,
    verbose: u8,
    http_pump: bool,
    runtime_features: &[&str],
) -> Option<CoherentLibraryBuild> {
    let workspace_root = cargo_target_dir_path(workspace_root);
    let stdlib_crate_dir = workspace_root.join("crates").join("perry-stdlib-static");
    let ext_http_crate_dir = workspace_root.join("crates").join("perry-ext-http");
    if !stdlib_crate_dir.is_dir() || (http_pump && !ext_http_crate_dir.is_dir()) {
        if matches!(format, OutputFormat::Text) && verbose > 0 {
            eprintln!(
                "  no-auto libraries: skipping rebuild — crate source not found at {} or {}",
                stdlib_crate_dir.display(),
                ext_http_crate_dir.display()
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
        "perry-no-auto-http-pump"
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
    let mut features = runtime_features.to_vec();
    if http_pump {
        cargo_cmd.arg("-p").arg("perry-ext-http");
        features.push("perry-stdlib/external-http-client-pump");
    }
    if !features.is_empty() {
        cargo_cmd.arg("--features").arg(features.join(","));
    }
    for (krate, _) in ext_crates {
        if (!http_pump || krate != "perry-ext-http")
            && workspace_root.join("crates").join(krate).is_dir()
        {
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

    let (runtime_name, stdlib_name, ext_http_name) = if is_windows_target(target) {
        (
            "perry_runtime.lib",
            "perry_stdlib.lib",
            "perry_ext_http.lib",
        )
    } else {
        (
            "libperry_runtime.a",
            "libperry_stdlib.a",
            "libperry_ext_http.a",
        )
    };
    let mut release_dir = pump_target_dir;
    if let Some(triple) = rust_target_triple(target) {
        release_dir = release_dir.join(triple);
    }
    let release_dir = release_dir.join("release");
    let runtime = release_dir.join(runtime_name);
    let stdlib = release_dir.join(stdlib_name);
    let mut ext_libs = if http_pump {
        vec![release_dir.join(ext_http_name)]
    } else {
        Vec::new()
    };
    for (krate, filename) in ext_crates {
        if (!http_pump || krate != "perry-ext-http")
            && workspace_root.join("crates").join(krate).is_dir()
        {
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
                if binding_bundles_tokio(module.strip_prefix("node:").unwrap_or(module)) {
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
