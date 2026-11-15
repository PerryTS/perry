use super::*;
use perry_ffi::{js_array_get, js_array_length, js_object_get_field};

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
    let result_ptr = unsafe { js_sqlite_pragma(h, pragma.as_raw() as *const _, std::ptr::null()) };
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

/// `all()` (record and raw mode), `get()` and `columns()` build their
/// values, records and result arrays under transient roots; the results
/// must read back exactly as before.
#[test]
fn rows_and_columns_read_back_through_their_roots() {
    perry_ffi::initialize_runtime_fixture();
    let h = make_in_memory_db();
    for sql in [
        "CREATE TABLE people (ident INTEGER, label_text TEXT)",
        "INSERT INTO people VALUES (1, 'alice-row'), (2, 'bobby-row'), (3, 'carol-row')",
    ] {
        let sql = alloc_string(sql);
        assert_eq!(unsafe { js_sqlite_exec(h, sql.as_raw() as *const _) }, 1);
    }
    let sql = alloc_string("SELECT ident, label_text FROM people ORDER BY ident");
    let stmt = unsafe { js_sqlite_prepare(h, sql.as_raw() as *const _) };
    let text = |v: JsValue| {
        read_string(unsafe { JsString::from_raw(v.as_string_ptr()) }).map(str::to_owned)
    };
    unsafe {
        let all = js_sqlite_stmt_all(stmt, std::ptr::null());
        assert_eq!(js_array_length(all), 3);
        let row = js_array_get(all, 1).as_pointer::<ObjectHeader>();
        assert_eq!(
            text(js_object_get_field(row, 1)).as_deref(),
            Some("bobby-row")
        );

        let one = JsValue::from_bits(js_sqlite_stmt_get(stmt, std::ptr::null()).to_bits())
            .as_pointer::<ObjectHeader>();
        assert_eq!(js_object_get_field(one, 0).to_number(), 1.0);
        assert_eq!(
            text(js_object_get_field(one, 1)).as_deref(),
            Some("alice-row")
        );

        let columns = js_sqlite_stmt_columns(stmt);
        assert_eq!(js_array_length(columns), 2);
        let label = js_array_get(columns, 1).as_pointer::<ObjectHeader>();
        // COLUMN_KEYS order: name, column, table, database, type.
        assert_eq!(
            text(js_object_get_field(label, 0)).as_deref(),
            Some("label_text")
        );
        assert_eq!(
            text(js_object_get_field(label, 2)).as_deref(),
            Some("people")
        );

        js_sqlite_stmt_raw(stmt);
        let raw = js_sqlite_stmt_all(stmt, std::ptr::null());
        assert_eq!(js_array_length(raw), 3);
        let last = js_array_get(raw, 2).as_pointer::<ArrayHeader>();
        assert_eq!(js_array_length(last), 2);
        assert_eq!(text(js_array_get(last, 1)).as_deref(), Some("carol-row"));
    }
}

#[test]
fn close_drops_handle() {
    let h = make_in_memory_db();
    assert_eq!(js_sqlite_close(h), 1);
    // Second close is a no-op (handle already gone).
    assert_eq!(js_sqlite_close(h), 0);
}

// SQLite authorizes these simple statements during preparation, not execution.
// Keep the counter boxed until the authorizer has been detached, including on panic.
struct PrepareCounter {
    db: Handle,
    count: Box<AuthorizationCount>,
}

struct AuthorizationCount {
    value: std::sync::atomic::AtomicUsize,
    action: std::ffi::c_int,
}

unsafe extern "C" fn count_select_preparations(
    context: *mut std::ffi::c_void,
    action: std::ffi::c_int,
    _first: *const std::ffi::c_char,
    _second: *const std::ffi::c_char,
    _database: *const std::ffi::c_char,
    _trigger: *const std::ffi::c_char,
) -> std::ffi::c_int {
    let count = &*context.cast::<AuthorizationCount>();
    if action == count.action {
        count.value.fetch_add(1, Ordering::Relaxed);
    }
    rusqlite::ffi::SQLITE_OK
}

impl PrepareCounter {
    fn install(db: Handle) -> Self {
        Self::install_action(db, rusqlite::ffi::SQLITE_SELECT)
    }

    fn install_action(db: Handle, action: std::ffi::c_int) -> Self {
        let counter = Self {
            db,
            count: Box::new(AuthorizationCount {
                value: std::sync::atomic::AtomicUsize::new(0),
                action,
            }),
        };
        perry_ffi::with_handle::<SqliteDbHandle, _, _>(db, |db| {
            let conn = db.conn.lock().unwrap();
            assert_eq!(
                unsafe {
                    rusqlite::ffi::sqlite3_set_authorizer(
                        conn.handle(),
                        Some(count_select_preparations),
                        (&*counter.count as *const AuthorizationCount)
                            .cast_mut()
                            .cast(),
                    )
                },
                rusqlite::ffi::SQLITE_OK
            );
        })
        .unwrap();
        counter
    }

    fn count(&self) -> usize {
        self.count.value.load(Ordering::Relaxed)
    }
}

impl Drop for PrepareCounter {
    fn drop(&mut self) {
        perry_ffi::with_handle::<SqliteDbHandle, _, _>(self.db, |db| {
            let conn = db.conn.lock().unwrap();
            unsafe {
                rusqlite::ffi::sqlite3_set_authorizer(conn.handle(), None, std::ptr::null_mut());
            }
        });
    }
}

#[test]
fn sqlite_preparation_get_all_reuse_validation() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    let counter = PrepareCounter::install(db);
    let sql = alloc_string("SELECT 7 AS value");
    let stmt = unsafe { js_sqlite_prepare(db, sql.as_raw()) };
    assert!(stmt > 0);
    assert_eq!(counter.count(), 1, "public prepare must compile the SQL");
    unsafe {
        for _ in 0..3 {
            let one = JsValue::from_bits(js_sqlite_stmt_get(stmt, std::ptr::null()).to_bits());
            assert_eq!(js_object_get_field(one.as_pointer(), 0).to_number(), 7.0);
            let all = js_sqlite_stmt_all(stmt, std::ptr::null());
            assert_eq!(js_array_length(all), 1);
            assert_eq!(
                js_object_get_field(js_array_get(all, 0).as_pointer(), 0).to_number(),
                7.0
            );
        }
    }
    assert_eq!(
        counter.count(),
        1,
        "resident SQL must reuse validation for get/all"
    );
    drop(counter);
    assert_eq!(js_sqlite_close(db), 1);
    drop_handle(stmt);
}

fn prepare(db: Handle, sql: &str) -> Handle {
    let sql = alloc_string(sql);
    let stmt = unsafe { js_sqlite_prepare(db, sql.as_raw()) };
    assert!(stmt > 0);
    stmt
}

fn exec(db: Handle, sql: &str) {
    let sql = alloc_string(sql);
    assert_eq!(unsafe { js_sqlite_exec(db, sql.as_raw()) }, 1);
}

unsafe fn params(value: i32) -> *mut ArrayHeader {
    js_array_push(js_array_alloc(1), JsValue::from_int32(value))
}

unsafe fn get(stmt: Handle, params: *const ArrayHeader) -> JsValue {
    JsValue::from_bits(js_sqlite_stmt_get(stmt, params).to_bits())
}

unsafe fn field_number(row: JsValue, index: u32) -> f64 {
    js_object_get_field(row.as_pointer(), index).to_number()
}

#[test]
fn sqlite_preparation_run_dml_reuses_validation_and_preserves_results() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    exec(db, "CREATE TABLE t (id INTEGER PRIMARY KEY, value INTEGER)");
    let counter = PrepareCounter::install_action(db, rusqlite::ffi::SQLITE_INSERT);
    let insert = prepare(db, "INSERT INTO t(value) VALUES (?1)");
    unsafe {
        assert!(js_sqlite_stmt_run(insert, std::ptr::null()).is_null());
        for n in 1..=3 {
            let result = js_sqlite_stmt_run(insert, params(n * 10));
            assert!(!result.is_null());
            assert_eq!(js_object_get_field(result, 0).to_number(), 1.0);
            assert_eq!(js_object_get_field(result, 1).to_number(), n as f64);
        }
    }
    assert_eq!(counter.count(), 1);
    drop(counter);
    let select = prepare(db, "SELECT value FROM t ORDER BY id");
    let returning = prepare(db, "INSERT INTO t(value) VALUES (40) RETURNING id");
    unsafe {
        let result = js_sqlite_stmt_run(select, std::ptr::null());
        assert!(!result.is_null());
        assert_eq!(js_object_get_field(result, 0).to_number(), 0.0);
        assert_eq!(js_object_get_field(result, 1).to_number(), 3.0);
        let result = js_sqlite_stmt_run(returning, std::ptr::null());
        assert!(!result.is_null());
        assert_eq!(js_object_get_field(result, 0).to_number(), 1.0);
        assert_eq!(js_object_get_field(result, 1).to_number(), 4.0);
        assert_eq!(
            js_array_length(js_sqlite_stmt_all(select, std::ptr::null())),
            4
        );
    }
    assert_eq!(js_sqlite_close(db), 1);
    for stmt in [insert, select, returning] {
        drop_handle(stmt);
    }
}

#[test]
fn sqlite_preparation_all_aborts_instead_of_returning_rows_read_before_an_error() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    // `abs` over the i64 minimum raises integer overflow on the second step:
    // the first row is produced, then the step fails.
    let stmt = prepare(
        db,
        "SELECT 1 AS a UNION ALL SELECT abs(-9223372036854775808) AS a",
    );
    unsafe {
        assert_eq!(
            js_array_length(js_sqlite_stmt_all(stmt, std::ptr::null())),
            0
        );
    }
    assert_eq!(js_sqlite_close(db), 1);
    drop_handle(stmt);
}

#[test]
fn sqlite_preparation_run_reports_step_errors_of_row_returning_statements() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    exec(db, "CREATE TABLE u (id INTEGER PRIMARY KEY)");
    exec(db, "INSERT INTO u VALUES (1)");
    let duplicate = prepare(db, "INSERT INTO u VALUES (1) RETURNING id");
    let count = prepare(db, "SELECT count(*) AS n FROM u");
    unsafe {
        // The drain must surface the failed step instead of reporting a
        // completed run, and the rejected row must not be stored.
        assert!(js_sqlite_stmt_run(duplicate, std::ptr::null()).is_null());
        let row = JsValue::from_bits(js_sqlite_stmt_get(count, std::ptr::null()).to_bits())
            .as_pointer::<ObjectHeader>();
        assert_eq!(js_object_get_field(row, 0).to_number(), 1.0);
    }
    assert_eq!(js_sqlite_close(db), 1);
    for stmt in [duplicate, count] {
        drop_handle(stmt);
    }
}

#[test]
fn sqlite_preparation_handles_keep_flags_and_reset_bindings_after_errors() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    let counter = PrepareCounter::install(db);
    let sql = "SELECT ?1 AS value";
    let record = prepare(db, sql);
    let raw = prepare(db, sql);
    unsafe {
        js_sqlite_stmt_raw(raw);
        js_sqlite_stmt_safe_integers(raw, f64::from_bits(JsValue::UNDEFINED.bits()));
        assert_eq!(field_number(get(record, params(11)), 0), 11.0);
        assert!(get(record, std::ptr::null()).is_undefined());
        assert_eq!(
            js_array_length(js_sqlite_stmt_all(record, std::ptr::null())),
            0
        );
        assert_eq!(field_number(get(record, params(23)), 0), 23.0);
        let row = get(raw, params(31)).as_pointer::<ArrayHeader>();
        assert_eq!(js_array_length(row), 1);
        let integer = js_array_get(row, 0);
        let expected_tag = bigint_value(alloc_bigint_from_str("31")).bits() >> 48;
        assert_eq!(integer.bits() >> 48, expected_tag);
        assert_eq!(field_number(get(record, params(41)), 0), 41.0);
    }
    assert_eq!(counter.count(), 1);
    drop(counter);
    assert_eq!(js_sqlite_close(db), 1);
    drop_handle(record);
    drop_handle(raw);
}

#[test]
fn sqlite_preparation_early_get_resets_before_all_and_eviction_recompiles() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    exec(db, "CREATE TABLE t (value INTEGER)");
    exec(db, "INSERT INTO t VALUES (1), (2), (3)");
    perry_ffi::with_handle::<SqliteDbHandle, _, _>(db, |db| {
        db.conn
            .lock()
            .unwrap()
            .set_prepared_statement_cache_capacity(1);
    })
    .unwrap();
    let counter = PrepareCounter::install(db);
    let a = prepare(db, "SELECT value FROM t ORDER BY value");
    unsafe {
        assert_eq!(field_number(get(a, std::ptr::null()), 0), 1.0);
        let all = js_sqlite_stmt_all(a, std::ptr::null());
        assert_eq!(js_array_length(all), 3);
        assert_eq!(field_number(js_array_get(all, 2), 0), 3.0);
    }
    assert_eq!(counter.count(), 1);
    let b = prepare(db, "SELECT 99 AS other");
    unsafe {
        assert_eq!(field_number(get(b, std::ptr::null()), 0), 99.0);
        assert_eq!(field_number(get(a, std::ptr::null()), 0), 1.0);
    }
    assert_eq!(counter.count(), 3, "capacity-one A/B/A must recompile A");
    drop(counter);
    assert_eq!(js_sqlite_close(db), 1);
    assert_eq!(js_sqlite_is_db_handle(db), 0);
    assert_eq!(js_sqlite_close(db), 0);
    // Statement handles retain owned SQL, not a borrowed connection/statement.
    assert_eq!(js_sqlite_is_stmt_handle(a), 1);
    drop_handle(a);
    drop_handle(b);
}

#[test]
fn sqlite_preparation_schema_reprepare_refreshes_rows_and_empty_metadata() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    exec(db, "CREATE TABLE t (first INTEGER)");
    exec(db, "INSERT INTO t VALUES (1)");
    let stmt = prepare(db, "SELECT * FROM t");
    unsafe {
        assert_eq!(field_number(get(stmt, std::ptr::null()), 0), 1.0);
    }
    exec(db, "ALTER TABLE t ADD COLUMN second INTEGER DEFAULT 2");
    unsafe {
        let row = get(stmt, std::ptr::null());
        assert_eq!(field_number(row, 1), 2.0);
        let columns = js_sqlite_stmt_columns(stmt);
        assert_eq!(js_array_length(columns), 2);
        let name = js_object_get_field(js_array_get(columns, 1).as_pointer(), 0);
        assert_eq!(
            read_string(JsString::from_raw(name.as_string_ptr())),
            Some("second")
        );
        js_sqlite_stmt_raw(stmt);
        let all = js_sqlite_stmt_all(stmt, std::ptr::null());
        assert_eq!(js_array_length(js_array_get(all, 0).as_pointer()), 2);
    }
    exec(db, "DELETE FROM t");
    exec(db, "ALTER TABLE t ADD COLUMN third TEXT");
    // Assert the very first empty completion, before another call can refresh it.
    let (names, rows, _, _) = query_owned_rows(stmt, &[], false).unwrap();
    assert_eq!(names, ["first", "second", "third"]);
    assert!(rows.is_empty());
    unsafe {
        assert!(get(stmt, std::ptr::null()).is_undefined());
        assert_eq!(
            js_array_length(js_sqlite_stmt_all(stmt, std::ptr::null())),
            0
        );
    }
    assert_eq!(js_sqlite_close(db), 1);
    drop_handle(stmt);
}

#[test]
fn sqlite_preparation_b1_rejects_unicode_boundaries_cold_and_warm() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    exec(db, "CREATE TABLE t (n INTEGER)");
    exec(db, "INSERT INTO t VALUES (17)");
    let mut accepted = Vec::new();
    for boundary in ['\u{00a0}', '\u{2003}', '\u{2028}'] {
        for sql in [
            format!("{boundary}SELECT n FROM t"),
            format!("SELECT n FROM t{boundary}"),
        ] {
            // SQLite's original lexer, not Rust's whitespace classification, is the oracle.
            perry_ffi::with_handle::<SqliteDbHandle, _, _>(db, |db| {
                assert!(db.conn.lock().unwrap().prepare(&sql).is_err());
            })
            .unwrap();
            for warm in [false, true] {
                perry_ffi::with_handle::<SqliteDbHandle, _, _>(db, |db| {
                    db.conn.lock().unwrap().flush_prepared_statement_cache();
                })
                .unwrap();
                if warm {
                    let stmt = prepare(db, "SELECT n FROM t");
                    unsafe {
                        assert_eq!(field_number(get(stmt, std::ptr::null()), 0), 17.0);
                    }
                    drop_handle(stmt);
                }
                let text = alloc_string(&sql);
                let stmt = unsafe { js_sqlite_prepare(db, text.as_raw()) };
                if stmt != -1 {
                    accepted.push((sql.clone(), warm));
                    drop_handle(stmt);
                }
                let check = prepare(db, "SELECT n FROM t");
                unsafe {
                    let rows = js_sqlite_stmt_all(check, std::ptr::null());
                    assert_eq!(js_array_length(rows), 1);
                    assert_eq!(field_number(js_array_get(rows, 0), 0), 17.0);
                }
                drop_handle(check);
            }
        }
    }
    assert_eq!(js_sqlite_close(db), 1);
    assert!(
        accepted.is_empty(),
        "originally invalid SQL accepted: {accepted:?}"
    );
}

#[test]
fn sqlite_preparation_b1_preserves_unicode_aliases_comments_and_ascii_whitespace() {
    perry_ffi::initialize_runtime_fixture();
    let db = make_in_memory_db();
    let mut cases = vec![
        " \t\r\nSELECT 23 AS alias \t\r\n".to_owned(),
        "/* before */ SELECT 23 AS alias /* after */".to_owned(),
    ];
    for boundary in ['\u{00a0}', '\u{2003}', '\u{2028}'] {
        cases.push(format!("SELECT 23 AS alias{boundary}"));
        cases.push(format!("SELECT 23 AS alias -- trailing{boundary}"));
    }
    let mut mismatches = Vec::new();
    for sql in cases {
        let expected_names = perry_ffi::with_handle::<SqliteDbHandle, _, _>(db, |db| {
            let conn = db.conn.lock().unwrap();
            let mut stmt = conn
                .prepare(&sql)
                .expect("positive control accepted by SQLite");
            let mut rows = stmt.query([]).unwrap();
            let row = rows.next().unwrap().unwrap();
            assert_eq!(row.get::<_, i64>(0).unwrap(), 23);
            let names = row
                .as_ref()
                .column_names()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            assert!(rows.next().unwrap().is_none());
            names
        })
        .unwrap();
        let stmt = prepare(db, &sql);
        for first_only in [true, false] {
            // This is the actual owned producer used by public get/all, not fresh columns().
            let (names, rows, _, _) = query_owned_rows(stmt, &[], first_only).unwrap();
            assert_eq!(rows, vec![vec![SqliteValue::Integer(23)]]);
            if names != expected_names {
                mismatches.push((sql.clone(), first_only, names, expected_names.clone()));
            }
        }
        drop_handle(stmt);
    }
    assert_eq!(js_sqlite_close(db), 1);
    assert!(
        mismatches.is_empty(),
        "cached SQL changed column names: {mismatches:?}"
    );
}
