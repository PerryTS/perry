//! `node:sqlite` `StatementSync` and `StatementSyncIterator`: ordinary
//! objects that own a native payload (#11919).
//!
//! A statement owns no C resource: it keeps its SQL and options and compiles
//! them on its database for every run, so closing the database never leaves a
//! statement holding a dead `sqlite3_stmt`. It enters C through its
//! database's owner (held in its JS state) and carries the database's
//! `OpenSerial`, so a statement of an earlier open reports "statement has been
//! finalized" after `close()` / `open()`.

use super::*;
use perry_runtime::closure::{ClosureHeader, JsThis};
use perry_runtime::gc::{RuntimeHandle, RuntimeHandleScope};
use perry_runtime::native_class_ids::{SQLITE_STATEMENT_ITERATOR, SQLITE_STATEMENT_SYNC};
use perry_runtime::native_payload::{
    self, NativePayloadFamily, OpenSerial, PayloadMiss, PayloadPrototype,
};
use perry_runtime::{
    buffer::{buffer_alloc, buffer_data_mut, mark_as_uint8array},
    js_array_alloc, js_array_get, js_array_length, js_array_push, js_nanbox_pointer,
    js_object_alloc_null_proto, js_object_set_field, js_string_from_bytes, ArrayHeader, JSValue,
    ObjectHeader,
};
use rusqlite::ffi;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

perry_runtime::state_key_memo!(static MEMO_DB);
perry_runtime::state_key_memo!(static MEMO_STMT);
perry_runtime::state_key_memo!(static MEMO_ROWS);

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
    expanded_sql: String,
    flags: StmtFlags,
    /// Bumped by every run; an iterator is valid while it matches.
    epoch: u64,
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

/// A `StatementSync` of the database `db` (open #`serial`).
pub(crate) unsafe fn new_statement(
    db: &RuntimeHandle<'_>,
    serial: OpenSerial,
    sql: CString,
    expanded_sql: String,
    flags: StmtFlags,
) -> f64 {
    let bytes = std::mem::size_of::<NodeStmt>() + sql.as_bytes().len() + expanded_sql.len();
    let payload = NodeStmt {
        serial,
        sql,
        expanded_sql,
        flags,
        epoch: 0,
    };
    let scope = RuntimeHandleScope::new();
    let stmt = scope.root_nanbox_f64(native_payload::alloc_with_state(
        &STMT_FAMILY,
        payload,
        bytes,
        &[],
        &[(b"db", db.get_nanbox_f64())],
    ));
    native_payload::define_own_accessor(
        stmt.get_nanbox_f64(),
        "sourceSQL",
        builtin!(stmt_source_sql_getter, 0),
        None,
        true,
        false,
    );
    native_payload::define_own_accessor(
        stmt.get_nanbox_f64(),
        "expandedSQL",
        builtin!(stmt_expanded_sql_getter, 0),
        None,
        true,
        false,
    );
    stmt.get_nanbox_f64()
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
    stmt: *mut ffi::sqlite3_stmt,
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
        let stmt = prepare_guarded(db, raw_db, sql);
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
            stmt,
            flags,
        }
    }

    pub(crate) fn raw_stmt(&self) -> *mut ffi::sqlite3_stmt {
        self.stmt
    }

    pub(crate) fn raw_db(&self) -> *mut ffi::sqlite3 {
        self.raw_db
    }

    /// Advance one row. `false` at the end (the statement is then reset).
    pub(crate) unsafe fn step(&mut self) -> bool {
        let (stmt, raw_db) = (self.stmt, self.raw_db);
        let ((rc, error), end) = guarded(self.db.get_nanbox_f64(), || {
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
        if stmt.is_null() || !db_is_open(self.db.get_nanbox_f64()) {
            return Ok(());
        }
        let (_, end) = guarded(self.db.get_nanbox_f64(), || ffi::sqlite3_finalize(stmt));
        end
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
        if !stmt.is_null() && db_is_open(self.db.get_nanbox_f64()) {
            ffi::sqlite3_finalize(stmt);
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
            let buf = buffer_alloc(len as u32);
            (*buf).length = len as u32;
            mark_as_uint8array(buf as usize);
            if len > 0 {
                let ptr = ffi::sqlite3_column_blob(raw_stmt, index);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(ptr as *const u8, buffer_data_mut(buf), len);
                }
            }
            JSValue::object_ptr(buf as *mut u8)
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
    let sql: *const CStr = stmt.sql.as_c_str();
    let params_arr = raw_addr_from_value(params) as *const ArrayHeader;
    let named = !params_arr.is_null()
        && js_array_length(params_arr) > 0
        && is_named_parameter_object(f64_from_jsvalue(js_array_get(params_arr, 0)));
    let stepper = Stepper::start(db, &*sql, flags, named, |raw_db, raw_stmt| {
        bind_node_sqlite_params(flags, raw_db, raw_stmt, params_arr)
    });
    if !stepper.raw_stmt().is_null() {
        let expanded = expanded_sql_of(stepper.raw_stmt());
        if let Ok(stmt) =
            native_payload::payload_mut::<NodeStmt>(this.get_nanbox_f64(), &STMT_FAMILY)
        {
            stmt.expanded_sql = expanded;
        }
    }
    stepper
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
        )
    }
}

extern "C" fn stmt_columns_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        let (stmt, db) = live_stmt(this.get_nanbox_f64());
        let flags = stmt.flags;
        let sql: *const CStr = stmt.sql.as_c_str();
        let db = scope.root_nanbox_f64(db);
        let stepper = Stepper::start(&db, &*sql, flags, false, |_, _| Ok(()));
        let columns = if stepper.raw_stmt().is_null() {
            js_array_alloc(0)
        } else {
            sqlite_columns_array(stepper.raw_stmt())
        };
        let columns = scope.root_raw_mut_ptr(columns);
        stepper.finish();
        js_nanbox_pointer(columns.get_raw_mut_ptr::<ArrayHeader>() as i64)
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
        f64_from_jsvalue(string_value(&stmt.expanded_sql))
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
