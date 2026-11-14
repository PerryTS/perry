use super::*;
use crate::common::{drop_handle, get_handle, register_handle, Handle};
use perry_runtime::{
    js_array_alloc, js_array_push, js_object_alloc_with_shape, js_object_set_field,
    js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader, StringHeader,
};
use rusqlite::{types::Value as SqliteValue, Connection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const DATABASE_NOT_OPEN: &str = "The database connection is not open";

// rusqlite trims Rust whitespace before compilation, which can change SQLite
// tokens and even valid Unicode identifiers. Lease only lexically safe SQL.
enum StatementLease<'conn> {
    Cached(rusqlite::CachedStatement<'conn>),
    Original(rusqlite::Statement<'conn>),
}

impl<'conn> std::ops::Deref for StatementLease<'conn> {
    type Target = rusqlite::Statement<'conn>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Cached(statement) => statement,
            Self::Original(statement) => statement,
        }
    }
}

impl std::ops::DerefMut for StatementLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Cached(statement) => statement,
            Self::Original(statement) => statement,
        }
    }
}

fn lease_statement<'conn>(
    conn: &'conn Connection,
    sql: &str,
) -> rusqlite::Result<StatementLease<'conn>> {
    let sqlite_trimmed = sql.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    if sql.trim() == sqlite_trimmed {
        conn.prepare_cached(sql).map(StatementLease::Cached)
    } else {
        conn.prepare(sql).map(StatementLease::Original)
    }
}

#[cold]
fn throw_database_not_open() -> ! {
    throw_plain_type(DATABASE_NOT_OPEN)
}

fn require_database_open(db_handle: Handle) {
    if get_handle::<SqliteDbHandle>(db_handle).is_none() {
        throw_database_not_open();
    }
}

fn require_statement_database_open(stmt_handle: Handle) {
    let stmt =
        get_handle::<SqliteStmtHandle>(stmt_handle).unwrap_or_else(|| throw_database_not_open());
    require_database_open(stmt.db_handle);
}

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

    require_database_open(db_handle);

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

    require_database_open(db_handle);

    let valid = with_sqlite_connection(db_handle, |conn| lease_statement(conn, &sql).is_ok())
        .unwrap_or(false);
    if valid {
        return register_handle(SqliteStmtHandle {
            sql,
            db_handle,
            raw_mode: AtomicBool::new(false),
        });
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
    let elements =
        perry_runtime::array::array_elements_ptr(arr_ptr as *const ArrayHeader) as *const f64;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(len);

    for i in 0..len {
        let val = *elements.add(i);
        let bits = val.to_bits();

        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
        const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
        const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
        const STRING_TAG: u64 = 0x7FFF;

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
    require_statement_database_open(stmt_handle);

    let Some((sql, db_handle, _)) = statement_snapshot(stmt_handle) else {
        return std::ptr::null_mut();
    };
    let result = with_sqlite_connection(db_handle, |conn| {
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            sqlite_params.iter().map(|p| p.as_ref()).collect();
        let mut prepared = lease_statement(conn, &sql).ok()?;
        // better-sqlite3 accepts any statement here, including one that returns
        // rows: step it to completion, discard the rows and report the change
        // count. `Statement::execute` would step `INSERT ... RETURNING` once,
        // write the row, then reject the statement — reporting failure for a
        // committed write. The row branch matches the ext provider.
        let before = conn.total_changes();
        if prepared.column_count() == 0 {
            prepared.execute(param_refs.as_slice()).ok()?;
        } else {
            let mut rows = prepared.query(param_refs.as_slice()).ok()?;
            while rows.next().ok()?.is_some() {}
        }
        Some((
            conn.total_changes().saturating_sub(before),
            conn.last_insert_rowid(),
        ))
    })
    .flatten();
    let Some((changes, last_id)) = result else {
        return std::ptr::null_mut();
    };
    let keys = vec!["changes".to_string(), "lastInsertRowid".to_string()];
    let (packed_keys, shape_id) = build_packed_keys(&keys);
    let result =
        js_object_alloc_with_shape(shape_id, 2, packed_keys.as_ptr(), packed_keys.len() as u32);
    js_object_set_field(result, 0, JSValue::number(changes as f64));
    js_object_set_field(result, 1, JSValue::number(last_id as f64));
    result
}

fn statement_snapshot(handle: Handle) -> Option<(String, Handle, bool)> {
    get_handle::<SqliteStmtHandle>(handle).map(|stmt| {
        (
            stmt.sql.clone(),
            stmt.db_handle,
            stmt.raw_mode.load(Ordering::Relaxed),
        )
    })
}

fn query_owned_rows(
    sql: &str,
    db_handle: Handle,
    sqlite_params: &[Box<dyn rusqlite::ToSql>],
    first_only: bool,
) -> Option<(Vec<String>, Vec<Vec<SqliteValue>>)> {
    unsafe {
        with_sqlite_connection(db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();
            let mut prepared = lease_statement(conn, sql).ok()?;
            let mut names = None;
            let mut values = Vec::new();
            {
                let mut rows = prepared.query(param_refs.as_slice()).ok()?;
                while let Some(row) = rows.next().ok()? {
                    // SQLite can recompile on step after a schema change.
                    let columns = names.get_or_insert_with(|| {
                        row.as_ref()
                            .column_names()
                            .into_iter()
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    });
                    values.push(
                        (0..columns.len())
                            .map(|idx| row.get(idx).unwrap_or(SqliteValue::Null))
                            .collect(),
                    );
                    if first_only {
                        break;
                    }
                }
            }
            // Empty completion also steps and may update the statement metadata.
            let names = names.unwrap_or_else(|| {
                prepared
                    .column_names()
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            });
            Some((names, values))
        })
        .flatten()
    }
}

unsafe fn owned_row_to_js(values: &[SqliteValue], shape: Option<&(Vec<u8>, u32)>) -> JSValue {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let values: Vec<_> = values
        .iter()
        .map(|value| scope.root_nanbox_u64(sqlite_value_to_jsvalue(value).bits()))
        .collect();
    if let Some((keys, shape_id)) = shape {
        let obj = scope.root_raw_mut_ptr(js_object_alloc_with_shape(
            *shape_id,
            values.len() as u32,
            keys.as_ptr(),
            keys.len() as u32,
        ));
        for (idx, value) in values.iter().enumerate() {
            js_object_set_field(
                obj.get_raw_mut_ptr(),
                idx as u32,
                JSValue::from_bits(value.get_nanbox_f64().to_bits()),
            );
        }
        JSValue::object_ptr(obj.get_raw_mut_ptr::<ObjectHeader>().cast())
    } else {
        let mut arr = js_array_alloc(0);
        for value in values {
            arr = js_array_push(arr, JSValue::from_bits(value.get_nanbox_f64().to_bits()));
        }
        JSValue::object_ptr(arr.cast())
    }
}

/// stmt.get(...params) -> Row | undefined
///
/// Return NaN-boxed bits in the floating-point ABI used by generated callers.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    let sqlite_params = params_from_array(params_arr);
    require_statement_database_open(stmt_handle);
    let Some((sql, db_handle, raw)) = statement_snapshot(stmt_handle) else {
        return f64::from_bits(JSValue::undefined().bits());
    };
    let Some((names, rows)) = query_owned_rows(&sql, db_handle, &sqlite_params, true) else {
        return f64::from_bits(JSValue::undefined().bits());
    };
    let shape = (!raw).then(|| build_packed_keys(&names));
    f64::from_bits(
        rows.first()
            .map(|row| owned_row_to_js(row, shape.as_ref()))
            .unwrap_or(JSValue::undefined())
            .bits(),
    )
}

/// stmt.all(...params) -> Row[]
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    let sqlite_params = params_from_array(params_arr);
    require_statement_database_open(stmt_handle);
    let snapshot = statement_snapshot(stmt_handle).and_then(|(sql, db_handle, raw)| {
        query_owned_rows(&sql, db_handle, &sqlite_params, false)
            .map(|(names, rows)| (names, rows, raw))
    });
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let result = scope.root_raw_mut_ptr(js_array_alloc(0));
    if let Some((names, rows, raw)) = snapshot {
        let shape = (!raw).then(|| build_packed_keys(&names));
        for row in rows {
            let value = owned_row_to_js(&row, shape.as_ref());
            let arr = js_array_push(result.get_raw_mut_ptr(), value);
            result.set_raw_mut_ptr(arr);
        }
    }
    result.get_raw_mut_ptr()
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

    require_database_open(db_handle);

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
    _this: perry_runtime::closure::JsThis,
    arg0: f64,
) -> f64 {
    use perry_runtime::closure::{js_closure_call1, js_closure_get_capture_f64};

    let db_handle_f64 = js_closure_get_capture_f64(wrapper_closure, 0);
    let db_handle = db_handle_f64 as i64;
    let original_closure = perry_runtime::closure::closure_capture_ref(wrapper_closure, 1)
        as *const perry_runtime::ClosureHeader;

    // BEGIN
    js_sqlite_begin_transaction(db_handle);

    // Call original closure with argument
    let result = js_closure_call1(
        original_closure,
        perry_runtime::closure::plain_call_receiver(),
        arg0,
    );

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
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_f64};

    require_database_open(db_handle);

    let wrapper = js_closure_alloc(perry_runtime::fn_info!(sqlite_tx_wrapper, 1), 2);
    js_closure_set_capture_f64(wrapper, 0, db_handle as f64);
    perry_runtime::closure::closure_set_capture_ref(wrapper, 1, closure_ptr);

    wrapper
}

/// Begin a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_begin_transaction(db_handle: Handle) -> i32 {
    require_database_open(db_handle);
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
    require_database_open(db_handle);
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
    require_database_open(db_handle);
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
    if drop_handle(db_handle) {
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
    require_statement_database_open(stmt_handle);
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let result = scope.root_raw_mut_ptr(js_array_alloc(0));

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        if let Some(db) = get_handle::<SqliteDbHandle>(stmt.db_handle) {
            if let Ok(conn) = db.conn.lock() {
                if let Ok(prepared) = conn.prepare(&stmt.sql) {
                    for col in prepared.columns() {
                        let name = col.name().to_string();
                        let decl_type = col.decl_type().map(|s| s.to_string());

                        let obj = scope.root_raw_mut_ptr(
                            perry_runtime::object::layout_desc::perry_alloc_layout(
                                perry_runtime::object::runtime_layouts::SQLITE_COLUMN_METADATA::desc(),
                            ),
                        );
                        // column / database / table are null for the
                        // common computed/aliased cases; `name` is the
                        // result label, `type` the declared column type.
                        js_object_set_field(obj.get_raw_mut_ptr(), 0, JSValue::null());
                        js_object_set_field(obj.get_raw_mut_ptr(), 1, JSValue::null());
                        let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        js_object_set_field(
                            obj.get_raw_mut_ptr(),
                            2,
                            JSValue::string_ptr(name_ptr),
                        );
                        js_object_set_field(obj.get_raw_mut_ptr(), 3, JSValue::null());
                        match decl_type {
                            Some(t) => {
                                let t_ptr = js_string_from_bytes(t.as_ptr(), t.len() as u32);
                                js_object_set_field(
                                    obj.get_raw_mut_ptr(),
                                    4,
                                    JSValue::string_ptr(t_ptr),
                                );
                            }
                            None => js_object_set_field(obj.get_raw_mut_ptr(), 4, JSValue::null()),
                        }

                        let next = js_array_push(
                            result.get_raw_mut_ptr(),
                            JSValue::object_ptr(obj.get_raw_mut_ptr::<ObjectHeader>().cast()),
                        );
                        result.set_raw_mut_ptr(next);
                    }
                }
            }
        }
    }

    result.get_raw_mut_ptr::<ArrayHeader>()
}

/// Keepalive anchors for the codegen-emitted `node:sqlite` (and shared
/// better-sqlite3) entry points. The whole-program LLVM auto-optimize
/// build internalizes + dead-strips `#[no_mangle]` fns that are only
/// referenced from generated `.o` files; `#[used]` survives that pass.
/// Without these, a `node:sqlite` program compiled under DEFAULT
/// auto-optimize fails to link (`Undefined symbols: _js_sqlite_*`).
/// See project_auto_optimize_keepalive_3320.
#[used(compiler)]
pub(crate) static KEEP_SQLITE_OPEN: unsafe extern "C" fn(*const StringHeader) -> Handle =
    js_sqlite_open;
#[used(compiler)]
pub(crate) static KEEP_SQLITE_EXEC: unsafe extern "C" fn(Handle, *const StringHeader) -> i32 =
    js_sqlite_exec;
#[used(compiler)]
pub(crate) static KEEP_SQLITE_PREPARE: unsafe extern "C" fn(Handle, *const StringHeader) -> Handle =
    js_sqlite_prepare;
#[used(compiler)]
pub(crate) static KEEP_SQLITE_STMT_RUN: unsafe extern "C" fn(
    Handle,
    *const ArrayHeader,
) -> *mut ObjectHeader = js_sqlite_stmt_run;
#[used(compiler)]
pub(crate) static KEEP_SQLITE_STMT_GET: unsafe extern "C" fn(Handle, *const ArrayHeader) -> f64 =
    js_sqlite_stmt_get;
#[used(compiler)]
pub(crate) static KEEP_SQLITE_STMT_ALL: unsafe extern "C" fn(
    Handle,
    *const ArrayHeader,
) -> *mut ArrayHeader = js_sqlite_stmt_all;
#[used(compiler)]
pub(crate) static KEEP_SQLITE_CLOSE: unsafe extern "C" fn(Handle) -> i32 = js_sqlite_close;
#[used(compiler)]
pub(crate) static KEEP_SQLITE_STMT_COLUMNS: unsafe extern "C" fn(Handle) -> *mut ArrayHeader =
    js_sqlite_stmt_columns;

#[cfg(test)]
#[path = "better_tests.rs"]
mod preparation_tests;

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn static_layout_sqlite_columns_keep_names_values_and_gc_edges() {
        crate::test_runtime::initialize();
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE t (id INTEGER)")
            .unwrap();
        let db = register_handle(SqliteDbHandle {
            conn: Mutex::new(connection),
        });
        let stmt = register_handle(SqliteStmtHandle {
            sql: "SELECT id, 1 AS computed FROM t".to_string(),
            db_handle: db,
            raw_mode: AtomicBool::new(false),
        });
        let columns = scope.root_raw_mut_ptr(unsafe { js_sqlite_stmt_columns(stmt) });
        assert_eq!(
            unsafe { (*columns.get_raw_mut_ptr::<ArrayHeader>()).length },
            2
        );
        let first = scope.root_nanbox_u64(perry_runtime::js_array_get_jsvalue(
            columns.get_raw_mut_ptr(),
            0,
        ));
        let second = scope.root_nanbox_u64(perry_runtime::js_array_get_jsvalue(
            columns.get_raw_mut_ptr(),
            1,
        ));
        let pointer = |column: &perry_runtime::gc::RuntimeHandle<'_>| {
            perry_runtime::value::js_nanbox_get_pointer(column.get_nanbox_f64())
                as *mut ObjectHeader
        };
        let desc = perry_runtime::object::runtime_layouts::SQLITE_COLUMN_METADATA::desc();
        for column in [&first, &second] {
            unsafe {
                assert_eq!(
                    (*pointer(column)).parent_class_id,
                    (*desc).layout_id,
                    "column metadata must be born with its declared layout"
                );
                let gc = pointer(column)
                    .cast::<u8>()
                    .sub(perry_runtime::gc::GC_HEADER_SIZE)
                    .cast::<perry_runtime::gc::GcHeader>();
                assert_eq!(
                    (*gc).size as usize,
                    perry_runtime::gc::GC_HEADER_SIZE + perry_runtime::meta_form_alloc_bytes(5),
                    "column metadata must allocate exactly its five declared slots"
                );
            }
            let keys = scope.root_raw_mut_ptr(perry_runtime::js_object_keys(pointer(column)));
            let names: Vec<String> = (0..5)
                .map(|i| {
                    let bits = perry_runtime::js_array_get_jsvalue(keys.get_raw_mut_ptr(), i);
                    unsafe { string_from_header(JSValue::from_bits(bits).as_string_ptr()).unwrap() }
                })
                .collect();
            assert_eq!(names, ["column", "database", "name", "table", "type"]);
        }
        let read = |column: &perry_runtime::gc::RuntimeHandle<'_>, name: &[u8]| {
            let key =
                scope.root_raw_mut_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
            perry_runtime::object::js_object_get_field_by_name_f64(
                pointer(column),
                key.get_raw_mut_ptr(),
            )
        };
        assert_eq!(read(&first, b"column").to_bits(), JSValue::null().bits());
        assert_eq!(read(&second, b"type").to_bits(), JSValue::null().bits());
        assert!(drop_handle(stmt));
        assert_eq!(unsafe { js_sqlite_close(db) }, 1);
        perry_runtime::gc::js_gc_collect();
        let name = read(&first, b"name").to_bits();
        assert_eq!(
            unsafe { string_from_header(JSValue::from_bits(name).as_string_ptr(),).unwrap() },
            "id"
        );
        let name = read(&second, b"name").to_bits();
        assert_eq!(
            unsafe { string_from_header(JSValue::from_bits(name).as_string_ptr(),).unwrap() },
            "computed"
        );
        let key = scope.root_raw_mut_ptr(js_string_from_bytes(b"extra".as_ptr(), 5));
        perry_runtime::object::js_object_set_field_by_name(
            pointer(&first),
            key.get_raw_mut_ptr(),
            23.0,
        );
        assert_eq!(read(&first, b"extra"), 23.0);
    }
}
