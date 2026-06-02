//! Native bindings for the npm `better-sqlite3` package.
//!
//! First database driver port (Phase 5 step 10) and acceptance
//! test for perry-ffi's v0.5.x `JsValue` + object/array surface.
//! Functionally equivalent to `crates/perry-stdlib/src/sqlite.rs`
//! minus `db.transaction(fn)` — that needs perry-ffi's closure
//! invocation surface, which is the next-after-this expansion.
//! Users can still use explicit `BEGIN` / `COMMIT` /
//! `ROLLBACK` calls (those work via `db.exec()`).
//!
//! This crate is also the canonical reference for #424 (Tursodb
//! port) — `perryts/tursodb-bindings` will look almost identical
//! once it lands, with `rusqlite::Connection` swapped for
//! `tursodb::Connection`.

use perry_ffi::{
    alloc_string, build_object_shape, drop_handle, get_handle, js_array_alloc, js_array_push,
    js_object_alloc_with_shape, js_object_set_field, read_string, register_handle, ArrayHeader,
    Handle, JsString, JsValue, ObjectHeader, StringHeader,
};
use rusqlite::{types::Value as SqliteValue, Connection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub struct SqliteDbHandle {
    pub conn: Mutex<Connection>,
}

pub struct SqliteStmtHandle {
    pub sql: String,
    pub db_handle: Handle,
    /// Per-statement "raw" mode flag — `stmt.raw([toggle])` flips this.
    /// When true, `stmt.all(...)` returns row arrays instead of objects
    /// and `stmt.get(...)` returns a single row array. drizzle's
    /// `PreparedQuery.values()` (the standard with-fields path) calls
    /// `this.stmt.raw().all(...params)` to get raw rows for its own
    /// `mapResultRow(fields, row, joinsNotNullableMap)` mapping.
    pub raw_mode: AtomicBool,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// Convert a SQLite column value to a perry-ffi `JsValue`.
fn sqlite_value_to_jsvalue(value: &SqliteValue) -> JsValue {
    match value {
        SqliteValue::Null => JsValue::NULL,
        SqliteValue::Integer(n) => {
            if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                JsValue::from_int32(*n as i32)
            } else {
                JsValue::from_number(*n as f64)
            }
        }
        SqliteValue::Real(n) => JsValue::from_number(*n),
        SqliteValue::Text(s) => JsValue::from_string_ptr(alloc_string(s).as_raw()),
        SqliteValue::Blob(b) => {
            // Hex-encode blobs as a string (matches perry-stdlib's
            // existing convention; avoids pulling in `hex`).
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut out = Vec::with_capacity(b.len() * 2);
            for &byte in b {
                out.push(HEX[(byte >> 4) as usize]);
                out.push(HEX[(byte & 0x0f) as usize]);
            }
            // SAFETY: HEX bytes are ASCII, output is valid UTF-8.
            let s = unsafe { std::str::from_utf8_unchecked(&out) };
            JsValue::from_string_ptr(alloc_string(s).as_raw())
        }
    }
}

/// Read parameters out of a JS array of mixed-type values. The
/// codegen pads omitted-arg slots with `TAG_UNDEFINED` bits which
/// look like a non-null pointer; treat any pointer with non-zero
/// upper-16 bits as "no params" (matches perry-stdlib's existing
/// behavior).
unsafe fn params_from_array(arr_ptr: *const ArrayHeader) -> Vec<Box<dyn rusqlite::ToSql>> {
    if arr_ptr.is_null() {
        return vec![];
    }
    let upper16 = (arr_ptr as usize as u64) >> 48;
    if upper16 != 0 {
        return vec![];
    }
    let len = (*arr_ptr).length as usize;
    let elements = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(len);

    for i in 0..len {
        let bits = *elements.add(i);
        let val = JsValue::from_bits(bits);

        if val.is_null() || val.is_undefined() {
            params.push(Box::new(rusqlite::types::Null));
        } else if val.is_bool() {
            params.push(Box::new(if val.to_bool() { 1i64 } else { 0i64 }));
        } else if val.is_string() {
            let ptr = val.as_string_ptr();
            if let Some(s) = read_str(ptr) {
                params.push(Box::new(s));
            } else {
                params.push(Box::new(rusqlite::types::Null));
            }
        } else if val.is_int32() {
            params.push(Box::new(val.to_int32() as i64));
        } else {
            // Real f64 number — coerce to int64 if it's a whole
            // number in range, else pass through as f64.
            let n = val.to_number();
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                params.push(Box::new(n as i64));
            } else {
                params.push(Box::new(n));
            }
        }
    }

    params
}

/// `new Database(filename)` — open or create a SQLite database.
/// Returns a Handle to the connection, or `-1` on error.
///
/// # Safety
///
/// `filename_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_open(filename_ptr: *const StringHeader) -> Handle {
    let Some(filename) = read_str(filename_ptr) else {
        return -1;
    };

    let conn = if filename == ":memory:" {
        Connection::open_in_memory()
    } else {
        // Mobile-platform path resolution (sandboxed CWD on iOS)
        // is intentionally NOT replicated here — that's
        // Phase 5-batch UI work and lives in the perry-ext-ios
        // wrapper. CLI / server programs work fine with absolute
        // paths or paths relative to a writable CWD.
        Connection::open(&filename)
    };

    match conn {
        Ok(c) => register_handle(SqliteDbHandle {
            conn: Mutex::new(c),
        }),
        Err(_) => -1,
    }
}

/// `db.exec(sql)` — execute one or more SQL statements (no
/// parameters). Returns `1` on success, `0` on error.
///
/// # Safety
///
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_exec(db_handle: Handle, sql_ptr: *const StringHeader) -> i32 {
    let Some(sql) = read_str(sql_ptr) else {
        return 0;
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

/// `db.prepare(sql)` — prepare a statement. Returns a Handle to
/// the statement, or `-1` on error.
///
/// # Safety
///
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_prepare(
    db_handle: Handle,
    sql_ptr: *const StringHeader,
) -> Handle {
    let Some(sql) = read_str(sql_ptr) else {
        return -1;
    };
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

/// `stmt.raw([toggle])` — toggle raw mode for the statement. Returns
/// the same statement handle so `stmt.raw().all(...)` chains. Raw mode
/// makes subsequent `stmt.all(...)` / `stmt.get(...)` return rows as
/// arrays of column values instead of objects keyed by column name.
///
/// drizzle's `PreparedQuery.values()` chains `this.stmt.raw().all(...)`
/// to get back row arrays it then maps via
/// `mapResultRow(fields, row, joinsNotNullableMap)` against declared
/// field projections. Without this method, `stmt.raw` is undefined and
/// `stmt.raw().all(...)` surfaces as `(number).all is not a function`
/// because perry's runtime returns a number sentinel when calling
/// `undefined()` instead of throwing immediately.
///
/// Argument handling: drizzle only ever calls `.raw()` with no arg —
/// real better-sqlite3 also accepts an explicit boolean to disable
/// raw mode (`.raw(false)`). We don't carry the toggle through the
/// codegen's NativeMethodCall dispatch yet (it requires an
/// `NA_F64`/`NA_BOOL` slot), so the no-arg form is the only path
/// exercised. Conservative behavior: always enable on call. If a
/// downstream use needs explicit `.raw(false)`, extend the dispatch
/// arg list and read the toggle here.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_raw(stmt_handle: Handle) -> Handle {
    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        stmt.raw_mode.store(true, Ordering::Relaxed);
    }
    stmt_handle
}

/// `stmt.run(...params)` — execute a non-query statement with
/// parameters. Returns `{ changes, lastInsertRowid }` as an
/// ObjectHeader pointer, or null on error.
///
/// # Safety
///
/// `params_arr` must be null or a Perry-runtime `ArrayHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_run(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    let sqlite_params = params_from_array(params_arr);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        if let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) {
            if let Ok(conn) = db.conn.lock() {
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    sqlite_params.iter().map(|p| p.as_ref()).collect();
                if let Ok(changes) = conn.execute(&stmt.sql, param_refs.as_slice()) {
                    let last_id = conn.last_insert_rowid();
                    let (packed_keys, shape_id) =
                        build_object_shape(&["changes", "lastInsertRowid"]);
                    let result = js_object_alloc_with_shape(
                        shape_id,
                        2,
                        packed_keys.as_ptr(),
                        packed_keys.len() as u32,
                    );
                    js_object_set_field(result, 0, JsValue::from_number(changes as f64));
                    js_object_set_field(result, 1, JsValue::from_number(last_id as f64));
                    return result;
                }
            }
        }
    }
    std::ptr::null_mut()
}

/// `stmt.get(...params)` — return a single row, or `undefined`.
///
/// Returns `f64` carrying the NaN-boxed bits — perry-stdlib's
/// existing copy explains the SysV AMD64 ABI mismatch
/// (`#[repr(transparent)] u64` returns in RAX but the LLVM call
/// site expects `double` in XMM0). Same trick here.
///
/// # Safety
///
/// `params_arr` must be null or a Perry-runtime `ArrayHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    let sqlite_params = params_from_array(params_arr);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        if let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) {
            if let Ok(conn) = db.conn.lock() {
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    sqlite_params.iter().map(|p| p.as_ref()).collect();
                if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                    let column_names: Vec<String> = prepared
                        .column_names()
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    let column_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();

                    let mut rows = prepared.query(param_refs.as_slice());
                    if let Ok(ref mut rows) = rows {
                        if let Ok(Some(row)) = rows.next() {
                            if raw {
                                let mut row_arr = js_array_alloc(0);
                                for (idx, _) in column_names.iter().enumerate() {
                                    let value: SqliteValue =
                                        row.get(idx).unwrap_or(SqliteValue::Null);
                                    row_arr =
                                        js_array_push(row_arr, sqlite_value_to_jsvalue(&value));
                                }
                                return f64::from_bits(JsValue::from_object_ptr(row_arr).bits());
                            }
                            let (packed_keys, shape_id) = build_object_shape(&column_refs);
                            let obj = js_object_alloc_with_shape(
                                shape_id,
                                column_names.len() as u32,
                                packed_keys.as_ptr(),
                                packed_keys.len() as u32,
                            );
                            for (idx, _) in column_names.iter().enumerate() {
                                let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                                js_object_set_field(
                                    obj,
                                    idx as u32,
                                    sqlite_value_to_jsvalue(&value),
                                );
                            }
                            return f64::from_bits(JsValue::from_object_ptr(obj).bits());
                        }
                    }
                }
            }
        }
    }
    f64::from_bits(JsValue::UNDEFINED.bits())
}

/// `stmt.all(...params)` — return every row as an array of
/// objects.
///
/// # Safety
///
/// `params_arr` must be null or a Perry-runtime `ArrayHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    let sqlite_params = params_from_array(params_arr);
    let mut result_array = js_array_alloc(0);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        if let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) {
            if let Ok(conn) = db.conn.lock() {
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    sqlite_params.iter().map(|p| p.as_ref()).collect();
                if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                    let column_names: Vec<String> = prepared
                        .column_names()
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    let column_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();
                    // Only build the object shape in non-raw mode — raw rows
                    // skip the per-row object allocation entirely.
                    let object_shape = if raw {
                        None
                    } else {
                        Some(build_object_shape(&column_refs))
                    };

                    let mut rows = prepared.query(param_refs.as_slice());
                    if let Ok(ref mut rows) = rows {
                        while let Ok(Some(row)) = rows.next() {
                            if raw {
                                // Raw mode: each row is an array of column
                                // values in declared column order. drizzle's
                                // `mapResultRow(fields, row, …)` indexes by
                                // numeric position into this array.
                                let mut row_arr = js_array_alloc(0);
                                for (idx, _) in column_names.iter().enumerate() {
                                    let value: SqliteValue =
                                        row.get(idx).unwrap_or(SqliteValue::Null);
                                    row_arr =
                                        js_array_push(row_arr, sqlite_value_to_jsvalue(&value));
                                }
                                result_array =
                                    js_array_push(result_array, JsValue::from_object_ptr(row_arr));
                            } else {
                                let (packed_keys, shape_id) = object_shape.as_ref().unwrap();
                                let obj = js_object_alloc_with_shape(
                                    *shape_id,
                                    column_names.len() as u32,
                                    packed_keys.as_ptr(),
                                    packed_keys.len() as u32,
                                );
                                for (idx, _) in column_names.iter().enumerate() {
                                    let value: SqliteValue =
                                        row.get(idx).unwrap_or(SqliteValue::Null);
                                    js_object_set_field(
                                        obj,
                                        idx as u32,
                                        sqlite_value_to_jsvalue(&value),
                                    );
                                }
                                result_array =
                                    js_array_push(result_array, JsValue::from_object_ptr(obj));
                            }
                        }
                    }
                }
            }
        }
    }
    result_array
}

/// `db.pragma(name, value?)` — execute a `PRAGMA` and return the
/// first row's first column as a string.
///
/// # Safety
///
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_pragma(
    db_handle: Handle,
    pragma_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut StringHeader {
    let Some(pragma) = read_str(pragma_ptr) else {
        return std::ptr::null_mut();
    };
    let value = read_str(value_ptr);

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
                        // PRAGMA results can be string OR integer
                        // (e.g. `user_version` is int 0). Try string
                        // first, fall back to int → string. Matches
                        // node-better-sqlite3's pragma_results
                        // behavior; perry-stdlib's existing copy
                        // didn't handle the int case (returned "")
                        // — fixed in this port.
                        if let Ok(s) = row.get::<_, String>(0) {
                            return alloc_string(&s).as_raw();
                        }
                        if let Ok(n) = row.get::<_, i64>(0) {
                            return alloc_string(&n.to_string()).as_raw();
                        }
                        if let Ok(n) = row.get::<_, f64>(0) {
                            return alloc_string(&n.to_string()).as_raw();
                        }
                    }
                }
            }
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn js_sqlite_begin_transaction(db_handle: Handle) -> i32 {
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

#[no_mangle]
pub extern "C" fn js_sqlite_commit(db_handle: Handle) -> i32 {
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

#[no_mangle]
pub extern "C" fn js_sqlite_rollback(db_handle: Handle) -> i32 {
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

/// `db.close()` — drop the connection. Drops the handle from
/// the registry; subsequent uses become no-ops.
#[no_mangle]
pub extern "C" fn js_sqlite_close(db_handle: Handle) -> i32 {
    if drop_handle(db_handle) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_sqlite_in_transaction(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if !conn.is_autocommit() { 1 } else { 0 };
        }
    }
    0
}

/// Returns `1` if `handle` currently resolves to a `SqliteDbHandle` in
/// this crate's perry-ffi handle registry, `0` otherwise. Used by the
/// V8 bridge in `perry-jsruntime::bridge::native_object_to_v8` to
/// decide whether to materialize a `v8::Object` proxy with `prepare` /
/// `exec` / `pragma` / `close` / `transaction` method callbacks when a
/// sqlite Database crosses the native→V8 boundary (drizzle's
/// `BetterSQLiteSession` does `this.client.prepare(query.sql)` from
/// session.js — refs #1022).
#[no_mangle]
pub extern "C" fn js_sqlite_is_db_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteDbHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Returns `1` if `handle` currently resolves to a `SqliteStmtHandle`
/// in this crate's perry-ffi handle registry, `0` otherwise. Mirror
/// of `js_sqlite_is_db_handle` for the Statement side of the V8 proxy
/// materialization — drizzle's PreparedQuery calls `stmt.run(...)` /
/// `stmt.all(...)` / `stmt.get(...)` / `stmt.raw().all(...)` on the
/// handle returned from `client.prepare(...)`. Refs #1022.
#[no_mangle]
pub extern "C" fn js_sqlite_is_stmt_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteStmtHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_in_memory_db() -> Handle {
        let path = alloc_string(":memory:");
        unsafe { js_sqlite_open(path.as_raw() as *const _) }
    }

    #[test]
    fn open_in_memory_returns_valid_handle() {
        let h = make_in_memory_db();
        assert!(h > 0, "open returned {}", h);
    }

    #[test]
    fn exec_create_then_insert() {
        let h = make_in_memory_db();
        let create = alloc_string("CREATE TABLE t (id INTEGER, name TEXT)");
        let r = unsafe { js_sqlite_exec(h, create.as_raw() as *const _) };
        assert_eq!(r, 1);
        let insert = alloc_string("INSERT INTO t VALUES (1, 'alice')");
        let r = unsafe { js_sqlite_exec(h, insert.as_raw() as *const _) };
        assert_eq!(r, 1);
    }

    #[test]
    fn pragma_returns_value() {
        let h = make_in_memory_db();
        let pragma = alloc_string("user_version");
        let result_ptr =
            unsafe { js_sqlite_pragma(h, pragma.as_raw() as *const _, std::ptr::null()) };
        assert!(!result_ptr.is_null());
        let s = read_string(unsafe { JsString::from_raw(result_ptr) }).expect("non-null");
        assert_eq!(s, "0");
    }

    #[test]
    fn transactions() {
        let h = make_in_memory_db();
        let create = alloc_string("CREATE TABLE t (id INTEGER)");
        unsafe { js_sqlite_exec(h, create.as_raw() as *const _) };
        assert_eq!(js_sqlite_begin_transaction(h), 1);
        assert_eq!(js_sqlite_in_transaction(h), 1);
        assert_eq!(js_sqlite_commit(h), 1);
        assert_eq!(js_sqlite_in_transaction(h), 0);
    }

    #[test]
    fn close_drops_handle() {
        let h = make_in_memory_db();
        assert_eq!(js_sqlite_close(h), 1);
        // Second close is a no-op (handle already gone).
        assert_eq!(js_sqlite_close(h), 0);
    }
}
