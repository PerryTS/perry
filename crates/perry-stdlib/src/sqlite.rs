//! SQLite modules: `node:sqlite`, `bun:sqlite` / `Bun.SQL` and the stdlib
//! copy of `better-sqlite3`.
//!
//! `node:sqlite` (`DatabaseSync`, `StatementSync`, `StatementSyncIterator`,
//! `SQLTagStore`, `Session`, `db.limits`) is ordinary objects that own a
//! native payload (#11919, `docs/native-payload-pattern.md`): see
//! `database_sync.rs`, `statement_sync.rs`, `sqlite_callbacks.rs`,
//! `tag_store.rs` and `session.rs`. `bun:sqlite` and `Bun.SQL` still use the
//! registry handles in `node_db.rs` / `bun.rs`; they register no JS callbacks.

use crate::common::Handle;
use rusqlite::Connection;
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

mod backup;
mod better;
mod bind;
mod bun;
mod connection;
mod database_sync;
mod dispatch;
mod node_db;
mod options;
mod session;
mod sqlite_callbacks;
mod statement_sync;
mod tag_store;

// Re-export every moved item (pub and pub(crate)) back into the `sqlite`
// module namespace so existing intra-crate paths (`crate::sqlite::Foo`)
// keep resolving and sibling modules reach one another via `use super::*`.
pub(crate) use backup::*;
pub(crate) use bind::*;
pub(crate) use bun::*;
pub(crate) use connection::*;
pub(crate) use database_sync::*;
pub(crate) use dispatch::*;
pub(crate) use node_db::*;
pub(crate) use options::*;
pub(crate) use session::*;
pub(crate) use sqlite_callbacks::*;
pub(crate) use statement_sync::*;
pub(crate) use tag_store::*;

/// SQLite database handle
pub struct SqliteDbHandle {
    pub conn: Mutex<Connection>,
}

/// `bun:sqlite` `Database` / `Bun.SQL` handle (registry-backed; no JS
/// callbacks can be registered on it).
pub struct BunSqliteDbHandle {
    pub conn: Mutex<Option<Connection>>,
    pub path: String,
    pub read_only: bool,
    pub read_write: bool,
    pub create: bool,
    pub enable_foreign_keys: bool,
    pub enable_dqs: bool,
    pub timeout_ms: i32,
    pub read_bigints: bool,
    pub return_arrays: bool,
    pub allow_bare_named_parameters: bool,
    pub allow_unknown_named_parameters: bool,
    pub allow_load_extension: bool,
    pub enable_load_extension: AtomicBool,
    pub defensive: AtomicBool,
    pub initial_limits: [Option<i32>; NODE_SQLITE_LIMIT_COUNT],
    pub statements: Mutex<HashSet<Handle>>,
}

/// `bun:sqlite` `Statement` handle.
pub struct BunSqliteStmtHandle {
    pub db_handle: Handle,
    pub sql: String,
    pub finalized: AtomicBool,
    pub read_bigints: AtomicBool,
    pub return_arrays: AtomicBool,
    pub allow_bare_named_parameters: AtomicBool,
    pub allow_unknown_named_parameters: AtomicBool,
}

impl BunSqliteStmtHandle {
    pub(crate) fn flags(&self) -> StmtFlags {
        use std::sync::atomic::Ordering::Relaxed;
        StmtFlags {
            read_bigints: self.read_bigints.load(Relaxed),
            return_arrays: self.return_arrays.load(Relaxed),
            allow_bare_named_parameters: self.allow_bare_named_parameters.load(Relaxed),
            allow_unknown_named_parameters: self.allow_unknown_named_parameters.load(Relaxed),
        }
    }
}

/// A statement's row/bind options, plain data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StmtFlags {
    pub(crate) read_bigints: bool,
    pub(crate) return_arrays: bool,
    pub(crate) allow_bare_named_parameters: bool,
    pub(crate) allow_unknown_named_parameters: bool,
}

#[derive(Clone)]
pub(crate) struct NodeSqliteOptions {
    open: bool,
    read_only: bool,
    read_write: bool,
    create: bool,
    enable_foreign_keys: bool,
    enable_dqs: bool,
    timeout_ms: i32,
    read_bigints: bool,
    return_arrays: bool,
    allow_bare_named_parameters: bool,
    allow_unknown_named_parameters: bool,
    allow_extension: bool,
    defensive: bool,
    initial_limits: [Option<i32>; NODE_SQLITE_LIMIT_COUNT],
}

impl Default for NodeSqliteOptions {
    fn default() -> Self {
        Self {
            open: true,
            read_only: false,
            read_write: true,
            create: true,
            enable_foreign_keys: true,
            enable_dqs: false,
            timeout_ms: 0,
            read_bigints: false,
            return_arrays: false,
            allow_bare_named_parameters: true,
            allow_unknown_named_parameters: false,
            allow_extension: false,
            defensive: true,
            initial_limits: [None; NODE_SQLITE_LIMIT_COUNT],
        }
    }
}

pub(crate) const NODE_SQLITE_LIMIT_COUNT: usize = 11;
pub(crate) const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const TAG_NULL_BITS: u64 = 0x7FFC_0000_0000_0002;
pub(crate) const JS_SAFE_INTEGER_MAX: i64 = 9_007_199_254_740_991;
pub(crate) const JS_SAFE_INTEGER_MIN: i64 = -9_007_199_254_740_991;

/// SQLite statement handle
pub struct SqliteStmtHandle {
    pub sql: String,
    pub db_handle: Handle,
    /// Per-statement raw mode flag — `stmt.raw([toggle])` enables this.
    /// In raw mode, `stmt.all(...)` returns array-of-arrays (one inner
    /// array per row, column values in declared order) and
    /// `stmt.get(...)` returns a single column-value array. drizzle's
    /// `PreparedQuery.values()` chains `this.stmt.raw().all(...)` to
    /// feed `mapResultRow(fields, row, joinsNotNullableMap)`. Without
    /// this method `stmt.raw` is undefined and the call surfaces as
    /// `(number).all is not a function` deeper in the chain. Refs #643.
    pub raw_mode: AtomicBool,
}
