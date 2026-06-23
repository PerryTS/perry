//! Standard Library for Perry
//!
//! Feature-gated implementations of Node.js APIs and npm packages.
//! Only compile what you actually use for smaller binaries.
//!
//! # Features
//! - `core` - Minimal runtime (always included)
//! - `http-server` - Native HTTP server (hyper-based)
//! - `http-client` - HTTP client (reqwest/node-fetch)
//! - `database` - All databases (postgres, mysql, sqlite, redis, mongodb)
//! - `crypto` - Cryptographic functions
//! - `compression` - zlib compression
//! - `full` - Everything (default)

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
// commander — feature-gated as of v0.5.555 so the well-known flip
// can route `import { Command } from 'commander'` to
// perry-ext-commander without duplicate `_js_commander_*` symbols.
#[cfg(feature = "bundled-commander")]
pub mod commander;
pub mod common;
pub mod domain;
// dayjs / date-fns — feature-gated as of v0.5.548 so the well-known
// flip can route `import 'dayjs'` / `import 'date-fns'` to
// perry-ext-dayjs without duplicate `_js_dayjs_*` symbols at link.
#[cfg(feature = "bundled-dayjs")]
pub mod dayjs;
// decimal feature-gated as of v0.5.547 — well-known flip routes
// to perry-ext-decimal.
#[cfg(feature = "bundled-decimal")]
pub mod decimal;
// dotenv is feature-gated as of v0.5.533 so the well-known bindings
// table (#466 Phase 4) can route `import 'dotenv'` to perry-ext-dotenv
// without duplicate _js_dotenv_* symbols at link time. Default-on
// preserves byte-identical behavior for programs that don't opt into
// the well-known path.
#[cfg(feature = "bundled-dotenv")]
pub mod dotenv;
// events feature-gated as of v0.5.546 so the well-known flip
// can route to perry-ext-events.
#[cfg(feature = "bundled-events")]
pub mod events;
// exponential_backoff feature-gated as of v0.5.542 so the
// well-known flip can route to perry-ext-exponential-backoff.
#[cfg(feature = "bundled-exponential-backoff")]
pub mod exponential_backoff;
pub mod lodash;
// moment — feature-gated as of v0.5.549 so the well-known flip can
// route `import 'moment'` to perry-ext-moment without duplicate
// `_js_moment_*` symbols at link.
#[cfg(feature = "bundled-moment")]
pub mod moment;
// lru_cache is feature-gated as of v0.5.539 so the well-known
// flip can route `import 'lru-cache'` to perry-ext-lru-cache.
#[cfg(feature = "bundled-lru-cache")]
pub mod lru_cache;
pub mod readline;
// slugify is feature-gated as of v0.5.536 so the well-known bindings
// flip can route `import 'slugify'` to perry-ext-slugify cleanly.
// Default-on through `default = ["full"]`.
#[cfg(feature = "bundled-slugify")]
pub mod slugify;
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

// Re-export core
pub use async_local_storage::*;
#[cfg(feature = "bundled-commander")]
pub use commander::*;
pub use common::*;
#[cfg(feature = "bundled-dayjs")]
pub use dayjs::*;
#[cfg(feature = "bundled-decimal")]
pub use decimal::*;
pub use domain::*;
#[cfg(feature = "bundled-dotenv")]
pub use dotenv::*;
#[cfg(feature = "bundled-events")]
pub use events::*;
#[cfg(feature = "bundled-exponential-backoff")]
pub use exponential_backoff::*;
pub use lodash::*;
#[cfg(feature = "bundled-lru-cache")]
pub use lru_cache::*;
#[cfg(feature = "bundled-moment")]
pub use moment::*;
pub use querystring::*;
pub use readline::*;
#[cfg(feature = "bundled-slugify")]
pub use slugify::*;
pub use string_decoder::*;
pub use vm::*;
pub use worker_threads::*;

// === HTTP Server ===
#[cfg(feature = "http-server")]
pub mod framework;
#[cfg(feature = "http-server")]
pub use framework::*;

// === Fastify-Compatible Framework ===
// The in-stdlib fastify adapter was removed: `import 'fastify'` is served
// exclusively by the external `perry-ext-fastify` crate via the well-known
// flip (see `well_known_bindings.toml` + `optimized_libs.rs`). perry-stdlib's
// per-tick bridge into the external crate lives behind the
// `external-fastify-pump` feature (drained from `async_bridge`).

// === Web Fetch API (fetch / Headers / Request / Response / Blob) ===
// #5174: gated on `web-fetch`, NOT `http-client`. The Web Fetch surface
// (reqwest-backed `fetch()` + the WHATWG data types) is independent of
// the bundled node:http client below, so a program that only needs
// `new Headers()` while routing `node:http` to perry-ext-http keeps
// these without dragging in the colliding bundled http.rs symbols.
// `http-client = ["web-fetch"]`, so `--features http-client` still
// compiles all of this exactly as before.
#[cfg(feature = "web-fetch")]
pub mod fetch;
#[cfg(feature = "web-fetch")]
pub use fetch::*;
// Issue #1211: Blob/File constructors + object-URL helpers split out
// of fetch.rs to keep that file under the 2,000-line lint gate.
#[cfg(feature = "web-fetch")]
pub mod fetch_blob;
#[cfg(feature = "web-fetch")]
pub use fetch_blob::*;

// === Bundled node:http client (http.request / http.get / axios) ===
// Stays on `http-client`. The well-known flip strips `http-client`
// (keeping `web-fetch`) when `node:http` routes to perry-ext-http, so
// these modules — which export the same `js_http_*` symbols as
// perry-ext-http — are absent and can't collide (#5174).
#[cfg(feature = "http-client")]
pub mod http;
#[cfg(feature = "http-client")]
pub use http::*;

#[cfg(feature = "http-client")]
pub mod axios;
#[cfg(feature = "http-client")]
pub use axios::*;

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
#[cfg(all(feature = "tls", not(target_os = "ios"), not(target_os = "android")))]
pub mod tls;
#[cfg(all(feature = "tls", not(target_os = "ios"), not(target_os = "android")))]
pub use tls::*;

// === Databases ===
// pg lives behind `bundled-pg` (v0.5.566); mysql2 lives behind
// `bundled-mysql2` (v0.5.567). Either feature pulls in sqlx, so
// the modules' `#[cfg(any(...))]` covers both bundled gates plus
// the legacy `database-postgres`/`database-mysql` umbrellas (kept
// for backwards-compat).
#[cfg(any(feature = "bundled-pg", feature = "bundled-mysql2"))]
pub mod pg;
#[cfg(any(feature = "bundled-pg", feature = "bundled-mysql2"))]
pub use pg::connection::*;
#[cfg(any(feature = "bundled-pg", feature = "bundled-mysql2"))]
pub use pg::pool::*;

#[cfg(any(feature = "bundled-pg", feature = "bundled-mysql2"))]
pub mod mysql2;
#[cfg(any(feature = "bundled-pg", feature = "bundled-mysql2"))]
pub use mysql2::connection::*;
#[cfg(any(feature = "bundled-pg", feature = "bundled-mysql2"))]
pub use mysql2::pool::*;

#[cfg(feature = "database-sqlite")]
pub mod sqlite;
#[cfg(feature = "database-sqlite")]
pub use sqlite::*;

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

#[cfg(feature = "bundled-ioredis")]
pub mod ioredis;
#[cfg(feature = "bundled-ioredis")]
pub use ioredis::*;

#[cfg(feature = "bundled-mongodb")]
pub mod mongodb;
#[cfg(feature = "bundled-mongodb")]
pub use mongodb::*;

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

// jsonwebtoken split out into `bundled-jsonwebtoken` (v0.5.538)
// for the same reason as bcrypt/argon2 — well-known flip
// independence. The `crypto` umbrella still pulls it in for
// backwards compat.
#[cfg(feature = "bundled-jsonwebtoken")]
pub mod jsonwebtoken;
#[cfg(feature = "bundled-jsonwebtoken")]
pub use jsonwebtoken::*;

#[cfg(feature = "crypto")]
pub mod crypto_e2e;
#[cfg(feature = "crypto")]
pub use crypto_e2e::*;

// === Compression ===
#[cfg(feature = "compression")]
pub mod zlib;
#[cfg(feature = "compression")]
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
#[cfg(feature = "bundled-cron")]
pub mod cron;
#[cfg(feature = "bundled-cron")]
pub use cron::*;

// Unconditional cron timer stubs — always present so the CLI event loop in
// `module_init.rs` can call `js_cron_timer_tick` / `js_cron_timer_has_pending`
// even when the `scheduler` feature is disabled (e.g. an auto-optimized build
// of a project that imports `node:crypto` but not `node-cron`). With the
// scheduler feature ENABLED, these symbols are provided by `cron.rs` instead;
// the `#[cfg(not(feature = "scheduler"))]` gate below prevents a duplicate
// symbol error in that case.
#[cfg(not(feature = "scheduler"))]
#[no_mangle]
pub extern "C" fn js_cron_timer_tick() -> i32 {
    0
}
#[cfg(not(feature = "scheduler"))]
#[no_mangle]
pub extern "C" fn js_cron_timer_has_pending() -> i32 {
    0
}

// === Rate Limiting ===
#[cfg(feature = "bundled-ratelimit")]
pub mod ratelimit;
#[cfg(feature = "bundled-ratelimit")]
pub use ratelimit::*;

// === Validation ===
// `validation` umbrella now expands to `bundled-validator`
// (v0.5.538). Per-binding gate lets the well-known flip swap the
// validator wrapper out without affecting the rest of the
// validation surface (none — there's just the one wrapper today,
// but the split unblocks future additions).
#[cfg(feature = "bundled-validator")]
pub mod validator;
#[cfg(feature = "bundled-validator")]
pub use validator::*;

// === IDs ===
// `bundled-uuid` / `bundled-nanoid` (v0.5.534) replace the old
// `ids` umbrella so the well-known flip (#466 Phase 4) can toggle
// each binding independently. The umbrella stays as
// `ids = ["bundled-uuid", "bundled-nanoid"]` so existing
// `--features ids` callers keep working byte-identically.
#[cfg(feature = "bundled-uuid")]
pub mod uuid;
#[cfg(feature = "bundled-uuid")]
pub use uuid::*;

#[cfg(feature = "bundled-nanoid")]
pub mod nanoid;
#[cfg(feature = "bundled-nanoid")]
pub use nanoid::*;

// === Container Module ===
#[cfg(feature = "container")]
pub mod container;
#[cfg(feature = "container")]
pub use container::*;
