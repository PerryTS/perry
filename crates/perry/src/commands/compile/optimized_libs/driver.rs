use super::*;

use std::collections::BTreeSet;

use crate::commands::stdlib_features::compute_required_features;
use crate::OutputFormat;

use super::super::CompilationContext;

/// Select the provider for TLS module/server state after well-known routing.
///
/// perry-ext-net (directly or through perry-ext-http) owns the socket/connect
/// symbols, but calls back into perry-stdlib for TLS SNI/ALPN preflight.
/// `external-net-tls` keeps that provider and binds the `tls` module's dynamic
/// `connect` to perry-ext-net's `js_tls_connect`. A TLS program that routes no
/// net transport (a server-only `node:tls` program) keeps the `tls` umbrella,
/// which since tokio lane L4 is `tls-runtime` alone — bundled net is gone.
pub(super) fn finalize_tls_transport_features(
    features: &mut BTreeSet<&'static str>,
    imports_tls: bool,
    external_net_transport: bool,
) {
    if external_net_transport {
        features.remove("tls");
        features.remove("bundled-net");
        features.insert("external-net-tls");
    } else if imports_tls {
        features.insert("tls");
    }
}

/// Choose the runtime/stdlib/wrapper archives a program links and the optional
/// features its generated installer installs.
///
/// perry links the prebuilt archives (`target/release`, or the ones installed
/// next to the perry binary) and never builds them at compile time. Each
/// archive carries every optional feature behind an install entry point
/// (`js_runtime_install_*` / `js_stdlib_install_*`); the features computed here
/// pick the entry points the generated installer calls, and the linker's
/// `--gc-sections` / `-dead_strip` / `/OPT:REF` drops whatever no installed
/// feature and no compiled call reaches.
pub(crate) fn build_optimized_libs(
    ctx: &CompilationContext,
    target: Option<&str>,
    cli_features: &[String],
    format: OutputFormat,
    verbose: u8,
) -> OptimizedLibs {
    let use_well_known = std::env::var_os("PERRY_DISABLE_WELL_KNOWN").is_none();
    let iteration_set = well_known_iteration_set(ctx);

    let imports_undici = iteration_set
        .iter()
        .any(|m| m.strip_prefix("node:").unwrap_or(m) == "undici");
    let imports_tls = iteration_set
        .iter()
        .any(|m| m.strip_prefix("node:").unwrap_or(m) == "tls");
    if imports_undici && !use_well_known {
        eprintln!(
            "error: `import 'undici'` requires the external perry-ext-undici wrapper, but the \
             well-known flip is disabled (PERRY_DISABLE_WELL_KNOWN). There is no in-stdlib undici \
             namespace fallback; unset PERRY_DISABLE_WELL_KNOWN so undici routes to \
             perry-ext-undici."
        );
        std::process::exit(1);
    }
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
    // copies for bisection — except for the bindings that no longer have
    // one (`net`, `ws`, `zlib`: `wrapper_is_sole_provider`), which route to their
    // wrapper either way. If a bundled `.a` is missing on disk,
    // each entry falls back to the perry-stdlib copy individually
    // (logged with `well-known: skipping` when verbose), so a
    // partially-built workspace still produces a working binary.
    let mut external_net_transport = false;
    // Web Fetch is selected independently from the external node:http
    // binding. `uses_fetch` adds `web-fetch` in compute_required_features.
    // Every routed module with a well-known binding links that binding's
    // prebuilt wrapper archive (`resolve_prebuilt_ext_libs`, below); here it
    // only adjusts the stdlib features the program needs installed.
    let routed_set = routed_modules(ctx);
    {
        for module in &routed_set {
            let module_normalized = module.strip_prefix("node:").unwrap_or(module);
            if super::super::well_known::lookup_well_known(module).is_none() {
                continue;
            }
            if matches!(module_normalized, "net" | "http" | "https" | "http2") {
                external_net_transport = true;
            }
            // Strip the perry-stdlib feature(s) this binding was
            // covering. `module_to_features` is the same table
            // `compute_required_features` consulted above, so we
            // know exactly what to remove.
            for feat in crate::commands::stdlib_features::module_to_features(module_normalized) {
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
            // `async-bridge` feature — the `perry_ffi_*` shim
            // module that wrappers like bcrypt / argon2 / ws / db
            // pull through linking lives in
            // `crates/perry-stdlib/src/perry_ffi_async.rs`.
            // Stripping `bundled-bcrypt` (etc.) without
            // re-asserting the bridge would leave the
            // wrapper's `.a` carrying unresolved `perry_ffi_*`
            // references. Detect async wrappers by checking
            // whether the original feature list contained an
            // async feature; if it did, ensure it stays.
            //
            // Every wrapper that replaces a bundled binding needs the
            // bridge, and nothing more: the tokio runtime that used to sit
            // behind `async-runtime` was deleted with tokio, and no wrapper
            // hands perry-stdlib a tokio future any more.
            let original_features =
                crate::commands::stdlib_features::module_to_features(module_normalized);
            if original_features.iter().any(|f| {
                matches!(
                    *f,
                    "bundled-bcrypt"
                        | "bundled-argon2"
                        | "bundled-nodemailer"
                        | "bundled-streams"
                        | "bundled-ws"
                        | "bundled-net"
                        | "http-client"
                )
            }) {
                features.insert("async-bridge");
            }
            // turnloop P8 group H deleted the bundled pg / mysql2 / mongodb
            // modules, and their wrappers (perry-ext-pg #10677,
            // perry-ext-mysql2 #10680, perry-ext-mongodb #11337) are gone too.
            // `well_known_bindings.toml` has no row for any of them, so this
            // loop — which `continue`s at `lookup_well_known` for any module
            // without a row — never reaches them: those imports compile the
            // real npm package, whose sockets are `net` / `tls` and select
            // whatever those select.
            // `undici` (#466): perry-ext-undici is thin glue over the
            // native Web Fetch stack. Its `setGlobalDispatcher` writes
            // the proxy config through `js_fetch_set_global_proxy`,
            // defined in perry-stdlib's `web-fetch` module (and mirrored
            // by perry-ext-fetch). A ProxyAgent-only program never calls
            // `fetch()` in a way `uses_fetch` detects, so re-assert
            // `web-fetch` here or the wrapper's extern reference dangles
            // at link time. (`web-fetch` implies `async-bridge`, which the
            // wrapper's JsPromise surface needs anyway.)
            if module_normalized == "undici" {
                features.insert("web-fetch");
            }
            // turnloop P6 — the same shape, for SMTP. `import 'nodemailer'`
            // strips `bundled-nodemailer` and routes to perry-ext-nodemailer,
            // which reaches perry-stdlib's turnloop SMTP engine through the
            // `js_perry_smtp_*` C seam. That engine is gated on
            // `turnloop-smtp-client`, which is NOT implied by
            // `bundled-nodemailer` precisely so the strip cannot take it — but
            // the rebuilt feature list is explicit, so it has to be re-asserted
            // here or the wrapper's externs dangle at link time.
            if module_normalized == "nodemailer" {
                features.insert("turnloop-smtp-client");
            }
            // v0.5.579 — when the flip strips `bundled-net`, activate
            // `external-net-pump` to retain the shared runtime and external
            // net dispatch adapters. perry-ext-net registers its pump and
            // keepalive contributor directly with runtime.
            if original_features.contains(&"bundled-net") {
                features.insert("external-net-pump");
            }
            // perry-ext-net owns client sockets after the well-known flip,
            // but TLS servers and TLSSocket introspection still live in
            // perry-stdlib. Retain that surface without re-enabling
            // `bundled-net` (and its colliding socket symbols).
            if original_features.contains(&"tls") {
                features.insert("external-tls-server");
            }
            // The same provider serves both builds. Resolve module exports
            // from the separately linked archive in an optimized build.
            if original_features.contains(&"compression-gzip")
                || original_features.contains(&"compression")
            {
                features.insert("external-zlib-pump");
                // The binding already carries every codec; avoid compiling
                // a second copy of its dependencies through the stdlib rlib.
                features.remove("compression-brotli");
                features.remove("compression-zstd");
            }
            // Closes #606 — same shape for ws. When the well-known flip
            // strips `bundled-ws` and routes to perry-ext-ws, retain
            // `external-ws-pump` for the shared async runtime. perry-ext-ws
            // registers its event queue and active handles when it initializes.
            if original_features.contains(&"bundled-ws") {
                features.insert("external-ws-pump");
            }
            // `node:http` / `node:https` / `node:http2` can also create
            // WebSocket client handles through `server.on("upgrade", ...)`.
            // The HTTP wrapper registers those upgraded streams in
            // perry-ext-ws, whose initialization registers its pump and
            // keepalive callback even without a direct `ws` import. Retain
            // `external-ws-pump` here for the shared async runtime.
            if matches!(module_normalized, "http" | "https" | "http2") {
                features.insert("external-ws-pump");
            }
            // Closes #604 — when the well-known flip routes `node:http` /
            // `node:https` / `node:http2` to perry-ext-http, activate
            // `external-http-server-pump` to retain its dispatch adapters and
            // shared async runtime. perry-ext-http self-registers the server
            // pump and keepalive callback.
            //
            // Gate strictly on the MODULE name (not on `http-client`
            // feature, which axios / node-fetch also map to), because those
            // packages do not need the server dispatch adapters.
            if matches!(module_normalized, "http" | "https" | "http2") {
                features.insert("external-http-server-pump");
                // perry-ext-http depends on perry-ext-net, whose TLS client
                // object calls back into perry-stdlib's main-thread
                // preflight hook. Retain that provider even for plain HTTP:
                // the static archive can pull the shared socket object before
                // a TLS-specific API is used, and otherwise leaves
                // `js_tls_client_preflight` undefined at link time.
                features.insert("external-tls-server");
            }
            features.extend(
                crate::commands::stdlib_features::routed_stream_dispatch_features(
                    module_normalized,
                )
                .iter()
                .copied(),
            );
        }
    }

    // `net` maps to `[bundled-net, tls]` because its bundled implementation
    // owns upgradeToTLS. External net/http owns connect/socket symbols but
    // still calls perry-stdlib's SNI/ALPN preflight provider. Select the split
    // `external-net-tls` adapter in that case; a direct TLS-only program keeps
    // the historical `tls` umbrella (and therefore bundled net).
    finalize_tls_transport_features(&mut features, imports_tls, external_net_transport);

    // The UI backends (perry-ui-gtk4 on Linux, perry-ui-macos, perry-ui-windows)
    // reach into perry-stdlib's async bridge from GLib/NSTimer/WM_TIMER
    // trampolines (js_stdlib_process_pending, js_promise_run_microtasks).
    // Those symbols live in perry-stdlib/src/common/async_bridge.rs which is
    // gated on `#[cfg(feature = "async-bridge")]`. For a bare UI program
    // whose user code imports zero stdlib modules, compute_required_features
    // returns an empty set and the auto-optimized stdlib is built with
    // --no-default-features — no bridge, no async_bridge module, no
    // symbol. Force `async-bridge` whenever the program pulls in a UI
    // backend so the trampolines resolve at link time. (The bridge has no
    // tokio; nothing in the workspace does.)
    if ctx.needs_ui {
        features.insert("async-bridge");
    }
    // zlib per-codec cherry-pick: `import 'node:zlib'` only selected the
    // gzip/deflate base above (`compression-gzip`). Layer the Brotli / zstd
    // backends on when HIR usage detection saw a matching API token, and
    // fall back to the full set when a deferred dynamic-code site could
    // name a codec from a runtime string (`zlib[name](...)` after `eval`).
    // Gated on the base feature still being present: if the well-known flip
    // routed `node:zlib` to perry-ext-zlib, the sub-features would imply
    // `compression-gzip` back on and duplicate the ext crate's `js_zlib_*`
    // symbols at link.
    if features.contains("compression-gzip") {
        if ctx.uses_zlib_brotli || perry_hir::has_deferred_dynamic_code_sites() {
            features.insert("compression-brotli");
        }
        if ctx.uses_zlib_zstd || perry_hir::has_deferred_dynamic_code_sites() {
            features.insert("compression-zstd");
        }
    }
    // WHATWG compression streams: `bundled-streams` no longer carries the
    // Brotli codec, because `web-fetch` implies it for `response.body` and the
    // format `match` kept the whole encoder live in every fetch program. Layer
    // `streams-brotli` on only when the program names CompressionStream /
    // DecompressionStream, or has deferred dynamic code that could.
    if features.contains("bundled-streams")
        && (ctx.uses_web_compression_stream || perry_hir::has_deferred_dynamic_code_sites())
    {
        features.insert("streams-brotli");
    }
    // perry-stdlib unconditionally re-bundles perry-updater (so user code
    // calling `perry/updater` resolves at link time without extra wiring).
    // perry-updater used to reference the extern `js_crypto_ed25519_verify`
    // from perry-stdlib's `crypto` feature, which is why `crypto` used to be
    // force-inserted here for EVERY auto-optimized build. The updater now
    // verifies in-crate via ed25519-dalek, and the remaining crypto entry
    // points are covered by three detection layers, so the force is gone
    // (stdlib cherry-pick — non-crypto programs save the whole crypto
    // surface: RSA/EC/Ed25519/Ed448/ML-KEM/x509/JWT/bcrypt/argon2):
    //   1. `import 'node:crypto'` / bcrypt / jsonwebtoken / … →
    //      `module_to_features` (compute_required_features above);
    //   2. bare `crypto.*` builtins and the WebCrypto namespace →
    //      `ctx.uses_crypto_builtins` (collect_modules/feature_detect.rs);
    //   3. any OTHER codegen-emitted `js_crypto_*` / `js_webcrypto_*` call
    //      (compiled-package lowering with no import in the entry module) →
    //      the prefix rule in `perry_codegen::ext_registry::record_ffi_call`
    //      feeds `ctx.extra_stdlib_features`, unioned in above.
    //
    // The promise bridge stays force-on: perry-stdlib's ALWAYS-ON modules
    // (worker_threads' promise bridge, readline's pump) compile against
    // `common::async_bridge`, which is `async-bridge`-gated — a bare
    // `--no-default-features` stdlib has never compiled, and the crypto
    // force used to satisfy the gate transitively.
    //
    // turnloop P8 lane L: this force used to be `async-runtime`, i.e. tokio,
    // so tokio was in every stdlib-linking binary. The final tokio lane
    // deleted `async-runtime` and tokio with it: no feature and no wrapper
    // can select a tokio runtime any more (`no_feature_selection_reaches_tokio`
    // in optimized_libs/tests.rs pins that).
    features.insert("async-bridge");

    if matches!(format, OutputFormat::Text) && verbose > 0 {
        eprintln!("  libraries: prebuilt runtime/stdlib archives with selected installs");
    }
    let well_known_libs =
        super::wrapper_libs::resolve_prebuilt_ext_libs(&routed_set, target, format);
    let libs = OptimizedLibs {
        prefer_well_known_before_stdlib: !well_known_libs.is_empty(),
        well_known_libs,
        ..OptimizedLibs::empty()
    };
    prebuilt_installs(libs, ctx, &features, cli_features)
}

/// The prebuilt archives carry every feature; the generated installer installs
/// the ones this program needs (exactly the set the retired auto-optimized
/// rebuild compiled), so the linker drops the rest.
fn prebuilt_installs(
    mut libs: OptimizedLibs,
    ctx: &CompilationContext,
    features: &BTreeSet<&'static str>,
    cli_features: &[String],
) -> OptimizedLibs {
    use crate::commands::stdlib_installs::FeatureInstalls;
    libs.stdlib_installs =
        FeatureInstalls::Selected(features.iter().map(|f| f.to_string()).collect());
    libs.runtime_installs = FeatureInstalls::Selected(
        auto_optimized_cross_features(ctx, features, cli_features)
            .iter()
            .filter_map(|f| f.strip_prefix("perry-runtime/"))
            .map(str::to_string)
            .collect(),
    );
    libs
}
