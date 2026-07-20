use super::*;
use crate::common::{for_each_handle_mut_of, get_handle, register_handle, Handle};
use perry_runtime::{
    buffer::{
        buffer_alloc, buffer_data, buffer_data_mut, is_any_array_buffer, is_data_view,
        is_registered_buffer, mark_as_uint8array, BufferHeader,
    },
    closure::{is_closure_ptr, js_closure_call1, js_closure_call_array, ClosureHeader},
    js_array_alloc, js_array_get, js_array_is_array, js_array_length, js_array_push,
    js_array_push_f64, js_get_string_pointer_unified, js_nanbox_pointer, js_object_alloc,
    js_object_alloc_null_proto, js_object_alloc_with_shape, js_object_get_field_by_name,
    js_object_set_field, js_object_set_field_by_name, js_object_set_keys, js_promise_rejected,
    js_promise_resolved, js_string_from_bytes, ArrayHeader, BigIntHeader, JSValue, ObjectHeader,
    Promise, StringHeader,
};
use rusqlite::{ffi, limits::Limit, types::Value as SqliteValue, Connection, OpenFlags};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_call(_arg0: f64, _arg1: f64) -> Handle {
    throw_illegal_constructor()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_new(_arg0: f64, _arg1: f64) -> Handle {
    throw_illegal_constructor()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_session_call(_arg0: f64, _arg1: f64) -> Handle {
    throw_illegal_constructor()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_session_new(_arg0: f64, _arg1: f64) -> Handle {
    throw_illegal_constructor()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_run(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    with_node_sqlite_statement(stmt_handle, params_arr, |conn, stmt, raw_stmt| {
        loop {
            let rc = ffi::sqlite3_step(raw_stmt);
            match rc {
                ffi::SQLITE_ROW => continue,
                ffi::SQLITE_DONE => break,
                _ => throw_sqlite_error_from_conn(conn),
            }
        }
        let read_bigints = stmt.read_bigints.load(Ordering::Relaxed);
        let changes = ffi::sqlite3_changes64(conn.handle());
        let last_insert_rowid = ffi::sqlite3_last_insert_rowid(conn.handle());
        let keys = vec!["changes".to_string(), "lastInsertRowid".to_string()];
        let (packed_keys, shape_id) = build_packed_keys(&keys);
        let obj =
            js_object_alloc_with_shape(shape_id, 2, packed_keys.as_ptr(), packed_keys.len() as u32);
        let changes_value = if read_bigints {
            JSValue::bigint_ptr(perry_runtime::bigint::js_bigint_from_i64(changes))
        } else {
            node_sqlite_integer_value(changes, false)
        };
        let rowid_value = if read_bigints {
            JSValue::bigint_ptr(perry_runtime::bigint::js_bigint_from_i64(last_insert_rowid))
        } else {
            node_sqlite_integer_value(last_insert_rowid, false)
        };
        js_object_set_field(obj, 0, changes_value);
        js_object_set_field(obj, 1, rowid_value);
        obj
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    with_node_sqlite_statement(stmt_handle, params_arr, |conn, stmt, raw_stmt| {
        match ffi::sqlite3_step(raw_stmt) {
            ffi::SQLITE_ROW => f64_from_jsvalue(node_sqlite_row_value(stmt, raw_stmt)),
            ffi::SQLITE_DONE => undefined_f64(),
            _ => throw_sqlite_error_from_conn(conn),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    with_node_sqlite_statement(stmt_handle, params_arr, |conn, stmt, raw_stmt| {
        let mut rows = js_array_alloc(0);
        loop {
            match ffi::sqlite3_step(raw_stmt) {
                ffi::SQLITE_ROW => {
                    rows = js_array_push(rows, node_sqlite_row_value(stmt, raw_stmt));
                }
                ffi::SQLITE_DONE => break,
                _ => throw_sqlite_error_from_conn(conn),
            }
        }
        rows
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_iterate(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    // Rows are materialized eagerly (a known divergence from Node's lazy
    // stepping — see the module TODO), but the iterator protocol matches
    // Node (#6561): exhaustion and `return()` produce
    // `{ done: true, value: null }`, and `return()` terminates iteration.
    let rows = js_node_sqlite_statement_sync_all(stmt_handle, params_arr);
    perry_runtime::array::array_values_iter_null_done(f64_from_jsvalue(JSValue::array_ptr(rows)))
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_columns(
    stmt_handle: Handle,
) -> *mut ArrayHeader {
    with_node_sqlite_statement(stmt_handle, std::ptr::null(), |_conn, _stmt, raw_stmt| {
        let column_count = ffi::sqlite3_column_count(raw_stmt);
        let mut result = js_array_alloc(column_count as u32);
        let keys = vec![
            "column".to_string(),
            "database".to_string(),
            "name".to_string(),
            "table".to_string(),
            "type".to_string(),
        ];
        for index in 0..column_count {
            let values = vec![
                sqlite_c_string_value(ffi::sqlite3_column_origin_name(raw_stmt, index)),
                sqlite_c_string_value(ffi::sqlite3_column_database_name(raw_stmt, index)),
                sqlite_c_string_value(ffi::sqlite3_column_name(raw_stmt, index)),
                sqlite_c_string_value(ffi::sqlite3_column_table_name(raw_stmt, index)),
                sqlite_c_string_value(ffi::sqlite3_column_decltype(raw_stmt, index)),
            ];
            let obj = make_null_proto_object(&keys, &values);
            result = js_array_push(result, JSValue::object_ptr(obj as *mut u8));
        }
        result
    })
}

pub(crate) unsafe fn set_node_statement_bool_option(
    stmt_handle: Handle,
    value: f64,
    field: &AtomicBool,
) -> i32 {
    if get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .map(|stmt| stmt.finalized.load(Ordering::Relaxed))
        .unwrap_or(true)
    {
        throw_invalid_state("statement has been finalized");
    }
    let js = value_from_f64(value);
    if !js.is_bool() {
        throw_type("The \"enabled\" argument must be a boolean");
    }
    field.store(js.as_bool(), Ordering::Relaxed);
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_set_read_bigints(
    stmt_handle: Handle,
    value: f64,
) -> i32 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    set_node_statement_bool_option(stmt_handle, value, &stmt.read_bigints)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_set_return_arrays(
    stmt_handle: Handle,
    value: f64,
) -> i32 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    set_node_statement_bool_option(stmt_handle, value, &stmt.return_arrays)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_set_allow_bare_named_parameters(
    stmt_handle: Handle,
    value: f64,
) -> i32 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    set_node_statement_bool_option(stmt_handle, value, &stmt.allow_bare_named_parameters)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_set_allow_unknown_named_parameters(
    stmt_handle: Handle,
    value: f64,
) -> i32 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    set_node_statement_bool_option(stmt_handle, value, &stmt.allow_unknown_named_parameters)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_source_sql(
    stmt_handle: Handle,
) -> *mut StringHeader {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    if stmt.finalized.load(Ordering::Relaxed) {
        throw_invalid_state("statement has been finalized");
    }
    js_string_from_bytes(stmt.sql.as_ptr(), stmt.sql.len() as u32)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_expanded_sql(
    stmt_handle: Handle,
) -> *mut StringHeader {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    if stmt.finalized.load(Ordering::Relaxed) {
        throw_invalid_state("statement has been finalized");
    }
    let expanded = stmt
        .expanded_sql
        .lock()
        .map(|sql| sql.clone())
        .unwrap_or_default();
    js_string_from_bytes(expanded.as_ptr(), expanded.len() as u32)
}

pub(crate) unsafe fn changeset_bytes_from_value(value: f64) -> Vec<u8> {
    let addr = raw_addr_from_value(value);
    if addr != 0 {
        if is_registered_buffer(addr) && !is_any_array_buffer(addr) && !is_data_view(addr) {
            let buf = addr as *const BufferHeader;
            let bytes = std::slice::from_raw_parts(buffer_data(buf), (*buf).length as usize);
            return bytes.to_vec();
        }
        if perry_runtime::typedarray::lookup_typed_array_kind(addr)
            == Some(perry_runtime::typedarray::KIND_UINT8)
        {
            let ptr = addr as *const perry_runtime::typedarray::TypedArrayHeader;
            if let Some(bytes) = perry_runtime::typedarray::typed_array_bytes(ptr) {
                return bytes.to_vec();
            }
        }
    }
    throw_type("The \"changeset\" argument must be a Uint8Array.");
}

pub(crate) unsafe fn sqlite_session_blob(
    session_handle: Handle,
    make_blob: unsafe extern "C" fn(
        *mut ffi::sqlite3_session,
        *mut c_int,
        *mut *mut c_void,
    ) -> c_int,
) -> *mut BufferHeader {
    let session_handle = get_handle::<NodeSqliteSessionHandle>(session_handle)
        .unwrap_or_else(|| throw_invalid_state("session is not open"));
    let db = get_handle::<NodeSqliteDbHandle>(session_handle.db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let conn_guard = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    let Some(conn) = conn_guard.as_ref() else {
        drop(conn_guard);
        throw_invalid_state("database is not open");
    };
    let session = session_handle
        .session
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("session is not open"));
    let Some(raw_session) = *session else {
        drop(session);
        drop(conn_guard);
        throw_invalid_state("session is not open");
    };

    let mut len: c_int = 0;
    let mut data: *mut c_void = std::ptr::null_mut();
    let rc = make_blob(
        raw_session as *mut ffi::sqlite3_session,
        &mut len,
        &mut data,
    );
    if rc != ffi::SQLITE_OK {
        let message = sqlite_error_message(conn);
        let errcode = ffi::sqlite3_extended_errcode(conn.handle());
        drop(session);
        drop(conn_guard);
        if !data.is_null() {
            ffi::sqlite3_free(data);
        }
        throw_sqlite_error_ext(&message, errcode);
    }

    let len = len.max(0) as usize;
    let buffer = buffer_alloc(len as u32);
    (*buffer).length = len as u32;
    mark_as_uint8array(buffer as usize);
    if len > 0 && !data.is_null() {
        std::ptr::copy_nonoverlapping(data as *const u8, buffer_data_mut(buffer), len);
    }
    if !data.is_null() {
        ffi::sqlite3_free(data);
    }
    buffer
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_session_changeset(
    session_handle: Handle,
) -> *mut BufferHeader {
    sqlite_session_blob(session_handle, ffi::sqlite3session_changeset)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_session_patchset(
    session_handle: Handle,
) -> *mut BufferHeader {
    sqlite_session_blob(session_handle, ffi::sqlite3session_patchset)
}

pub(crate) unsafe fn node_sqlite_session_close(
    session_handle: Handle,
    swallow_errors: bool,
) -> i32 {
    let Some(session_handle_ref) = get_handle::<NodeSqliteSessionHandle>(session_handle) else {
        if swallow_errors {
            return 1;
        }
        throw_invalid_state("session is not open");
    };
    let Some(db) = get_handle::<NodeSqliteDbHandle>(session_handle_ref.db_handle) else {
        if swallow_errors {
            return 1;
        }
        throw_invalid_state("database is not open");
    };
    {
        let conn = match db.conn.lock() {
            Ok(conn) => conn,
            Err(_) => {
                if swallow_errors {
                    return 1;
                }
                throw_invalid_state("database is not open");
            }
        };
        if conn.is_none() {
            if swallow_errors {
                return 1;
            }
            drop(conn);
            throw_invalid_state("database is not open");
        }
    }

    if let Ok(mut sessions) = db.sessions.lock() {
        sessions.remove(&session_handle);
    }
    let mut session = match session_handle_ref.session.lock() {
        Ok(session) => session,
        Err(_) => {
            if swallow_errors {
                return 1;
            }
            throw_invalid_state("session is not open");
        }
    };
    let Some(raw_session) = session.take() else {
        if swallow_errors {
            return 1;
        }
        drop(session);
        throw_invalid_state("session is not open");
    };
    ffi::sqlite3session_delete(raw_session as *mut ffi::sqlite3_session);
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_session_close(session_handle: Handle) -> i32 {
    node_sqlite_session_close(session_handle, false)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_session_dispose(session_handle: Handle) -> i32 {
    node_sqlite_session_close(session_handle, true)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_create_session(
    db_handle: Handle,
    options_value: f64,
) -> Handle {
    validate_optional_object(options_value);
    let db_name = string_option(options_value, "db", Some("main")).unwrap_or_else(|| "main".into());
    let table_name = string_option(options_value, "table", None);
    ensure_open_node_database(db_handle);

    let db_name_c = CString::new(db_name)
        .unwrap_or_else(|_| throw_type("The \"options.db\" argument must not contain null bytes"));
    let table_name_c = table_name.as_ref().map(|name| {
        CString::new(name.as_str()).unwrap_or_else(|_| {
            throw_type("The \"options.table\" argument must not contain null bytes")
        })
    });

    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let conn_guard = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    let Some(conn) = conn_guard.as_ref() else {
        drop(conn_guard);
        throw_invalid_state("database is not open");
    };

    let mut raw_session: *mut ffi::sqlite3_session = std::ptr::null_mut();
    let rc = ffi::sqlite3session_create(conn.handle(), db_name_c.as_ptr(), &mut raw_session);
    if rc != ffi::SQLITE_OK {
        let message = sqlite_error_message(conn);
        let errcode = ffi::sqlite3_extended_errcode(conn.handle());
        drop(conn_guard);
        throw_sqlite_error_ext(&message, errcode);
    }
    let table_ptr = table_name_c
        .as_ref()
        .map(|name| name.as_ptr())
        .unwrap_or(std::ptr::null());
    let rc = ffi::sqlite3session_attach(raw_session, table_ptr);
    if rc != ffi::SQLITE_OK {
        let message = sqlite_error_message(conn);
        let errcode = ffi::sqlite3_extended_errcode(conn.handle());
        ffi::sqlite3session_delete(raw_session);
        drop(conn_guard);
        throw_sqlite_error_ext(&message, errcode);
    }
    drop(conn_guard);

    let handle = register_handle(NodeSqliteSessionHandle {
        db_handle,
        session: Mutex::new(Some(raw_session as usize)),
    });
    if let Ok(mut sessions) = db.sessions.lock() {
        sessions.insert(handle);
    }
    handle
}

pub(crate) struct ChangesetApplyContext {
    filter: Option<*const ClosureHeader>,
    on_conflict: Option<*const ClosureHeader>,
}

pub(crate) unsafe extern "C" fn node_sqlite_changeset_filter(
    ctx: *mut c_void,
    table: *const c_char,
) -> c_int {
    let ctx = &mut *(ctx as *mut ChangesetApplyContext);
    let Some(filter) = ctx.filter else {
        return 1;
    };
    let table = if table.is_null() {
        ""
    } else {
        CStr::from_ptr(table).to_str().unwrap_or("")
    };
    let table_value = JSValue::string_ptr(js_string_from_bytes(table.as_ptr(), table.len() as u32));
    let result = js_closure_call1(filter, f64::from_bits(table_value.bits()));
    (perry_runtime::value::js_is_truthy(result) != 0) as c_int
}

pub(crate) unsafe extern "C" fn node_sqlite_changeset_conflict(
    ctx: *mut c_void,
    conflict: c_int,
    _iter: *mut ffi::sqlite3_changeset_iter,
) -> c_int {
    let ctx = &mut *(ctx as *mut ChangesetApplyContext);
    let Some(on_conflict) = ctx.on_conflict else {
        return ffi::SQLITE_CHANGESET_ABORT;
    };
    let result = js_closure_call1(
        on_conflict,
        f64::from_bits(JSValue::number(conflict as f64).bits()),
    );
    let result = value_from_f64(result);
    if result.is_int32() {
        return result.as_int32() as c_int;
    }
    if result.is_number() {
        let number = result.as_number();
        if number.is_finite()
            && number.fract() == 0.0
            && number >= c_int::MIN as f64
            && number <= c_int::MAX as f64
        {
            return number as c_int;
        }
    }
    ffi::SQLITE_CHANGESET_ABORT
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_apply_changeset(
    db_handle: Handle,
    changeset_value: f64,
    options_value: f64,
) -> f64 {
    ensure_open_node_database(db_handle);
    let changeset = changeset_bytes_from_value(changeset_value);
    validate_optional_object(options_value);
    let filter = function_option(options_value, "filter").and_then(closure_ptr_from_value);
    let on_conflict = function_option(options_value, "onConflict").and_then(closure_ptr_from_value);
    let mut context = ChangesetApplyContext {
        filter,
        on_conflict,
    };

    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let conn_guard = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    let Some(conn) = conn_guard.as_ref() else {
        drop(conn_guard);
        throw_invalid_state("database is not open");
    };
    let rc = ffi::sqlite3changeset_apply(
        conn.handle(),
        changeset.len() as c_int,
        changeset.as_ptr() as *mut c_void,
        if context.filter.is_some() {
            Some(node_sqlite_changeset_filter)
        } else {
            None
        },
        Some(node_sqlite_changeset_conflict),
        &mut context as *mut ChangesetApplyContext as *mut c_void,
    );
    match rc {
        ffi::SQLITE_OK => bool_f64(true),
        ffi::SQLITE_ABORT => bool_f64(false),
        _ => {
            let message = sqlite_error_message(conn);
            let errcode = ffi::sqlite3_extended_errcode(conn.handle());
            drop(conn_guard);
            throw_sqlite_error_ext(&message, errcode);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_enable_load_extension(
    db_handle: Handle,
    allow_value: f64,
) -> i32 {
    let allow = {
        let js = value_from_f64(allow_value);
        if !js.is_bool() {
            throw_type("The \"allow\" argument must be a boolean");
        }
        js.as_bool()
    };

    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    if allow && !db.allow_load_extension {
        throw_invalid_state(
            "Cannot enable extension loading because it was disabled at database creation.",
        );
    }

    let conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    let config_error = conn
        .as_ref()
        .and_then(|conn| configure_node_sqlite_load_extension(conn, allow).err());
    drop(conn);
    if let Some(err) = config_error {
        throw_sqlite_error(&err);
    }
    db.enable_load_extension.store(allow, Ordering::Relaxed);
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_load_extension(
    db_handle: Handle,
    path_value: f64,
) -> i32 {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("database is not open"));
        if conn.is_none() {
            drop(conn);
            throw_invalid_state("database is not open");
        }
    }

    if !db.allow_load_extension || !db.enable_load_extension.load(Ordering::Relaxed) {
        throw_invalid_state("extension loading is not allowed");
    }

    let path = string_from_value(path_value, "path");
    let c_path = CString::new(path)
        .unwrap_or_else(|_| throw_type("The \"path\" argument must not contain null bytes"));
    let conn_guard = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    let Some(conn) = conn_guard.as_ref() else {
        drop(conn_guard);
        throw_invalid_state("database is not open");
    };
    let mut error_message = std::ptr::null_mut();
    let rc = ffi::sqlite3_load_extension(
        conn.handle(),
        c_path.as_ptr(),
        std::ptr::null(),
        &mut error_message,
    );
    if rc == ffi::SQLITE_OK {
        return 1;
    }

    let message = if error_message.is_null() {
        CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
            .to_string_lossy()
            .into_owned()
    } else {
        let message = CStr::from_ptr(error_message).to_string_lossy().into_owned();
        ffi::sqlite3_free(error_message.cast());
        message
    };
    drop(conn_guard);
    throw_load_sqlite_extension(&message)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_location(
    db_handle: Handle,
    db_name_value: f64,
) -> f64 {
    ensure_open_node_database(db_handle);
    let db_name = if value_from_f64(db_name_value).is_undefined() {
        "main".to_string()
    } else {
        string_from_value(db_name_value, "dbName")
    };
    let c_name = CString::new(db_name)
        .unwrap_or_else(|_| throw_type("The \"dbName\" argument must not contain null bytes"));
    with_open_node_connection(db_handle, |conn| {
        let filename =
            unsafe { rusqlite::ffi::sqlite3_db_filename(conn.handle(), c_name.as_ptr()) };
        if filename.is_null() {
            return null_f64();
        }
        let filename = unsafe { CStr::from_ptr(filename) }.to_str().unwrap_or("");
        if filename.is_empty() {
            null_f64()
        } else {
            let ptr = js_string_from_bytes(filename.as_ptr(), filename.len() as u32);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_limits(db_handle: Handle) -> Handle {
    ensure_open_node_database(db_handle);
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let mut limits_handle = db
        .limits_handle
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    if let Some(handle) = *limits_handle {
        return handle;
    }
    let handle = register_handle(NodeSqliteLimitsHandle { db_handle });
    *limits_handle = Some(handle);
    handle
}
