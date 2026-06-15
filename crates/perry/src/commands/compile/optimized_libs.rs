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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::commands::stdlib_features::{compute_required_features, features_to_cargo_arg};
use crate::OutputFormat;

use super::library_search::{find_harmonyos_sdk, harmonyos_cross_env};
use super::{find_perry_workspace_root, rust_target_triple, CompilationContext};

/// (#1529) Android's `libperry_app.so` is loaded via `dlopen`, so its TLS
/// relocations must use the global-dynamic model — the aarch64-linux-android
/// default (Initial-Executable) crashes at load with
/// `TLS symbol "(null)" ... using IE access model`. The model is selected by a
/// `tls-model` rustc flag, but that flag is exposed as a stable `-C` codegen
/// option on some toolchains and is still nightly-gated (`-Z`) on others.
/// Passing the `-C` form to a toolchain that only knows the `-Z` form aborts
/// *every* Android build with `error: unknown codegen option: tls-model`.
/// (This slipped past CI because release CI builds the runtime libs with plain
/// `cargo build` and never compiles a full Android app through this path.)
///
/// Probe the active rustc and return the spelling it accepts. When only the
/// `-Z` form is available, also set `RUSTC_BOOTSTRAP=1` on `cmd` so the gated
/// flag is honored on a stable toolchain without requiring a nightly install.
pub(crate) fn android_global_dynamic_tls_rustflag(cmd: &mut Command) -> &'static str {
    let c_form_supported = Command::new("rustc")
        .args(["-C", "help"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("tls-model"))
        .unwrap_or(false);
    if c_form_supported {
        "-C tls-model=global-dynamic"
    } else {
        cmd.env("RUSTC_BOOTSTRAP", "1");
        "-Z tls-model=global-dynamic"
    }
}

#[cfg(windows)]
fn cargo_target_dir_path(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", rest))
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(not(windows))]
fn cargo_target_dir_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn cargo_target_dir_env_path(_target_dir: &Path, relative_target_dir: &Path) -> PathBuf {
    relative_target_dir.to_path_buf()
}

#[cfg(not(windows))]
fn cargo_target_dir_env_path(target_dir: &Path, _relative_target_dir: &Path) -> PathBuf {
    target_dir.to_path_buf()
}

fn auto_target_dir_paths(workspace_root: &Path, hash: u64) -> (PathBuf, PathBuf) {
    let workspace_root = cargo_target_dir_path(workspace_root.to_path_buf());
    let relative_target_dir = PathBuf::from("target").join(format!("perry-auto-{:016x}", hash));
    let target_dir = cargo_target_dir_path(workspace_root.join(&relative_target_dir));
    let cargo_env_dir = cargo_target_dir_env_path(&target_dir, &relative_target_dir);
    (target_dir, cargo_env_dir)
}

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
    /// LLVM bitcode (`.bc`) for additional crates (UI, geisterhand).
    pub extra_bc: Vec<PathBuf>,
    /// Extra `.a` archives to add to the link line — one per
    /// well-known native binding (#466 Phase 4) that the compile
    /// pipeline routed away from the perry-stdlib copy. Whenever an
    /// entry is added here, the corresponding perry-stdlib feature
    /// is *also* stripped from the rebuild so the link line stays
    /// free of duplicate `_js_*` symbols.
    pub well_known_libs: Vec<PathBuf>,
    /// True when the stdlib archive is the prebuilt full archive rather
    /// than an optimized rebuild with well-known features stripped. In that
    /// fallback shape, wrapper archives must appear before stdlib so their
    /// duplicate Node binding symbols satisfy the object files first.
    pub prefer_well_known_before_stdlib: bool,
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
            prefer_well_known_before_stdlib: false,
        }
    }
}

fn well_known_iteration_set(ctx: &CompilationContext) -> BTreeSet<String> {
    let mut iteration_set: BTreeSet<String> = ctx.native_module_imports.iter().cloned().collect();
    if let Ok(forced) = std::env::var("PERRY_FORCE_WELL_KNOWN") {
        for module in forced.split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace()) {
            let module = module.trim();
            if module.is_empty() {
                continue;
            }
            if super::well_known::lookup_well_known(module).is_some() {
                iteration_set.insert(module.strip_prefix("node:").unwrap_or(module).to_string());
            }
        }
    }
    iteration_set
}

/// Resolve well-known wrapper archives without rebuilding runtime/stdlib.
///
/// Used when automatic runtime/stdlib specialization is disabled. The
/// no-auto path still needs wrapper archives for FFI symbols that are not
/// defined by the full prebuilt stdlib, such as the `perry-ext-http` server
/// entry points recorded by the codegen FFI registry. Prefer already-built
/// archives, but when the Perry workspace source is available, build a missing
/// wrapper once in the caller's cargo target dir so fresh dev checkouts still
/// link no-auto parity cases correctly.
pub(super) fn resolve_no_auto_optimized_libs(
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
    let use_well_known = std::env::var_os("PERRY_DISABLE_WELL_KNOWN").is_none();
    let iteration_set = well_known_iteration_set(ctx);

    // `PERRY_NO_AUTO_OPTIMIZE=1` — opt out of the per-app feature-set
    // specialization and use the prebuilt `target/release/libperry_*.a`
    // built with the default `full` feature set. Used by CI doc-tests
    // (`scripts/run_doc_tests.sh`) where the workspace is pre-built
    // once and 80+ tests would otherwise re-trigger a multi-minute
    // cargo rebuild per test (each test's distinct import set hashes
    // to a different `target/perry-auto-<hash>` cache dir). Trades
    // binary size for ~80% wall-time reduction on doc-tests.
    //
    // The runtime/stdlib link path still falls through to
    // `find_runtime_library` / `find_stdlib_library`, which probe
    // `target/release/` and `target/<target-triple>/release/`. Keep the
    // well-known wrapper lookup active, though: native-table rows such
    // as `http.request(...)` and `http.createServer(...)` emit symbols
    // owned by `perry-ext-http`, and the full prebuilt stdlib does not
    // define those wrapper-only entry points.
    if std::env::var_os("PERRY_NO_AUTO_OPTIMIZE").is_some() {
        return resolve_no_auto_optimized_libs(ctx, target, format, verbose);
    }
    // (compute_required_features + features_to_cargo_arg imported at module top)
    let mut features = compute_required_features(
        &ctx.native_module_imports,
        ctx.uses_fetch,
        ctx.uses_crypto_builtins,
    );

    // Follow-up to #835/#846: codegen-side FFI registry recorded
    // Stdlib-resident symbols that the front-end emitted without a
    // matching `import "<module>"` in the user TS (Effect's `Stream`
    // lowering, etc.). The drain in `compile.rs` populated
    // `ctx.extra_stdlib_features` with the perry-stdlib Cargo feature
    // each symbol needs. Union those in so the rebuild compiles the
    // providing module — without this, the auto-optimize stdlib
    // (--no-default-features) drops e.g. `pub mod streams` and the
    // link fails with "Undefined symbols: _js_readable_stream_…".
    for feat in &ctx.extra_stdlib_features {
        features.insert(*feat);
    }

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
    //
    // When the user's TS code (or any compilePackages-resolved module like
    // hono) constructs `new Headers(...)` / `new Request(...)` / `new Response(...)`,
    // the HIR sets `ctx.uses_fetch = true` (see
    // `crates/perry-hir/src/destructuring.rs::1469-1492` + the explicit
    // `fetch(...)` arms in `lower/expr_call.rs`). Keep `http-client` below
    // so perry-stdlib supplies both the constructors and the erased-type
    // Request/Response/Headers/Blob dispatch registries. Do not synthesize
    // the `"fetch"` well-known binding from `uses_fetch`: perry-ext-fetch has
    // separate registries, so a builtin `new Request()` constructed there
    // would make `(req as any).url` miss stdlib's dispatch path.
    if use_well_known {
        for module in &iteration_set {
            let module_normalized = module.strip_prefix("node:").unwrap_or(module);
            let Some(binding) = super::well_known::lookup_well_known(module) else {
                continue;
            };
            // Workspace root is required for both the prebuilt-path
            // probe AND for the rebuild-in-auto-optimize path.
            let workspace_root_opt = find_perry_workspace_root();
            let Some(workspace_root) = workspace_root_opt.as_ref() else {
                continue;
            };
            let needs_shared_tokio = binding_needs_shared_tokio(module_normalized);
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
            for feat in crate::commands::stdlib_features::module_to_features(module_normalized) {
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
            let original_features =
                crate::commands::stdlib_features::module_to_features(module_normalized);
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
            // #1843 — when the flip strips `compression` and routes
            // `node:zlib` to perry-ext-zlib, activate `external-zlib-pump`
            // so perry-stdlib's main-thread pump + active-handles gate drain
            // perry-ext-zlib's deferred stream-event queue and route
            // `gz.write()`/`.on()`/`.pipe()` (lost-static-type) calls into its
            // `js_ext_zlib_dispatch_method`. Without this the events stay
            // queued forever (`createGzip().on('data')` never fires).
            if original_features.contains(&"compression") {
                features.insert("external-zlib-pump");
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
            // `node:http` / `node:https` / `node:http2` can also create
            // WebSocket client handles through `server.on("upgrade", ...)`.
            // The HTTP wrapper registers those upgraded streams in
            // perry-ext-ws, so stdlib must pump the external WS queue even
            // when user code does not import `ws` directly. Without this,
            // `ws.send(...)` from the upgrade callback works for the greeting,
            // but later browser/client frames remain queued forever and
            // `ws.on("message", ...)` never fires.
            if matches!(module_normalized, "http" | "https" | "http2") {
                features.insert("external-ws-pump");
            }
            // Same shape for fastify. The compat-sweep fastify fixture
            // hit a hang at `await app.listen(...)` because
            // perry-ext-fastify's `js_fastify_listen` entered a blocking
            // event loop that never returned. With `listen()` now non-
            // blocking, the per-server mpsc receiver lives inside the
            // FastifyServerHandle and is drained by
            // `js_fastify_process_pending`. Activating this feature
            // wires that pump call into perry-stdlib's
            // `js_stdlib_process_pending` / `_has_active_handles` so
            // requests flow on the main TS thread once the flip routes
            // `import 'fastify'` to perry-ext-fastify.
            if original_features.contains(&"bundled-fastify") {
                features.insert("external-fastify-pump");
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
            if matches!(module_normalized, "http" | "https" | "http2") {
                features.insert("external-http-server-pump");
            }
            // Issue #769 — when `node:http` / `node:https` routes to
            // perry-ext-http, also activate the client-side pump so the
            // response/error queue produced by `http.request` /
            // `http.get` (perry-ext-http's `js_http_request`,
            // `js_http_get`) actually gets drained. Without this the
            // request fires but the user callback never runs.
            if matches!(module_normalized, "http" | "https") {
                features.insert("external-http-client-pump");
            }
            // Issue #4995 — when `node:events` routes to perry-ext-events,
            // have js_stdlib_init_dispatch eagerly register the ext crate's
            // EventEmitter constructor as the runtime's events construct
            // dispatcher. Without this, a dynamic `new` on the bound
            // `events.EventEmitter` export value (`require('events')`,
            // default import, aliased ctor) falls through to the
            // empty-object path until the first static construction has
            // lazily registered the hooks.
            if module_normalized == "events" {
                features.insert("external-events-construct");
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
            // Not verbose-gated: the fallback links the full-feature
            // prebuilt stdlib (sqlite/crypto/tokio/…), which typically
            // adds 5MB+ of code the linker cannot dead-strip (the
            // dynamic dispatch table pins every module). Users should
            // know why the binary is big and how to opt back in.
            if matches!(format, OutputFormat::Text) && verbose == 0 {
                eprintln!(
                    "  note: Perry workspace source not found — linking the prebuilt \
                     full stdlib (larger binary). Set PERRY_WORKSPACE_ROOT to a \
                     source checkout to enable size-optimized rebuilds."
                );
            }
            if matches!(format, OutputFormat::Text) && verbose > 0 {
                let (rt_name, std_name) = match target {
                    Some("windows") | Some("windows-winui") => {
                        ("perry_runtime.lib", "perry_stdlib.lib")
                    }
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
            // #2532 — out-of-tree (released / out-of-source) install:
            // we can't rebuild perry-stdlib with a stripped feature set,
            // so the link uses the prebuilt full `libperry_stdlib.a`.
            // That full stdlib does NOT carry the `perry-ext-*` host
            // functions — `node:http`'s server lives in perry-ext-http /
            // perry-ext-http-server, which aren't perry-stdlib deps — so
            // an out-of-box `node:http` server otherwise fails to link
            // with `Undefined symbols: _js_node_http_create_server…`.
            // Resolve the well-known ext staticlibs the program needs
            // from the same search path the runtime/stdlib lookups use
            // (PERRY_LIB_DIR / PERRY_RUNTIME_DIR, the exe dir, Homebrew
            // `../lib`, …) and hand them back so they join the link line
            // after the full stdlib.
            let well_known_libs = if use_well_known {
                resolve_prebuilt_ext_libs(&iteration_set, target, format, verbose)
            } else {
                Vec::new()
            };
            // Out-of-tree size salvage: release packaging ships a
            // panic=abort prebuilt runtime variant alongside the unwind
            // one (stage-npm.sh / release-packages.yml). When the app
            // links runtime-only (no stdlib) and pulls in nothing that
            // needs `catch_unwind`, prefer it — same ~12-18% saving the
            // workspace rebuild gets from panic=abort, no source needed.
            // Unix-only by construction: Windows always links stdlib
            // (codegen declares all stdlib externs there), and mixing an
            // abort runtime with the unwind stdlib is not supported.
            let runtime = if panic_abort_safe && !ctx.needs_stdlib {
                let found = super::library_search::find_runtime_abort_library(target);
                if found.is_some() && matches!(format, OutputFormat::Text) && verbose > 0 {
                    eprintln!("  auto-optimize: using prebuilt panic=abort runtime");
                }
                found
            } else {
                None
            };
            return OptimizedLibs {
                runtime,
                prefer_well_known_before_stdlib: !well_known_libs.is_empty(),
                well_known_libs,
                ..OptimizedLibs::empty()
            };
        }
    };
    let workspace_root = cargo_target_dir_path(workspace_root);

    // Hash the (features, panic_mode, target, wasm-host) tuple into the
    // target dir name so cargo treats each combination as its own
    // incremental cache. `wasm-host` lives on `perry-runtime` (not
    // perry-stdlib), so it isn't part of `feature_arg`; bake it in here
    // separately so a wasm program's build doesn't get served from a
    // cached non-wasm dir (which would lack `js_webassembly_*` symbols)
    // and vice versa (would carry unresolved `perry_wasm_host_*` refs).
    //
    // The compiler version is part of the key too. Codegen emits calls to
    // runtime entrypoints (e.g. `js_promise_run_promise_jobs`,
    // `js_mark_entry_module_esm`) that grow with each release; the object
    // cache is already version-invalidated (see build_cache.rs — it misses on
    // `perry_version != CARGO_PKG_VERSION`), so on a persistent build host a
    // newer compiler emits the new calls while this version-blind dir would
    // hand back a stale `libperry_runtime.a` lacking those symbols — an
    // "undefined symbol" link failure for exactly the newly-added entrypoints.
    // Keying on the version forces a matching rebuild whenever perry upgrades.
    // Cheap djb2 — no need for the SipHash overhead.
    let target_str = target.unwrap_or("host");
    let key_input = format!(
        "{}|{}|{}|wasm={}|regex={}|temporal={}|ee={}|url={}|norm={}|seg={}|diag={}|dgram={}|v={}",
        feature_arg,
        panic_abort_safe,
        target_str,
        ctx.needs_wasm_runtime,
        ctx.uses_regex,
        ctx.uses_temporal,
        ctx.uses_event_emitter,
        ctx.uses_url,
        ctx.uses_string_normalize,
        ctx.uses_intl_segmenter,
        ctx.uses_diagnostics,
        ctx.uses_dgram,
        env!("CARGO_PKG_VERSION"),
    );
    let mut hash: u64 = 5381;
    for b in key_input.as_bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(*b as u64);
    }
    let (target_dir, cargo_env_dir) = auto_target_dir_paths(&workspace_root, hash);

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
        // Keep Windows auto-target paths in the non-verbatim form before
        // handing them to Cargo or downstream MSVC tools. Other platforms
        // keep the previous absolute env path behavior.
        .env("CARGO_TARGET_DIR", &cargo_env_dir)
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
    // Issue #76 — enable perry-runtime's `wasm-host` feature when the
    // program references `WebAssembly.*`. Without this the shim TU stays
    // out of libperry_runtime.a, so unrelated programs don't drag in
    // unresolved `perry_wasm_host_*` references at link time.
    if ctx.needs_wasm_runtime {
        cross_features.push("perry-runtime/wasm-host".to_string());
    }
    // Enable the regex engine (`regex` + `fancy-regex`, ~1.2 MB) only when the
    // program can actually produce or use a RegExp — detected in
    // collect_modules. A program that never evaluates a regex literal/`RegExp`,
    // a regex-coercing string method, or a glob API links none of it. The
    // RegExp identity/display layer is always compiled, so non-regex programs
    // still format/compare values correctly with the engine absent.
    if ctx.uses_regex {
        cross_features.push("perry-runtime/regex-engine".to_string());
    }
    // Enable the TC39 Temporal engine (`temporal_rs` + tz/calendar deps,
    // ~580 KB) only when the program references `Temporal.*`. JS `Date` is a
    // separate implementation and does not require this.
    if ctx.uses_temporal {
        cross_features.push("perry-runtime/temporal".to_string());
    }
    // Enable the WHATWG URL host/IDNA engine (`url`+`idna`+transitive
    // `percent_encoding`, ~195 KB) only when the program uses a URL API.
    if ctx.uses_url {
        cross_features.push("perry-runtime/url-engine".to_string());
    }
    // `String.prototype.normalize` tables (~113 KB) and `Intl.Segmenter`
    // UAX #29 tables (~73 KB) — each enabled only on its specific usage.
    if ctx.uses_string_normalize {
        cross_features.push("perry-runtime/string-normalize".to_string());
    }
    if ctx.uses_intl_segmenter {
        cross_features.push("perry-runtime/intl-segmenter".to_string());
    }
    // Cold-path diagnostic JSON serializers (~95 KB incl. the `serde_json`
    // pulled only by them) — enabled only when the program uses a heap-snapshot
    // API or `process.report`. The env-driven GC/typed-feedback dev trace JSON
    // ride this feature and stay off in size-optimized binaries.
    if ctx.uses_diagnostics {
        cross_features.push("perry-runtime/diagnostics".to_string());
    }
    // Per-Node-module gating: `node:dgram`'s implementation + dispatch arm are
    // behind `mod-dgram`, enabled only when the program uses dgram (detected via
    // `module: "dgram"` in the HIR). codegen only emits the `js_dgram_*` externs
    // for dgram programs, so detection is complete (no dangling symbols).
    if ctx.uses_dgram {
        cross_features.push("perry-runtime/mod-dgram".to_string());
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
    // #1508: same shape for Android — cc-rs can't find the NDK clang
    // otherwise (silent on Unix where `clang` happens to exist, hard fail
    // on Windows with `clang.exe not found`).
    if matches!(target, Some("android") | Some("android-x86_64")) {
        if let Some(ndk) = std::env::var_os("ANDROID_NDK_HOME") {
            for (k, v) in
                super::library_search::android_cross_env(std::path::Path::new(&ndk), target)
            {
                cargo_cmd.env(k, v);
            }
        }
    }
    // RUSTFLAGS is the only path that works without a custom cargo profile,
    // and cargo correctly reuses incremental artifacts that were built with
    // the same RUSTFLAGS. The hash-keyed CARGO_TARGET_DIR keeps builds with
    // distinct flag sets from clobbering each other's cache.
    let mut rustflags: Vec<&str> = Vec::new();
    if panic_abort_safe {
        // Override the workspace profile's `panic = "unwind"` for the
        // duration of this invocation.
        rustflags.push("-C panic=abort");
    }
    // #1529 — Android loads `libperry_app.so` via `dlopen` at runtime
    // (PerryActivity's System.loadLibrary), but Rust's default TLS model for
    // the aarch64-linux-android target is Initial-Executable, which is only
    // valid for libraries present at process startup. A dlopen'd library
    // crashes with `TLS symbol "(null)" ... using IE access model`. The
    // runtime/stdlib use `thread_local!` heavily (per-thread arena, GC state,
    // shadow stack), so those IE TLS relocations get baked into the final
    // cdylib. Force global-dynamic so the dynamic linker can resolve TLS
    // slots after the process has started.
    if matches!(target, Some("android") | Some("android-x86_64")) {
        rustflags.push(android_global_dynamic_tls_rustflag(&mut cargo_cmd));
    }
    if !rustflags.is_empty() {
        cargo_cmd.env("RUSTFLAGS", rustflags.join(" "));
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
        Some("windows") | Some("windows-winui") => "perry_runtime.lib",
        #[cfg(target_os = "windows")]
        None => "perry_runtime.lib",
        _ => "libperry_runtime.a",
    };
    let stdlib_name = match target {
        Some("windows") | Some("windows-winui") => "perry_stdlib.lib",
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
        // Cargo emits `lib<lib>.a` on Unix but `<lib>.lib` on Windows/MSVC.
        // Hardcoding the Unix name here meant a Windows build never found
        // the rebuilt ext staticlib (e.g. perry-ext-ws), silently skipped
        // it, and failed the final link with unresolved `js_*` symbols.
        let lib_filename =
            super::well_known::ext_staticlib_filename(lib, rust_target_triple(target));
        let lib_path = release_dir.join(&lib_filename);
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
                    .join(&lib_filename);
                if triple_path.exists() {
                    triple_path
                } else {
                    workspace_root
                        .join("target")
                        .join("release")
                        .join(&lib_filename)
                }
            } else {
                workspace_root
                    .join("target")
                    .join("release")
                    .join(&lib_filename)
            };
            if fallback.exists() {
                if matches!(format, OutputFormat::Text) {
                    eprintln!(
                        "  well-known: rebuild produced no `{}` in {} — \
                         using workspace fallback (CONTEXT panic risk on tokio I/O)",
                        lib_filename,
                        release_dir.display()
                    );
                }
                well_known_libs.push(fallback);
            } else if matches!(format, OutputFormat::Text) {
                eprintln!(
                    "  well-known: rebuild produced no `{}` for `{}`; \
                     skipping — link will likely fail with unresolved js_* symbols.",
                    lib_filename, krate
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
                .env("CARGO_TARGET_DIR", &cargo_env_dir)
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

        // Emit .bc for additional crates (UI, geisterhand).
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
                Some("windows-winui") => "perry-ui-windows-winui",
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
        prefer_well_known_before_stdlib: false,
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
fn resolve_prebuilt_ext_libs(
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
        let Some(binding) = super::well_known::lookup_well_known(module) else {
            continue;
        };
        if !seen.insert(binding.lib.clone()) {
            continue;
        }
        let filename =
            super::well_known::ext_staticlib_filename(&binding.lib, rust_target_triple(target));
        match super::library_search::find_library(&filename, target) {
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

fn build_missing_prebuilt_ext_lib(
    workspace_root: &Path,
    binding: &super::well_known::WellKnownBinding,
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
        | "http2"
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
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock poisoned")
    }

    fn set_env_var(key: &str, value: Option<&str>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

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

    #[test]
    fn builtin_fetch_usage_does_not_synthesize_well_known_fetch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = CompilationContext::new(dir.path().to_path_buf());
        ctx.uses_fetch = true;

        let modules = well_known_iteration_set(&ctx);

        assert!(
            !modules.contains("fetch"),
            "built-in Web Fetch should stay on perry-stdlib so erased-type dispatch shares the constructor registry"
        );
    }

    #[test]
    fn explicit_node_fetch_import_still_routes_to_well_known_fetch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = CompilationContext::new(dir.path().to_path_buf());
        ctx.native_module_imports.insert("node-fetch".to_string());

        let modules = well_known_iteration_set(&ctx);

        assert!(modules.contains("node-fetch"));
    }

    #[test]
    fn forced_well_known_env_extends_iteration_set() {
        let _guard = env_lock();
        let old_force_well_known = std::env::var("PERRY_FORCE_WELL_KNOWN").ok();

        set_env_var(
            "PERRY_FORCE_WELL_KNOWN",
            Some("http, node:net ws definitely-not-real"),
        );
        let ctx = CompilationContext::new(std::env::current_dir().expect("cwd"));
        let modules = well_known_iteration_set(&ctx);

        set_env_var("PERRY_FORCE_WELL_KNOWN", old_force_well_known.as_deref());

        assert!(modules.contains("http"));
        assert!(modules.contains("net"));
        assert!(modules.contains("ws"));
        assert!(!modules.contains("node:net"));
        assert!(!modules.contains("definitely-not-real"));
    }

    #[test]
    fn no_auto_still_resolves_prebuilt_well_known_archives() {
        let _guard = env_lock();
        let old_lib_dir = std::env::var("PERRY_LIB_DIR").ok();
        let old_runtime_dir = std::env::var("PERRY_RUNTIME_DIR").ok();
        let old_disable_well_known = std::env::var("PERRY_DISABLE_WELL_KNOWN").ok();

        let dir = tempfile::tempdir().expect("tempdir");
        let http =
            super::super::well_known::lookup_well_known("http").expect("http well-known binding");
        let net =
            super::super::well_known::lookup_well_known("net").expect("net well-known binding");
        let ws = super::super::well_known::lookup_well_known("ws").expect("ws well-known binding");
        let http_lib = dir
            .path()
            .join(super::super::well_known::ext_staticlib_filename(
                &http.lib,
                rust_target_triple(None),
            ));
        let net_lib = dir
            .path()
            .join(super::super::well_known::ext_staticlib_filename(
                &net.lib,
                rust_target_triple(None),
            ));
        let ws_lib = dir
            .path()
            .join(super::super::well_known::ext_staticlib_filename(
                &ws.lib,
                rust_target_triple(None),
            ));
        std::fs::write(&http_lib, b"!<arch>\n").expect("write fake http archive");
        std::fs::write(&net_lib, b"!<arch>\n").expect("write fake net archive");
        std::fs::write(&ws_lib, b"!<arch>\n").expect("write fake ws archive");

        set_env_var(
            "PERRY_LIB_DIR",
            Some(dir.path().to_str().expect("utf8 temp path")),
        );
        set_env_var("PERRY_RUNTIME_DIR", None);
        set_env_var("PERRY_DISABLE_WELL_KNOWN", None);

        let mut ctx = CompilationContext::new(dir.path().to_path_buf());
        ctx.native_module_imports.insert("http".to_string());
        ctx.native_module_imports.insert("net".to_string());
        ctx.native_module_imports.insert("ws".to_string());
        let libs = resolve_no_auto_optimized_libs(&ctx, None, OutputFormat::Json, 0);

        set_env_var("PERRY_LIB_DIR", old_lib_dir.as_deref());
        set_env_var("PERRY_RUNTIME_DIR", old_runtime_dir.as_deref());
        set_env_var(
            "PERRY_DISABLE_WELL_KNOWN",
            old_disable_well_known.as_deref(),
        );

        assert_eq!(libs.runtime, None);
        assert_eq!(libs.stdlib, None);
        assert!(
            libs.well_known_libs.contains(&http_lib),
            "expected no-auto well-known libs to include {http_lib:?}, got {:?}",
            libs.well_known_libs
        );
        assert!(
            libs.well_known_libs.contains(&net_lib),
            "expected no-auto well-known libs to include {net_lib:?}, got {:?}",
            libs.well_known_libs
        );
        assert!(
            libs.well_known_libs.contains(&ws_lib),
            "expected no-auto well-known libs to include {ws_lib:?}, got {:?}",
            libs.well_known_libs
        );
    }

    #[cfg(windows)]
    #[test]
    fn cargo_target_dir_strips_windows_verbatim_prefixes() {
        let drive = cargo_target_dir_path(PathBuf::from(
            r"\\?\D:\Projects\perry\target\perry-auto-deadbeef",
        ));
        assert_eq!(
            drive,
            PathBuf::from(r"D:\Projects\perry\target\perry-auto-deadbeef")
        );

        let unc = cargo_target_dir_path(PathBuf::from(
            r"\\?\UNC\server\share\perry\target\perry-auto-deadbeef",
        ));
        assert_eq!(
            unc,
            PathBuf::from(r"\\server\share\perry\target\perry-auto-deadbeef")
        );
    }

    #[cfg(windows)]
    #[test]
    fn auto_target_dir_uses_relative_cargo_env_path_on_windows() {
        let workspace = PathBuf::from(r"\\?\D:\Projects\perry");
        let (target_dir, cargo_env_dir) = auto_target_dir_paths(&workspace, 0xdeadbeef);

        assert!(
            !cargo_env_dir.is_absolute(),
            "CARGO_TARGET_DIR should stay relative so Cargo build scripts do not receive verbatim Windows paths"
        );
        assert_eq!(
            cargo_env_dir,
            PathBuf::from("target").join("perry-auto-00000000deadbeef")
        );
        assert_eq!(
            target_dir,
            PathBuf::from(r"D:\Projects\perry\target\perry-auto-00000000deadbeef")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn auto_target_dir_keeps_absolute_cargo_env_path_off_windows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (target_dir, cargo_env_dir) = auto_target_dir_paths(dir.path(), 0xdeadbeef);

        assert!(
            cargo_env_dir.is_absolute(),
            "non-Windows hosts should keep the previous absolute CARGO_TARGET_DIR behavior"
        );
        assert_eq!(target_dir, cargo_env_dir);
    }

    #[cfg(unix)]
    #[test]
    fn no_auto_builds_missing_well_known_archive_from_workspace_source() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = env_lock();
        let old_path = std::env::var_os("PATH");
        let old_cargo_target_dir = std::env::var_os("CARGO_TARGET_DIR");

        let workspace = tempfile::tempdir().expect("tempdir");
        for dir in [
            "crates/perry-runtime",
            "crates/perry-ui-geisterhand",
            "crates/perry-ext-http",
        ] {
            std::fs::create_dir_all(workspace.path().join(dir)).expect("mkdir workspace marker");
        }

        let fake_bin = workspace.path().join("fake-bin");
        std::fs::create_dir_all(&fake_bin).expect("mkdir fake bin");
        let fake_cargo = fake_bin.join("cargo");
        std::fs::write(
            &fake_cargo,
            r#"#!/bin/sh
case "$*" in
  *"-p perry-ext-http"*) ;;
  *) exit 43 ;;
esac
mkdir -p "$CARGO_TARGET_DIR/release"
printf '!<arch>\n' > "$CARGO_TARGET_DIR/release/libperry_ext_http.a"
"#,
        )
        .expect("write fake cargo");
        let mut perms = std::fs::metadata(&fake_cargo)
            .expect("fake cargo metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_cargo, perms).expect("chmod fake cargo");

        let target_dir = workspace.path().join("out-target");
        let test_path = match old_path.as_ref() {
            Some(path) => {
                let mut paths = vec![fake_bin.clone()];
                paths.extend(std::env::split_paths(path));
                std::env::join_paths(paths).expect("join PATH")
            }
            None => fake_bin.clone().into_os_string(),
        };
        std::env::set_var("PATH", test_path);
        std::env::set_var("CARGO_TARGET_DIR", &target_dir);

        let binding =
            super::super::well_known::lookup_well_known("http").expect("http well-known binding");
        let filename = super::super::well_known::ext_staticlib_filename(
            &binding.lib,
            rust_target_triple(None),
        );
        let got = build_missing_prebuilt_ext_lib(
            workspace.path(),
            binding,
            &filename,
            None,
            OutputFormat::Json,
            0,
        );

        if let Some(path) = old_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(dir) = old_cargo_target_dir {
            std::env::set_var("CARGO_TARGET_DIR", dir);
        } else {
            std::env::remove_var("CARGO_TARGET_DIR");
        }

        assert_eq!(
            got.expect("missing archive should be built from workspace source"),
            target_dir.join("release/libperry_ext_http.a")
        );
    }
}
