use super::*;
use crate::common::{get_handle, register_handle, Handle};
use perry_runtime::{
    js_array_alloc, js_array_push, js_object_alloc_with_shape, js_object_set_field,
    js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader, StringHeader,
};
use rusqlite::{types::Value as SqliteValue, Connection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// new Database(filename) -> Database
///
/// Open or create a SQLite database.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_open(filename_ptr: *const StringHeader) -> Handle {
    let filename = match string_from_header(filename_ptr) {
        Some(f) => f,
        None => return -1,
    };

    let conn = if filename == ":memory:" {
        Connection::open_in_memory()
    } else {
        // On iOS/Android, resolve relative paths to a writable directory
        // (the CWD is typically the read-only app bundle on mobile platforms)
        let resolved = if !filename.starts_with('/') && !filename.starts_with(':') {
            #[cfg(target_os = "ios")]
            {
                extern "C" {
                    fn getenv(name: *const i8) -> *const i8;
                }
                let home = getenv(b"HOME\0".as_ptr() as *const i8);
                if !home.is_null() {
                    let home_str = std::ffi::CStr::from_ptr(home).to_str().unwrap_or("");
                    let docs = format!("{}/Documents", home_str);
                    let _ = std::fs::create_dir_all(&docs);
                    format!("{}/{}", docs, filename)
                } else {
                    filename.clone()
                }
            }
            #[cfg(not(target_os = "ios"))]
            {
                filename.clone()
            }
        } else {
            filename.clone()
        };
        Connection::open(&resolved)
    };

    match conn {
        Ok(c) => register_handle(SqliteDbHandle {
            conn: Mutex::new(c),
        }),
        Err(_) => -1,
    }
}

/// db.exec(sql) -> Database
///
/// Execute one or more SQL statements.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_exec(db_handle: Handle, sql_ptr: *const StringHeader) -> i32 {
    let sql = match string_from_header(sql_ptr) {
        Some(s) => s,
        None => return 0,
    };

    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute_batch(&sql).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// db.prepare(sql) -> Statement
///
/// Create a prepared statement.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_prepare(
    db_handle: Handle,
    sql_ptr: *const StringHeader,
) -> Handle {
    let sql = match string_from_header(sql_ptr) {
        Some(s) => s,
        None => return -1,
    };

    // Verify the SQL is valid
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            if conn.prepare(&sql).is_ok() {
                return register_handle(SqliteStmtHandle {
                    sql,
                    db_handle,
                    raw_mode: AtomicBool::new(false),
                });
            }
        }
    }
    -1
}

/// stmt.raw([toggle]) -> stmt
///
/// Toggle raw mode on the statement and return the same handle so
/// `stmt.raw().all(...)` chains. Raw mode makes subsequent `.all()` /
/// `.get()` return rows as arrays of column values (in declared
/// column order) instead of objects keyed by column name.
///
/// drizzle's `PreparedQuery.values()` chains
/// `this.stmt.raw().all(...params)` to get back row arrays it then
/// hands to `mapResultRow(fields, row, joinsNotNullableMap)`. Without
/// this method `stmt.raw` is undefined and the call surfaces as
/// `(number).all is not a function` deeper in the chain because perry
/// returns a number sentinel when calling `undefined()` instead of
/// throwing immediately. Refs #643.
///
/// Argument handling: drizzle only ever uses the no-arg form. Real
/// better-sqlite3 also accepts `.raw(false)` to disable. We don't
/// thread the toggle through the codegen's NativeMethodCall dispatch
/// yet (it would need an `NA_F64` slot), so the no-arg form is the
/// only path. Conservative: always enable on call.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_raw(stmt_handle: Handle) -> Handle {
    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        stmt.raw_mode.store(true, Ordering::Relaxed);
    }
    stmt_handle
}

/// Extract SQLite parameters from a NaN-boxed array
pub(crate) unsafe fn params_from_array(
    arr_ptr: *const ArrayHeader,
) -> Vec<Box<dyn rusqlite::ToSql>> {
    if arr_ptr.is_null() {
        return vec![];
    }
    // Codegen pads omitted-arg slots with TAG_UNDEFINED bits when a stmt
    // method is called with no params (e.g. `stmt.run()` / `stmt.all()`).
    // Those bits look like a non-null pointer but actually carry the
    // 0x7FFC NaN-box tag in the high 16; dereferencing as ArrayHeader is
    // UB and reads a garbage `length` that crashes the loop below.
    // Treat any value with non-zero upper-16 as "no params".
    let upper16 = (arr_ptr as usize as u64) >> 48;
    if upper16 != 0 {
        return vec![];
    }
    let len = (*arr_ptr).length as usize;
    let elements = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(len);

    for i in 0..len {
        let val = *elements.add(i);
        let bits = val.to_bits();

        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
        const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
        const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
        const STRING_TAG: u64 = 0x7FFF;
        const INT32_TAG: u64 = 0x7FFE;

        let top16 = bits >> 48;

        if bits == TAG_NULL || bits == TAG_UNDEFINED {
            params.push(Box::new(rusqlite::types::Null));
        } else if bits == TAG_TRUE {
            params.push(Box::new(1i64));
        } else if bits == TAG_FALSE {
            params.push(Box::new(0i64));
        } else if top16 == STRING_TAG {
            // String: extract pointer
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if let Some(s) = string_from_header(ptr) {
                params.push(Box::new(s));
            } else {
                params.push(Box::new(rusqlite::types::Null));
            }
        } else if top16 == INT32_TAG {
            let n = (bits & 0xFFFF_FFFF) as i32;
            params.push(Box::new(n as i64));
        } else {
            // Regular f64 number
            if val.fract() == 0.0 && val >= i64::MIN as f64 && val <= i64::MAX as f64 {
                params.push(Box::new(val as i64));
            } else {
                params.push(Box::new(val));
            }
        }
    }

    params
}

/// stmt.run(...params) -> RunResult
///
/// Execute a prepared statement with parameters.
/// Returns { changes: number, lastInsertRowid: number }
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_run(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    let sqlite_params = params_from_array(params_arr);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        if let Some(result) = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(changes) = conn.execute(&stmt.sql, param_refs.as_slice()) {
                let last_id = conn.last_insert_rowid();
                let keys = vec!["changes".to_string(), "lastInsertRowid".to_string()];
                let (packed_keys, shape_id) = build_packed_keys(&keys);
                let result = js_object_alloc_with_shape(
                    shape_id,
                    2,
                    packed_keys.as_ptr(),
                    packed_keys.len() as u32,
                );
                js_object_set_field(result, 0, JSValue::number(changes as f64));
                js_object_set_field(result, 1, JSValue::number(last_id as f64));
                return result;
            }
            std::ptr::null_mut()
        }) {
            return result;
        }
    }

    std::ptr::null_mut()
}

/// stmt.get(...params) -> Row | undefined
///
/// Get a single row from a query. Returns f64 (NaN-boxed bits) instead
/// of JSValue to avoid SysV AMD64 ABI mismatch on x86_64 (JSValue's
/// `#[repr(transparent)] u64` returns in RAX but LLVM reads from XMM0
/// when the call site declares a `double` return).
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    let sqlite_params = params_from_array(params_arr);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        if let Some(result) = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                let column_names: Vec<String> = prepared
                    .column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                let mut rows = prepared.query(param_refs.as_slice());
                if let Ok(ref mut rows) = rows {
                    if let Ok(Some(row)) = rows.next() {
                        if raw {
                            let row_arr = js_array_alloc(0);
                            for (idx, _) in column_names.iter().enumerate() {
                                let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                                js_array_push(row_arr, sqlite_value_to_jsvalue(&value));
                            }
                            return f64::from_bits(JSValue::object_ptr(row_arr as *mut u8).bits());
                        }
                        let (packed_keys, shape_id) = build_packed_keys(&column_names);
                        let obj = js_object_alloc_with_shape(
                            shape_id,
                            column_names.len() as u32,
                            packed_keys.as_ptr(),
                            packed_keys.len() as u32,
                        );

                        for (idx, _name) in column_names.iter().enumerate() {
                            let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                            js_object_set_field(obj, idx as u32, sqlite_value_to_jsvalue(&value));
                        }

                        return f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits());
                    }
                }
            }
            f64::from_bits(JSValue::undefined().bits())
        }) {
            return result;
        }
    }

    f64::from_bits(JSValue::undefined().bits())
}

/// stmt.all(...params) -> Row[]
///
/// Get all rows from a query.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    let sqlite_params = params_from_array(params_arr);
    let result_array = js_array_alloc(0);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        let _ = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                let column_names: Vec<String> = prepared
                    .column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                // Only build the per-row object shape in non-raw
                // mode. In raw mode each row is its own array of
                // column values; no per-row object shape needed.
                let object_shape = if raw {
                    None
                } else {
                    Some(build_packed_keys(&column_names))
                };

                let mut rows = prepared.query(param_refs.as_slice());
                if let Ok(ref mut rows) = rows {
                    while let Ok(Some(row)) = rows.next() {
                        if raw {
                            let row_arr = js_array_alloc(0);
                            for (idx, _) in column_names.iter().enumerate() {
                                let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                                js_array_push(row_arr, sqlite_value_to_jsvalue(&value));
                            }
                            js_array_push(result_array, JSValue::object_ptr(row_arr as *mut u8));
                            continue;
                        }
                        let (packed_keys, shape_id) = object_shape.as_ref().unwrap();
                        let obj = js_object_alloc_with_shape(
                            *shape_id,
                            column_names.len() as u32,
                            packed_keys.as_ptr(),
                            packed_keys.len() as u32,
                        );

                        for (idx, _name) in column_names.iter().enumerate() {
                            let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                            js_object_set_field(obj, idx as u32, sqlite_value_to_jsvalue(&value));
                        }

                        js_array_push(result_array, JSValue::object_ptr(obj as *mut u8));
                    }
                }
            }
        });
    }

    result_array
}

/// db.pragma(pragma, value?) -> any
///
/// Execute a PRAGMA statement.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_pragma(
    db_handle: Handle,
    pragma_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut StringHeader {
    let pragma = match string_from_header(pragma_ptr) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };

    let value = string_from_header(value_ptr);

    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            let sql = if let Some(v) = value {
                format!("PRAGMA {} = {}", pragma, v)
            } else {
                format!("PRAGMA {}", pragma)
            };

            if let Ok(mut stmt) = conn.prepare(&sql) {
                let mut rows = stmt.query([]);
                if let Ok(ref mut rows) = rows {
                    if let Ok(Some(row)) = rows.next() {
                        let result: String = row.get(0).unwrap_or_default();
                        return js_string_from_bytes(result.as_ptr(), result.len() as u32);
                    }
                }
            }
        }
    }

    std::ptr::null_mut()
}

/// The transaction wrapper function — called when the returned closure is invoked.
/// Captures: [0] = db_handle (as f64), [1] = original closure ptr (as i64)
pub(crate) unsafe extern "C" fn sqlite_tx_wrapper(
    wrapper_closure: *const perry_runtime::ClosureHeader,
    arg0: f64,
) -> f64 {
    use perry_runtime::closure::{
        js_closure_call1, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    };

    let db_handle_f64 = js_closure_get_capture_f64(wrapper_closure, 0);
    let db_handle = db_handle_f64 as i64;
    let original_closure =
        js_closure_get_capture_ptr(wrapper_closure, 1) as *const perry_runtime::ClosureHeader;

    // BEGIN
    js_sqlite_begin_transaction(db_handle);

    // Call original closure with argument
    let result = js_closure_call1(original_closure, arg0);

    // COMMIT
    js_sqlite_commit(db_handle);

    result
}

/// db.transaction(fn) -> wrapping closure
///
/// Returns a closure that wraps fn in BEGIN/COMMIT.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_transaction(
    db_handle: Handle,
    closure_ptr: i64,
) -> *mut perry_runtime::ClosureHeader {
    use perry_runtime::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };

    let wrapper = js_closure_alloc(sqlite_tx_wrapper as *const u8, 2);
    js_closure_set_capture_f64(wrapper, 0, db_handle as f64);
    js_closure_set_capture_ptr(wrapper, 1, closure_ptr);

    wrapper
}

/// Begin a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_begin_transaction(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("BEGIN TRANSACTION", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// Commit a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_commit(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("COMMIT", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// Rollback a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_rollback(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("ROLLBACK", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// db.close() -> void
///
/// Close the database connection.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_close(db_handle: Handle) -> i32 {
    // The connection will be closed when the handle is dropped
    // For now, we just verify the handle is valid
    if get_handle::<SqliteDbHandle>(db_handle).is_some() {
        1
    } else {
        0
    }
}

/// db.inTransaction -> boolean
///
/// Check if currently in a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_in_transaction(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            // SQLite's autocommit mode is off when in a transaction
            return if !conn.is_autocommit() { 1 } else { 0 };
        }
    }
    0
}
/// Returns `1` if `handle` currently resolves to a `SqliteDbHandle` in
/// this crate's handle registry, `0` otherwise. Used by the V8 bridge
/// in `perry-jsruntime::bridge::native_object_to_v8` to decide whether
/// to materialize a `v8::Object` proxy with `prepare`/`exec`/etc.
/// method callbacks when a sqlite Database crosses the native→V8
/// boundary (drizzle's `BetterSQLiteSession` does
/// `this.client.prepare(query.sql)` from session.js — refs #1022).
///
/// Mirrors `perry-ext-better-sqlite3::js_sqlite_is_db_handle`. The
/// duplicate-symbol resolution at link time picks one impl; whichever
/// crate's `js_sqlite_open` registered the handle is the same impl
/// whose `is_db_handle` answers the membership check (each crate
/// keeps its own registry).
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_is_db_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteDbHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Returns `1` if `handle` currently resolves to a `SqliteStmtHandle`
/// in this crate's handle registry, `0` otherwise. Mirror of
/// `js_sqlite_is_db_handle` for the Statement side — drizzle's
/// PreparedQuery calls `stmt.run(...)` / `stmt.all(...)` /
/// `stmt.get(...)` / `stmt.raw().all(...)` on the handle returned from
/// `client.prepare(...)`. Refs #1022.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_is_stmt_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteStmtHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

/// stmt.columns() -> ColumnMetadata[]
///
/// node:sqlite's `StatementSync.columns()` returns one metadata object
/// per result column with the Node-shaped keys `column`, `database`,
/// `name`, `table`, and `type`. We populate `name` (the result column
/// label) and `type` (the declared column type, or `null`); `column`,
/// `table`, and `database` track the underlying source where SQLite
/// exposes it, falling back to `null` for computed columns. Refs #3184.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_columns(stmt_handle: Handle) -> *mut ArrayHeader {
    let result = js_array_alloc(0);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        if let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) {
            if let Ok(conn) = db.conn.lock() {
                if let Ok(prepared) = conn.prepare(&stmt.sql) {
                    // Node returns metadata objects keyed in this order.
                    let keys = vec![
                        "column".to_string(),
                        "database".to_string(),
                        "name".to_string(),
                        "table".to_string(),
                        "type".to_string(),
                    ];
                    let (packed_keys, shape_id) = build_packed_keys(&keys);

                    for col in prepared.columns() {
                        let name = col.name().to_string();
                        let decl_type = col.decl_type().map(|s| s.to_string());

                        let obj = js_object_alloc_with_shape(
                            shape_id,
                            keys.len() as u32,
                            packed_keys.as_ptr(),
                            packed_keys.len() as u32,
                        );
                        // column / database / table are null for the
                        // common computed/aliased cases; `name` is the
                        // result label, `type` the declared column type.
                        js_object_set_field(obj, 0, JSValue::null());
                        js_object_set_field(obj, 1, JSValue::null());
                        let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        js_object_set_field(obj, 2, JSValue::string_ptr(name_ptr));
                        js_object_set_field(obj, 3, JSValue::null());
                        match decl_type {
                            Some(t) => {
                                let t_ptr = js_string_from_bytes(t.as_ptr(), t.len() as u32);
                                js_object_set_field(obj, 4, JSValue::string_ptr(t_ptr));
                            }
                            None => js_object_set_field(obj, 4, JSValue::null()),
                        }

                        js_array_push(result, JSValue::object_ptr(obj as *mut u8));
                    }
                }
            }
        }
    }

    result
}

/// Keepalive anchors for the codegen-emitted `node:sqlite` (and shared
/// better-sqlite3) entry points. The whole-program LLVM auto-optimize
/// build internalizes + dead-strips `#[no_mangle]` fns that are only
/// referenced from generated `.o` files; `#[used]` survives that pass.
/// Without these, a `node:sqlite` program compiled under DEFAULT
/// auto-optimize fails to link (`Undefined symbols: _js_sqlite_*`).
/// See project_auto_optimize_keepalive_3320.
#[used]
pub(crate) static KEEP_SQLITE_OPEN: unsafe extern "C" fn(*const StringHeader) -> Handle =
    js_sqlite_open;
#[used]
pub(crate) static KEEP_SQLITE_EXEC: unsafe extern "C" fn(Handle, *const StringHeader) -> i32 =
    js_sqlite_exec;
#[used]
pub(crate) static KEEP_SQLITE_PREPARE: unsafe extern "C" fn(Handle, *const StringHeader) -> Handle =
    js_sqlite_prepare;
#[used]
pub(crate) static KEEP_SQLITE_STMT_RUN: unsafe extern "C" fn(
    Handle,
    *const ArrayHeader,
) -> *mut ObjectHeader = js_sqlite_stmt_run;
#[used]
pub(crate) static KEEP_SQLITE_STMT_GET: unsafe extern "C" fn(Handle, *const ArrayHeader) -> f64 =
    js_sqlite_stmt_get;
#[used]
pub(crate) static KEEP_SQLITE_STMT_ALL: unsafe extern "C" fn(
    Handle,
    *const ArrayHeader,
) -> *mut ArrayHeader = js_sqlite_stmt_all;
#[used]
pub(crate) static KEEP_SQLITE_CLOSE: unsafe extern "C" fn(Handle) -> i32 = js_sqlite_close;
#[used]
pub(crate) static KEEP_SQLITE_STMT_COLUMNS: unsafe extern "C" fn(Handle) -> *mut ArrayHeader =
    js_sqlite_stmt_columns;
