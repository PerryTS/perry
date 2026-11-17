use super::*;
use std::ffi::{c_char, c_int, c_void};
use std::sync::atomic::AtomicUsize;

struct PrepareCounter {
    db: Handle,
    count: Box<Authorizations>,
}

struct Authorizations {
    operation: c_int,
    count: AtomicUsize,
}

unsafe extern "C" fn count_select(
    context: *mut c_void,
    operation: c_int,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    let state = &*(context as *const Authorizations);
    if operation == state.operation {
        state.count.fetch_add(1, Ordering::Relaxed);
    }
    rusqlite::ffi::SQLITE_OK
}

impl PrepareCounter {
    unsafe fn install(db: Handle) -> Self {
        Self::install_for(db, rusqlite::ffi::SQLITE_SELECT)
    }

    unsafe fn install_for(db: Handle, operation: c_int) -> Self {
        let count = Box::new(Authorizations {
            operation,
            count: AtomicUsize::new(0),
        });
        let context = (&*count as *const Authorizations).cast_mut().cast();
        let result = with_sqlite_connection(db, |conn| {
            rusqlite::ffi::sqlite3_set_authorizer(conn.handle(), Some(count_select), context)
        });
        assert_eq!(result, Some(rusqlite::ffi::SQLITE_OK));
        Self { db, count }
    }

    fn get(&self) -> usize {
        self.count.count.load(Ordering::Relaxed)
    }
}

impl Drop for PrepareCounter {
    fn drop(&mut self) {
        // SQLite must not retain the callback context after its Box is freed.
        unsafe {
            with_sqlite_connection(self.db, |conn| {
                rusqlite::ffi::sqlite3_set_authorizer(conn.handle(), None, std::ptr::null_mut())
            });
        }
    }
}

#[test]
fn sqlite_preparation_repeated_get_and_all_reuse_validation() {
    crate::test_runtime::initialize();
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    unsafe {
        let filename = scope.root_raw_mut_ptr(js_string_from_bytes(b":memory:".as_ptr(), 8));
        let db = js_sqlite_open(filename.get_raw_mut_ptr());
        assert!(db > 0);
        let counter = PrepareCounter::install(db);
        let sql = scope.root_raw_mut_ptr(js_string_from_bytes(b"SELECT 7 AS n".as_ptr(), 13));
        let stmt = js_sqlite_prepare(db, sql.get_raw_mut_ptr());
        assert!(stmt > 0);
        assert_eq!(counter.get(), 1, "the observer must see public validation");
        js_sqlite_stmt_raw(stmt);
        for _ in 0..3 {
            let row = scope.root_nanbox_f64(js_sqlite_stmt_get(stmt, std::ptr::null()));
            let row_ptr = perry_runtime::value::js_nanbox_get_pointer(row.get_nanbox_f64())
                as *const ArrayHeader;
            assert_eq!(
                JSValue::from_bits(perry_runtime::js_array_get_jsvalue(row_ptr, 0)).as_number(),
                7.0
            );
            let all = scope.root_raw_mut_ptr(js_sqlite_stmt_all(stmt, std::ptr::null()));
            assert_eq!((*all.get_raw_mut_ptr::<ArrayHeader>()).length, 1);
        }
        let observed = counter.get();
        drop(counter);
        assert!(drop_handle(stmt));
        assert_eq!(js_sqlite_close(db), 1);
        assert_eq!(
            observed, 1,
            "resident SQL must reuse its actual compilation"
        );
    }
}

struct Database {
    db: Handle,
    statements: Vec<Handle>,
}

impl Database {
    unsafe fn new() -> Self {
        crate::test_runtime::initialize();
        let name = js_string_from_bytes(b":memory:".as_ptr(), 8);
        let db = js_sqlite_open(name);
        assert!(db > 0);
        Self {
            db,
            statements: Vec::new(),
        }
    }

    unsafe fn prepare(&mut self, sql: &str) -> Handle {
        let text = js_string_from_bytes(sql.as_ptr(), sql.len() as u32);
        let stmt = js_sqlite_prepare(self.db, text);
        assert!(stmt > 0, "{sql}");
        self.statements.push(stmt);
        stmt
    }

    unsafe fn exec(&self, sql: &str) {
        let text = js_string_from_bytes(sql.as_ptr(), sql.len() as u32);
        assert_eq!(js_sqlite_exec(self.db, text), 1);
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        for stmt in self.statements.drain(..) {
            drop_handle(stmt);
        }
        unsafe {
            js_sqlite_close(self.db);
        }
    }
}

unsafe fn params(
    scope: &perry_runtime::gc::RuntimeHandleScope,
    values: &[f64],
) -> *mut ArrayHeader {
    let arr = scope.root_raw_mut_ptr(js_array_alloc(0));
    for value in values {
        arr.set_raw_mut_ptr(js_array_push(
            arr.get_raw_mut_ptr(),
            JSValue::number(*value),
        ));
    }
    arr.get_raw_mut_ptr()
}

unsafe fn field(object: *mut ObjectHeader, name: &str) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(object);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::object::js_object_get_field_by_name_f64(object.get_raw_mut_ptr(), key)
}

unsafe fn raw_number(value: f64, column: u32) -> f64 {
    let ptr = perry_runtime::value::js_nanbox_get_pointer(value) as *const ArrayHeader;
    JSValue::from_bits(perry_runtime::js_array_get_jsvalue(ptr, column)).as_number()
}

#[test]
fn sqlite_preparation_run_reuses_dml_and_preserves_execute_semantics() {
    unsafe {
        let mut db = Database::new();
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        db.exec("CREATE TABLE t (n INTEGER)");
        let counter = PrepareCounter::install_for(db.db, rusqlite::ffi::SQLITE_INSERT);
        let stmt = db.prepare("INSERT INTO t(n) VALUES (?1)");
        for n in 1..=3 {
            let p = params(&scope, &[n as f64]);
            let result = scope.root_raw_mut_ptr(js_sqlite_stmt_run(stmt, p));
            assert!(!result.get_raw_mut_ptr::<ObjectHeader>().is_null());
            assert_eq!(field(result.get_raw_mut_ptr(), "changes"), 1.0);
            assert_eq!(field(result.get_raw_mut_ptr(), "lastInsertRowid"), n as f64);
        }
        assert!(js_sqlite_stmt_run(stmt, std::ptr::null()).is_null());
        let p = params(&scope, &[9.0]);
        let result = scope.root_raw_mut_ptr(js_sqlite_stmt_run(stmt, p));
        assert_eq!(field(result.get_raw_mut_ptr(), "changes"), 1.0);
        assert_eq!(field(result.get_raw_mut_ptr(), "lastInsertRowid"), 4.0);
        assert_eq!(counter.get(), 1);
        // better-sqlite3's run() accepts row-returning statements: it steps
        // them to completion, discards the rows and reports the change count.
        let select = db.prepare("SELECT n FROM t");
        let result = scope.root_raw_mut_ptr(js_sqlite_stmt_run(select, std::ptr::null()));
        assert!(!result.get_raw_mut_ptr::<ObjectHeader>().is_null());
        assert_eq!(field(result.get_raw_mut_ptr(), "changes"), 0.0);
        assert_eq!(field(result.get_raw_mut_ptr(), "lastInsertRowid"), 4.0);
        let returning = db.prepare("INSERT INTO t VALUES (4) RETURNING n");
        let result = scope.root_raw_mut_ptr(js_sqlite_stmt_run(returning, std::ptr::null()));
        assert!(!result.get_raw_mut_ptr::<ObjectHeader>().is_null());
        assert_eq!(field(result.get_raw_mut_ptr(), "changes"), 1.0);
        assert_eq!(field(result.get_raw_mut_ptr(), "lastInsertRowid"), 5.0);
        let count = db.prepare("SELECT count(*) AS n FROM t");
        js_sqlite_stmt_raw(count);
        assert_eq!(
            raw_number(js_sqlite_stmt_get(count, std::ptr::null()), 0),
            5.0
        );
    }
}

#[test]
fn sqlite_preparation_all_aborts_instead_of_returning_rows_read_before_an_error() {
    unsafe {
        let mut db = Database::new();
        // `abs` over the i64 minimum raises integer overflow on the second
        // step: the first row is produced, then the step fails.
        let stmt = db.prepare("SELECT 1 AS a UNION ALL SELECT abs(-9223372036854775808) AS a");
        let rows = js_sqlite_stmt_all(stmt, std::ptr::null());
        assert_eq!(perry_runtime::js_array_length(rows), 0);
    }
}

#[test]
fn sqlite_preparation_run_reports_step_errors_of_row_returning_statements() {
    unsafe {
        let mut db = Database::new();
        db.exec("CREATE TABLE u (id INTEGER PRIMARY KEY)");
        db.exec("INSERT INTO u VALUES (1)");
        let duplicate = db.prepare("INSERT INTO u VALUES (1) RETURNING id");
        // The step fails on the constraint: run() must report it, not a
        // completed statement, and must leave the stored rows unchanged.
        assert!(js_sqlite_stmt_run(duplicate, std::ptr::null()).is_null());
        let count = db.prepare("SELECT count(*) AS n FROM u");
        js_sqlite_stmt_raw(count);
        assert_eq!(
            raw_number(js_sqlite_stmt_get(count, std::ptr::null()), 0),
            1.0
        );
    }
}

#[test]
fn sqlite_preparation_shared_sql_keeps_flags_bindings_and_reset() {
    unsafe {
        let mut db = Database::new();
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        db.exec("CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (1), (2)");
        let counter = PrepareCounter::install(db.db);
        let object =
            db.prepare("SELECT CASE n WHEN 1 THEN ?1 ELSE 99 END AS n FROM t ORDER BY t.n");
        assert_eq!(counter.get(), 1, "one SELECT authorization per compilation");
        let raw = db.prepare("SELECT CASE n WHEN 1 THEN ?1 ELSE 99 END AS n FROM t ORDER BY t.n");
        js_sqlite_stmt_raw(raw);
        let p = params(&scope, &[11.0]);
        let row = scope.root_nanbox_f64(js_sqlite_stmt_get(object, p));
        assert_eq!(
            field(
                perry_runtime::value::js_nanbox_get_pointer(row.get_nanbox_f64())
                    as *mut ObjectHeader,
                "n"
            ),
            11.0
        );
        assert_eq!(raw_number(js_sqlite_stmt_get(raw, p), 0), 11.0);
        // Wrong arity must not retain the successful call's parameter.
        assert_eq!(
            js_sqlite_stmt_get(raw, std::ptr::null()).to_bits(),
            JSValue::undefined().bits()
        );
        assert_eq!((*js_sqlite_stmt_all(raw, std::ptr::null())).length, 0);
        let p = params(&scope, &[22.0]);
        let all = scope.root_raw_mut_ptr(js_sqlite_stmt_all(raw, p));
        assert_eq!((*all.get_raw_mut_ptr::<ArrayHeader>()).length, 2);
        let first = perry_runtime::js_array_get_jsvalue(all.get_raw_mut_ptr(), 0);
        assert_eq!(raw_number(f64::from_bits(first), 0), 22.0);
        let second = perry_runtime::js_array_get_jsvalue(all.get_raw_mut_ptr(), 1);
        assert_eq!(raw_number(f64::from_bits(second), 0), 99.0);
        assert_eq!(counter.get(), 1);
    }
}

#[test]
fn sqlite_preparation_capacity_one_evicts_and_close_drops_cache() {
    unsafe {
        let mut db = Database::new();
        with_sqlite_connection(db.db, |conn| conn.set_prepared_statement_cache_capacity(1));
        let counter = PrepareCounter::install(db.db);
        let a = db.prepare("SELECT 1 AS n");
        let b = db.prepare("SELECT 2 AS n");
        js_sqlite_stmt_raw(a);
        js_sqlite_stmt_raw(b);
        assert_eq!(raw_number(js_sqlite_stmt_get(a, std::ptr::null()), 0), 1.0);
        assert_eq!(raw_number(js_sqlite_stmt_get(b, std::ptr::null()), 0), 2.0);
        assert_eq!(raw_number(js_sqlite_stmt_get(a, std::ptr::null()), 0), 1.0);
        assert_eq!(counter.get(), 5);
        drop(counter);
        assert_eq!(js_sqlite_close(db.db), 1);
        assert!(get_handle::<SqliteDbHandle>(db.db).is_none());
        assert_eq!(js_sqlite_close(db.db), 0);
        // The numeric statement is retained, but its database no longer exists.
        assert!(get_handle::<SqliteStmtHandle>(a).is_some());
    }
}

#[test]
fn sqlite_preparation_schema_recompile_updates_rows_and_fresh_columns() {
    unsafe {
        let mut db = Database::new();
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        db.exec("CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (1)");
        let row_stmt = db.prepare("SELECT * FROM t");
        let empty_stmt = db.prepare("SELECT * FROM t WHERE 0");
        assert_eq!(
            (*js_sqlite_stmt_all(empty_stmt, std::ptr::null())).length,
            0
        );
        let first = scope.root_nanbox_f64(js_sqlite_stmt_get(row_stmt, std::ptr::null()));
        assert_eq!(
            field(
                perry_runtime::value::js_nanbox_get_pointer(first.get_nanbox_f64())
                    as *mut ObjectHeader,
                "n"
            ),
            1.0
        );
        db.exec("ALTER TABLE t ADD COLUMN extra INTEGER DEFAULT 8");
        let row = scope.root_nanbox_f64(js_sqlite_stmt_get(row_stmt, std::ptr::null()));
        assert_eq!(
            field(
                perry_runtime::value::js_nanbox_get_pointer(row.get_nanbox_f64())
                    as *mut ObjectHeader,
                "extra"
            ),
            8.0
        );
        let all = scope.root_raw_mut_ptr(js_sqlite_stmt_all(row_stmt, std::ptr::null()));
        let row = perry_runtime::js_array_get_jsvalue(all.get_raw_mut_ptr(), 0);
        assert_eq!(
            field(
                perry_runtime::value::js_nanbox_get_pointer(f64::from_bits(row))
                    as *mut ObjectHeader,
                "extra"
            ),
            8.0
        );
        assert_eq!(
            (*js_sqlite_stmt_all(empty_stmt, std::ptr::null())).length,
            0
        );
        let columns = scope.root_raw_mut_ptr(js_sqlite_stmt_columns(empty_stmt));
        assert_eq!((*columns.get_raw_mut_ptr::<ArrayHeader>()).length, 2);
        db.exec("INSERT INTO t(n) VALUES (2)");
        js_sqlite_stmt_raw(row_stmt);
        assert_eq!(
            raw_number(js_sqlite_stmt_get(row_stmt, std::ptr::null()), 1),
            8.0
        );
    }
}

static TRANSACTION_CALLBACKS: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn reenter_transaction(
    closure: *const perry_runtime::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    value: f64,
) -> f64 {
    let stmt = perry_runtime::closure::js_closure_get_capture_f64(closure, 0) as Handle;
    let db = get_handle::<SqliteStmtHandle>(stmt).unwrap().db_handle;
    assert!(get_handle::<SqliteDbHandle>(db)
        .unwrap()
        .conn
        .try_lock()
        .is_ok());
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let p = params(&scope, &[value]);
    let row = scope.root_nanbox_f64(js_sqlite_stmt_get(stmt, p));
    assert_eq!(raw_number(row.get_nanbox_f64(), 0), value);
    let p = params(&scope, &[value + 1.0]);
    let all = scope.root_raw_mut_ptr(js_sqlite_stmt_all(stmt, p));
    assert_eq!((*all.get_raw_mut_ptr::<ArrayHeader>()).length, 1);
    TRANSACTION_CALLBACKS.fetch_add(1, Ordering::Relaxed);
    value
}

#[test]
fn sqlite_preparation_transaction_callback_reenters_cached_queries() {
    unsafe {
        let mut db = Database::new();
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let counter = PrepareCounter::install(db.db);
        let stmt = db.prepare("SELECT ?1 AS n");
        js_sqlite_stmt_raw(stmt);
        let callback = scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(reenter_transaction, 1),
            1,
        ));
        perry_runtime::closure::js_closure_set_capture_f64(
            callback.get_raw_mut_ptr(),
            0,
            stmt as f64,
        );
        let wrapper = scope.root_raw_mut_ptr(js_sqlite_transaction(
            db.db,
            callback.get_raw_mut_ptr::<perry_runtime::ClosureHeader>() as i64,
        ));
        TRANSACTION_CALLBACKS.store(0, Ordering::Relaxed);
        for n in [4.0, 5.0] {
            assert_eq!(
                perry_runtime::closure::js_closure_call1(
                    wrapper.get_raw_mut_ptr(),
                    perry_runtime::closure::plain_call_receiver(),
                    n,
                ),
                n
            );
        }
        assert_eq!(TRANSACTION_CALLBACKS.load(Ordering::Relaxed), 2);
        assert_eq!(counter.get(), 1);
    }
}

#[test]
fn sqlite_preparation_b1_preserves_original_unicode_sql_lexing() {
    unsafe {
        let mut failures = Vec::new();
        for warm in [false, true] {
            let mut db = Database::new();
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            db.exec("CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (17)");
            if warm {
                let stmt = db.prepare("SELECT n FROM t");
                js_sqlite_stmt_raw(stmt);
                assert_eq!(
                    raw_number(js_sqlite_stmt_get(stmt, std::ptr::null()), 0),
                    17.0
                );
            }
            for whitespace in ['\u{00a0}', '\u{2003}', '\u{2028}'] {
                for sql in [
                    format!("{whitespace}SELECT n FROM t"),
                    format!("SELECT n FROM t{whitespace}"),
                ] {
                    assert_eq!(
                        with_sqlite_connection(db.db, |conn| conn.prepare(&sql).is_err()),
                        Some(true)
                    );
                    let text = scope
                        .root_raw_mut_ptr(js_string_from_bytes(sql.as_ptr(), sql.len() as u32));
                    let stmt = js_sqlite_prepare(db.db, text.get_raw_mut_ptr());
                    if stmt != -1 {
                        db.statements.push(stmt);
                        failures.push(format!("accepted {sql:?}, warm={warm}"));
                    }
                }
            }
            for (sql, name, expected) in [
                (" \t\nSELECT n FROM t\r\n ", "n", 17.0),
                ("/* leading */ SELECT n FROM t -- trailing", "n", 17.0),
                ("SELECT 23 AS alias\u{2003}", "alias\u{2003}", 23.0),
                ("SELECT 23 AS alias -- trailing\u{2003}", "alias", 23.0),
            ] {
                // Admit controls with SQLite's original bytes, including its actual column name.
                let admitted = with_sqlite_connection(db.db, |conn| {
                    let mut stmt = conn.prepare(sql).unwrap();
                    let names = stmt.column_names().map(str::to_owned).collect::<Vec<_>>();
                    let value = stmt.query_row([], |row| row.get::<_, f64>(0)).unwrap();
                    (names, value)
                })
                .unwrap();
                assert_eq!(admitted, (vec![name.to_owned()], expected));
                let stmt = db.prepare(sql);
                for _ in 0..2 {
                    let row = scope.root_nanbox_f64(js_sqlite_stmt_get(stmt, std::ptr::null()));
                    let ptr = perry_runtime::value::js_nanbox_get_pointer(row.get_nanbox_f64())
                        as *mut ObjectHeader;
                    if field(ptr, name) != expected {
                        failures.push(format!("changed column/value for {sql:?}, warm={warm}"));
                    }
                }
            }
            let unchanged = db.prepare("SELECT n FROM t");
            js_sqlite_stmt_raw(unchanged);
            assert_eq!(
                raw_number(js_sqlite_stmt_get(unchanged, std::ptr::null()), 0),
                17.0
            );
            let count = db.prepare("SELECT count(*) AS n FROM t");
            js_sqlite_stmt_raw(count);
            assert_eq!(
                raw_number(js_sqlite_stmt_get(count, std::ptr::null()), 0),
                1.0
            );
        }
        assert!(
            failures.is_empty(),
            "original SQL semantics changed: {failures:?}"
        );
    }
}
