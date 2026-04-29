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

// Core modules - always available
pub mod async_local_storage;
pub mod commander;
pub mod common;
pub mod dayjs;
pub mod decimal;
pub mod dotenv;
pub mod events;
pub mod exponential_backoff;
pub mod lodash;
pub mod lru_cache;
pub mod moment;
pub mod slugify;
pub mod worker_threads;

// Re-export core
pub use async_local_storage::*;
pub use commander::*;
pub use common::*;
pub use dayjs::*;
pub use decimal::*;
pub use dotenv::*;
pub use events::*;
pub use exponential_backoff::*;
pub use lodash::*;
pub use lru_cache::*;
pub use moment::*;
pub use slugify::*;
pub use worker_threads::*;

// === HTTP Server ===
#[cfg(feature = "http-server")]
pub mod framework;
#[cfg(feature = "http-server")]
pub use framework::*;

// === Fastify-Compatible Framework ===
#[cfg(feature = "http-server")]
pub mod fastify;
#[cfg(feature = "http-server")]
pub use fastify::*;

// === HTTP Client ===
#[cfg(feature = "http-client")]
pub mod fetch;
#[cfg(feature = "http-client")]
pub use fetch::*;

#[cfg(feature = "http-client")]
pub mod http;
#[cfg(feature = "http-client")]
pub use http::*;

#[cfg(feature = "http-client")]
pub mod axios;
#[cfg(feature = "http-client")]
pub use axios::*;

// === WebSocket ===
#[cfg(feature = "websocket")]
pub mod ws;
#[cfg(feature = "websocket")]
pub use ws::*;

// === Raw TCP sockets (net.Socket) + TLS (tls.connect, socket.upgradeToTLS) ===
// Desktop only; iOS/Android stdlib are stubs for now.
#[cfg(all(feature = "net", not(target_os = "ios"), not(target_os = "android")))]
pub mod net;
#[cfg(all(feature = "net", not(target_os = "ios"), not(target_os = "android")))]
pub use net::*;

// === Databases ===
#[cfg(any(feature = "database-postgres", feature = "database-mysql"))]
pub mod pg;
#[cfg(any(feature = "database-postgres", feature = "database-mysql"))]
pub use pg::connection::*;
#[cfg(any(feature = "database-postgres", feature = "database-mysql"))]
pub use pg::pool::*;

#[cfg(any(feature = "database-postgres", feature = "database-mysql"))]
pub mod mysql2;
#[cfg(any(feature = "database-postgres", feature = "database-mysql"))]
pub use mysql2::connection::*;
#[cfg(any(feature = "database-postgres", feature = "database-mysql"))]
pub use mysql2::pool::*;

#[cfg(feature = "database-sqlite")]
pub mod sqlite;
#[cfg(feature = "database-sqlite")]
pub use sqlite::*;

#[cfg(feature = "database-redis")]
pub mod ioredis;
#[cfg(feature = "database-redis")]
pub use ioredis::*;

#[cfg(feature = "database-mongodb")]
pub mod mongodb;
#[cfg(feature = "database-mongodb")]
pub use mongodb::*;

// === Crypto ===
#[cfg(feature = "crypto")]
pub mod crypto;
#[cfg(feature = "crypto")]
pub use crypto::*;

// === Ethers (blockchain utilities) ===
#[cfg(feature = "crypto")]
pub mod ethers;
#[cfg(feature = "crypto")]
pub use ethers::*;

#[cfg(feature = "crypto")]
pub mod bcrypt;
#[cfg(feature = "crypto")]
pub use bcrypt::*;

#[cfg(feature = "crypto")]
pub mod argon2;
#[cfg(feature = "crypto")]
pub use argon2::*;

#[cfg(feature = "crypto")]
pub mod jsonwebtoken;
#[cfg(feature = "crypto")]
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
#[cfg(feature = "email")]
pub mod nodemailer;
#[cfg(feature = "email")]
pub use nodemailer::*;

// === Image Processing ===
#[cfg(feature = "image")]
pub mod sharp;
#[cfg(feature = "image")]
pub use sharp::*;

// === HTML Parsing ===
#[cfg(feature = "html-parser")]
pub mod cheerio;
#[cfg(feature = "html-parser")]
pub use cheerio::*;

// === Scheduler ===
#[cfg(feature = "scheduler")]
pub mod cron;
#[cfg(feature = "scheduler")]
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
#[cfg(feature = "rate-limit")]
pub mod ratelimit;
#[cfg(feature = "rate-limit")]
pub use ratelimit::*;

// === Validation ===
#[cfg(feature = "validation")]
pub mod validator;
#[cfg(feature = "validation")]
pub use validator::*;

// === IDs ===
#[cfg(feature = "ids")]
pub mod uuid;
#[cfg(feature = "ids")]
pub use uuid::*;

#[cfg(feature = "ids")]
pub mod nanoid;
#[cfg(feature = "ids")]
pub use nanoid::*;
