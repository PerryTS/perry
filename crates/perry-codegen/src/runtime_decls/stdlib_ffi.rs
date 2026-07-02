//! Stdlib / FFI runtime function declarations (extracted from runtime_decls.rs).
//!
//! The body of `declare_stdlib_ffi` was a single ~2000-line function; it has
//! been split into topical sibling modules (under `stdlib_ffi/`), each exposing
//! one `declare_*` helper. The trunk just calls them in the original order so
//! the emitted declarations are byte-for-byte identical.

use super::*;

mod data_stores;
mod language_core;
mod net_http;
mod streams_events;
mod third_party;
mod utilities;
mod web;

use data_stores::declare_data_stores;
use language_core::declare_core;
use net_http::declare_net_http;
use streams_events::declare_streams_events;
use third_party::declare_third_party;
use utilities::declare_utilities;
use web::declare_web;

/// Stdlib / FFI runtime functions. Without these declarations, user code
/// that touches any of the third-party stdlib modules (http, mysql2, pg,
/// redis, mongodb, bcrypt, jsonwebtoken, axios, sharp, cron, WebSocket,
/// zlib, etc.) emits `use of undefined value '@js_*'` at clang -c time
/// because the IR references the name without a preceding `declare`.
///
/// Signatures cross-checked against `crates/perry-runtime/src/` and
/// `crates/perry-stdlib/src/`.
pub fn declare_stdlib_ffi(module: &mut LlModule) {
    // node:vm/repl/worker_threads + HTTP/HTTPS/HTTP2 client, server, agents.
    declare_net_http(module);
    // PostgreSQL, Redis/ioredis, MongoDB, SQLite, OS, Crypto, Nanoid.
    declare_data_stores(module);
    // bcrypt/argon2, perry/ads, perry/thread, JWT, axios, sharp, cron,
    // async_hooks/AsyncLocalStorage, DisposableStack, zlib, Buffer,
    // child_process, cheerio.
    declare_third_party(module);
    // URL / URLSearchParams + WebSocket.
    declare_web(module);
    // @perryts/pdf, commander, dotenv, date libs, decimal.js, ethers, lodash,
    // lru-cache.
    declare_utilities(module);
    // node:stream, EventEmitter, domain, StringDecoder, querystring, fastify,
    // nodemailer, rate-limit, validator.
    declare_streams_events(module);
    // Date, String, Object, Math, Atomics, Number, JSON, Map/Set, Error,
    // Promise, text encoding, closures, NaN-boxing, GC, console, fetch, net,
    // performance, async-step, slugify, class registration, runtime init/
    // module-loader, well-known Symbol hooks, Object.groupBy, JSX adapter.
    declare_core(module);
}
