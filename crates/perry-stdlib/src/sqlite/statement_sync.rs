//! `node:sqlite` `StatementSync` and `StatementSyncIterator`: ordinary
//! objects that own a native payload (#11919).
//!
//! A statement owns the `sqlite3_stmt` its `prepare()` compiled, as node's
//! does: every run resets and rebinds it, so the authorizer runs once per
//! `prepare()` and `expandedSQL` reads the last bindings. The connection
//! finalizes it on close (its `live` token then reads false and the
//! statement's drop leaves it alone); otherwise the statement's drop
//! finalizes it. A run that re-enters a statement already stepping (a
//! callback calling the same statement) compiles its own copy for that run.
//! It enters C through its database's owner (held in its JS state) and
//! carries the database's `OpenSerial`, so a statement of an earlier open
//! reports "statement has been finalized" after `close()` / `open()`.

use super::*;
use perry_runtime::closure::{ClosureHeader, JsThis};
use perry_runtime::gc::{RuntimeHandle, RuntimeHandleScope};
use perry_runtime::native_class_ids::{SQLITE_STATEMENT_ITERATOR, SQLITE_STATEMENT_SYNC};
use perry_runtime::native_payload::{
    self, NativePayloadFamily, OpenSerial, PayloadMiss, PayloadPrototype,
};
use perry_runtime::{
    buffer::bytes::{from_slice, Brand},
    js_array_alloc, js_array_get, js_array_length, js_array_push, js_nanbox_pointer,
    js_object_alloc_null_proto, js_object_set_field, js_string_from_bytes, ArrayHeader, JSValue,
    ObjectHeader,
};
use rusqlite::ffi;
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::rc::Rc;

perry_runtime::state_key_memo!(static MEMO_DB);
perry_runtime::state_key_memo!(static MEMO_STMT);
perry_runtime::state_key_memo!(static MEMO_ROWS);
perry_runtime::birth_memo!(static STMT_BIRTH);
perry_runtime::birth_memo!(static ITER_BIRTH);

/// node's own enumerable, non-configurable getter `name` (no setter).
pub(crate) fn own_getter(
    name: &'static str,
    get: *const perry_runtime::closure::JsFunctionInfo,
) -> native_payload::OwnAccessor {
    native_payload::OwnAccessor {
        name,
        get,
        set: None,
        enumerable: true,
        configurable: false,
    }
}

macro_rules! builtin {
    ($body:path, $n:tt) => {
        perry_runtime::fn_info!($body, $n; with_declared($n), with_flags(perry_runtime::closure::FN_BUILTIN))
    };
}

macro_rules! builtin_rest {
    ($body:path) => {
        perry_runtime::fn_info!(
            $body, 1;
            with_rest(0),
            with_declared(0),
            with_flags(perry_runtime::closure::FN_BUILTIN)
        )
    };
}

pub(crate) static STMT_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: SQLITE_STATEMENT_SYNC,
    links_owner: false,
    name: "StatementSync",
    constructor_export: Some(("sqlite", "StatementSync")),
    constructor_length: 0,
    install_prototype: install_stmt_prototype,
};

pub(crate) static ITER_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: SQLITE_STATEMENT_ITERATOR,
    links_owner: false,
    name: "StatementSyncIterator",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: install_iter_prototype,
};

pub(crate) struct NodeStmt {
    serial: OpenSerial,
    sql: CString,
    /// The compiled statement (null for SQL with no statement). Reset
    /// whenever no run is stepping it, so finalizing it calls nothing back.
    raw: *mut ffi::sqlite3_stmt,
    /// The open's liveness token (see `NodeDb::live`).
    conn: Rc<Cell<bool>>,
    /// A run is using `raw`.
    running: bool,
    flags: StmtFlags,
    /// Bumped by every run; an iterator is valid while it matches.
    epoch: u64,
}

impl Drop for NodeStmt {
    fn drop(&mut self) {
        // Always reset here (see `raw`): no callback can run.
        if self.conn.get() && !self.raw.is_null() {
            unsafe { ffi::sqlite3_finalize(self.raw) };
        }
    }
}

pub(crate) struct NodeStmtIter {
    epoch: u64,
    index: u32,
    done: bool,
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_call(_arg0: f64, _arg1: f64) -> f64 {
    throw_illegal_constructor()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_new(_arg0: f64, _arg1: f64) -> f64 {
    throw_illegal_constructor()
}

/// A `StatementSync` of the database `db` (open #`serial`) owning the
/// compiled `raw`.
pub(crate) unsafe fn new_statement(
    db: &RuntimeHandle<'_>,
    serial: OpenSerial,
    conn: Rc<Cell<bool>>,
    sql: CString,
    raw: *mut ffi::sqlite3_stmt,
    flags: StmtFlags,
) -> f64 {
    let compiled = if raw.is_null() {
        0
    } else {
        ffi::sqlite3_stmt_status(raw, ffi::SQLITE_STMTSTATUS_MEMUSED, 0).max(0) as usize
    };
    let bytes = std::mem::size_of::<NodeStmt>() + sql.as_bytes().len() + compiled;
    let payload = NodeStmt {
        serial,
        sql,
        raw,
        conn,
        running: false,
        flags,
        epoch: 0,
    };
    native_payload::alloc_with_state(
        &STMT_FAMILY,
        payload,
        bytes,
        &[],
        &[(b"db", db.get_nanbox_f64())],
        &[
            own_getter("sourceSQL", builtin!(stmt_source_sql_getter, 0)),
            own_getter("expandedSQL", builtin!(stmt_expanded_sql_getter, 0)),
        ],
        &STMT_BIRTH,
    )
}

/// `this`'s payload, or node's "finalized" error when its database is
/// closed or was reopened since it was prepared.
unsafe fn live_stmt<'a>(this: f64) -> (&'a mut NodeStmt, f64) {
    let stmt = match native_payload::payload_mut::<NodeStmt>(this, &STMT_FAMILY) {
        Ok(stmt) => stmt,
        Err(PayloadMiss::Closed) => throw_invalid_state("statement has been finalized"),
        Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
    };
    let db = native_payload::state_get_memo(this, &STMT_FAMILY, b"db", &MEMO_DB);
    match native_payload::payload_mut::<NodeDb>(db, &DB_FAMILY) {
        Ok(open) if open.serial == stmt.serial => (stmt, db),
        _ => throw_invalid_state("statement has been finalized"),
    }
}

// ---- execution ---------------------------------------------------------------

/// One compiled statement being stepped on its database. Every step runs in
/// its own guard; conversion happens outside it; any error finalizes the
/// statement before the throw.
pub(crate) struct Stepper<'a, 's> {
    db: &'a RuntimeHandle<'s>,
    raw_db: *mut ffi::sqlite3,
    link: perry_runtime::native_payload::OwnerLink,
    stmt: *mut ffi::sqlite3_stmt,
    /// The statement whose compiled `stmt` this run uses: the run resets it
    /// where a compiled-for-this-run statement is finalized. Null for the
    /// latter. Stable: the caller roots the statement for the whole run.
    owned: *mut NodeStmt,
    pub(crate) flags: StmtFlags,
}

impl<'a, 's> Stepper<'a, 's> {
    /// Compile `sql` (guarded: the authorizer runs) and bind with `bind`.
    /// A bind error finalizes the statement before it is thrown; when
    /// `bind_runs_js` (a named-parameter object, whose getters are user
    /// code) a JS throw from inside `bind` is caught for the same cleanup.
    pub(crate) unsafe fn start(
        db: &'a RuntimeHandle<'s>,
        sql: &CStr,
        flags: StmtFlags,
        bind_runs_js: bool,
        bind: impl FnOnce(*mut ffi::sqlite3, *mut ffi::sqlite3_stmt) -> Result<(), BindError>,
    ) -> Self {
        let raw_db = db_payload(db.get_nanbox_f64()).raw;
        let (stmt, link) = prepare_guarded_with_link(db, raw_db, sql);
        if !stmt.is_null() {
            let bound = if bind_runs_js {
                match perry_runtime::exception::catch_js_throw(|| bind(raw_db, stmt)) {
                    Ok(bound) => bound,
                    Err(error) => {
                        let scope = RuntimeHandleScope::new();
                        let error = scope.root_nanbox_f64(error);
                        // Never stepped: finalizing calls nothing back.
                        ffi::sqlite3_finalize(stmt);
                        perry_runtime::exception::js_throw(error.get_nanbox_f64());
                    }
                }
            } else {
                bind(raw_db, stmt)
            };
            if let Err(error) = bound {
                ffi::sqlite3_finalize(stmt);
                error.throw();
            }
        }
        Stepper {
            db,
            raw_db,
            link,
            stmt,
            owned: std::ptr::null_mut(),
            flags,
        }
    }

    /// Run `owner`'s compiled statement: clear and bind it (as
    /// [`Stepper::start`] binds). It is reset, never finalized, when the
    /// run ends.
    unsafe fn start_owned(
        db: &'a RuntimeHandle<'s>,
        owner: *mut NodeStmt,
        flags: StmtFlags,
        bind_runs_js: bool,
        bind: impl FnOnce(*mut ffi::sqlite3, *mut ffi::sqlite3_stmt) -> Result<(), BindError>,
    ) -> Self {
        let raw_db = db_payload(db.get_nanbox_f64()).raw;
        let link = match native_payload::owner_link(db.get_nanbox_f64(), &DB_FAMILY) {
            Ok(link) => link,
            Err(PayloadMiss::Closed) => throw_invalid_state("database is not open"),
            Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
        };
        let stmt = (*owner).raw;
        (*owner).running = true;
        // Every run binds from scratch. Clearing calls nothing back.
        ffi::sqlite3_clear_bindings(stmt);
        let bound = if bind_runs_js {
            match perry_runtime::exception::catch_js_throw(|| bind(raw_db, stmt)) {
                Ok(bound) => bound,
                Err(error) => {
                    (*owner).running = false;
                    perry_runtime::exception::js_throw(error);
                }
            }
        } else {
            bind(raw_db, stmt)
        };
        if let Err(error) = bound {
            (*owner).running = false;
            error.throw();
        }
        Stepper {
            db,
            raw_db,
            link,
            stmt,
            owned: owner,
            flags,
        }
    }

    pub(crate) fn raw_db(&self) -> *mut ffi::sqlite3 {
        self.raw_db
    }

    /// Advance one row. `false` at the end (the statement is then reset).
    pub(crate) unsafe fn step(&mut self) -> bool {
        let (stmt, raw_db) = (self.stmt, self.raw_db);
        let guard = native_payload::enter_link(self.link)
            .unwrap_or_else(|_| throw_invalid_state("database is not open"));
        let ((rc, error), end) = guard.call(|| {
            let rc = ffi::sqlite3_step(stmt);
            let error =
                (rc != ffi::SQLITE_ROW && rc != ffi::SQLITE_DONE).then(|| capture_error(raw_db));
            if rc != ffi::SQLITE_ROW {
                // Aggregates still open finish here, inside the guard, so a
                // pending exception keeps them from running JS.
                ffi::sqlite3_reset(stmt);
            }
            (rc, error)
        });
        if let Err(end) = end {
            self.forget_or_finalize();
            throw_call_end(end);
        }
        if let Some(error) = error {
            self.forget_or_finalize();
            throw_captured(error);
        }
        rc == ffi::SQLITE_ROW
    }

    /// The current row as a JS value; on a conversion error the statement
    /// is finalized (guarded) before the throw.
    pub(crate) unsafe fn row(&mut self) -> JSValue {
        match row_value_checked(self.flags, self.stmt, self.flags.return_arrays) {
            Ok(row) => row,
            Err(error) => {
                let scope = RuntimeHandleScope::new();
                let error = scope.root_nanbox_f64(error);
                self.abandon();
                perry_runtime::exception::js_throw(error.get_nanbox_f64())
            }
        }
    }

    /// Stop early (after `get()`): finalize under a guard, since aggregates
    /// still open run their `result` callbacks; rethrow what they threw.
    pub(crate) unsafe fn finish_early(mut self) {
        let end = self.finalize_guarded();
        if let Err(end) = end {
            throw_call_end(end);
        }
    }

    /// Finish after `step()` returned `false`.
    pub(crate) unsafe fn finish(mut self) {
        self.forget_or_finalize();
    }

    unsafe fn finalize_guarded(&mut self) -> Result<(), perry_runtime::native_payload::CallEnd> {
        let stmt = std::mem::replace(&mut self.stmt, std::ptr::null_mut());
        let owned = self.release_owned();
        if stmt.is_null() || !db_is_open(self.db.get_nanbox_f64()) {
            return Ok(());
        }
        // Aggregates still open run their `result` callbacks either way.
        let (_, end) = guarded(self.db.get_nanbox_f64(), || {
            if owned {
                ffi::sqlite3_reset(stmt)
            } else {
                ffi::sqlite3_finalize(stmt)
            }
        });
        end
    }

    /// End the use of an owned statement; true when this run had one.
    unsafe fn release_owned(&mut self) -> bool {
        let owned = std::mem::replace(&mut self.owned, std::ptr::null_mut());
        if owned.is_null() {
            return false;
        }
        (*owned).running = false;
        true
    }

    /// Finalize, dropping whatever the callbacks threw (an earlier error
    /// is already on its way out).
    unsafe fn abandon(&mut self) {
        let _ = self.finalize_guarded();
    }

    /// After a guarded call: a deferred close released inside it finalized
    /// every statement of the connection, this one included.
    unsafe fn forget_or_finalize(&mut self) {
        let stmt = std::mem::replace(&mut self.stmt, std::ptr::null_mut());
        let owned = self.release_owned();
        if stmt.is_null() || !db_is_open(self.db.get_nanbox_f64()) {
            return;
        }
        if !owned {
            ffi::sqlite3_finalize(stmt);
        } else if ffi::sqlite3_stmt_busy(stmt) != 0 {
            // Stopped on a row: reset it (open aggregates finish) under a
            // guard; what they throw loses to the error already leaving.
            let _ = guarded(self.db.get_nanbox_f64(), || ffi::sqlite3_reset(stmt));
        }
    }
}

/// A row as a JS value without throwing (node's range error as `Err`).
pub(crate) unsafe fn row_value_checked(
    flags: StmtFlags,
    raw_stmt: *mut ffi::sqlite3_stmt,
    return_arrays: bool,
) -> Result<JSValue, f64> {
    let column_count = ffi::sqlite3_column_count(raw_stmt).max(0);
    let scope = RuntimeHandleScope::new();
    let mut values = Vec::with_capacity(column_count as usize);
    for index in 0..column_count {
        let value = column_value_checked(raw_stmt, index, flags.read_bigints)?;
        values.push(scope.root_nanbox_u64(value.bits()));
    }
    if return_arrays {
        let arr = scope.root_raw_mut_ptr(js_array_alloc(column_count as u32));
        for value in &values {
            let next = js_array_push(
                arr.get_raw_mut_ptr(),
                JSValue::from_bits(value.get_nanbox_u64()),
            );
            arr.set_raw_mut_ptr(next);
        }
        return Ok(JSValue::array_ptr(arr.get_raw_mut_ptr::<ArrayHeader>()));
    }
    let mut names = Vec::with_capacity(column_count as usize);
    for index in 0..column_count {
        let name_ptr = ffi::sqlite3_column_name(raw_stmt, index);
        names.push(if name_ptr.is_null() {
            String::new()
        } else {
            CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
        });
    }
    let obj = scope.root_raw_mut_ptr(js_object_alloc_null_proto(0, names.len() as u32));
    set_object_keys_from_names(obj.get_raw_mut_ptr::<ObjectHeader>(), &names);
    for (idx, value) in values.iter().enumerate() {
        js_object_set_field(
            obj.get_raw_mut_ptr::<ObjectHeader>(),
            idx as u32,
            JSValue::from_bits(value.get_nanbox_u64()),
        );
    }
    Ok(JSValue::object_ptr(
        obj.get_raw_mut_ptr::<ObjectHeader>() as *mut u8
    ))
}

unsafe fn column_value_checked(
    raw_stmt: *mut ffi::sqlite3_stmt,
    index: c_int,
    read_bigints: bool,
) -> Result<JSValue, f64> {
    Ok(match ffi::sqlite3_column_type(raw_stmt, index) {
        ffi::SQLITE_NULL => JSValue::null(),
        ffi::SQLITE_INTEGER => {
            integer_value_checked(ffi::sqlite3_column_int64(raw_stmt, index), read_bigints)?
        }
        ffi::SQLITE_FLOAT => JSValue::number(ffi::sqlite3_column_double(raw_stmt, index)),
        ffi::SQLITE_TEXT => {
            let ptr = ffi::sqlite3_column_text(raw_stmt, index);
            if ptr.is_null() {
                return Ok(JSValue::null());
            }
            let len = ffi::sqlite3_column_bytes(raw_stmt, index) as usize;
            JSValue::string_ptr(js_string_from_bytes(ptr, len as u32))
        }
        ffi::SQLITE_BLOB => {
            let len = ffi::sqlite3_column_bytes(raw_stmt, index) as usize;
            let ptr = ffi::sqlite3_column_blob(raw_stmt, index) as *const u8;
            let bytes: &[u8] = if len > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, len)
            } else {
                &[]
            };
            JSValue::from_bits(from_slice(Brand::Uint8Array, bytes).to_bits())
        }
        _ => JSValue::null(),
    })
}

/// Run a statement to completion and return `{ changes, lastInsertRowid }`.
pub(crate) unsafe fn run_to_completion(mut stepper: Stepper<'_, '_>) -> f64 {
    while stepper.step() {}
    let (raw_db, read_bigints) = (stepper.raw_db(), stepper.flags.read_bigints);
    stepper.finish();
    js_nanbox_pointer(run_result_object(raw_db, read_bigints) as i64)
}

pub(crate) unsafe fn first_row(mut stepper: Stepper<'_, '_>) -> f64 {
    if !stepper.step() {
        stepper.finish();
        return undefined_f64();
    }
    let scope = RuntimeHandleScope::new();
    let row = scope.root_nanbox_u64(stepper.row().bits());
    stepper.finish_early();
    row.get_nanbox_f64()
}

pub(crate) unsafe fn all_rows(mut stepper: Stepper<'_, '_>) -> f64 {
    let scope = RuntimeHandleScope::new();
    let rows = scope.root_raw_mut_ptr(js_array_alloc(0));
    let row = scope.root_nanbox_u64(JSValue::undefined().bits());
    while stepper.step() {
        row.set_nanbox_u64(stepper.row().bits());
        let next = js_array_push(
            rows.get_raw_mut_ptr(),
            JSValue::from_bits(row.get_nanbox_u64()),
        );
        rows.set_raw_mut_ptr(next);
    }
    stepper.finish();
    js_nanbox_pointer(rows.get_raw_mut_ptr::<ArrayHeader>() as i64)
}

/// Prepare this statement's SQL on its database and bind `params` (the
/// rest array of a run/get/all/iterate call). Bumps the iteration epoch.
unsafe fn start_statement<'a, 's>(
    this: &RuntimeHandle<'s>,
    db: &'a RuntimeHandle<'s>,
    params: f64,
) -> Stepper<'a, 's> {
    let (stmt, _) = live_stmt(this.get_nanbox_f64());
    stmt.epoch += 1;
    let flags = stmt.flags;
    // The payload is stable and only dropped once `this` is unreachable;
    // `this` is rooted for the whole call.
    let owner: *mut NodeStmt = stmt;
    let sql: *const CStr = stmt.sql.as_c_str();
    let params_arr = raw_addr_from_value(params) as *const ArrayHeader;
    let named = !params_arr.is_null()
        && js_array_length(params_arr) > 0
        && is_named_parameter_object(f64_from_jsvalue(js_array_get(params_arr, 0)));
    let bind = |raw_db, raw_stmt| bind_node_sqlite_params(flags, raw_db, raw_stmt, params_arr);
    if (*owner).raw.is_null() || (*owner).running {
        Stepper::start(db, &*sql, flags, named, bind)
    } else {
        Stepper::start_owned(db, owner, flags, named, bind)
    }
}

macro_rules! with_statement {
    ($this:expr, $params:expr, |$stepper:ident| $body:expr) => {{
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64($this);
        let (_, db) = live_stmt(this.get_nanbox_f64());
        let db = scope.root_nanbox_f64(db);
        let params = scope.root_nanbox_f64($params);
        let $stepper = start_statement(&this, &db, params.get_nanbox_f64());
        $body
    }};
}

extern "C" fn stmt_run_thunk(_c: *const ClosureHeader, this: JsThis, params: f64) -> f64 {
    unsafe { with_statement!(this.as_f64(), params, |stepper| run_to_completion(stepper)) }
}

extern "C" fn stmt_get_thunk(_c: *const ClosureHeader, this: JsThis, params: f64) -> f64 {
    unsafe { with_statement!(this.as_f64(), params, |stepper| first_row(stepper)) }
}

extern "C" fn stmt_all_thunk(_c: *const ClosureHeader, this: JsThis, params: f64) -> f64 {
    unsafe { with_statement!(this.as_f64(), params, |stepper| all_rows(stepper)) }
}

extern "C" fn stmt_iterate_thunk(_c: *const ClosureHeader, this: JsThis, params: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this_root = scope.root_nanbox_f64(this.as_f64());
        let rows = with_statement!(this_root.get_nanbox_f64(), params, |stepper| all_rows(
            stepper
        ));
        let rows = scope.root_nanbox_f64(rows);
        // Rows are materialized eagerly; the iterator protocol matches node.
        let (stmt, _) = live_stmt(this_root.get_nanbox_f64());
        let epoch = stmt.epoch;
        native_payload::alloc_with_state(
            &ITER_FAMILY,
            NodeStmtIter {
                epoch,
                index: 0,
                done: false,
            },
            std::mem::size_of::<NodeStmtIter>(),
            &[],
            &[
                (b"stmt", this_root.get_nanbox_f64()),
                (b"rows", rows.get_nanbox_f64()),
            ],
            &[],
            &ITER_BIRTH,
        )
    }
}

extern "C" fn stmt_columns_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        let (stmt, _) = live_stmt(this.get_nanbox_f64());
        // Column metadata of the compiled statement: nothing runs or calls back.
        let columns = if stmt.raw.is_null() {
            js_array_alloc(0)
        } else {
            sqlite_columns_array(stmt.raw)
        };
        js_nanbox_pointer(columns as i64)
    }
}

unsafe fn set_flag(this: f64, value: f64, set: impl FnOnce(&mut StmtFlags, bool)) -> f64 {
    let (stmt, _) = live_stmt(this);
    let js = value_from_f64(value);
    if !js.is_bool() {
        throw_type("The \"enabled\" argument must be a boolean");
    }
    set(&mut stmt.flags, js.as_bool());
    undefined_f64()
}

extern "C" fn stmt_set_read_bigints_thunk(_c: *const ClosureHeader, this: JsThis, v: f64) -> f64 {
    unsafe { set_flag(this.as_f64(), v, |f, on| f.read_bigints = on) }
}

extern "C" fn stmt_set_return_arrays_thunk(_c: *const ClosureHeader, this: JsThis, v: f64) -> f64 {
    unsafe { set_flag(this.as_f64(), v, |f, on| f.return_arrays = on) }
}

extern "C" fn stmt_set_allow_bare_thunk(_c: *const ClosureHeader, this: JsThis, v: f64) -> f64 {
    unsafe { set_flag(this.as_f64(), v, |f, on| f.allow_bare_named_parameters = on) }
}

extern "C" fn stmt_set_allow_unknown_thunk(_c: *const ClosureHeader, this: JsThis, v: f64) -> f64 {
    unsafe {
        set_flag(this.as_f64(), v, |f, on| {
            f.allow_unknown_named_parameters = on
        })
    }
}

extern "C" fn stmt_source_sql_getter(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let (stmt, _) = live_stmt(this.as_f64());
        let bytes = stmt.sql.as_bytes();
        f64_from_jsvalue(JSValue::string_ptr(js_string_from_bytes(
            bytes.as_ptr(),
            bytes.len() as u32,
        )))
    }
}

extern "C" fn stmt_expanded_sql_getter(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let (stmt, _) = live_stmt(this.as_f64());
        // The compiled statement keeps the last run's bindings.
        f64_from_jsvalue(string_value(&expanded_sql_of(stmt.raw)))
    }
}

fn install_stmt_prototype(proto: &mut PayloadPrototype) {
    proto.method("run", builtin_rest!(stmt_run_thunk), 0);
    proto.method("get", builtin_rest!(stmt_get_thunk), 0);
    proto.method("all", builtin_rest!(stmt_all_thunk), 0);
    proto.method("iterate", builtin_rest!(stmt_iterate_thunk), 0);
    proto.method("columns", builtin!(stmt_columns_thunk, 0), 0);
    proto.method(
        "setReadBigInts",
        builtin!(stmt_set_read_bigints_thunk, 1),
        1,
    );
    proto.method(
        "setReturnArrays",
        builtin!(stmt_set_return_arrays_thunk, 1),
        1,
    );
    proto.method(
        "setAllowBareNamedParameters",
        builtin!(stmt_set_allow_bare_thunk, 1),
        1,
    );
    proto.method(
        "setAllowUnknownNamedParameters",
        builtin!(stmt_set_allow_unknown_thunk, 1),
        1,
    );
}

// ---- StatementSyncIterator ------------------------------------------------------

fn install_iter_prototype(proto: &mut PayloadPrototype) {
    proto.inherit(native_payload::iterator_prototype());
    proto.method("next", builtin!(iter_next_thunk, 0), 0);
    proto.method("return", builtin!(iter_return_thunk, 0), 0);
}

fn done_result() -> f64 {
    native_payload::iter_result_done_value(true, null_f64())
}

extern "C" fn iter_next_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let this = this.as_f64();
        let iter = match native_payload::payload_mut::<NodeStmtIter>(this, &ITER_FAMILY) {
            Ok(iter) => iter,
            Err(PayloadMiss::Closed) => return done_result(),
            Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
        };
        if iter.done {
            return done_result();
        }
        let expected = iter.epoch;
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this);
        let stmt_value = native_payload::state_get_memo(
            this.get_nanbox_f64(),
            &ITER_FAMILY,
            b"stmt",
            &MEMO_STMT,
        );
        let (stmt, _) = live_stmt(stmt_value);
        if stmt.epoch != expected {
            throw_invalid_state("iterator was invalidated");
        }
        let rows = native_payload::state_get_memo(
            this.get_nanbox_f64(),
            &ITER_FAMILY,
            b"rows",
            &MEMO_ROWS,
        );
        let arr = raw_addr_from_value(rows) as *const ArrayHeader;
        let Ok(iter) =
            native_payload::payload_mut::<NodeStmtIter>(this.get_nanbox_f64(), &ITER_FAMILY)
        else {
            return done_result();
        };
        if arr.is_null() || iter.index >= js_array_length(arr) {
            iter.done = true;
            native_payload::state_set_memo(
                this.get_nanbox_f64(),
                &ITER_FAMILY,
                b"rows",
                undefined_f64(),
                &MEMO_ROWS,
            );
            return done_result();
        }
        let row = f64_from_jsvalue(js_array_get(arr, iter.index));
        iter.index += 1;
        native_payload::iter_result_done_value(false, row)
    }
}

extern "C" fn iter_return_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let this = this.as_f64();
        match native_payload::payload_mut::<NodeStmtIter>(this, &ITER_FAMILY) {
            Ok(iter) => iter.done = true,
            Err(PayloadMiss::Closed) => {}
            Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
        }
        native_payload::state_set_memo(this, &ITER_FAMILY, b"rows", undefined_f64(), &MEMO_ROWS);
        done_result()
    }
}
