//! Registry-backed `bun:sqlite` `Database` / `Statement` handles, also used
//! by `Bun.SQL`'s SQLite adapter. `node:sqlite` no longer lives here: its
//! objects own native payloads (`database_sync.rs` and siblings). These
//! handles register no JS callbacks, so nothing here calls into JS from C.

use super::*;
use crate::common::{get_handle, register_handle, Handle};
use perry_runtime::{
    buffer::{is_any_array_buffer, is_data_view, is_registered_buffer, BufferHeader},
    js_array_alloc, js_array_push, js_get_string_pointer_unified, js_nanbox_pointer,
    js_object_alloc_with_shape, js_object_set_field, js_string_from_bytes, ArrayHeader, JSValue,
    ObjectHeader, StringHeader,
};
use rusqlite::{ffi, Connection};
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub(crate) unsafe fn register_bun_sqlite_database(
    path: String,
    options: NodeSqliteOptions,
    type_name: &str,
) -> Handle {
    let open = options.open;
    let handle = register_handle(BunSqliteDbHandle {
        conn: Mutex::new(None),
        path,
        read_only: options.read_only,
        read_write: options.read_write,
        create: options.create,
        enable_foreign_keys: options.enable_foreign_keys,
        enable_dqs: options.enable_dqs,
        timeout_ms: options.timeout_ms,
        read_bigints: options.read_bigints,
        return_arrays: options.return_arrays,
        allow_bare_named_parameters: options.allow_bare_named_parameters,
        allow_unknown_named_parameters: options.allow_unknown_named_parameters,
        allow_load_extension: options.allow_extension,
        enable_load_extension: AtomicBool::new(options.allow_extension),
        defensive: AtomicBool::new(options.defensive),
        initial_limits: options.initial_limits,
        statements: Mutex::new(HashSet::new()),
    });
    let type_symbol =
        perry_runtime::symbol::js_symbol_for(f64_from_jsvalue(string_value("sqlite-type")));
    perry_runtime::symbol::js_object_set_symbol_property(
        js_nanbox_pointer(handle),
        type_symbol,
        f64_from_jsvalue(string_value(type_name)),
    );
    if open {
        bun_sqlite_database_open(handle);
    }
    handle
}

pub(crate) unsafe fn bun_sqlite_database_open(db_handle: Handle) {
    let db = get_handle::<BunSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("database is not open"));
        if conn.is_some() {
            drop(conn);
            throw_invalid_state("database is already open");
        }
    }
    let opened = match open_node_sqlite_connection(db) {
        Ok(opened) => opened,
        Err(err) => {
            perry_runtime::exception::js_throw(sqlite_error_value(sqlite_error_from_rusqlite(err)))
        }
    };
    if let Err(err) =
        configure_node_sqlite_defensive(opened.handle(), db.defensive.load(Ordering::Relaxed))
    {
        throw_sqlite_error(&err);
    }
    if let Err(err) = configure_node_sqlite_dqs(opened.handle(), db.enable_dqs) {
        throw_sqlite_error(&err);
    }
    if let Err(err) = configure_node_sqlite_load_extension(
        opened.handle(),
        db.enable_load_extension.load(Ordering::Relaxed),
    ) {
        throw_sqlite_error(&err);
    }
    let mut conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    *conn = Some(opened);
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_close(db_handle: Handle) -> i32 {
    let db = get_handle::<BunSqliteDbHandle>(db_handle)
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
    finalize_bun_sqlite_statements(db);
    if let Ok(mut conn) = db.conn.lock() {
        *conn = None;
    }
    1
}

pub(crate) unsafe fn bun_sqlite_database_dispose(db_handle: Handle) {
    if let Some(db) = get_handle::<BunSqliteDbHandle>(db_handle) {
        finalize_bun_sqlite_statements(db);
        if let Ok(mut conn) = db.conn.lock() {
            *conn = None;
        }
    }
}

pub(crate) unsafe fn bun_sqlite_database_is_open(db_handle: Handle) -> f64 {
    let is_open = get_handle::<BunSqliteDbHandle>(db_handle)
        .and_then(|db| db.conn.lock().ok().map(|conn| conn.is_some()))
        .unwrap_or(false);
    bool_f64(is_open)
}

pub(crate) unsafe fn bun_sqlite_database_in_transaction(db_handle: Handle) -> f64 {
    with_open_bun_connection(db_handle, |conn| bool_f64(!conn.is_autocommit()))
}

pub(crate) unsafe fn bun_sqlite_database_exec(db_handle: Handle, sql_value: f64) {
    ensure_open_bun_database(db_handle);
    let sql = string_from_value(sql_value, "sql");
    let result = with_open_bun_connection(db_handle, |conn| node_sqlite_exec_batch(conn, &sql));
    if let Err((message, errcode)) = result {
        throw_sqlite_error_ext(&message, errcode);
    }
}

/// `db.query(sql)` / `db.prepare(sql)`: a statement handle. The SQL is
/// compiled once here to surface syntax errors; every run re-prepares it.
pub(crate) unsafe fn bun_sqlite_database_prepare(db_handle: Handle, sql_value: f64) -> Handle {
    ensure_open_bun_database(db_handle);
    let sql = {
        let js = value_from_f64(sql_value);
        if !js.is_any_string() {
            throw_type("The \"sql\" argument must be of type string");
        }
        let ptr = js_get_string_pointer_unified(sql_value) as *const StringHeader;
        string_from_header(ptr)
            .unwrap_or_else(|| throw_type("The \"sql\" argument must be of type string"))
    };
    let db = get_handle::<BunSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    with_open_bun_connection(db_handle, |conn| {
        drop(prepare_node_raw_statement(conn, &sql));
    });
    let handle = register_handle(BunSqliteStmtHandle {
        db_handle,
        sql,
        finalized: AtomicBool::new(false),
        read_bigints: AtomicBool::new(db.read_bigints),
        return_arrays: AtomicBool::new(db.return_arrays),
        allow_bare_named_parameters: AtomicBool::new(db.allow_bare_named_parameters),
        allow_unknown_named_parameters: AtomicBool::new(db.allow_unknown_named_parameters),
    });
    if let Ok(mut statements) = db.statements.lock() {
        statements.insert(handle);
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_serialize(
    db_handle: Handle,
    schema_value: f64,
) -> *mut BufferHeader {
    ensure_open_bun_database(db_handle);
    let schema = if value_from_f64(schema_value).is_undefined() {
        "main".to_string()
    } else {
        string_from_value(schema_value, "attachedDb")
    };
    let schema = CString::new(schema)
        .unwrap_or_else(|_| throw_type("The \"attachedDb\" argument must be a string"));
    with_open_bun_connection(db_handle, |conn| {
        sqlite_serialize_to_buffer(conn.handle(), &schema)
    })
}

/// `sqlite3_serialize` of `schema` into a fresh `Uint8Array`.
pub(crate) unsafe fn sqlite_serialize_to_buffer(
    db: *mut ffi::sqlite3,
    schema: &CStr,
) -> *mut BufferHeader {
    let mut size = 0;
    let image = ffi::sqlite3_serialize(db, schema.as_ptr(), &mut size, 0);
    if image.is_null() || size < 0 {
        throw_sqlite_error_from_db(db);
    }
    let len = size as usize;
    let buffer = JSValue::from_bits(
        perry_runtime::buffer::bytes::from_slice(
            perry_runtime::buffer::bytes::Brand::Uint8Array,
            std::slice::from_raw_parts(image, len),
        )
        .to_bits(),
    )
    .as_pointer::<perry_runtime::buffer::BufferHeader>()
    .cast_mut();
    ffi::sqlite3_free(image.cast());
    buffer
}

/// The bytes of a `deserialize()` image argument (a Uint8Array or Buffer).
pub(crate) unsafe fn sqlite_image_bytes(image_value: f64) -> Vec<u8> {
    let raw = raw_addr_from_value(image_value);
    let bytes = if perry_runtime::typedarray::lookup_typed_array_kind(raw)
        == Some(perry_runtime::typedarray::KIND_UINT8)
        || (raw >= 0x1000
            && is_registered_buffer(raw)
            && !is_any_array_buffer(raw)
            && !is_data_view(raw))
    {
        perry_runtime::buffer::bytes::no_gc(|scope| {
            let value = f64::from_bits(JSValue::pointer(raw as *const u8).bits());
            perry_runtime::buffer::bytes::bytes(value, scope)
                .ok()
                .map(<[u8]>::to_vec)
        })
    } else {
        None
    }
    .unwrap_or_else(|| throw_type("The \"data\" argument must be a Uint8Array."));
    if bytes.is_empty() {
        throw_arg_value("The \"data\" argument must not be empty.");
    }
    bytes
}

/// Replace `main` with an image (no JS can run inside).
pub(crate) unsafe fn sqlite_deserialize_main(db: *mut ffi::sqlite3, bytes: &[u8]) {
    let allocation = ffi::sqlite3_malloc64(bytes.len() as u64).cast::<u8>();
    if allocation.is_null() {
        throw_sqlite_error("out of memory");
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), allocation, bytes.len());
    let schema = b"main\0";
    let rc = ffi::sqlite3_deserialize(
        db,
        schema.as_ptr().cast(),
        allocation,
        bytes.len() as i64,
        bytes.len() as i64,
        ffi::SQLITE_DESERIALIZE_FREEONCLOSE | ffi::SQLITE_DESERIALIZE_RESIZEABLE,
    );
    if rc != ffi::SQLITE_OK {
        ffi::sqlite3_free(allocation.cast());
        throw_sqlite_error_from_db(db);
    }
}

pub(crate) unsafe fn bun_sqlite_database_deserialize(db_handle: Handle, image_value: f64) {
    ensure_open_bun_database(db_handle);
    let bytes = sqlite_image_bytes(image_value);
    with_open_bun_connection(db_handle, |conn| {
        sqlite_deserialize_main(conn.handle(), &bytes)
    });
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_load_extension(
    db_handle: Handle,
    path_value: f64,
) -> i32 {
    let db = get_handle::<BunSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    ensure_open_bun_database(db_handle);
    if !db.allow_load_extension || !db.enable_load_extension.load(Ordering::Relaxed) {
        throw_invalid_state("extension loading is not allowed");
    }
    let path = string_from_value(path_value, "path");
    let c_path = CString::new(path)
        .unwrap_or_else(|_| throw_type("The \"path\" argument must not contain null bytes"));
    with_open_bun_connection(db_handle, |conn| {
        sqlite_load_extension_or_throw(conn.handle(), &c_path)
    });
    1
}

/// `sqlite3_load_extension`, throwing node's `ERR_LOAD_SQLITE_EXTENSION`.
pub(crate) unsafe fn sqlite_load_extension_or_throw(db: *mut ffi::sqlite3, path: &CStr) {
    let mut error_message = std::ptr::null_mut();
    let rc = ffi::sqlite3_load_extension(db, path.as_ptr(), std::ptr::null(), &mut error_message);
    if rc == ffi::SQLITE_OK {
        return;
    }
    let message = if error_message.is_null() {
        sqlite_errmsg_raw(db)
    } else {
        let message = CStr::from_ptr(error_message).to_string_lossy().into_owned();
        ffi::sqlite3_free(error_message.cast());
        message
    };
    throw_load_sqlite_extension(&message)
}

pub(crate) unsafe fn configure_node_sqlite_defensive(
    db: *mut ffi::sqlite3,
    active: bool,
) -> Result<(), String> {
    let mut current = 0;
    let rc = ffi::sqlite3_db_config(
        db,
        ffi::SQLITE_DBCONFIG_DEFENSIVE,
        if active { 1 } else { 0 },
        &mut current,
    );
    if rc == ffi::SQLITE_OK {
        return Ok(());
    }
    Err(sqlite_errmsg_raw(db))
}

pub(crate) unsafe fn configure_node_sqlite_dqs(
    db: *mut ffi::sqlite3,
    enabled: bool,
) -> Result<(), String> {
    for option in [ffi::SQLITE_DBCONFIG_DQS_DDL, ffi::SQLITE_DBCONFIG_DQS_DML] {
        let mut current = 0;
        let rc = ffi::sqlite3_db_config(db, option, if enabled { 1 } else { 0 }, &mut current);
        if rc != ffi::SQLITE_OK {
            return Err(sqlite_errmsg_raw(db));
        }
    }
    Ok(())
}

pub(crate) unsafe fn configure_node_sqlite_load_extension(
    db: *mut ffi::sqlite3,
    enable: bool,
) -> Result<(), String> {
    let mut current = 0;
    let rc = ffi::sqlite3_db_config(
        db,
        ffi::SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION,
        if enable { 1 } else { 0 },
        &mut current,
    );
    if rc == ffi::SQLITE_OK {
        return Ok(());
    }
    Err(sqlite_errmsg_raw(db))
}

pub(crate) fn sqlite_function_name(name: String) -> CString {
    let bytes = name.as_bytes();
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap_or_else(|_| CString::new("").unwrap())
}

// ---- bun:sqlite Statement -------------------------------------------------

pub(crate) unsafe fn with_bun_sqlite_statement<R, F>(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
    action: F,
) -> R
where
    F: FnOnce(&Connection, StmtFlags, *mut ffi::sqlite3_stmt) -> R,
{
    let stmt = get_handle::<BunSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    if stmt.finalized.load(Ordering::Relaxed) {
        throw_invalid_state("statement has been finalized");
    }
    let conn_ptr = bun_connection_ptr(stmt.db_handle);
    let conn = &*conn_ptr;
    let flags = stmt.flags();
    let raw = prepare_node_raw_statement(conn, &stmt.sql);
    let raw_ptr = raw.ptr;
    if let Err(error) = bind_node_sqlite_params(flags, conn.handle(), raw_ptr, params_arr) {
        drop(raw);
        error.throw();
    }
    let result = action(conn, flags, raw_ptr);
    drop(raw);
    result
}

unsafe fn bun_connection_ptr(db_handle: Handle) -> *const Connection {
    let db = get_handle::<BunSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let conn_guard = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    if let Some(conn) = conn_guard.as_ref() {
        conn as *const Connection
    } else {
        drop(conn_guard);
        throw_invalid_state("database is not open");
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_statement_run(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    with_bun_sqlite_statement(stmt_handle, params_arr, |conn, flags, raw_stmt| {
        loop {
            match ffi::sqlite3_step(raw_stmt) {
                ffi::SQLITE_ROW => continue,
                ffi::SQLITE_DONE => break,
                _ => throw_sqlite_error_from_conn(conn),
            }
        }
        run_result_object(conn.handle(), flags.read_bigints)
    })
}

/// `{ changes, lastInsertRowid }` after a statement ran to completion.
pub(crate) unsafe fn run_result_object(
    db: *mut ffi::sqlite3,
    read_bigints: bool,
) -> *mut ObjectHeader {
    let changes = ffi::sqlite3_changes64(db);
    let last_insert_rowid = ffi::sqlite3_last_insert_rowid(db);
    let keys = vec!["changes".to_string(), "lastInsertRowid".to_string()];
    let (packed_keys, shape_id) = build_packed_keys(&keys);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(js_object_alloc_with_shape(
        shape_id,
        2,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    ));
    let changes_value = scope.root_nanbox_u64(
        if read_bigints {
            JSValue::bigint_ptr(perry_runtime::bigint::js_bigint_from_i64(changes))
        } else {
            node_sqlite_integer_value(changes, false)
        }
        .bits(),
    );
    let rowid_value = if read_bigints {
        JSValue::bigint_ptr(perry_runtime::bigint::js_bigint_from_i64(last_insert_rowid))
    } else {
        node_sqlite_integer_value(last_insert_rowid, false)
    };
    let obj = obj.get_raw_mut_ptr::<ObjectHeader>();
    js_object_set_field(obj, 0, JSValue::from_bits(changes_value.get_nanbox_u64()));
    js_object_set_field(obj, 1, rowid_value);
    obj
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_statement_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    with_bun_sqlite_statement(stmt_handle, params_arr, |conn, flags, raw_stmt| {
        match ffi::sqlite3_step(raw_stmt) {
            ffi::SQLITE_ROW => f64_from_jsvalue(node_sqlite_row_value(flags, raw_stmt)),
            ffi::SQLITE_DONE => undefined_f64(),
            _ => throw_sqlite_error_from_conn(conn),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_statement_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    with_bun_sqlite_statement(stmt_handle, params_arr, |conn, flags, raw_stmt| {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let rows = scope.root_raw_mut_ptr(js_array_alloc(0));
        let row = scope.root_nanbox_u64(JSValue::undefined().bits());
        loop {
            match ffi::sqlite3_step(raw_stmt) {
                ffi::SQLITE_ROW => {
                    row.set_nanbox_u64(node_sqlite_row_value(flags, raw_stmt).bits());
                    let next = js_array_push(
                        rows.get_raw_mut_ptr(),
                        JSValue::from_bits(row.get_nanbox_u64()),
                    );
                    rows.set_raw_mut_ptr(next);
                }
                ffi::SQLITE_DONE => break,
                _ => throw_sqlite_error_from_conn(conn),
            }
        }
        rows.get_raw_mut_ptr::<ArrayHeader>()
    })
}

pub(crate) unsafe fn bun_sqlite_statement_iterate(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    let rows = js_bun_sqlite_statement_all(stmt_handle, params_arr);
    perry_runtime::array::array_values_iter(f64_from_jsvalue(JSValue::array_ptr(rows)))
}

pub(crate) unsafe fn bun_sqlite_statement_columns(stmt_handle: Handle) -> *mut ArrayHeader {
    with_bun_sqlite_statement(stmt_handle, std::ptr::null(), |_conn, _flags, raw_stmt| {
        sqlite_columns_array(raw_stmt)
    })
}

/// node's `columns()` descriptor array for a prepared statement.
pub(crate) unsafe fn sqlite_columns_array(raw_stmt: *mut ffi::sqlite3_stmt) -> *mut ArrayHeader {
    let column_count = ffi::sqlite3_column_count(raw_stmt);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let result = scope.root_raw_mut_ptr(js_array_alloc(column_count.max(0) as u32));
    let keys = [
        "column".to_string(),
        "database".to_string(),
        "name".to_string(),
        "table".to_string(),
        "type".to_string(),
    ];
    for index in 0..column_count {
        let mut values = Vec::with_capacity(5);
        let mut roots = Vec::with_capacity(5);
        for ptr in [
            ffi::sqlite3_column_origin_name(raw_stmt, index),
            ffi::sqlite3_column_database_name(raw_stmt, index),
            ffi::sqlite3_column_name(raw_stmt, index),
            ffi::sqlite3_column_table_name(raw_stmt, index),
            ffi::sqlite3_column_decltype(raw_stmt, index),
        ] {
            roots.push(scope.root_nanbox_u64(sqlite_c_string_value(ptr).bits()));
        }
        for root in &roots {
            values.push(JSValue::from_bits(root.get_nanbox_u64()));
        }
        let obj = make_null_proto_object(&keys, &values);
        let next = js_array_push(
            result.get_raw_mut_ptr(),
            JSValue::object_ptr(obj as *mut u8),
        );
        result.set_raw_mut_ptr(next);
    }
    result.get_raw_mut_ptr::<ArrayHeader>()
}

pub(crate) unsafe fn bun_sqlite_statement_source(stmt_handle: Handle) -> *mut StringHeader {
    let stmt = get_handle::<BunSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    js_string_from_bytes(stmt.sql.as_ptr(), stmt.sql.len() as u32)
}
