//! Auto-rebuild perry-runtime + perry-stdlib with the smallest matching
//! Cargo feature set so the compiled `.o` only links the runtime APIs
//! the user's TS code actually uses.
//!
//! Tier 2.1 follow-up (v0.5.341) — extracts `OptimizedLibs` + the
//! `build_optimized_libs` driver from `compile.rs`. ~390 LOC of
//! self-contained library-build orchestration. Both `runtime` and
//! `stdlib` halves fall back to the prebuilt libraries gracefully on
//! any failure (no source on disk, no cargo, build error). Cargo's
//! incremental cache is keyed per (target dir, feature set), and we
//! use a hash-keyed target dir so consecutive runs with the same
//! profile are no-ops after the first build.

use std::path::PathBuf;
use std::process::Command;

use crate::commands::stdlib_features::{compute_required_features, features_to_cargo_arg};
use crate::OutputFormat;

use super::library_search::{find_harmonyos_sdk, harmonyos_cross_env};
use super::{find_perry_workspace_root, rust_target_triple, CompilationContext};

pub struct OptimizedLibs {
    /// Path to the rebuilt `libperry_runtime.a` (or `perry_runtime.lib`).
    /// `None` means "fall back to the prebuilt one in target/release/".
    pub runtime: Option<PathBuf>,
    /// Path to the rebuilt `libperry_stdlib.a`. `None` means "fall back
    /// to the prebuilt full stdlib".
    pub stdlib: Option<PathBuf>,
    /// LLVM bitcode (`.bc`) for perry-runtime (Phase J).
    pub runtime_bc: Option<PathBuf>,
    /// LLVM bitcode (`.bc`) for perry-stdlib (Phase J).
    pub stdlib_bc: Option<PathBuf>,
    /// LLVM bitcode (`.bc`) for additional crates (UI, jsruntime, geisterhand).
    pub extra_bc: Vec<PathBuf>,
    /// Extra `.a` archives to add to the link line — one per
    /// well-known native binding (#466 Phase 4) that the compile
    /// pipeline routed away from the perry-stdlib copy. Whenever an
    /// entry is added here, the corresponding perry-stdlib feature
    /// is *also* stripped from the rebuild so the link line stays
    /// free of duplicate `_js_*` symbols.
    pub well_known_libs: Vec<PathBuf>,
}

impl OptimizedLibs {
    pub(super) fn empty() -> Self {
        OptimizedLibs {
            runtime: None,
            stdlib: None,
            runtime_bc: None,
            stdlib_bc: None,
            extra_bc: Vec::new(),
            well_known_libs: Vec::new(),
        }
    }
}

/// Rebuild perry-runtime + perry-stdlib in a single cargo invocation with
/// the chosen Cargo features and panic mode, and return paths to the
/// resulting archives. Both halves fall back to the prebuilt libraries
/// gracefully on any failure (no source on disk, no cargo, build error).
///
/// This is the auto-mode workhorse — it lets the compile driver pick the
/// smallest matching profile for the user's TS code without any manual
/// flags. Cargo's incremental cache is keyed per (target dir, feature
/// set), and we use a hash-keyed target dir so consecutive runs with the
/// same profile are no-ops after the first build.
pub(super) fn build_optimized_libs(
    ctx: &CompilationContext,
    target: Option<&str>,
    cli_features: &[String],
    format: OutputFormat,
    verbose: u8,
) -> OptimizedLibs {
    // `PERRY_NO_AUTO_OPTIMIZE=1` — opt out of the per-app feature-set
    // specialization and use the prebuilt `target/release/libperry_*.a`
    // built with the default `full` feature set. Used by CI doc-tests
    // (`scripts/run_doc_tests.sh`) where the workspace is pre-built
    // once and 80+ tests would otherwise re-trigger a multi-minute
    // cargo rebuild per test (each test's distinct import set hashes
    // to a different `target/perry-auto-<hash>` cache dir). Trades
    // binary size for ~80% wall-time reduction on doc-tests.
    //
    // Returning `OptimizedLibs::empty()` makes the link path fall
    // through to `find_runtime_library` / `find_stdlib_library`,
    // which probe `target/release/` and `target/<target-triple>/release/`
    // in that order. The workflow's prebuild step is responsible for
    // making sure those paths exist.
    if std::env::var_os("PERRY_NO_AUTO_OPTIMIZE").is_some() {
        if matches!(format, OutputFormat::Text) && verbose > 0 {
            eprintln!(
                "  auto-optimize: skipped (PERRY_NO_AUTO_OPTIMIZE=1); \
                 using prebuilt target/release/libperry_*.a"
            );
        }
        return OptimizedLibs::empty();
    }
    // (compute_required_features + features_to_cargo_arg imported at module top)
    let mut features = compute_required_features(
        &ctx.native_module_imports,
        ctx.uses_fetch,
        ctx.uses_crypto_builtins,
    );

    // #466 Phase 4 step 2: well-known bindings flip. For each
    // imported module that has an entry in `well_known_bindings.toml`
    // *and* whose bundled `.a` is on disk, drop the corresponding
    // perry-stdlib feature so the rebuild stops emitting that
    // module's symbols, then queue the bundled `.a` to be added to
    // the link line. Net result: the program links against the
    // external wrapper instead of the perry-stdlib copy, with no
    // duplicate-symbol risk.
    //
    // **Default-on as of v0.5.573** — Phase 5 dogfood completed in
    // v0.5.572 (34 perry-ext-* wrappers covering every previously
    // in-tree binding). The env-var gate (`PERRY_USE_WELL_KNOWN=1`)
    // that gated the introductory cycle is now inverted:
    // `PERRY_DISABLE_WELL_KNOWN=1` reverts to perry-stdlib's
    // copies for bisection. If a bundled `.a` is missing on disk,
    // each entry falls back to the perry-stdlib copy individually
    // (logged with `well-known: skipping` when verbose), so a
    // partially-built workspace still produces a working binary.
    let use_well_known = std::env::var_os("PERRY_DISABLE_WELL_KNOWN").is_none();
    let mut well_known_libs: Vec<PathBuf> = Vec::new();
    // #507 — wrappers whose own crate-level `[dependencies]` pull tokio
    // (TcpStream, hyper, reqwest, mongodb, sqlx, tokio-tungstenite,
    // lettre, …) need to share a single tokio compilation with
    // perry-stdlib's runtime. If they're built in a different
    // target-dir than perry-stdlib (the workspace `target/release/`
    // vs. the auto-optimize `target/perry-auto-<hash>/release/`), the
    // mangled hash on `tokio::runtime::context::CONTEXT` differs
    // between the two staticlibs — both end up in the final binary as
    // distinct TLS variables. perry-stdlib's runtime sets one;
    // `Handle::current()` from inside the wrapper reads the other
    // (empty) one and panics with "there is no reactor running".
    //
    // Fix is to rebuild these crates IN the auto-optimize cargo
    // invocation (`-p <crate>`), which forces a single tokio
    // compilation. Both staticlibs then reference the same mangled
    // CONTEXT symbol; the linker dedups; one TLS variable in the
    // final binary; `Handle::current()` works.
    //
    // CPU-only wrappers (bcrypt, argon2, sharp, …) don't need this —
    // they only use perry-ffi's `spawn_blocking` shim, which routes
    // through perry-stdlib's tokio. Their workspace-built .a stays
    // fine.
    let mut tokio_using_bindings: Vec<(String, String, Option<String>)> = Vec::new();
    // Closes #589: hono + node:http combinations dropped js_headers_new /
    // js_response_new / js_request_new at link time. The well-known flip
    // strips perry-stdlib's `http-client` feature when `node:http` is
    // imported and routes to perry-ext-http — but perry-ext-http only
    // exports the HTTP-client surface (`js_http_*` / `js_node_http_*`),
    // not the Web Fetch ctors that hono's compiled output references.
    // perry-ext-fetch is the staticlib that ships those symbols.
    //
    // When the user's TS code (or any compilePackages-resolved module like
    // hono) constructs `new Headers(...)` / `new Request(...)` / `new Response(...)`,
    // the HIR sets `ctx.uses_fetch = true` (see
    // `crates/perry-hir/src/destructuring.rs::1469-1492` + the explicit
    // `fetch(...)` arms in `lower/expr_call.rs`). If that flag is set but
    // the user didn't *also* import `'fetch'` / `'node-fetch'` (so the
    // well-known table won't pull perry-ext-fetch in on its own), we
    // synthetically add `"fetch"` here so the iteration below routes
    // perry-ext-fetch into the link line. The `'fetch'` binding strips
    // no perry-stdlib feature (see stdlib_features.rs — fetch falls
    // through to `_ => &[]`), so this is a pure-add.
    let mut iteration_set: std::collections::BTreeSet<String> =
        ctx.native_module_imports.iter().cloned().collect();
    if ctx.uses_fetch && !iteration_set.contains("fetch") && !iteration_set.contains("node-fetch") {
        iteration_set.insert("fetch".to_string());
    }
    if use_well_known {
        for module in &iteration_set {
            let Some(binding) = super::well_known::lookup_well_known(module) else {
                continue;
            };
            // Workspace root is required for both the prebuilt-path
            // probe AND for the rebuild-in-auto-optimize path.
            let workspace_root_opt = find_perry_workspace_root();
            let Some(workspace_root) = workspace_root_opt.as_ref() else {
                continue;
            };
            let needs_shared_tokio = binding_needs_shared_tokio(module);
            // For CPU-only wrappers we can use the workspace-built
            // copy directly. Skip the binding entirely if no .a
            // exists on disk (partial build / release tarball
            // missing the wrapper).
            if !needs_shared_tokio {
                let Some(lib_path) = super::well_known::bundled_staticlib_path_for_target(
                    workspace_root,
                    binding,
                    rust_target_triple(target),
                ) else {
                    if matches!(format, OutputFormat::Text) && verbose > 0 {
                        eprintln!(
                            "  well-known: skipping `{}` — bundled `lib{}.a` not found \
                             in target/release; falling back to perry-stdlib copy.",
                            module, binding.lib
                        );
                    }
                    continue;
                };
                if matches!(format, OutputFormat::Text) {
                    println!(
                        "  well-known: routing `{}` → {} ({})",
                        module,
                        lib_path.display(),
                        binding.tracking.as_deref().unwrap_or("no tracking issue")
                    );
                }
                well_known_libs.push(lib_path);
            } else {
                // Tokio-using: defer path resolution until after the
                // auto-optimize cargo build. Verify the source crate
                // exists on disk first (so we can actually build it).
                let crate_dir = workspace_root.join("crates").join(&binding.krate);
                if !crate_dir.is_dir() {
                    if matches!(format, OutputFormat::Text) && verbose > 0 {
                        eprintln!(
                            "  well-known: skipping `{}` — crate `{}` source not on disk; \
                             falling back to perry-stdlib copy.",
                            module, binding.krate
                        );
                    }
                    continue;
                }
                if matches!(format, OutputFormat::Text) {
                    println!(
                        "  well-known: routing `{}` → rebuilding `{}` with shared tokio (#507) ({})",
                        module,
                        binding.krate,
                        binding.tracking.as_deref().unwrap_or("no tracking issue")
                    );
                }
                tokio_using_bindings.push((
                    binding.krate.clone(),
                    binding.lib.clone(),
                    binding.tracking.clone(),
                ));
            }
            // Strip the perry-stdlib feature(s) this binding was
            // covering. `module_to_features` is the same table
            // `compute_required_features` consulted above, so we
            // know exactly what to remove.
            for feat in crate::commands::stdlib_features::module_to_features(module) {
                // Fix #589: `node:http` / `node:https` / `node:http2`
                // map to `http-client`, but that feature also covers
                // the Web Fetch FFIs (`js_headers_new`,
                // `js_response_new`, `js_request_new`). When a
                // compilePackages package — typically hono — uses
                // `new Headers()` / `new Response()` while the user
                // also imports `node:http`, stripping `http-client`
                // breaks the link with undefined `js_headers_new` /
                // `js_response_new` symbols. perry-ext-http only
                // bundles the server side (perry-ext-http-server). So
                // keep `http-client` if `uses_fetch` is set —
                // perry-stdlib's fetch.rs stays in the build to
                // satisfy the Web Fetch references; the well-known
                // staticlib is still added for the server side.
                if *feat == "http-client" && ctx.uses_fetch {
                    continue;
                }
                // Refs #643: keep `database-sqlite` enabled even when
                // `better-sqlite3` routes to perry-ext-better-sqlite3.
                // perry-stdlib's `dispatch_sqlite_stmt` (the dynamic
                // receiver path used by drizzle's
                // `this.stmt.raw().all(...)` chain) is gated on this
                // feature; stripping it removes the dispatch arm
                // entirely and the `.raw()` / `.all()` call falls
                // through to the no-such-method sentinel. The
                // duplicate `js_sqlite_*` symbols (one from each
                // crate) are resolved by the linker picking one impl;
                // perry-ext typically wins because it appears later on
                // the link line. The dispatch arm calls those symbols
                // via extern "C", so it routes through whichever impl
                // the linker picked.
                if *feat == "database-sqlite" {
                    continue;
                }
                features.remove(*feat);
            }
            // perry-ffi's async surface (#466 Phase 1.1 / Phase 5
            // step 5+) is gated behind perry-stdlib's
            // `async-runtime` feature — the `perry_ffi_*` shim
            // module that wrappers like bcrypt / argon2 / ws / db
            // pull through linking lives in
            // `crates/perry-stdlib/src/perry_ffi_async.rs` and
            // can only be compiled when tokio is in the build.
            // Stripping `bundled-bcrypt` (etc.) without
            // re-asserting `async-runtime` would leave the
            // wrapper's `.a` carrying unresolved `perry_ffi_*`
            // references. Detect async wrappers by checking
            // whether the original feature list contained an
            // async feature; if it did, ensure it stays.
            let original_features = crate::commands::stdlib_features::module_to_features(module);
            if original_features.iter().any(|f| {
                matches!(
                    *f,
                    "bundled-bcrypt"
                        | "bundled-argon2"
                        | "bundled-nodemailer"
                        | "bundled-ioredis"
                        | "bundled-pg"
                        | "bundled-mysql2"
                        | "bundled-mongodb"
                        | "bundled-ws"
                        | "bundled-net"
                        | "http-client"
                        | "bundled-streams"
                        | "bundled-fastify"
                )
            }) {
                features.insert("async-runtime");
            }
            // v0.5.579 — when the flip strips `bundled-net`, activate
            // `external-net-pump` so perry-stdlib's
            // `js_stdlib_process_pending` knows to call into
            // perry-ext-net's queue. Without this the call site is
            // `#[cfg]`-gated off and tokio events stay queued forever.
            if original_features.contains(&"bundled-net") {
                features.insert("external-net-pump");
            }
            // Closes #606 — same shape for ws. When the well-known flip
            // strips `bundled-ws` and routes to perry-ext-ws, activate
            // `external-ws-pump` so perry-stdlib's main-thread pump and
            // active-handles gate know to call into perry-ext-ws's
            // queue. Without this, perry-ext-ws's accept loop pushes
            // events that nobody drains, and the program exits or hangs
            // before any handler fires.
            if original_features.contains(&"bundled-ws") {
                features.insert("external-ws-pump");
            }
            // Closes #604 — when the well-known flip routes `node:http` /
            // `node:https` / `node:http2` to perry-ext-http (which bundles
            // perry-ext-http-server), activate `external-http-server-pump`
            // so perry-stdlib's main-thread pump and active-handles gate
            // call into perry-ext-http-server's queue each tick. Without
            // this, the http server's accept-loop tokio task pushes
            // requests that nobody drains, and the program hangs (pre-#604
            // listen() blocked the main thread; post-#604 listen() is
            // non-blocking but needs the pump to fire).
            //
            // Gate strictly on the MODULE name (not on `http-client`
            // feature, which axios / node-fetch also map to) — those
            // bring perry-ext-axios / perry-ext-fetch which don't define
            // `js_node_http_server_*` symbols. Activating the pump for
            // them would drop unresolved externs at link time.
            let module_normalized = module.strip_prefix("node:").unwrap_or(module);
            if matches!(module_normalized, "http" | "https" | "http2") {
                features.insert("external-http-server-pump");
            }
        }
    }

    // The UI backends (perry-ui-gtk4 on Linux, perry-ui-macos, perry-ui-windows)
    // reach into perry-stdlib's async bridge from GLib/NSTimer/WM_TIMER
    // trampolines (js_stdlib_process_pending, js_promise_run_microtasks).
    // Those symbols live in perry-stdlib/src/common/async_bridge.rs which is
    // gated on `#[cfg(feature = "async-runtime")]`. For a bare UI program
    // whose user code imports zero stdlib modules, compute_required_features
    // returns an empty set and the auto-optimized stdlib is built with
    // --no-default-features — no `async-runtime`, no async_bridge module, no
    // symbol. Force `async-runtime` whenever the program pulls in a UI
    // backend so the trampolines resolve at link time.
    if ctx.needs_ui {
        features.insert("async-runtime");
    }
    // perry-stdlib unconditionally re-bundles perry-updater (so user code
    // calling `perry/updater` resolves at link time without extra wiring).
    // perry-updater's `perry_updater_verify_signature_v2` references the
    // extern `js_crypto_ed25519_verify`, which lives in perry-stdlib's
    // crypto module — gated by `#[cfg(feature = "crypto")]`. With
    // --no-default-features the symbol is absent and the link fails on
    // every program (regardless of whether the user touched crypto APIs).
    // Force `crypto` on whenever the auto-optimize path rebuilds stdlib
    // so the bundled updater always has a resolvable target.
    features.insert("crypto");
    let feature_arg = features_to_cargo_arg(&features);

    // panic = "abort" is safe whenever no `catch_unwind` callers are
    // reachable. Today those live in:
    //   - perry-runtime/src/thread.rs (perry/thread `spawn`)
    //   - perry-ui-{macos,ios}/* (UI callback isolation)
    //   - perry-runtime plugin host (`needs_plugins` → -rdynamic +
    //     -force_load paths that may rely on unwind tables for plugin
    //     dylibs)
    //   - geisterhand registry callbacks
    // Whenever the user binary doesn't pull any of those in, switching
    // to `abort` saves ~12-18 % off the final binary by dropping
    // __TEXT,__eh_frame, __TEXT,__gcc_except_tab, __TEXT,__unwind_info
    // and the matching landing pads / Drop glue.
    let panic_abort_safe =
        !ctx.needs_ui && !ctx.needs_thread && !ctx.needs_plugins && !ctx.needs_geisterhand;

    // Locate the workspace. Without source we can't rebuild — fall back
    // to whatever's prebuilt next to perry on disk. The fallback names are
    // platform-specific so the log doesn't claim Perry is searching for a
    // `.a` on Windows (it isn't — `find_runtime_library` / `find_stdlib_library`
    // route to `perry_runtime.lib` + `perry_stdlib.lib` on Windows hosts).
    let workspace_root = match find_perry_workspace_root() {
        Some(p) => p,
        None => {
            if matches!(format, OutputFormat::Text) && verbose > 0 {
                let (rt_name, std_name) = match target {
                    Some("windows") => ("perry_runtime.lib", "perry_stdlib.lib"),
                    None if cfg!(target_os = "windows") => {
                        ("perry_runtime.lib", "perry_stdlib.lib")
                    }
                    _ => ("libperry_runtime.a", "libperry_stdlib.a"),
                };
                eprintln!(
                    "  auto-optimize: Perry workspace source not found, \
                     using prebuilt {} + {}",
                    rt_name, std_name
                );
            }
            return OptimizedLibs::empty();
        }
    };

    // Hash the (features, panic_mode, target) tuple into the target dir
    // name so cargo treats each combination as its own incremental cache.
    // Cheap djb2 — no need for the SipHash overhead.
    let target_str = target.unwrap_or("host");
    let key_input = format!("{}|{}|{}", feature_arg, panic_abort_safe, target_str);
    let mut hash: u64 = 5381;
    for b in key_input.as_bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(*b as u64);
    }
    let target_dir = workspace_root.join(format!("target/perry-auto-{:016x}", hash));

    if matches!(format, OutputFormat::Text) {
        let panic_str = if panic_abort_safe { "abort" } else { "unwind" };
        let feat_str = if features.is_empty() {
            "(no optional features)".to_string()
        } else {
            feature_arg.clone()
        };
        println!(
            "  auto-optimize: rebuilding runtime+stdlib (panic={}, features={})",
            panic_str, feat_str
        );
    }

    // Tier-3 Apple targets (tvOS, watchOS) aren't shipped with a prebuilt
    // libstd; cargo needs `+nightly -Zbuild-std` to synthesize core/alloc/std
    // from source for the cross-compile.
    let is_tier3 = matches!(
        target,
        Some("tvos") | Some("tvos-simulator") | Some("watchos") | Some("watchos-simulator")
    );

    let mut cargo_cmd = Command::new("cargo");
    if is_tier3 {
        cargo_cmd.arg("+nightly");
    }
    cargo_cmd
        .current_dir(&workspace_root)
        .env("CARGO_TARGET_DIR", &target_dir)
        .arg("build")
        .arg("--release")
        .arg("-p")
        .arg("perry-runtime")
        .arg("-p")
        .arg("perry-stdlib")
        .arg("--no-default-features");
    // #507 — rebuild tokio-using ext crates in the same cargo
    // invocation as perry-stdlib so cargo unifies tokio across them.
    // Without this, each crate's tokio.rlib lives in a different
    // target-dir with a different mangled hash, and perry-ext-*'s
    // `Handle::current()` reads a different CONTEXT TLS variable
    // than the one perry-stdlib's runtime entered.
    for (krate, _lib, _tracking) in &tokio_using_bindings {
        cargo_cmd.arg("-p").arg(krate);
    }
    if is_tier3 {
        cargo_cmd.arg("-Zbuild-std=std,panic_abort");
    }
    // Both perry-runtime and perry-stdlib accept their own feature lists.
    // Cargo's `--features` takes `crate/feature` syntax for cross-crate
    // selection — we always enable perry-stdlib's stdlib-side bridge so
    // perry-runtime exports the right symbols, and the user-derived
    // stdlib features.
    let mut cross_features: Vec<String> = vec![
        // perry-runtime's "full" feature gates plugin + os.hostname/homedir.
        // Auto-mode keeps it on so existing behavior is preserved; the
        // panic mode is what shrinks the binary.
        "perry-runtime/full".to_string(),
    ];
    for f in &features {
        cross_features.push(format!("perry-stdlib/{}", f));
    }
    // CLI `--features` values that target the runtime (game-loop entry-point
    // shims gated behind `ios-game-loop` / `watchos-game-loop` in
    // `perry-runtime/Cargo.toml`) need `perry-runtime/<f>` passed through, not
    // `perry-stdlib/<f>` — they gate a Rust module, not an npm dep surface.
    for f in cli_features {
        if f == "ios-game-loop" || f == "watchos-game-loop" || f == "ohos-napi" {
            cross_features.push(format!("perry-runtime/{}", f));
        }
    }
    if !cross_features.is_empty() {
        cargo_cmd.arg("--features").arg(cross_features.join(","));
    }
    if let Some(triple) = rust_target_triple(target) {
        cargo_cmd.arg("--target").arg(triple);
    }
    // HarmonyOS cross-compile needs the OHOS SDK's clang on PATH for C
    // dependencies (notably libmimalloc-sys) — without --sysroot the build
    // fails in build.rs with "'pthread.h' file not found".
    if matches!(target, Some("harmonyos") | Some("harmonyos-simulator")) {
        match find_harmonyos_sdk() {
            Some(sdk) => {
                for (k, v) in harmonyos_cross_env(&sdk, target) {
                    cargo_cmd.env(k, v);
                }
            }
            None => {
                if matches!(format, OutputFormat::Text) {
                    eprintln!(
                        "  auto-optimize: OHOS SDK not found — set OHOS_SDK_HOME to the DevEco Studio \
                         SDK root (the dir containing native/llvm/bin/clang). Skipping auto-optimize."
                    );
                }
                return OptimizedLibs::empty();
            }
        }
    }
    if panic_abort_safe {
        // Override the workspace profile's `panic = "unwind"` for the
        // duration of this invocation. RUSTFLAGS is the only path that
        // works without a custom cargo profile, and cargo correctly
        // reuses incremental artifacts that were built with the same
        // RUSTFLAGS. The hash-keyed CARGO_TARGET_DIR keeps the abort
        // and unwind builds from clobbering each other's cache.
        cargo_cmd.env("RUSTFLAGS", "-C panic=abort");
    }

    // Closes #25 (the v0.5.384 NJOBS 6→3 retreat): serialize parallel
    // `perry compile` invocations that target the SAME `target/perry-auto
    // -<hash>` directory via an OS-level file lock. Cargo has its own
    // target-dir lock (`.cargo-lock`) that prevents concurrent COMPILES,
    // but the FILE OUTPUT is rename'd at link end — meaning worker B's
    // clang can read `libperry_runtime.a` while worker A's cargo is
    // mid-rename and see errno=2. The race window is sub-second but
    // fired reliably at NJOBS=6 on the macos-14 compile-smoke runner.
    //
    // The lock is per-hash, so different feature combos still build in
    // parallel. fslock is portable (flock on Unix, LockFileEx on
    // Windows) and was already a transitive dep — no new crate cost.
    //
    // Best-effort: if the dir create or lock acquisition fails for any
    // reason, fall through and run cargo unguarded. The retry loop in
    // the smoke script's compile_one already handles the residual race
    // window if any worker still slips through.
    let _build_lock = {
        let _ = std::fs::create_dir_all(&target_dir);
        let lock_path = target_dir.join(".perry-auto-build.lock");
        match fslock::LockFile::open(&lock_path) {
            Ok(mut lf) => {
                let _ = lf.lock();
                Some(lf)
            }
            Err(_) => None,
        }
    };

    let status = match cargo_cmd.status() {
        Ok(s) => s,
        Err(e) => {
            if matches!(format, OutputFormat::Text) {
                eprintln!(
                    "  auto-optimize: failed to spawn cargo ({}), \
                     using prebuilt libraries",
                    e
                );
            }
            return OptimizedLibs::empty();
        }
    };
    if !status.success() {
        if matches!(format, OutputFormat::Text) {
            eprintln!(
                "  auto-optimize: cargo build failed (exit {}), \
                 using prebuilt libraries",
                status
            );
        }
        return OptimizedLibs::empty();
    }

    // Resolve both archive paths.
    let runtime_name = match target {
        Some("windows") => "perry_runtime.lib",
        #[cfg(target_os = "windows")]
        None => "perry_runtime.lib",
        _ => "libperry_runtime.a",
    };
    let stdlib_name = match target {
        Some("windows") => "perry_stdlib.lib",
        #[cfg(target_os = "windows")]
        None => "perry_stdlib.lib",
        _ => "libperry_stdlib.a",
    };
    let release_dir = if let Some(triple) = rust_target_triple(target) {
        target_dir.join(triple).join("release")
    } else {
        target_dir.join("release")
    };
    let runtime_path = release_dir.join(runtime_name);
    let stdlib_path = release_dir.join(stdlib_name);

    if matches!(format, OutputFormat::Text) {
        if let Ok(meta) = std::fs::metadata(&runtime_path) {
            println!(
                "  auto-optimize: built {} ({:.1} MB)",
                runtime_path.display(),
                meta.len() as f64 / (1024.0 * 1024.0)
            );
        }
        if let Ok(meta) = std::fs::metadata(&stdlib_path) {
            println!(
                "  auto-optimize: built {} ({:.1} MB)",
                stdlib_path.display(),
                meta.len() as f64 / (1024.0 * 1024.0)
            );
        }
    }

    // #507 — resolve the `.a` paths for each tokio-using ext crate
    // we rebuilt above. They live next to perry-stdlib.a in the
    // auto-optimize target-dir, with the SAME tokio compilation
    // bundled in. The linker will dedup duplicate tokio symbols
    // across the staticlibs because the mangled hashes match.
    for (krate, lib, _tracking) in &tokio_using_bindings {
        let lib_path = release_dir.join(format!("lib{}.a", lib));
        if !lib_path.exists() {
            // Fall back to the workspace target copy. The linker will
            // still produce a working binary for this wrapper if the
            // user code path doesn't actually exercise the tokio
            // CONTEXT — useful as a safety net rather than hard-failing.
            // Prefer the target-specific dir when cross-compiling so we
            // don't link host-platform Mach-O into a Linux ELF.
            let fallback = if let Some(triple) = rust_target_triple(target) {
                let triple_path = workspace_root
                    .join("target")
                    .join(triple)
                    .join("release")
                    .join(format!("lib{}.a", lib));
                if triple_path.exists() {
                    triple_path
                } else {
                    workspace_root
                        .join("target")
                        .join("release")
                        .join(format!("lib{}.a", lib))
                }
            } else {
                workspace_root
                    .join("target")
                    .join("release")
                    .join(format!("lib{}.a", lib))
            };
            if fallback.exists() {
                if matches!(format, OutputFormat::Text) {
                    eprintln!(
                        "  well-known: rebuild produced no `lib{}.a` in {} — \
                         using workspace fallback (CONTEXT panic risk on tokio I/O)",
                        lib,
                        release_dir.display()
                    );
                }
                well_known_libs.push(fallback);
            } else if matches!(format, OutputFormat::Text) {
                eprintln!(
                    "  well-known: rebuild produced no `lib{}.a` for `{}`; \
                     skipping — link will likely fail with unresolved js_* symbols.",
                    lib, krate
                );
            }
            continue;
        }
        if matches!(format, OutputFormat::Text) {
            if let Ok(meta) = std::fs::metadata(&lib_path) {
                println!(
                    "  auto-optimize: built {} ({:.1} MB)",
                    lib_path.display(),
                    meta.len() as f64 / (1024.0 * 1024.0)
                );
            }
        }
        well_known_libs.push(lib_path);
    }

    // Phase J: when PERRY_LLVM_BITCODE_LINK=1, also emit LLVM bitcode
    // (.bc) for whole-program LTO via `cargo rustc --emit=llvm-bc,link`.
    let bitcode_requested = std::env::var("PERRY_LLVM_BITCODE_LINK").ok().as_deref() == Some("1");
    let (runtime_bc, stdlib_bc, extra_bc) = if bitcode_requested {
        if matches!(format, OutputFormat::Text) {
            println!("  auto-optimize: emitting LLVM bitcode for whole-program LTO");
        }

        let mut bc_rustflags = String::new();
        if panic_abort_safe {
            bc_rustflags.push_str("-C panic=abort ");
        }
        bc_rustflags.push_str("-C codegen-units=1");

        let emit_bc = |crate_name: &str| -> Option<PathBuf> {
            let mut cmd = Command::new("cargo");
            cmd.current_dir(&workspace_root)
                .env("CARGO_TARGET_DIR", &target_dir)
                .env("RUSTFLAGS", &bc_rustflags)
                .arg("rustc")
                .arg("--release")
                .arg("-p")
                .arg(crate_name)
                .arg("--no-default-features");
            if !cross_features.is_empty() {
                cmd.arg("--features").arg(cross_features.join(","));
            }
            if let Some(triple) = rust_target_triple(target) {
                cmd.arg("--target").arg(triple);
            }
            cmd.arg("--").arg("--emit=llvm-bc,link");

            match cmd.status() {
                Ok(s) if s.success() => {}
                Ok(s) => {
                    if matches!(format, OutputFormat::Text) {
                        eprintln!(
                            "  auto-optimize: cargo rustc --emit=llvm-bc for {} failed (exit {})",
                            crate_name, s
                        );
                    }
                    return None;
                }
                Err(e) => {
                    if matches!(format, OutputFormat::Text) {
                        eprintln!(
                            "  auto-optimize: failed to spawn cargo rustc for {} ({})",
                            crate_name, e
                        );
                    }
                    return None;
                }
            }

            // Glob for the .bc file in deps/
            let deps_dir = release_dir.join("deps");
            let crate_underscore = crate_name.replace('-', "_");
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&deps_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with(&format!("{}-", crate_underscore))
                        && name_str.ends_with(".bc")
                        && !name_str.contains(".rcgu")
                    {
                        candidates.push(entry.path());
                    }
                }
            }
            candidates.sort_by(|a, b| {
                let ma = a.metadata().and_then(|m| m.modified()).ok();
                let mb = b.metadata().and_then(|m| m.modified()).ok();
                mb.cmp(&ma)
            });
            if let Some(bc_path) = candidates.first() {
                if matches!(format, OutputFormat::Text) {
                    if let Ok(meta) = std::fs::metadata(bc_path) {
                        println!(
                            "  auto-optimize: bitcode {} ({:.1} MB)",
                            bc_path.display(),
                            meta.len() as f64 / (1024.0 * 1024.0)
                        );
                    }
                }
                Some(bc_path.clone())
            } else {
                if matches!(format, OutputFormat::Text) {
                    eprintln!(
                        "  auto-optimize: no .bc file found for {} in {}",
                        crate_name,
                        deps_dir.display()
                    );
                }
                None
            }
        };

        let rt_bc = emit_bc("perry-runtime");
        let sl_bc = emit_bc("perry-stdlib");

        // Emit .bc for additional crates (UI, jsruntime, geisterhand).
        // HarmonyOS has no `perry-ui-harmonyos` crate by design — UI is
        // emitted as ArkUI source via the codegen-arkts harvest, and
        // any `perry_ui_*` / `perry_system_*` / `perry_updater_*` symbols
        // that survive into the .so resolve via the no-op stubs auto-
        // generated by `perry-runtime/build.rs` (#395 + #399). The
        // harmonyos branch in compile.rs unconditionally clears
        // `needs_ui` for that target so we never reach this match arm
        // with `Some("harmonyos*")`.
        let mut extra = Vec::new();
        if ctx.needs_ui {
            let ui_crate = match target {
                Some("ios-simulator")
                | Some("ios")
                | Some("ios-widget")
                | Some("ios-widget-simulator") => "perry-ui-ios",
                Some("visionos-simulator") | Some("visionos") => "perry-ui-visionos",
                Some("android") => "perry-ui-android",
                Some("watchos-simulator") | Some("watchos") => "perry-ui-watchos",
                Some("tvos-simulator") | Some("tvos") => "perry-ui-tvos",
                Some("linux") => "perry-ui-gtk4",
                Some("windows") => "perry-ui-windows",
                Some("macos") => "perry-ui-macos",
                _ => {
                    if cfg!(target_os = "linux") {
                        "perry-ui-gtk4"
                    } else {
                        "perry-ui-macos"
                    }
                }
            };
            if let Some(bc) = emit_bc(ui_crate) {
                extra.push(bc);
            }
        }
        if ctx.needs_geisterhand {
            if let Some(bc) = emit_bc("perry-ui-geisterhand") {
                extra.push(bc);
            }
        }
        if ctx.needs_js_runtime {
            if let Some(bc) = emit_bc("perry-jsruntime") {
                extra.push(bc);
            }
        }

        (rt_bc, sl_bc, extra)
    } else {
        (None, None, Vec::new())
    };

    OptimizedLibs {
        runtime: if runtime_path.exists() {
            Some(runtime_path)
        } else {
            None
        },
        stdlib: if stdlib_path.exists() {
            Some(stdlib_path)
        } else {
            None
        },
        runtime_bc,
        stdlib_bc,
        extra_bc,
        well_known_libs,
    }
}

/// True if this binding's wrapper crate has its own tokio dependency
/// for I/O (TcpStream, hyper, reqwest, mongodb, sqlx, redis,
/// tokio-tungstenite, lettre, …) and must therefore share a single
/// tokio compilation with perry-stdlib's runtime.
///
/// Closes #507 — when these wrappers are built in a different
/// target-dir than perry-stdlib, each gets its own private copy of
/// tokio's `CONTEXT` thread-local. perry-stdlib's runtime sets one;
/// the wrapper's `Handle::current()` reads the other (empty) one
/// and panics with "there is no reactor running".
///
/// Wrappers that only use perry-ffi's `spawn_blocking` shim (bcrypt,
/// argon2, sharp, …) route their async work through perry-stdlib's
/// tokio and don't need this — their own crate has no tokio dep.
fn binding_needs_shared_tokio(module: &str) -> bool {
    matches!(
        module,
        // Raw TCP / TLS sockets
        "net"
        // WebSocket client/server
        | "ws"
        // HTTP / HTTPS via reqwest/hyper
        | "http"
        | "https"
        // HTTP clients (reqwest, hyper)
        | "axios"
        | "node-fetch"
        | "fetch"
        // HTTP server (hyper)
        | "fastify"
        // Database drivers (mongodb, sqlx, redis)
        | "mongodb"
        | "pg"
        | "mysql2"
        | "mysql2/promise"
        | "ioredis"
        | "redis"
        // Mail (lettre)
        | "nodemailer"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Closes #507. The well-known flip's "shared tokio" allowlist
    /// must match the set of perry-ext-* crates whose own
    /// `Cargo.toml` pulls tokio. If a new wrapper is added that uses
    /// tokio for I/O without being added here, programs importing it
    /// will panic with "there is no reactor running" the first time
    /// the wrapper calls `Handle::current()` on a tokio worker.
    #[test]
    fn net_needs_shared_tokio() {
        assert!(binding_needs_shared_tokio("net"));
    }

    #[test]
    fn cpu_only_wrappers_do_not_need_shared_tokio() {
        // bcrypt / argon2 / sharp / dotenv all route through
        // perry-stdlib's `spawn_blocking` shim; their own crate has
        // no tokio dep, so there's no CONTEXT collision risk.
        assert!(!binding_needs_shared_tokio("bcrypt"));
        assert!(!binding_needs_shared_tokio("argon2"));
        assert!(!binding_needs_shared_tokio("sharp"));
        assert!(!binding_needs_shared_tokio("dotenv"));
    }

    #[test]
    fn unknown_modules_default_to_workspace_path() {
        // Defensive default: if a module isn't in the allowlist,
        // treat it as CPU-only (existing v0.5.586 behavior).
        assert!(!binding_needs_shared_tokio("definitely-not-a-real-package"));
    }
}
