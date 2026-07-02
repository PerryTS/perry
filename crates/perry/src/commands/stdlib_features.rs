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

/// Look up the perry-stdlib feature(s) required to support a single
/// imported native module. Returns an empty slice for modules that need
/// no optional stdlib feature (covered by always-on dependencies like
/// chrono / lru / decimal).
pub fn module_to_features(module: &str) -> &'static [&'static str] {
    let normalized = module.strip_prefix("node:").unwrap_or(module);
    match normalized {
        // ── HTTP server (Hyper) ───────────────────────────────────────
        // fastify needs no perry-stdlib feature: the in-stdlib adapter was
        // removed, so `import 'fastify'` is served entirely by the external
        // perry-ext-fastify crate via the well-known flip (which links the
        // wrapper and enables `external-fastify-pump`). See optimized_libs.rs.
        "fastify" => &[],

        // ── Web Streams API ──────────────────────────────────────────
        // Per-binding gate `bundled-streams` (v0.5.572) — the
        // well-known flip routes `import 'streams'` to perry-ext-streams.
        // `node:stream/web` imports are normalized to `stream/web` before
        // the resolver records the known-submodule key `stream_web`; both
        // spellings need the same feature for auto-optimized stdlib builds.
        "streams" | "stream/web" | "stream_web" | "fs/promises" => &["bundled-streams"],

        // ── HTTP client (reqwest) ─────────────────────────────────────
        // `http` / `https` / `http2` join the `http-client` umbrella since
        // they bottom out in reqwest just like axios + node-fetch — and
        // perry-ext-http-server (issue #577) needs the same async-runtime
        // bridge for `perry_ffi_spawn_blocking_with_reactor`. The
        // well-known flip swaps perry-stdlib's http.rs for perry-ext-http
        // (v0.5.571); `http2` flips to the same staticlib via the rlib
        // dep on perry-ext-http-server. Programs that import `streams`
        // should NOT also use the well-known flip — streams stays in
        // perry-stdlib until its own port lands.
        "axios" | "node-fetch" | "http" | "https" | "http2" => &["http-client"],

        // ── WebSocket ─────────────────────────────────────────────────
        // `websocket` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-ws` (v0.5.571) so the
        // well-known flip can route to perry-ext-ws.
        "ws" => &["bundled-ws"],

        // ── Raw TCP sockets (net.Socket) ──────────────────────────────
        // `upgradeToTLS` is a method on net.Socket, so any program using
        // `net` must link the TLS runtime too — otherwise `sock.upgradeToTLS`
        // fails at link time with `_js_net_socket_upgrade_tls` undefined.
        // The binary-size cost is small; programs that explicitly want
        // zero TLS bytes can still opt in via the lower-level feature flags.
        // Per-binding gate is `bundled-net` (v0.5.571) so the well-known
        // flip can route to perry-ext-net.
        "net" => &["bundled-net", "tls"],

        // ── TLS (tls.connect, socket.upgradeToTLS) ───────────────────
        "tls" => &["tls"],

        // ── Databases ─────────────────────────────────────────────────
        // `database-mysql` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-mysql2` (v0.5.567).
        "mysql2" | "mysql2/promise" => &["bundled-mysql2"],
        // `database-postgres` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-pg` (v0.5.566) so the
        // well-known flip can route to perry-ext-pg.
        "pg" => &["bundled-pg"],
        "better-sqlite3" => &["database-sqlite"],
        // node:sqlite (#3183/#3184) shares the rusqlite-backed
        // `database-sqlite` feature with better-sqlite3 — DatabaseSync /
        // StatementSync route to the same `js_sqlite_*` runtime.
        "sqlite" => &["database-sqlite"],
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
        // Redis is detected via the ioredis class name in collect_modules,
        // but if it shows up as an explicit import we still need the feature.
        // `database-redis` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-ioredis` (v0.5.565) so the
        // well-known flip can route to perry-ext-ioredis.
        "ioredis" | "redis" => &["bundled-ioredis"],
        // `database-mongodb` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-mongodb` (v0.5.568) so the
        // well-known flip can route to perry-ext-mongodb.
        "mongodb" => &["bundled-mongodb"],

        // ── Crypto ────────────────────────────────────────────────────
        // bcrypt split off into its own `bundled-bcrypt` feature in
        // v0.5.537 so the well-known flip can route it to
        // perry-ext-bcrypt without taking the rest of the crypto
        // surface offline. The `crypto` umbrella still includes
        // bundled-bcrypt for backwards compat — programs that import
        // bcrypt also typically use sha256/jwt/etc., which keeps the
        // umbrella worthwhile.
        "bcrypt" => &["bundled-bcrypt"],
        "jsonwebtoken" => &["bundled-jsonwebtoken"],
        "crypto" => &["crypto"],
        // ethers ships utility functions (formatUnits, parseUnits,
        // getAddress, keccak256, …). The keccak256 implementation is
        // hand-rolled inside `bundled-ethers`, so we no longer need
        // the broader `crypto` umbrella to satisfy ethers imports —
        // the well-known flip routes ethers calls to
        // perry-ext-ethers and strips the perry-stdlib copy.
        "ethers" => &["bundled-ethers"],
        // perry/updater's signature verification routes through
        // js_crypto_ed25519_verify in perry-stdlib::crypto, so importing
        // perry/updater pulls in the crypto feature transitively.
        "perry/updater" => &["crypto"],

        // ── Compression (zlib) ────────────────────────────────────────
        "zlib" => &["compression"],

        // ── Email (lettre) ────────────────────────────────────────────
        // `email` umbrella retained for backwards-compat; per-binding
        // gate is `bundled-nodemailer` (v0.5.558).
        "nodemailer" => &["bundled-nodemailer"],

        // ── Image processing (sharp) ──────────────────────────────────
        // `image` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-sharp` (v0.5.551) so the
        // well-known flip can route to perry-ext-sharp.
        "sharp" => &["bundled-sharp"],

        // ── HTML parsing (cheerio / scraper) ──────────────────────────
        // `html-parser` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-cheerio` (v0.5.550) so the
        // well-known flip can route to perry-ext-cheerio.
        "cheerio" => &["bundled-cheerio"],

        // ── Scheduler (cron) ──────────────────────────────────────────
        // `scheduler` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-cron` (v0.5.564) so the
        // well-known flip can route to perry-ext-cron.
        "cron" | "node-cron" => &["bundled-cron"],

        // ── Validation (validator.js) ─────────────────────────────────
        // `validation` umbrella retained for backwards-compat;
        // per-binding gate is `bundled-validator` (v0.5.538).
        "validator" => &["bundled-validator"],

        // ── argon2 ────────────────────────────────────────────────────
        // argon2 split off into `bundled-argon2` (v0.5.537) — same
        // reason as bcrypt above. Note: NATIVE_MODULES doesn't list
        // `argon2` in v0.5.532's manifest because perry-stdlib's
        // existing dispatch routes it through a different code path,
        // but the feature mapping is still useful for future parity.
        "argon2" => &["bundled-argon2"],

        // ── IDs (uuid / nanoid) ───────────────────────────────────────
        // Per-binding split as of v0.5.534 (#466 Phase 4 step 2)
        // so the well-known flip can swap each one out
        // independently. The `ids` umbrella stays in
        // perry-stdlib/Cargo.toml as `bundled-uuid + bundled-nanoid`
        // for backwards compat, but feature-set computation goes
        // straight to the per-binding feature.
        "uuid" => &["bundled-uuid"],
        "nanoid" => &["bundled-nanoid"],

        // ── Container ─────────────────────────────────────────────────
        "perry/container" | "perry/container-compose" | "perry/compose" | "perry/workloads" => {
            &["container"]
        }

        // Slugify gained the `bundled-slugify` feature in v0.5.536 so
        // the well-known flip can swap it out for perry-ext-slugify.
        // Default-on via `default = ["full"]` keeps existing
        // `import 'slugify'` calls byte-identical.
        "slugify" => &["bundled-slugify"],
        // lru-cache: feature-gated v0.5.539; well-known flip
        // routes to perry-ext-lru-cache.
        "lru-cache" => &["bundled-lru-cache"],
        // exponential-backoff: feature-gated v0.5.542 alongside
        // the perry-ffi closure-invocation surface that powers
        // its `backOff(fn)` retry loop.
        "exponential-backoff" => &["bundled-exponential-backoff"],
        // events: feature-gated v0.5.546 alongside perry-ffi's
        // GC-root-scanner surface that keeps EventEmitter
        // listener closures alive between .on() and .emit().
        "events" => &["bundled-events"],
        // decimal.js / bignumber.js: feature-gated v0.5.547 —
        // well-known flip routes to perry-ext-decimal.
        "decimal.js" | "bignumber.js" => &["bundled-decimal"],
        // dayjs / date-fns: feature-gated v0.5.548 — well-known
        // flip routes to perry-ext-dayjs.
        "dayjs" | "date-fns" => &["bundled-dayjs"],
        // moment: feature-gated v0.5.549 — well-known flip routes
        // to perry-ext-moment.
        "moment" => &["bundled-moment"],
        // rate-limiter-flexible: feature-gated v0.5.552 — well-known
        // flip routes to perry-ext-ratelimit.
        "rate-limiter-flexible" => &["bundled-ratelimit"],
        // commander: feature-gated v0.5.555 — well-known flip routes
        // to perry-ext-commander.
        "commander" => &["bundled-commander"],
        // dotenv was always-on through v0.5.532; gated behind
        // `bundled-dotenv` from v0.5.533 onwards so the well-known
        // bindings flip (#466 Phase 4 step 2) can swap perry-stdlib's
        // copy out for `perry-ext-dotenv` without duplicate
        // `_js_dotenv_*` symbols at link time. The well-known path
        // strips this feature from the set; the default path leaves
        // it on so byte-identical behavior is preserved.
        "dotenv" | "dotenv/config" => &["bundled-dotenv"],

        // readline (#347) — needs the async-runtime feature so the
        // event-loop pump tick drains its line / data / keypress
        // queues. Without async-runtime, `import readline` still
        // compiles (rl.close() fires synchronously) but live stdin
        // events won't propagate to user callbacks.
        "readline" => &["async-runtime"],

        // Modules with no optional perry-stdlib dependency (decimal.js,
        // bignumber.js, lru-cache, commander, exponential-backoff, http,
        // https, events, async_hooks, worker_threads, …) — handled by
        // always-on stdlib code.
        _ => &[],
    }
}

/// Compute the union of perry-stdlib features required to cover every
/// native module the project imports, plus features needed to satisfy
/// non-import-based usage flags (e.g. `uses_fetch` ⇒ `http-client`).
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
    // (`Headers` / `Request` / `Response` / `Blob`) bottom out in reqwest
    // but do NOT need perry-stdlib's bundled node:http client. #5174: ask
    // for `web-fetch` (just `src/fetch/` + `src/fetch_blob.rs`), not the
    // `http-client` umbrella that also pulls in `src/http.rs` / `src/axios.rs`.
    // When the program ALSO imports `node:http`, that import adds
    // `http-client` separately and the well-known flip strips it down to
    // `web-fetch` — so the bundled client never collides with perry-ext-http.
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

/// Render a feature set as the comma-separated string Cargo expects on
/// `--features`.
pub fn features_to_cargo_arg(features: &BTreeSet<&'static str>) -> String {
    features.iter().copied().collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_web_imports_enable_bundled_streams() {
        assert_eq!(module_to_features("stream/web"), &["bundled-streams"]);
        assert_eq!(module_to_features("stream_web"), &["bundled-streams"]);
        assert_eq!(module_to_features("fs/promises"), &["bundled-streams"]);

        let mut imports = BTreeSet::new();
        imports.insert("stream_web".to_string());
        imports.insert("fs/promises".to_string());

        let features = compute_required_features(&imports, false, false);
        assert!(features.contains("bundled-streams"));
    }
}
