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
pub(crate) fn build_optimized_libs(
    ctx: &CompilationContext,
    target: Option<&str>,
    cli_features: &[String],
    format: OutputFormat,
    verbose: u8,
) -> OptimizedLibs {
    let use_well_known = std::env::var_os("PERRY_DISABLE_WELL_KNOWN").is_none();
    let iteration_set = well_known_iteration_set(ctx);

    // fastify has no in-stdlib fallback (the bundled adapter was removed): it can
    // only be served by perry-ext-fastify *and* a stdlib rebuilt with
    // `external-fastify-pump` (the per-tick bridge that drains its request queue).
    // Two fallback paths can't provide that pair, so fail clearly up front instead
    // of producing a binary that hangs or fails to link with `js_fastify_*`:
    //   - `PERRY_DISABLE_WELL_KNOWN` — the flip never routes fastify at all.
    //   - `PERRY_NO_AUTO_OPTIMIZE` — uses the prebuilt `full` stdlib, which is NOT
    //     built with `external-fastify-pump` (and isn't rebuilt), so even though
    //     perry-ext-fastify links, requests would never drain (silent hang).
    let imports_fastify = iteration_set
        .iter()
        .any(|m| m.strip_prefix("node:").unwrap_or(m) == "fastify");
    if imports_fastify && !use_well_known {
        eprintln!(
            "error: `import 'fastify'` requires the external perry-ext-fastify wrapper, but the \
             well-known flip is disabled (PERRY_DISABLE_WELL_KNOWN). The in-stdlib fastify adapter \
             was removed; unset PERRY_DISABLE_WELL_KNOWN so fastify routes to perry-ext-fastify."
        );
        std::process::exit(1);
    }
    if imports_fastify && std::env::var_os("PERRY_NO_AUTO_OPTIMIZE").is_some() {
        eprintln!(
            "error: `import 'fastify'` is not supported with PERRY_NO_AUTO_OPTIMIZE: the prebuilt \
             stdlib is not compiled with `external-fastify-pump`, so fastify requests would never \
             drain (the request loop hangs). The in-stdlib fastify adapter was removed; build \
             without PERRY_NO_AUTO_OPTIMIZE so the stdlib is rebuilt with the fastify pump wired in."
        );
        std::process::exit(1);
    }

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
            let Some(binding) = super::super::well_known::lookup_well_known(module) else {
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
                let Some(lib_path) = super::super::well_known::bundled_staticlib_path_for_target(
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
                    // fastify has no in-stdlib fallback (the bundled adapter was
                    // removed) — it can only be served by perry-ext-fastify. Fail
                    // clearly instead of silently falling back to a copy that no
                    // longer exists (which would link with unresolved js_fastify_*).
                    if module_normalized == "fastify" {
                        eprintln!(
                            "error: `import 'fastify'` requires the external perry-ext-fastify \
                             wrapper, but its source crate was not found at `{}`. The in-stdlib \
                             fastify adapter was removed; build or restore perry-ext-fastify.",
                            crate_dir.display()
                        );
                        std::process::exit(1);
                    }
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
                // Fix #589 / #5174: `node:http` / `node:https` /
                // `node:http2` map to `http-client`, but that umbrella
                // covers BOTH the bundled node:http client
                // (`src/http.rs` + `src/axios.rs`) AND the Web Fetch
                // FFIs (`js_headers_new`, `js_response_new`,
                // `js_request_new`, …). When a program uses
                // `new Headers()` / `new Response()` (directly or via a
                // compilePackages package like hono) while also
                // importing `node:http`, we must keep the Web Fetch
                // half but drop the bundled client — otherwise its
                // `js_http_process_pending` (and the rest of the
                // `js_http_*` surface) duplicate perry-ext-http's
                // symbols, and perry-ext-http's aux-pump call binds to
                // perry-stdlib's empty-queue copy, wedging the
                // in-process response pump (#5174). Since `http-client
                // = ["web-fetch"]`, strip the umbrella and re-assert
                // `web-fetch`: fetch.rs/fetch_blob.rs stay,
                // http.rs/axios.rs go. The well-known staticlib
                // (perry-ext-http / perry-ext-http-server) is still
                // added for the actual node:http surface.
                if *feat == "http-client" && ctx.uses_fetch {
                    features.remove("http-client");
                    features.insert("web-fetch");
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
            // Same shape for fastify. fastify is served exclusively by
            // perry-ext-fastify (the in-stdlib adapter was removed), so this
            // fires whenever `import 'fastify'` is routed here — not off a
            // (now-gone) `bundled-fastify` feature. `external-fastify-pump`
            // wires perry-ext-fastify's `js_fastify_process_pending` /
            // `js_fastify_has_active` into perry-stdlib's
            // `js_stdlib_process_pending` / `_has_active_handles` so requests
            // flow on the main TS thread; it pulls `async-runtime` (shared
            // tokio) transitively via its Cargo feature deps.
            if module_normalized == "fastify" {
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
                let found = super::super::library_search::find_runtime_abort_library(target);
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
    let key_input = auto_optimized_cache_key(&feature_arg, panic_abort_safe, target, ctx);
    let mut hash: u64 = 5381;
    for b in key_input.as_bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(*b as u64);
    }
    let (target_dir, cargo_env_dir) = auto_target_dir_paths(&workspace_root, hash);
    let cross_features = auto_optimized_cross_features(ctx, &features, cli_features);
    let release_dir = if let Some(triple) = rust_target_triple(target) {
        target_dir.join(triple).join("release")
    } else {
        target_dir.join("release")
    };
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
    let runtime_path = release_dir.join(runtime_name);
    let stdlib_path = release_dir.join(stdlib_name);
    // Content fingerprint of the sources that land in the archives — makes the
    // stamp (and therefore the freshness gate) immune to mtime scrambling from
    // cache restores / fresh checkouts (#5892 layer 2, #5778 warm-cache trap).
    let source_fingerprint =
        auto_optimized_source_fingerprint(&workspace_root, &tokio_using_bindings);
    let build_stamp = auto_optimized_build_stamp(
        &key_input,
        target,
        &cross_features,
        &tokio_using_bindings,
        &source_fingerprint,
    );
    let build_stamp_path = target_dir.join(".perry-auto-build.stamp");

    // Closes #25 (the v0.5.384 NJOBS 6->3 retreat): serialize parallel
    // `perry compile` invocations that target the SAME `target/perry-auto
    // -<hash>` directory via an OS-level file lock. Cargo has its own
    // target-dir lock (`.cargo-lock`) that prevents concurrent COMPILES,
    // but the FILE OUTPUT is rename'd at link end -- meaning worker B's
    // clang can read `libperry_runtime.a` while worker A's cargo is
    // mid-rename and see errno=2. The race window is sub-second but
    // fired reliably at NJOBS=6 on the macos-14 compile-smoke runner.
    //
    // The lock is per-hash, so different feature combos still build in
    // parallel. fslock is portable (flock on Unix, LockFileEx on
    // Windows) and was already a transitive dep -- no new crate cost.
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

    let bitcode_requested = std::env::var("PERRY_LLVM_BITCODE_LINK").ok().as_deref() == Some("1");
    if !bitcode_requested
        && auto_optimized_archives_are_fresh(
            &workspace_root,
            &runtime_path,
            &stdlib_path,
            &tokio_using_bindings,
            &build_stamp_path,
            &build_stamp,
        )
    {
        // The "archives fresh" fast-path must still carry the NON-tokio
        // routed well-known libs collected by the routing loop above (e.g.
        // perry-ext-zlib for `node:zlib`, perry-ext-events, …). They live in
        // the outer `well_known_libs`; `resolve_auto_well_known_libs` only
        // resolves the tokio-using bindings. Without merging them, the routed
        // CPU-only ext staticlibs are dropped here and the link fails with
        // undefined `js_ext_zlib_*` / `js_zlib_*` (and other non-tokio
        // well-known) symbols — even though the archive is on disk.
        let mut well_known_libs = well_known_libs;
        well_known_libs.extend(resolve_auto_well_known_libs(
            &workspace_root,
            &release_dir,
            &tokio_using_bindings,
            target,
            format,
        ));
        return OptimizedLibs {
            runtime: Some(runtime_path),
            stdlib: Some(stdlib_path),
            runtime_bc: None,
            stdlib_bc: None,
            extra_bc: Vec::new(),
            prefer_well_known_before_stdlib: !well_known_libs.is_empty(),
            well_known_libs,
        };
    }

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
        // #5422 — the staticlib (.a) is now emitted by the perry-runtime-static
        // / perry-stdlib-static wrapper crates, not perry-runtime/perry-stdlib
        // themselves (which are rlib-only). The `perry-runtime/<feat>` strings in
        // `cross_features` still resolve because perry-runtime is in each
        // wrapper's dependency graph (cargo accepts the `dep/feature` form).
        .arg("-p")
        .arg("perry-runtime-static")
        .arg("-p")
        .arg("perry-stdlib-static")
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
    if matches!(
        target,
        Some("android") | Some("android-x86_64") | Some("wearos")
    ) {
        if let Some(ndk) = std::env::var_os("ANDROID_NDK_HOME") {
            for (k, v) in
                super::super::library_search::android_cross_env(std::path::Path::new(&ndk), target)
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
    if matches!(
        target,
        Some("android") | Some("android-x86_64") | Some("wearos")
    ) {
        rustflags.push(android_global_dynamic_tls_rustflag(&mut cargo_cmd));
    }
    if !rustflags.is_empty() {
        cargo_cmd.env("RUSTFLAGS", rustflags.join(" "));
    }

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
    let _ = std::fs::write(&build_stamp_path, &build_stamp);

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
            super::super::well_known::ext_staticlib_filename(lib, rust_target_triple(target));
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
                Some("android") | Some("wearos") => "perry-ui-android",
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
        prefer_well_known_before_stdlib: !well_known_libs.is_empty(),
        well_known_libs,
    }
}
