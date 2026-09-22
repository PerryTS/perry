//! Standard Library for Perry
//!
//! Feature-gated implementations of Node.js APIs and npm packages.
//! Only compile what you actually use for smaller binaries.
//!
//! # Features
//! - `core` - Minimal runtime (always included)
//! - `http-server` - Native HTTP server (hyper-based)
//! - `http-client` - Web Fetch and Axios compatibility surface
//! - `database` - In-stdlib databases (sqlite only; postgres/mysql/redis/mongodb
//!   are served by the perry-ext-* wrappers)
//! - `http-client` - Web Fetch compatibility surface
//! - `database` - All databases (postgres, mysql, sqlite, redis, mongodb)
//! - `crypto` - Cryptographic functions
//! - `compression` - zlib compression
//! - `full` - Everything (default)

// Anchors are `#[used(compiler)]`: retained by rustc, not ld64 dead-strip roots.
#![feature(used_with_arg)]
// Re-export the updater crate so its #[no_mangle] FFI symbols are
// retained in libperry_stdlib.a (Cargo would otherwise drop unused
// rlib deps during the staticlib bundle step).
pub use perry_updater;

// `extern "C"` shims that perry-ffi declares for use by external
// native binding crates (#466 Phase 1 + 5 — async surface). Gated
// on `async-runtime` because the underlying async_bridge does;
// every wrapper that depends on these (bcrypt, argon2, ws, db
// drivers, …) already triggers `async-runtime` through its own
// per-binding feature, so the linkage is automatic.
#[cfg(feature = "async-runtime")]
pub mod perry_ffi_async;

// Core modules - always available
pub mod async_local_storage;
pub mod common;
pub mod domain;
// decimal feature-gated as of v0.5.547 — well-known flip routes
// to perry-ext-decimal.
#[cfg(feature = "bundled-decimal")]
pub mod decimal;
// dotenv is feature-gated as of v0.5.533 so the well-known bindings
// table (#466 Phase 4) can route `import 'dotenv'` to perry-ext-dotenv
// without duplicate _js_dotenv_* symbols at link time. Default-on
// preserves byte-identical behavior for programs that don't opt into
// the well-known path.
// events feature-gated as of v0.5.546 so the well-known flip
// can route to perry-ext-events.
#[cfg(feature = "bundled-events")]
pub mod events;
pub mod lodash;
pub mod readline;
// string_decoder — issue #848. Native StringDecoder with real `write` /
// `end` methods + `lastNeed` / `lastTotal` / `lastChar` getters wired
// through HANDLE_METHOD_DISPATCH / HANDLE_PROPERTY_DISPATCH.
pub mod string_decoder;
// querystring — node:querystring legacy URL-encoded form parser.
// Greenfield implementation (Node ships it deprecated since v11 but
// many npm packages still import it).
pub mod querystring;
// vm — node:vm import/require shape plus narrowed local execution helpers.
// The wrappers here retain direct-call FFI symbols.
pub mod vm;
pub mod worker_threads;

// Binary-safe multipart parsing is shared by the independent HTTP-server and
// Web Fetch feature families. Keep the core outside `framework`, which is not
// compiled for a minimal `web-fetch` build.
#[cfg(any(feature = "http-server", feature = "web-fetch"))]
mod multipart_parser;

// Re-export core
pub use async_local_storage::*;
pub use common::*;
#[cfg(feature = "bundled-decimal")]
pub use decimal::*;
pub use domain::*;
#[cfg(feature = "bundled-events")]
pub use events::*;
pub use lodash::*;
pub use querystring::*;
pub use readline::*;
pub use string_decoder::*;
pub use vm::*;
pub use worker_threads::*;

// === HTTP Server ===
#[cfg(feature = "http-server")]
pub mod framework;
#[cfg(feature = "http-server")]
pub use framework::*;

// === Fastify ===
// The npm `fastify` binding was removed (#466): `import 'fastify'` now
// compiles the real npm package from source, same as any other package
// under the wildcard resolution.

// === turnloop P6: the shared client TLS session ===
// Driven by both outbound engines below (`turnloop_client`, `turnloop_smtp`).
#[cfg(any(feature = "turnloop-http-client", feature = "turnloop-smtp-client"))]
pub(crate) mod turnloop_tls_client;

// === turnloop P6: SMTP on turnloop handles ===
// `turnloop-smtp`'s sans-I/O `Connection` over a turnloop socket. Gated on its
// own feature rather than `bundled-nodemailer` so the `js_smtp_*` entry points
// survive the well-known flip that strips the bundled surface — that is how
// perry-ext-nodemailer reaches this engine.
#[cfg(feature = "turnloop-smtp-client")]
pub mod turnloop_smtp;

// === turnloop P6: outbound HTTP/1.1 on turnloop handles ===
// The transport `fetch` and `axios` take whenever this agent has a loop —
// directly when this thread owns it, and through turnloop P10's `agent_post`
// when another thread of the same agent does. The reqwest client stays beside
// it for what the engine declines: an undrivable proxy, a URL the fetch policy
// layer rejects, and a genuine absence of an agent loop (the
// `tokio-wait-driver` A/B arm, or a host where `Loop::new` failed). See
// `turnloop_client`'s module note.
#[cfg(feature = "turnloop-http-client")]
pub mod turnloop_client;

// === Web Fetch API (fetch / Headers / Request / Response / Blob) ===
// #5174: gated on `web-fetch`, not `http-client`, so Web Fetch stays
// independent from the external node:http implementation.
#[cfg(feature = "web-fetch")]
pub mod fetch;

// #9719: `perry-ext-http`'s `server/bun_server.rs` declares these two bridge
// symbols in an UNCONDITIONAL `extern "C"` block (only its `#[cfg(test)]` arm
// stubs them), but their definitions live in `fetch::bun_server_bridge`, which
// this crate compiles only under `web-fetch`. The auto-optimize feature set for
// a `node:http` program does not include `web-fetch`, so a cold link of any
// program importing `node:http` failed with
// `Undefined symbols: _js_bun_http_response_snapshot_json`. CI never saw it:
// plain `cargo build` / `cargo test` do not link the auto-optimize ext-http
// path, and any warm `target/perry-auto-*` cache hides it from a manual check.
//
// The definitions genuinely need Fetch machinery (`FETCH_RESPONSES`,
// `consume_response_body`), so they cannot simply move out; and making the http
// features depend on `web-fetch` would link `reqwest` — an HTTP *client* — into
// every `node:http` *server* build. Instead the symbols always exist, and
// without Web Fetch they answer "nothing to bridge", which is exactly right:
// with no `fetch` module there are no `Response` objects to snapshot.
#[cfg(not(feature = "web-fetch"))]
mod bun_server_bridge_absent {
    use perry_runtime::string::StringHeader;

    /// No Web Fetch in this build, so no `Response` registry to snapshot from.
    /// Matches the real symbol's own not-found return (`null_mut`).
    #[no_mangle]
    pub extern "C" fn js_bun_http_response_snapshot_json(
        _response_handle: f64,
    ) -> *mut StringHeader {
        std::ptr::null_mut()
    }

    /// Mirrors the real bridge's failure return for an unusable snapshot.
    #[no_mangle]
    pub unsafe extern "C" fn js_bun_http_request_from_json(
        _snapshot_ptr: *const StringHeader,
    ) -> f64 {
        f64::from_bits(perry_runtime::value::JSValue::undefined().bits())
    }
}
#[cfg(feature = "web-fetch")]
pub use fetch::*;
// Issue #1211: Blob/File constructors + object-URL helpers split out
// of fetch.rs to keep that file under the 2,000-line lint gate.
#[cfg(feature = "web-fetch")]
pub mod fetch_blob;
#[cfg(feature = "web-fetch")]
pub use fetch_blob::*;

// === Web Streams API (issue #237) ===
// Per-binding gate (v0.5.572): `bundled-streams` is the only flag
// that toggles `pub mod streams`. The well-known flip strips
// `bundled-streams` to route to perry-ext-streams without
// duplicate-symbol risk. `http-client` callers still get streams
// transitively because the umbrella pulls in `bundled-streams`
// (declared in this crate's Cargo.toml). Default-on through
// `default = ["full"]` and through `--features http-client`,
// matching v0.5.571's behaviour byte-for-byte.
#[cfg(feature = "bundled-streams")]
pub mod streams;
#[cfg(feature = "bundled-streams")]
pub use streams::*;

// === WebSocket ===
#[cfg(feature = "bundled-ws")]
pub mod ws;
#[cfg(feature = "bundled-ws")]
pub use ws::*;

// === Raw TCP sockets (net.Socket) + TLS (tls.connect, socket.upgradeToTLS) ===
// Desktop only; iOS/Android stdlib are stubs for now.
#[cfg(all(
    feature = "bundled-net",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub mod net;
#[cfg(all(
    feature = "bundled-net",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub use net::*;
#[cfg(all(
    feature = "tls-runtime",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub mod tls;
#[cfg(all(
    feature = "tls-runtime",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub use tls::*;

// === Databases ===
// The bundled `pg` / `mysql2` / `ioredis` / `mongodb` modules were deleted in
// turnloop P8 group H. `import 'pg'` / `'mysql2'` / `'ioredis'` / `'redis'` /
// `'iovalkey'` / `'mongodb'` are served exclusively by the perry-ext-*
// wrappers through the well-known flip, which is the only path they have taken
// since v0.5.565-568; each wrapper defines a strict superset of the symbols the
// bundled copy did. Only sqlite remains in-stdlib.
// Both in-tree database wrappers that lived here are gone: the `pg`
// module + `bundled-pg` feature (#10677) and the `mysql2` module +
// `bundled-mysql2` feature (#10680), the pre-#466 native
// reimplementations of the `pg` and `mysql2` npm packages. Perry now
// compiles both real packages from source instead of shipping bundled
// reimplementations.

#[cfg(feature = "database-sqlite")]
pub mod sqlite;
#[cfg(feature = "database-sqlite")]
pub use sqlite::*;

// Bun's unified SQL tag currently delegates to the SQLite adapter.  Keep the
// module beside the database feature so minimal-stdlib builds that import
// `"bun"` pull in both the constructor symbol and its rusqlite backend.
#[cfg(feature = "database-sqlite")]
pub mod bun_sql;
#[cfg(feature = "database-sqlite")]
pub use bun_sql::*;

// Unconditional sqlite-handle existence shims — referenced by
// `perry-jsruntime::bridge` to decide whether a small-handle pointer
// crossing the native→V8 boundary is a SqliteDbHandle / SqliteStmtHandle
// (drizzle's BetterSQLiteSession reads `this.client.prepare(...)` from
// session.js; without a real proxy object the call lands on `null`).
// Defined here as 0-returning stubs when the `database-sqlite` feature
// is OFF so the bridge's extern declarations always link. When the
// feature is ON, `sqlite::js_sqlite_is_*_handle` are the real impls
// (this stub is `#[cfg(not(...))]`'d out to avoid duplicate symbols).
// Refs #1022.
#[cfg(not(feature = "database-sqlite"))]
#[no_mangle]
pub extern "C" fn js_sqlite_is_db_handle(_handle: i64) -> i32 {
    0
}
#[cfg(not(feature = "database-sqlite"))]
#[no_mangle]
pub extern "C" fn js_sqlite_is_stmt_handle(_handle: i64) -> i32 {
    0
}

// === Crypto ===
#[cfg(feature = "crypto")]
pub mod crypto;
#[cfg(feature = "crypto")]
pub use crypto::*;

// Web Crypto: crypto.subtle.{digest,importKey,sign,verify} — issue #561.
// Lives alongside the Node `crypto` module since both share the same
// SHA / HMAC primitives. Always built when `crypto` is on (no
// well-known flip; the surface is small enough to bundle directly).
#[cfg(feature = "crypto")]
pub mod webcrypto;
#[cfg(feature = "crypto")]
pub use webcrypto::*;

// === Ethers (blockchain utilities) ===
// Feature-gated as of v0.5.556 so the well-known flip can route
// `import { parseUnits } from 'ethers'` to perry-ext-ethers.
// Default-on through `crypto` (which is on by default) so
// existing programs keep their byte-identical behavior.
#[cfg(feature = "bundled-ethers")]
pub mod ethers;
#[cfg(feature = "bundled-ethers")]
pub use ethers::*;

// bcrypt + argon2 split out from the broad `crypto` feature in
// v0.5.537 so the well-known flip can swap each one out
// individually. The `crypto` umbrella still pulls them both in
// (`crypto = [..., "bundled-bcrypt", "bundled-argon2"]`) so legacy
// `--features crypto` builds keep producing byte-identical archives.
#[cfg(feature = "bundled-bcrypt")]
pub mod bcrypt;
#[cfg(feature = "bundled-bcrypt")]
pub use bcrypt::*;

#[cfg(feature = "bundled-argon2")]
pub mod argon2;
#[cfg(feature = "bundled-argon2")]
pub use argon2::*;

// for the same reason as bcrypt/argon2 — well-known flip
// independence. The `crypto` umbrella still pulls it in for
// backwards compat.

#[cfg(feature = "crypto")]
pub mod crypto_e2e;
#[cfg(feature = "crypto")]
pub use crypto_e2e::*;

// === Compression ===
// Gated on `compression-gzip` (the base codec family) rather than the
// `compression` umbrella so the auto-optimize rebuild can cherry-pick
// codecs: `compression-brotli` / `compression-zstd` imply
// `compression-gzip`, and `compression` is the union of all three.
#[cfg(feature = "compression-gzip")]
pub mod zlib;
#[cfg(feature = "compression-gzip")]
pub use zlib::*;

// === Email ===
#[cfg(feature = "bundled-nodemailer")]
pub mod nodemailer;
#[cfg(feature = "bundled-nodemailer")]
pub use nodemailer::*;

// === Image Processing ===
#[cfg(feature = "bundled-sharp")]
pub mod sharp;
#[cfg(feature = "bundled-sharp")]
pub use sharp::*;

// === HTML Parsing ===
#[cfg(feature = "bundled-cheerio")]
pub mod cheerio;
#[cfg(feature = "bundled-cheerio")]
pub use cheerio::*;

// === Scheduler ===
// The native `cron` binding (perry-ext-cron / perry-stdlib's own
// `cron.rs`) was removed — real `cron` npm source compiles via
// `perry.compilePackages` instead. These two symbols stay unconditional:
// the CLI event loop in `module_init.rs` calls `js_cron_timer_tick` /
// `js_cron_timer_has_pending` every iteration regardless of whether a
// program uses cron at all, so they must always resolve to something —
// now always this 0-returning stub.
#[no_mangle]
pub extern "C" fn js_cron_timer_tick() -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_timer_has_pending() -> i32 {
    0
}

// === Rate Limiting ===

// === IDs ===
// Nothing left to gate: `bundled-uuid` went with the uuid binding
// (#10701) and `bundled-nanoid` with the nanoid binding (#10693);
// real `uuid` / `nanoid` now compile from npm source. The `ids`
// umbrella stays (empty) in Cargo.toml so existing
// `--features ids` callers keep working.

// === Container Module ===
#[cfg(feature = "container")]
pub mod container;
#[cfg(feature = "container")]
pub use container::*;
