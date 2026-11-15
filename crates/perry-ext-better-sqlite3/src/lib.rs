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

mod dispatch_ext;

use perry_ffi::{
    alloc_bigint_from_str, alloc_string, bigint_value, build_object_shape, drop_handle,
    js_array_alloc, js_array_push, js_object_alloc_with_shape, js_object_set_field, read_string,
    register_handle, ArrayHeader, ErrorKind, Handle, JsString, JsValue, ObjectHeader, StringHeader,
    TransientRootScope, TransientRootedNanbox,
};
use rusqlite::{types::Value as SqliteValue, CachedStatement, Connection, Statement};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const DATABASE_NOT_OPEN: &str = "The database connection is not open";

enum StatementLease<'conn> {
    Cached(CachedStatement<'conn>),
    Uncached(Statement<'conn>),
}

impl<'conn> Deref for StatementLease<'conn> {
    type Target = Statement<'conn>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Cached(stmt) => stmt,
            Self::Uncached(stmt) => stmt,
        }
    }
}

impl DerefMut for StatementLease<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Cached(stmt) => stmt,
            Self::Uncached(stmt) => stmt,
        }
    }
}

fn prepare_statement<'conn>(
    conn: &'conn Connection,
    sql: &str,
) -> rusqlite::Result<StatementLease<'conn>> {
    // rusqlite trims before lookup AND compilation. Only SQLite's five ASCII
    // whitespace characters may be removed without changing its lexer input.
    let sqlite_trimmed = sql.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    if sql.trim() == sqlite_trimmed {
        conn.prepare_cached(sql).map(StatementLease::Cached)
    } else {
        conn.prepare(sql).map(StatementLease::Uncached)
    }
}

#[cold]
fn throw_database_not_open() -> ! {
    perry_ffi::throw_with_code(DATABASE_NOT_OPEN, "", ErrorKind::TypeError)
}

fn require_database_open(db_handle: Handle) {
    if perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |_| ()).is_none() {
        throw_database_not_open();
    }
}

fn statement_snapshot(handle: Handle) -> Option<(String, Handle, bool, bool)> {
    perry_ffi::with_handle::<SqliteStmtHandle, _, _>(handle, |stmt| {
        (
            stmt.sql.clone(),
            stmt.db_handle,
            stmt.raw_mode.load(Ordering::Relaxed),
            stmt.safe_integers.load(Ordering::Relaxed),
        )
    })
}

fn require_statement_database_open(stmt_handle: Handle) {
    let db_handle =
        perry_ffi::with_handle::<SqliteStmtHandle, _, _>(stmt_handle, |stmt| stmt.db_handle)
            .unwrap_or_else(|| throw_database_not_open());
    require_database_open(db_handle);
}

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
    /// Per-statement "safe integers" flag — `stmt.safeIntegers([toggle])`
    /// flips this. When true, an INTEGER column comes back as a JS BigInt
    /// instead of a number, which is the only way to carry a value outside
    /// the double-safe range without losing bits. ActiveRecord's SQLite
    /// adapter turns it on for a statement whose declared column type
    /// matches /bigint/i (`_maybeEnableReadBigInts`).
    pub safe_integers: AtomicBool,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// Convert a SQLite column value to a perry-ffi `JsValue`.
/// `safe_integers` reports an INTEGER column as a BigInt.
///
/// `from_number` loses bits above 2^53, so a statement that opted into safe
/// integers must not take that path for any integer, not only a large one:
/// better-sqlite3 returns BigInt for every INTEGER column once the flag is
/// set, and a caller that tests `typeof v === "bigint"` has to see it
/// uniformly.
fn sqlite_value_to_jsvalue_with(value: &SqliteValue, safe_integers: bool) -> JsValue {
    match value {
        SqliteValue::Null => JsValue::NULL,
        SqliteValue::Integer(n) => {
            if safe_integers {
                bigint_value(alloc_bigint_from_str(&n.to_string()))
            } else if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
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
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(len);

    for i in 0..len {
        let bits = perry_ffi::js_array_get(arr_ptr, i as u32).bits();
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
        Ok(c) => {
            dispatch_ext::ensure_registered();
            register_handle(SqliteDbHandle {
                conn: Mutex::new(c),
            })
        }
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
    require_database_open(db_handle);
    perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        db.conn
            .lock()
            .ok()
            .map(|conn| i32::from(conn.execute_batch(&sql).is_ok()))
    })
    .flatten()
    .unwrap_or(0)
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
    require_database_open(db_handle);
    let valid = perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        db.conn
            .lock()
            .ok()
            .is_some_and(|conn| prepare_statement(&conn, &sql).is_ok())
    })
    .unwrap_or(false);
    if valid {
        register_handle(SqliteStmtHandle {
            sql,
            db_handle,
            raw_mode: AtomicBool::new(false),
            safe_integers: AtomicBool::new(false),
        })
    } else {
        -1
    }
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
    perry_ffi::with_handle::<SqliteStmtHandle, _, _>(stmt_handle, |stmt| {
        stmt.raw_mode.store(true, Ordering::Relaxed);
    });
    stmt_handle
}

/// `stmt.safeIntegers([toggle])` — read INTEGER columns as BigInt. Returns
/// the same statement handle so the call chains, as `stmt.raw()` does.
///
/// # Safety
///
/// `stmt_handle` must be a registry id; a foreign or dead one is ignored.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_safe_integers(stmt_handle: Handle, toggle: f64) -> Handle {
    // better-sqlite3 defaults the argument to true: `safeIntegers()` with no
    // argument turns the mode ON, so an absent argument is not "off".
    let toggle = JsValue::from_bits(toggle.to_bits());
    let on = toggle.is_undefined() || toggle.to_bool();
    perry_ffi::with_handle::<SqliteStmtHandle, _, _>(stmt_handle, |stmt| {
        stmt.safe_integers.store(on, Ordering::Relaxed);
    });
    stmt_handle
}

/// `stmt.columns()` — one record per result column, with better-sqlite3's
/// five keys: `name`, `column`, `table`, `database`, `type`.
///
/// `name` is the column's name in the result set and `type` its declared
/// type; the other three name where the value came from and are null for a
/// column that is an expression rather than a table column, which is what
/// better-sqlite3 reports too. ActiveRecord's SQLite adapter reads `type` to
/// decide whether to turn on safe integers.
///
/// # Safety
///
/// `stmt_handle` must be a registry id; a foreign or dead one yields an
/// empty array.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_columns(stmt_handle: Handle) -> *mut ArrayHeader {
    require_statement_database_open(stmt_handle);
    let Some((sql, db_handle, _, _)) = statement_snapshot(stmt_handle) else {
        return js_array_alloc(0);
    };
    let metadata = perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        let conn = db.conn.lock().ok()?;
        let prepared = conn.prepare(&sql).ok()?;
        let decl_types: Vec<Option<String>> = prepared
            .columns()
            .iter()
            .map(|c| c.decl_type().map(str::to_string))
            .collect();
        let origins: Vec<(String, Option<String>, Option<String>, Option<String>)> = prepared
            .columns_with_metadata()
            .iter()
            .map(|c| {
                (
                    c.name().to_string(),
                    c.database_name().map(str::to_string),
                    c.table_name().map(str::to_string),
                    c.origin_name().map(str::to_string),
                )
            })
            .collect();
        Some((decl_types, origins))
    })
    .flatten();
    let Some((decl_types, origins)) = metadata else {
        return js_array_alloc(0);
    };

    let shape = build_object_shape(&COLUMN_KEYS);
    let records_scope = TransientRootScope::enter();
    let records: Vec<TransientRootedNanbox> = origins
        .iter()
        .enumerate()
        .map(|(idx, (name, database, table, column))| {
            let decl_type = decl_types.get(idx).cloned().flatten();
            let scope = TransientRootScope::enter();
            // Slot order matches COLUMN_KEYS. Each string is rooted before
            // the next allocation, and the record is born after all five.
            let values: Vec<TransientRootedNanbox> = [
                Some(name.as_str()),
                column.as_deref(),
                table.as_deref(),
                database.as_deref(),
                decl_type.as_deref(),
            ]
            .into_iter()
            .map(|value| {
                scope.root_nanbox(bits(match value {
                    // better-sqlite3 reports an absent origin as null, not
                    // as undefined or an empty string.
                    None => JsValue::NULL,
                    Some(text) => JsValue::from_string_ptr(alloc_string(text).as_raw()),
                }))
            })
            .collect();
            let record = record_from_rooted(&scope, &shape, &values);
            drop(scope);
            // Nothing allocates between the inner scope's exit and this root.
            records_scope.root_nanbox(bits(JsValue::from_object_ptr(record)))
        })
        .collect();
    array_from_rooted(&records)
}

fn bits(value: JsValue) -> f64 {
    f64::from_bits(value.bits())
}

/// A shaped record over `values`, which the caller rooted before this birth.
/// The record is rooted in `scope` while its fields are written and re-read
/// for each store.
unsafe fn record_from_rooted(
    scope: &TransientRootScope,
    (packed_keys, shape_id): &(Vec<u8>, u32),
    values: &[TransientRootedNanbox],
) -> *mut ObjectHeader {
    let record = scope.root_addr(js_object_alloc_with_shape(
        *shape_id,
        values.len() as u32,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    ) as i64);
    for (field, value) in values.iter().enumerate() {
        js_object_set_field(
            record.get() as *mut ObjectHeader,
            field as u32,
            JsValue::from_bits(value.get().to_bits()),
        );
    }
    record.get() as *mut ObjectHeader
}

/// One result row as JS values, each rooted before the next conversion (a
/// string, a BigInt or a Buffer column allocates).
fn rooted_row_values(
    scope: &TransientRootScope,
    row: &[SqliteValue],
    safe_ints: bool,
) -> Vec<TransientRootedNanbox> {
    row.iter()
        .map(|value| scope.root_nanbox(bits(sqlite_value_to_jsvalue_with(value, safe_ints))))
        .collect()
}

/// An array of rooted values in order. Each push returns the array to use
/// next and nothing else is held, so no earlier pointer outlives a push.
unsafe fn array_from_rooted(values: &[TransientRootedNanbox]) -> *mut ArrayHeader {
    values
        .iter()
        .fold(js_array_alloc(values.len() as u32), |arr, v| {
            js_array_push(arr, JsValue::from_bits(v.get().to_bits()))
        })
}

/// One row as better-sqlite3 returns it: an array in raw mode, else a record
/// keyed by column name.
unsafe fn row_value(
    row: &[SqliteValue],
    raw_or_shape: Option<&(Vec<u8>, u32)>,
    safe_ints: bool,
) -> JsValue {
    let scope = TransientRootScope::enter();
    let values = rooted_row_values(&scope, row, safe_ints);
    match raw_or_shape {
        None => JsValue::from_object_ptr(array_from_rooted(&values)),
        Some(shape) => JsValue::from_object_ptr(record_from_rooted(&scope, shape, &values)),
    }
}

/// The keys of a `Statement#columns()` record, in better-sqlite3's own
/// insertion order — `Object.keys()` on the result observes it, and node
/// reports `["name","column","table","database","type"]`.
///
/// Deliberately NOT the runtime's `SQLITE_COLUMN_METADATA` layout, which
/// perry-stdlib's same-named function uses: that one serves `node:sqlite`,
/// whose documented key order is alphabetical. The two APIs agree on the
/// five names and differ on their order, so each keeps its own.
const COLUMN_KEYS: [&str; 5] = ["name", "column", "table", "database", "type"];

/// Build better-sqlite3's `RunResult` — `{ changes, lastInsertRowid }`.
unsafe fn run_result(changes: f64, last_id: i64) -> *mut ObjectHeader {
    let result = unsafe {
        perry_ffi::runtime_layouts::alloc_runtime_record(
            perry_ffi::runtime_layouts::RECORD_F0_0::desc(),
        )
    };
    js_object_set_field(result, 0, JsValue::from_number(changes));
    js_object_set_field(result, 1, JsValue::from_number(last_id as f64));
    result
}

/// `stmt.run(...params)` — run the statement and report
/// `{ changes, lastInsertRowid }` as an ObjectHeader pointer, or null
/// on error.
///
/// better-sqlite3 accepts **any** statement here, including one that
/// returns rows: `run()` steps it to completion, discards the rows and
/// reports the connection's change count, so `select 1` gives
/// `{ changes: 0, lastInsertRowid: 0 }` on a fresh connection.
/// The cached statement is leased only while the connection is locked. Row-returning
/// statements are drained rather than passed to `Statement::execute`, which rejects
/// them with `ExecuteReturnedResults`.
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
    require_statement_database_open(stmt_handle);
    let Some((sql, db_handle, _, _)) = statement_snapshot(stmt_handle) else {
        return std::ptr::null_mut();
    };
    let result = perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        let conn = db.conn.lock().ok()?;
        let params = sqlite_params.iter().map(|p| p.as_ref()).collect::<Vec<_>>();
        let before = conn.total_changes();
        let mut prepared = prepare_statement(&conn, &sql).ok()?;
        if prepared.column_count() == 0 {
            prepared.execute(params.as_slice()).ok()?;
        } else {
            let mut rows = prepared.query(params.as_slice()).ok()?;
            // A step error must not be reported as a completed run: a
            // `INSERT ... RETURNING` can fail after the row count moved.
            while rows.next().ok()?.is_some() {}
        }
        Some((
            conn.total_changes().saturating_sub(before),
            conn.last_insert_rowid(),
        ))
    })
    .flatten();
    result
        .map(|(changes, last_id)| run_result(changes as f64, last_id))
        .unwrap_or(std::ptr::null_mut())
}

fn query_owned_rows(
    stmt_handle: Handle,
    sqlite_params: &[Box<dyn rusqlite::ToSql>],
    first_only: bool,
) -> Option<(Vec<String>, Vec<Vec<SqliteValue>>, bool, bool)> {
    let (sql, db_handle, raw, safe_ints) = statement_snapshot(stmt_handle)?;
    perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        let conn = db.conn.lock().ok()?;
        let mut prepared = prepare_statement(&conn, &sql).ok()?;
        let param_refs = sqlite_params
            .iter()
            .map(|value| value.as_ref())
            .collect::<Vec<_>>();
        let mut rows = prepared.query(param_refs.as_slice()).ok()?;
        let mut result = Vec::new();
        // A step error aborts the result instead of returning the rows read so
        // far as if they were the whole answer.
        while let Some(row) = rows.next().ok()? {
            result.push(
                (0..row.as_ref().column_count())
                    .map(|idx| row.get(idx).unwrap_or(SqliteValue::Null))
                    .collect(),
            );
            if first_only {
                break;
            }
        }
        drop(rows);
        // SQLite can recompile cached SQL on step, even when it returns no rows.
        let names = prepared
            .column_names()
            .iter()
            .map(|name| name.to_string())
            .collect();
        Some((names, result, raw, safe_ints))
    })
    .flatten()
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
    require_statement_database_open(stmt_handle);
    if let Some((names, rows, raw, safe_ints)) = query_owned_rows(stmt_handle, &sqlite_params, true)
    {
        if let Some(row) = rows.first() {
            let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
            let shape = (!raw).then(|| build_object_shape(&refs));
            return bits(row_value(row, shape.as_ref(), safe_ints));
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
    require_statement_database_open(stmt_handle);
    let scope = TransientRootScope::enter();
    let mut built = Vec::new();
    if let Some((names, rows, raw, safe_ints)) =
        query_owned_rows(stmt_handle, &sqlite_params, false)
    {
        let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let shape = (!raw).then(|| build_object_shape(&refs));
        for row in &rows {
            built.push(scope.root_nanbox(bits(row_value(row, shape.as_ref(), safe_ints))));
        }
    }
    array_from_rooted(&built)
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
    require_database_open(db_handle);
    let value = read_str(value_ptr);

    let text = perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        let conn = db.conn.lock().ok()?;
        let sql = match value {
            Some(v) => format!("PRAGMA {} = {}", pragma, v),
            None => format!("PRAGMA {}", pragma),
        };
        let mut stmt = conn.prepare(&sql).ok()?;
        let mut rows = stmt.query([]).ok()?;
        let row = rows.next().ok()??;
        row.get::<_, String>(0)
            .ok()
            .or_else(|| row.get::<_, i64>(0).ok().map(|n| n.to_string()))
            .or_else(|| row.get::<_, f64>(0).ok().map(|n| n.to_string()))
    })
    .flatten();
    text.map(|s| alloc_string(&s).as_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn js_sqlite_begin_transaction(db_handle: Handle) -> i32 {
    require_database_open(db_handle);
    perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        db.conn
            .lock()
            .ok()
            .map(|conn| i32::from(conn.execute("BEGIN TRANSACTION", []).is_ok()))
    })
    .flatten()
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn js_sqlite_commit(db_handle: Handle) -> i32 {
    require_database_open(db_handle);
    perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        db.conn
            .lock()
            .ok()
            .map(|conn| i32::from(conn.execute("COMMIT", []).is_ok()))
    })
    .flatten()
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn js_sqlite_rollback(db_handle: Handle) -> i32 {
    require_database_open(db_handle);
    perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        db.conn
            .lock()
            .ok()
            .map(|conn| i32::from(conn.execute("ROLLBACK", []).is_ok()))
    })
    .flatten()
    .unwrap_or(0)
}

/// `db.close()` — drop the connection. Drops the handle from
/// the registry; subsequent operations throw the same closed-connection
/// TypeError as better-sqlite3.
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
    perry_ffi::with_handle::<SqliteDbHandle, _, _>(db_handle, |db| {
        db.conn
            .lock()
            .ok()
            .map(|conn| i32::from(!conn.is_autocommit()))
    })
    .flatten()
    .unwrap_or(0)
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
    if perry_ffi::with_handle::<SqliteDbHandle, _, _>(handle, |_| ()).is_some() {
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
    if perry_ffi::with_handle::<SqliteStmtHandle, _, _>(handle, |_| ()).is_some() {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod static_record_layout_tests {
    #[test]
    fn static_layout_binding_records_keep_all_declared_properties() {
        perry_ffi::initialize_runtime_fixture();
        let roots = perry_ffi::TransientRootScope::enter();
        let record = roots.root_addr(unsafe {
            perry_ffi::runtime_layouts::alloc_runtime_record(
                perry_ffi::runtime_layouts::RECORD_F0_0::desc(),
            )
        } as i64);
        for slot in 0..2 {
            assert_eq!(
                unsafe {
                    perry_ffi::js_object_get_field(
                        record.get() as *const perry_ffi::ObjectHeader,
                        slot,
                    )
                }
                .bits(),
                perry_ffi::JsValue::UNDEFINED.bits()
            );
        }
    }
}
