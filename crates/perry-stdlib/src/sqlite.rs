//! SQLite module (better-sqlite3 compatible)
//!
//! Native implementation of the 'better-sqlite3' npm package using rusqlite.
//! Provides synchronous SQLite database operations.

use crate::common::{for_each_handle_mut_of, Handle};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Mutex, OnceLock};

mod backup;
mod better;
mod bind;
mod bun;
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
pub(crate) use bun::*;
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
    pub iteration_epoch: AtomicU64,
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

pub(crate) static NODE_SQLITE_CUSTOM_FUNCTIONS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
pub(crate) static NODE_SQLITE_CUSTOM_AGGREGATES: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
pub(crate) static NODE_SQLITE_ACTIVE_AGGREGATES: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

thread_local! {
    // The mutable-root scanner registry is thread-local, so this latch must be too.
    static NODE_SQLITE_GC_SCANNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

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
    ensure_node_sqlite_thread_exit_hooks_registered();
    NODE_SQLITE_GC_SCANNER.with(|registered| {
        if registered.get() {
            return;
        }
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:node_sqlite",
            scan_node_sqlite_roots_mut,
        );
        registered.set(true);
    });
}

/// #11471: the three node:sqlite sets and `NodeSqliteDbHandle::authorizer_callback`
/// hold JS values of the thread that called `db.function()` / `db.aggregate()` /
/// `db.setAuthorizer()`. Registered before any of them is filled: every insert
/// path runs `ensure_node_sqlite_gc_scanner_registered` first.
fn ensure_node_sqlite_thread_exit_hooks_registered() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        perry_runtime::arena::thread_exit::register_thread_exit_range_hook(
            release_node_sqlite_callbacks_in_freed_ranges,
        );
        crate::common::handle::register_handle_payload_releaser::<NodeSqliteDbHandle>(
            release_node_sqlite_authorizer_in_freed_ranges,
        );
    });
}

/// Thread-exit hook (#11471). The boxes named by the three sets are owned by
/// SQLite (freed by xDestroy / xFinal, which tolerate an already-unregistered
/// pointer), so this never frees them: it unregisters every box holding a
/// value in the dying thread's arena (so no surviving thread's scanner visits
/// it) and overwrites those values with `undefined`, so a late SQLite call
/// throws "value is not a function" instead of jumping into reused memory.
fn release_node_sqlite_callbacks_in_freed_ranges(
    freed: &perry_runtime::arena::thread_exit::FreedRanges,
) {
    fn scrub(slot: &mut f64, freed: &perry_runtime::arena::thread_exit::FreedRanges) -> bool {
        if freed.holds_value(*slot) {
            *slot = f64::from_bits(TAG_UNDEFINED_BITS);
            true
        } else {
            false
        }
    }
    if let Some(functions) = NODE_SQLITE_CUSTOM_FUNCTIONS.get() {
        let mut functions = functions.lock().unwrap_or_else(|p| p.into_inner());
        functions.retain(|raw| {
            let func = *raw as *mut NodeSqliteCustomFunction;
            // SAFETY: a registered box stays allocated until xDestroy, which
            // unregisters it under this lock first.
            func.is_null() || !scrub(unsafe { &mut (*func).callback }, freed)
        });
    }
    if let Some(aggregates) = NODE_SQLITE_CUSTOM_AGGREGATES.get() {
        let mut aggregates = aggregates.lock().unwrap_or_else(|p| p.into_inner());
        aggregates.retain(|raw| {
            let aggregate = *raw as *mut NodeSqliteCustomAggregate;
            if aggregate.is_null() {
                return true;
            }
            // SAFETY: as above (xDestroy unregisters under this lock).
            let aggregate = unsafe { &mut *aggregate };
            let mut dead = scrub(&mut aggregate.start, freed);
            dead |= scrub(&mut aggregate.step, freed);
            if let Some(result) = aggregate.result.as_mut() {
                dead |= scrub(result, freed);
            }
            if let Some(inverse) = aggregate.inverse.as_mut() {
                dead |= scrub(inverse, freed);
            }
            !dead
        });
    }
    if let Some(states) = NODE_SQLITE_ACTIVE_AGGREGATES.get() {
        let mut states = states.lock().unwrap_or_else(|p| p.into_inner());
        states.retain(|raw| {
            let state = *raw as *mut NodeSqliteAggregateState;
            // SAFETY: xFinal unregisters under this lock before freeing.
            state.is_null() || !scrub(unsafe { &mut (*state).state }, freed)
        });
    }
}

/// `HANDLES` payload releaser (#11471): clear a DatabaseSync's authorizer when
/// the closure lives in the dying thread's arena. The authorizer trampoline
/// reads `None` as "allow" (`SQLITE_OK`), so the connection stays usable. The
/// database itself is kept: it holds no other GC value.
fn release_node_sqlite_authorizer_in_freed_ranges(
    db: &mut NodeSqliteDbHandle,
    freed: &perry_runtime::arena::thread_exit::FreedRanges,
) -> bool {
    let mut callback = db
        .authorizer_callback
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if callback.is_some_and(|value| freed.holds_value(value)) {
        *callback = None;
    }
    false
}

#[cfg(test)]
pub(crate) mod thread_exit_probe {
    //! #11471 test probes over the node:sqlite callback tables.
    use super::*;

    /// Register a scalar-function box exactly as `db.function()` does, minus
    /// the `sqlite3_create_function_v2` call. Returns the box address.
    pub(crate) fn register_function_for_test(callback: f64) -> usize {
        let info = Box::into_raw(Box::new(NodeSqliteCustomFunction {
            callback,
            use_bigint_arguments: false,
        }));
        register_node_sqlite_custom_function(info);
        info as usize
    }

    /// Register an aggregate box as `db.aggregate()` does. Returns its address.
    pub(crate) fn register_aggregate_for_test(step: f64) -> usize {
        let aggregate = Box::into_raw(Box::new(NodeSqliteCustomAggregate {
            start: f64::from_bits(TAG_NULL_BITS),
            step,
            result: None,
            inverse: None,
            use_bigint_arguments: false,
        }));
        register_node_sqlite_custom_aggregate(aggregate);
        aggregate as usize
    }

    /// Register a running aggregate state as xStep does. Returns its address.
    pub(crate) fn register_aggregate_state_for_test(state: f64) -> usize {
        ensure_node_sqlite_gc_scanner_registered();
        let state = Box::into_raw(Box::new(NodeSqliteAggregateState { state }));
        register_node_sqlite_aggregate_state(state);
        state as usize
    }

    /// A closed DatabaseSync whose authorizer is `callback`, registered in
    /// `HANDLES` as `new DatabaseSync()` + `db.setAuthorizer()` leave it.
    pub(crate) fn register_db_with_authorizer_for_test(callback: f64) -> Handle {
        ensure_node_sqlite_gc_scanner_registered();
        crate::common::register_handle(NodeSqliteDbHandle {
            conn: Mutex::new(None),
            path: String::new(),
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
            allow_load_extension: false,
            enable_load_extension: AtomicBool::new(false),
            defensive: AtomicBool::new(true),
            authorizer_callback: Mutex::new(Some(callback)),
            initial_limits: [None; NODE_SQLITE_LIMIT_COUNT],
            limits_handle: Mutex::new(None),
            sessions: Mutex::new(HashSet::new()),
            statements: Mutex::new(HashSet::new()),
        })
    }

    pub(crate) fn function_registered(addr: usize) -> bool {
        node_sqlite_custom_functions()
            .lock()
            .unwrap()
            .contains(&addr)
    }

    pub(crate) fn aggregate_registered(addr: usize) -> bool {
        node_sqlite_custom_aggregates()
            .lock()
            .unwrap()
            .contains(&addr)
    }

    pub(crate) fn aggregate_state_registered(addr: usize) -> bool {
        node_sqlite_active_aggregates()
            .lock()
            .unwrap()
            .contains(&addr)
    }

    pub(crate) fn authorizer(db: Handle) -> Option<f64> {
        crate::common::with_handle::<NodeSqliteDbHandle, _, _>(db, |db| {
            *db.authorizer_callback.lock().unwrap()
        })
        .flatten()
    }

    /// Free the probe boxes (SQLite's xDestroy / xFinal would in real use).
    pub(crate) fn free_boxes_for_test(function: usize, aggregate: usize, state: usize) {
        unsafe {
            unregister_node_sqlite_custom_function(function as *mut NodeSqliteCustomFunction);
            drop(Box::from_raw(function as *mut NodeSqliteCustomFunction));
            unregister_node_sqlite_custom_aggregate(aggregate as *mut NodeSqliteCustomAggregate);
            drop(Box::from_raw(aggregate as *mut NodeSqliteCustomAggregate));
            unregister_node_sqlite_aggregate_state(state as *mut NodeSqliteAggregateState);
            drop(Box::from_raw(state as *mut NodeSqliteAggregateState));
        }
    }
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
    ensure_node_sqlite_thread_exit_hooks_registered();
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
