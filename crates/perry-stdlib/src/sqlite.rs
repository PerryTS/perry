//! SQLite module (better-sqlite3 compatible)
//!
//! Native implementation of the 'better-sqlite3' npm package using rusqlite.
//! Provides synchronous SQLite database operations.

use crate::common::{for_each_handle_mut_of, Handle};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, Once, OnceLock};

mod backup;
mod better;
mod bind;
mod connection;
mod dispatch;
mod node_db;
mod node_stmt_session;
mod node_tag_store;
mod options;

// Re-export every moved item (pub and pub(crate)) back into the `sqlite`
// module namespace so existing intra-crate paths (`crate::sqlite::Foo`)
// keep resolving and sibling modules reach one another via `use super::*`.
pub(crate) use backup::*;
pub(crate) use bind::*;
pub(crate) use connection::*;
pub(crate) use dispatch::*;
pub(crate) use node_db::*;
pub(crate) use node_stmt_session::*;
pub(crate) use node_tag_store::*;
pub(crate) use options::*;

/// SQLite database handle
pub struct SqliteDbHandle {
    pub conn: Mutex<Connection>,
}

/// Node `node:sqlite` DatabaseSync handle.
///
/// Kept separate from `SqliteDbHandle` so the historical better-sqlite3
/// close/exec/prepare behavior remains unchanged.
pub struct NodeSqliteDbHandle {
    pub conn: Mutex<Option<Connection>>,
    pub path: String,
    pub read_only: bool,
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
    pub authorizer_callback: Mutex<Option<f64>>,
    pub initial_limits: [Option<i32>; NODE_SQLITE_LIMIT_COUNT],
    pub limits_handle: Mutex<Option<Handle>>,
    pub sessions: Mutex<HashSet<Handle>>,
    pub statements: Mutex<HashSet<Handle>>,
}

pub struct NodeSqliteLimitsHandle {
    pub db_handle: Handle,
}

pub struct NodeSqliteSessionHandle {
    pub db_handle: Handle,
    pub session: Mutex<Option<usize>>,
}

pub struct NodeSqliteTagStoreHandle {
    pub db_handle: Handle,
    pub capacity: usize,
    pub cache: Mutex<NodeSqliteTagStoreCache>,
}

pub struct NodeSqliteTagStoreCache {
    statements: HashMap<String, Handle>,
    recency: VecDeque<String>,
}

impl NodeSqliteTagStoreCache {
    pub(crate) fn new() -> Self {
        Self {
            statements: HashMap::new(),
            recency: VecDeque::new(),
        }
    }

    pub(crate) fn touch(&mut self, sql: &str) {
        self.recency.retain(|cached| cached != sql);
        self.recency.push_back(sql.to_string());
    }

    pub(crate) fn get(&mut self, sql: &str) -> Option<Handle> {
        let handle = *self.statements.get(sql)?;
        self.touch(sql);
        Some(handle)
    }

    pub(crate) fn remove(&mut self, sql: &str) -> Option<Handle> {
        self.recency.retain(|cached| cached != sql);
        self.statements.remove(sql)
    }

    pub(crate) fn put(&mut self, sql: String, handle: Handle, capacity: usize) -> Vec<Handle> {
        let mut finalized = Vec::new();
        if capacity == 0 {
            finalized.push(handle);
            return finalized;
        }

        if let Some(previous) = self.statements.insert(sql.clone(), handle) {
            finalized.push(previous);
        }
        self.touch(&sql);

        while self.statements.len() > capacity {
            let Some(oldest) = self.recency.pop_front() else {
                break;
            };
            if let Some(evicted) = self.statements.remove(&oldest) {
                finalized.push(evicted);
            }
        }
        finalized
    }

    pub(crate) fn clear(&mut self) -> Vec<Handle> {
        self.recency.clear();
        self.statements.drain().map(|(_, handle)| handle).collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.statements.len()
    }
}

pub struct NodeSqliteStmtHandle {
    pub db_handle: Handle,
    pub sql: String,
    pub finalized: AtomicBool,
    pub read_bigints: AtomicBool,
    pub return_arrays: AtomicBool,
    pub allow_bare_named_parameters: AtomicBool,
    pub allow_unknown_named_parameters: AtomicBool,
    pub expanded_sql: Mutex<String>,
}

pub(crate) struct NodeSqliteStmtOptions {
    read_bigints: bool,
    return_arrays: bool,
    allow_bare_named_parameters: bool,
    allow_unknown_named_parameters: bool,
}

pub(crate) struct NodeSqliteCustomFunction {
    callback: f64,
    use_bigint_arguments: bool,
}

pub(crate) struct NodeSqliteCustomAggregate {
    start: f64,
    step: f64,
    result: Option<f64>,
    inverse: Option<f64>,
    use_bigint_arguments: bool,
}

pub(crate) struct NodeSqliteAggregateState {
    state: f64,
}

#[derive(Clone)]
pub(crate) struct NodeSqliteOptions {
    open: bool,
    read_only: bool,
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

pub(crate) static NODE_SQLITE_GC_SCANNER: Once = Once::new();
pub(crate) static NODE_SQLITE_CUSTOM_FUNCTIONS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
pub(crate) static NODE_SQLITE_CUSTOM_AGGREGATES: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
pub(crate) static NODE_SQLITE_ACTIVE_AGGREGATES: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

pub(crate) fn node_sqlite_custom_functions() -> &'static Mutex<HashSet<usize>> {
    NODE_SQLITE_CUSTOM_FUNCTIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn node_sqlite_custom_aggregates() -> &'static Mutex<HashSet<usize>> {
    NODE_SQLITE_CUSTOM_AGGREGATES.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn node_sqlite_active_aggregates() -> &'static Mutex<HashSet<usize>> {
    NODE_SQLITE_ACTIVE_AGGREGATES.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn ensure_node_sqlite_gc_scanner_registered() {
    NODE_SQLITE_GC_SCANNER.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:node_sqlite",
            scan_node_sqlite_roots_mut,
        );
    });
}

pub(crate) fn scan_node_sqlite_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    {
        let functions = node_sqlite_custom_functions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for raw in functions.iter() {
            let func = *raw as *mut NodeSqliteCustomFunction;
            if !func.is_null() {
                unsafe {
                    visitor.visit_nanbox_f64_slot(&mut (*func).callback);
                }
            }
        }
    }
    {
        let aggregates = node_sqlite_custom_aggregates()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for raw in aggregates.iter() {
            let aggregate = *raw as *mut NodeSqliteCustomAggregate;
            if !aggregate.is_null() {
                unsafe {
                    visitor.visit_nanbox_f64_slot(&mut (*aggregate).start);
                    visitor.visit_nanbox_f64_slot(&mut (*aggregate).step);
                    if let Some(result) = (*aggregate).result.as_mut() {
                        visitor.visit_nanbox_f64_slot(result);
                    }
                    if let Some(inverse) = (*aggregate).inverse.as_mut() {
                        visitor.visit_nanbox_f64_slot(inverse);
                    }
                }
            }
        }
    }
    {
        let states = node_sqlite_active_aggregates()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for raw in states.iter() {
            let state = *raw as *mut NodeSqliteAggregateState;
            if !state.is_null() {
                unsafe {
                    visitor.visit_nanbox_f64_slot(&mut (*state).state);
                }
            }
        }
    }
    for_each_handle_mut_of::<NodeSqliteDbHandle, _>(|db| {
        if let Ok(mut callback) = db.authorizer_callback.lock() {
            if let Some(value) = callback.as_mut() {
                visitor.visit_nanbox_f64_slot(value);
            }
        }
    });
}

pub(crate) fn register_node_sqlite_custom_function(ptr: *mut NodeSqliteCustomFunction) {
    ensure_node_sqlite_gc_scanner_registered();
    if !ptr.is_null() {
        node_sqlite_custom_functions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(ptr as usize);
    }
}

pub(crate) fn unregister_node_sqlite_custom_function(ptr: *mut NodeSqliteCustomFunction) -> bool {
    if !ptr.is_null() {
        return node_sqlite_custom_functions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(ptr as usize));
    }
    false
}

pub(crate) fn register_node_sqlite_custom_aggregate(ptr: *mut NodeSqliteCustomAggregate) {
    ensure_node_sqlite_gc_scanner_registered();
    if !ptr.is_null() {
        node_sqlite_custom_aggregates()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(ptr as usize);
    }
}

pub(crate) fn unregister_node_sqlite_custom_aggregate(ptr: *mut NodeSqliteCustomAggregate) -> bool {
    if !ptr.is_null() {
        return node_sqlite_custom_aggregates()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(ptr as usize));
    }
    false
}

pub(crate) fn register_node_sqlite_aggregate_state(ptr: *mut NodeSqliteAggregateState) {
    if !ptr.is_null() {
        node_sqlite_active_aggregates()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(ptr as usize);
    }
}

pub(crate) fn unregister_node_sqlite_aggregate_state(ptr: *mut NodeSqliteAggregateState) -> bool {
    if !ptr.is_null() {
        return node_sqlite_active_aggregates()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(ptr as usize));
    }
    false
}

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
