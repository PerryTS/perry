//! Map TypeScript native module imports → perry-stdlib Cargo features.
//!
//! Used by `--minimal-stdlib` to compute the minimal feature set needed
//! for a project, then rebuild perry-stdlib with `--no-default-features
//! --features <list>` so the linker only sees the subsystems the project
//! actually uses.
//!
//! Modules handled by perry-runtime alone (fs, path, os, buffer, perry/ui,
//! perry/i18n, etc. — see `perry_hir::requires_stdlib`) are skipped here
//! because they don't trigger stdlib linkage at all.

use std::collections::BTreeSet;

/// Dynamic stream dispatch must use the same provider as the routed wrapper:
/// the stdlib adapter installs for a routed `http` / `https`.
pub fn routed_stream_dispatch_features(module: &str) -> &'static [&'static str] {
    match module.strip_prefix("node:").unwrap_or(module) {
        "http" | "https" => &["external-http-client-pump"],
        _ => &[],
    }
}

/// Look up the perry-stdlib feature(s) required to support a single
/// imported native module. Returns an empty slice for modules that need
/// no optional stdlib feature (covered by always-on dependencies like
/// chrono / lru / decimal).
pub fn module_to_features(module: &str) -> &'static [&'static str] {
    let normalized = module.strip_prefix("node:").unwrap_or(module);
    match normalized {
        // ── HTTP server (Hyper) ───────────────────────────────────────
        // ── Web Streams API ──────────────────────────────────────────
        // Per-binding gate `bundled-streams` — stream APIs are provided by
        // perry-stdlib's canonical implementation.
        // `node:stream/web` imports are normalized to `stream/web` before
        // the resolver records the known-submodule key `stream_web`; both
        // spellings need the same feature for auto-optimized stdlib builds.
        "streams" | "stream/web" | "stream_web" => &["bundled-streams"],

        // ── Web Fetch compatibility surface ───────────────────────────
        // Node HTTP/HTTPS/HTTP2 are provided by perry-ext-http and need
        // no perry-stdlib feature. node-fetch still uses the legacy
        // umbrella for compatibility.
        "node-fetch" => &["http-client"],

        // ── WebSocket ─────────────────────────────────────────────────
        // perry-ext-ws, Perry's own WebSocket engine (the global
        // `WebSocket` client), under its Perry-owned well-known key. npm
        // `ws` has no arm: it compiles from source over `net` / `http`.
        // `bundled-ws` is what the flip loop keys `external-ws-pump` on.
        "perry/websocket" => &["bundled-ws"],

        // ── Raw TCP sockets (net.Socket) ──────────────────────────────
        // `upgradeToTLS` and the TLSSocket members live on net.Socket's
        // prototype only in programs that reach TLS (perry-ext-net's
        // `js_ext_net_tls_install`, pushed by run_pipeline when
        // `optimized_libs::reaches_tls` holds); a net-only program's link
        // keeps no rustls. Per-binding gate is `bundled-net` (v0.5.571) so
        // the well-known flip can route to perry-ext-net.
        "net" => &["bundled-net", "tls"],

        // ── TLS (tls.connect, socket.upgradeToTLS) ───────────────────
        "tls" => &["tls"],

        // ── Databases ─────────────────────────────────────────────────
        // pg / mysql2 need no perry-stdlib feature: the bundled copies were
        // deleted in turnloop P8 group H, and none of the database wrappers
        // exists any more (perry-ext-pg #10677, perry-ext-mysql2 #10680,
        // perry-ext-mongodb #11337). The real npm packages compile from
        // source over `net` / `tls`, so they need nothing here. `mongodb`
        // has no arm at all: it falls through to the default like any other
        // compiled-from-source package.
        "mysql2" | "mysql2/promise" => &[],
        "pg" => &[],
        "better-sqlite3" => &["database-sqlite"],
        // node:sqlite (#3183/#3184) shares the rusqlite-backed
        // `database-sqlite` feature with better-sqlite3 — DatabaseSync /
        // StatementSync route to the same `js_sqlite_*` runtime.
        "sqlite" | "bun:sqlite" => &["database-sqlite"],
        // tursodb (#424) lives in the external
        // `PerryTS/tursodb-bindings` repo (`bun add @perryts/tursodb`)
        // since v0.5.557 — perry's package.json `perry.nativeLibrary`
        // resolution path picks it up from `node_modules/`. No
        // perry-stdlib feature gate to manage.
        "tursodb" => &[],
        // iroh (#425) lives in the external `PerryTS/iroh-bindings`
        // repo (`bun add @perryts/iroh`) since v0.5.557 — same model
        // as tursodb above.
        "iroh" => &[],

        // ── Crypto ────────────────────────────────────────────────────
        // bcrypt split off into its own `bundled-bcrypt` feature in
        // v0.5.537 so the well-known flip can route it to
        // perry-ext-bcrypt without taking the rest of the crypto
        // surface offline. The `crypto` umbrella still includes
        // bundled-bcrypt for backwards compat — programs that import
        // bcrypt also typically use sha256/jwt/etc., which keeps the
        // umbrella worthwhile.
        "bcrypt" => &["bundled-bcrypt"],
        "crypto" => &["crypto"],
        // perry/updater's signature verification routes through
        // js_crypto_ed25519_verify in perry-stdlib::crypto, so importing
        // perry/updater pulls in the crypto feature transitively.
        "perry/updater" => &["crypto"],

        // ── Compression (zlib) ────────────────────────────────────────
        // Per-codec split (stdlib cherry-pick): a `node:zlib` import only
        // guarantees the gzip/deflate family (`compression-gzip`, which
        // gates `pub mod zlib` itself). The Brotli / zstd codec backends
        // (`compression-brotli` / `compression-zstd`, each implying
        // `compression-gzip`) are added by `build_optimized_libs` when
        // HIR usage detection saw a Brotli / zstd API token
        // (`ctx.uses_zlib_brotli` / `ctx.uses_zlib_zstd`, set in
        // `collect_modules/feature_detect.rs`) — and unconditionally
        // when deferred dynamic code could name a codec at runtime.
        // The `compression` umbrella (= all three) stays for
        // backwards-compat `--features compression` callers.
        "zlib" => &["compression-gzip"],

        // ── Image processing (sharp) ──────────────────────────────────
        // `image` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-sharp` (v0.5.551) so the
        // well-known flip can route to perry-ext-sharp.
        "sharp" => &["bundled-sharp"],

        // ── Scheduler (cron) ──────────────────────────────────────────
        // `scheduler` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-cron` (v0.5.564) so the
        // well-known flip can route to perry-ext-cron. `node-cron`'s own
        // ── argon2 ────────────────────────────────────────────────────
        // argon2 split off into `bundled-argon2` (v0.5.537) — same
        // reason as bcrypt above. Note: NATIVE_MODULES doesn't list
        // `argon2` in v0.5.532's manifest because perry-stdlib's
        // existing dispatch routes it through a different code path,
        // but the feature mapping is still useful for future parity.
        "argon2" => &["bundled-argon2"],

        // ── IDs (uuid / nanoid) ───────────────────────────────────────
        // No entries: the uuid binding (#10701) and the nanoid
        // binding (#10693) are gone, so `import "uuid"` /
        // `import "nanoid"` compile the real npm packages from
        // source and need no perry-stdlib feature. The `ids`
        // umbrella survives (empty) in perry-stdlib/Cargo.toml
        // for backwards compat only.

        // ── Container ─────────────────────────────────────────────────
        "perry/container" | "perry/container-compose" | "perry/compose" | "perry/workloads" => {
            &["container"]
        }

        // events: feature-gated v0.5.546 alongside perry-ffi's
        // GC-root-scanner surface that keeps EventEmitter
        // listener closures alive between .on() and .emit().
        "events" => &["bundled-events"],
        // readline (#347) — needs the promise bridge so the
        // event-loop pump tick drains its line / data / keypress
        // queues. Without it, `import readline` still compiles
        // (rl.close() fires synchronously) but live stdin events
        // won't propagate to user callbacks. The bridge is tokio-free
        // (`async-bridge`) since turnloop P8 lane L.
        "readline" => &["async-bridge"],

        // Modules with no optional perry-stdlib dependency (http, https,
        // http2, events, async_hooks, worker_threads, …) are provided by
        // external bindings or always-on runtime code.
        _ => &[],
    }
}

/// Compute the union of perry-stdlib features required to cover every
/// native module the project imports, plus features needed to satisfy
/// non-import-based usage flags (e.g. `uses_fetch` ⇒ `web-fetch`).
pub fn compute_required_features(
    native_module_imports: &BTreeSet<String>,
    uses_fetch: bool,
    uses_crypto_builtins: bool,
) -> BTreeSet<&'static str> {
    let mut features = BTreeSet::new();
    for module in native_module_imports {
        for feat in module_to_features(module) {
            features.insert(*feat);
        }
    }
    // Built-in `fetch()` / `node-fetch` and the WHATWG data types
    // (`Headers` / `Request` / `Response` / `Blob`) use the Web Fetch
    // feature directly, without enabling Axios.
    if uses_fetch {
        features.insert("web-fetch");
    }
    // Perry's bare `crypto.randomBytes` / `sha256` / etc. builtins bottom
    // out in the perry-stdlib `crypto` feature.
    if uses_crypto_builtins {
        features.insert("crypto");
    }
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zlib_import_maps_to_gzip_base_only() {
        // Per-codec cherry-pick: `node:zlib` selects the gzip/deflate base;
        // Brotli / zstd are layered on by `build_optimized_libs` from the
        // `ctx.uses_zlib_brotli` / `ctx.uses_zlib_zstd` HIR gates. The
        // legacy `compression` umbrella must NOT be selected here — it
        // would defeat the split by always pulling all three codecs.
        assert_eq!(module_to_features("zlib"), &["compression-gzip"]);
        assert_eq!(module_to_features("node:zlib"), &["compression-gzip"]);

        let mut imports = BTreeSet::new();
        imports.insert("zlib".to_string());
        let features = compute_required_features(&imports, false, false);
        assert!(features.contains("compression-gzip"));
        assert!(!features.contains("compression"));
        assert!(!features.contains("compression-brotli"));
        assert!(!features.contains("compression-zstd"));
    }

    #[test]
    fn crypto_features_join_only_on_crypto_usage() {
        // The stdlib cherry-pick removed the driver's unconditional
        // `crypto` force — the feature must now come exclusively from the
        // import mapping or the builtin-usage flag (plus the codegen
        // `js_crypto_*` prefix net, tested in perry-codegen).
        let no_imports = BTreeSet::new();
        let features = compute_required_features(&no_imports, false, false);
        assert!(!features.contains("crypto"));

        let features = compute_required_features(&no_imports, false, true);
        assert!(features.contains("crypto"));

        let mut imports = BTreeSet::new();
        imports.insert("crypto".to_string());
        let features = compute_required_features(&imports, false, false);
        assert!(features.contains("crypto"));
    }

    #[test]
    fn stream_web_imports_enable_bundled_streams() {
        assert_eq!(module_to_features("stream/web"), &["bundled-streams"]);
        assert_eq!(module_to_features("stream_web"), &["bundled-streams"]);
        assert!(module_to_features("fs/promises").is_empty());

        let mut imports = BTreeSet::new();
        imports.insert("stream_web".to_string());
        imports.insert("fs/promises".to_string());

        let features = compute_required_features(&imports, false, false);
        assert!(features.contains("bundled-streams"));
    }

    #[test]
    fn fs_promises_alone_does_not_install_web_streams() {
        let imports = BTreeSet::from(["fs/promises".to_string()]);
        assert!(!compute_required_features(&imports, false, false).contains("bundled-streams"));
    }

    #[test]
    fn node_http_uses_external_binding_without_legacy_client_feature() {
        assert!(module_to_features("http").is_empty());
        assert!(module_to_features("node:https").is_empty());
        assert!(module_to_features("http2").is_empty());
        assert_eq!(module_to_features("node-fetch"), &["http-client"]);
    }

    #[test]
    fn bundled_database_copies_map_to_no_stdlib_features() {
        // turnloop P8 group H deleted perry-stdlib's bundled pg / mysql2 /
        // mongodb modules. Naming a feature here would ask cargo
        // for a gate that no longer exists. Their wrappers are gone too
        // (#10677 / #10680 / #11337), so all three compile the npm package.
        for module in ["pg", "mysql2", "mysql2/promise", "mongodb", "node:mongodb"] {
            assert_eq!(
                module_to_features(module),
                &[] as &[&str],
                "{module} must select no perry-stdlib feature"
            );
        }
        // sqlite is NOT part of that set — rusqlite is not a tokio driver
        // and the bundled module stays.
        assert_eq!(module_to_features("better-sqlite3"), &["database-sqlite"]);
        assert_eq!(module_to_features("node:sqlite"), &["database-sqlite"]);
    }
}
