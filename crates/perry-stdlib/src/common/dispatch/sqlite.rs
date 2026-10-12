/// Dispatch method calls on SQLite Statement handles. Routes the
/// dynamic-receiver chain `this.stmt.raw().all(...params)` (drizzle's
/// PreparedQuery.values()) and similar shapes where the codegen
/// can't see the static stmt type. The runtime paths
/// (`js_sqlite_stmt_*`) take a pre-packed args array, so this
/// function repacks the f64 slice into a fresh JS array via
/// `js_array_alloc` + `js_array_push` before delegating.
///
/// Gated on `database-sqlite` — symbol/feature reasoning lives at
/// the caller arm in `js_handle_method_dispatch`. The extern
/// `js_sqlite_stmt_*` declarations resolve to whichever crate's impl
/// the linker picked (perry-stdlib's vs perry-ext-better-sqlite3's),
/// so this dispatch routes to the same impl that `js_sqlite_prepare`
/// used to allocate the handle. Refs #643.
#[cfg(feature = "database-sqlite")]
pub(crate) unsafe fn dispatch_sqlite_stmt(handle: i64, method: &str, args: &[f64]) -> f64 {
    use perry_runtime::js_nanbox_pointer;
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let arg_handles = scope.root_nanbox_f64_slice(args);
    // Pack args into a fresh JS array. Each `f64` is already a
    // NaN-boxed value as the codegen produces. js_array_push takes a
    // perry_ffi::JsValue (NaN-boxed), but the runtime helpers in
    // perry-stdlib accept JSValue::from_bits — convert via raw bits.
    let arr = perry_runtime::js_array_alloc(0);
    let arr_handle = scope.root_raw_mut_ptr(arr);
    for handle in &arg_handles {
        let v = handle.get_nanbox_f64();
        let arr = perry_runtime::js_array_push(
            arr_handle.get_raw_mut_ptr(),
            perry_runtime::JSValue::from_bits(v.to_bits()),
        );
        arr_handle.set_raw_mut_ptr(arr);
    }
    let arr_handle = arr_handle.get_raw_mut_ptr::<perry_runtime::ArrayHeader>();

    // Route through extern "C" so we hit the *linked* impl
    // (perry-stdlib's vs perry-ext-better-sqlite3's — only one wins
    // the link race when both crates expose `js_sqlite_*`). Calling
    // `crate::sqlite::js_sqlite_stmt_*` directly would always invoke
    // perry-stdlib's local impl, so handles registered by perry-ext's
    // `js_sqlite_prepare` (different TypeId) wouldn't downcast in
    // perry-stdlib's get_handle. The extern path delegates to whichever
    // crate's `js_sqlite_prepare` actually ran, keeping handle and
    // lookup TypeIds consistent. Refs #643.
    extern "C" {
        fn js_sqlite_stmt_raw(stmt_handle: i64) -> i64;
        fn js_sqlite_stmt_all(
            stmt_handle: i64,
            params_arr: *const perry_runtime::ArrayHeader,
        ) -> *mut perry_runtime::ArrayHeader;
        fn js_sqlite_stmt_get(
            stmt_handle: i64,
            params_arr: *const perry_runtime::ArrayHeader,
        ) -> f64;
        fn js_sqlite_stmt_run(
            stmt_handle: i64,
            params_arr: *const perry_runtime::ArrayHeader,
        ) -> *mut perry_runtime::ObjectHeader;
    }

    match method {
        "raw" => {
            let new_handle = js_sqlite_stmt_raw(handle);
            // NaN-box as a pointer so subsequent dynamic dispatch sees
            // it as a heap-pointer-shaped value (the runtime detects
            // small-handle range and routes back here).
            js_nanbox_pointer(new_handle)
        }
        "all" => {
            let arr_ptr = js_sqlite_stmt_all(handle, arr_handle);
            js_nanbox_pointer(arr_ptr as i64)
        }
        "get" => {
            // Already returns f64 (NaN-boxed bits).
            js_sqlite_stmt_get(handle, arr_handle)
        }
        "run" => {
            let obj_ptr = js_sqlite_stmt_run(handle, arr_handle);
            if obj_ptr.is_null() {
                f64::from_bits(perry_runtime::JSValue::undefined().bits())
            } else {
                js_nanbox_pointer(obj_ptr as i64)
            }
        }
        _ => f64::from_bits(perry_runtime::JSValue::undefined().bits()),
    }
}

/// Dispatch method calls on a SQLite Database handle (`db.prepare(sql)`,
/// `db.exec(sql)`, `db.close()`) — the Database counterpart to
/// `dispatch_sqlite_stmt`. Reached when codegen lost the static type
/// through a class field (e.g. drizzle's
/// `BetterSQLiteSession.prepareQuery` reads `this.client` typed as
/// `any` and calls `.prepare(query.sql)`). The static NATIVE_MODULE
/// dispatch-table path (#465) covers typed receivers; this arm is
/// the runtime fallback. Returns `JSValue::undefined()` if the handle
/// isn't a SqliteDb — the caller falls through to the next
/// dispatcher.
///
/// Like `dispatch_sqlite_stmt`, we route through `extern "C"` so the
/// linked impl wins (perry-stdlib's vs perry-ext-better-sqlite3's),
/// keeping handle and lookup TypeIds consistent regardless of which
/// crate registered the Database handle.
#[cfg(feature = "database-sqlite")]
pub(crate) unsafe fn dispatch_sqlite_db(handle: i64, method: &str, args: &[f64]) -> f64 {
    use perry_runtime::js_nanbox_pointer;

    extern "C" {
        fn js_sqlite_prepare(db_handle: i64, sql_ptr: *const perry_runtime::StringHeader) -> i64;
        fn js_sqlite_exec(db_handle: i64, sql_ptr: *const perry_runtime::StringHeader) -> i32;
        fn js_sqlite_close(db_handle: i64) -> i32;
    }

    // Helper: extract a StringHeader pointer from a NaN-boxed f64. Inline SSO
    // SQL strings such as `BEGIN` and `END` have no heap header; materialize
    // them in the native-call scratch ring before passing them to SQLite.
    let arg_str_ptr = |idx: usize| -> *const perry_runtime::StringHeader {
        if idx >= args.len() {
            return std::ptr::null();
        }
        let value = perry_runtime::JSValue::from_bits(args[idx].to_bits());
        if !value.is_any_string() {
            return std::ptr::null();
        }
        perry_runtime::value::js_ffi_arg_ptr(args[idx]) as *const perry_runtime::StringHeader
    };

    match method {
        "prepare" => {
            let sql_ptr = arg_str_ptr(0);
            if sql_ptr.is_null() {
                return f64::from_bits(perry_runtime::JSValue::undefined().bits());
            }
            let stmt_handle = js_sqlite_prepare(handle, sql_ptr);
            // -1 means prepare failed (invalid SQL or not-a-Database
            // handle — the registry lookup inside `js_sqlite_prepare`
            // returns None for the latter). Returning undefined lets
            // the outer dispatcher fall through to other arms (e.g.
            // when the handle is actually a SignHandle or FastifyApp
            // with a coincidentally-named "prepare" method).
            if stmt_handle < 0 {
                return f64::from_bits(perry_runtime::JSValue::undefined().bits());
            }
            // NaN-box as POINTER so subsequent `.run(...)` / `.all(...)`
            // / `.get(...)` calls re-enter the small-handle dispatch
            // path and route to `dispatch_sqlite_stmt`.
            js_nanbox_pointer(stmt_handle)
        }
        "exec" => {
            let sql_ptr = arg_str_ptr(0);
            if sql_ptr.is_null() {
                return f64::from_bits(perry_runtime::JSValue::undefined().bits());
            }
            let _ = js_sqlite_exec(handle, sql_ptr);
            // better-sqlite3 returns the Database for chaining; mirror
            // that so `db.exec("...").exec("...")` chains.
            js_nanbox_pointer(handle)
        }
        "close" => {
            let _ = js_sqlite_close(handle);
            f64::from_bits(perry_runtime::JSValue::undefined().bits())
        }
        _ => f64::from_bits(perry_runtime::JSValue::undefined().bits()),
    }
}

#[cfg(all(test, feature = "database-sqlite"))]
mod tests {
    use super::dispatch_sqlite_db;
    use perry_runtime::{js_nanbox_pointer, js_string_from_bytes, value::js_ffi_arg_ptr, JSValue};

    fn short(text: &'static [u8]) -> f64 {
        assert!(text.len() <= perry_runtime::value::SHORT_STRING_MAX_LEN);
        f64::from_bits(JSValue::short_string_unchecked(text).bits())
    }

    #[test]
    fn dynamic_sqlite_dispatch_accepts_sso_begin_end_and_prepare_sql() {
        extern "C" {
            fn js_sqlite_open(filename: *const perry_runtime::StringHeader) -> i64;
            fn js_sqlite_exec(db: i64, sql: *const perry_runtime::StringHeader) -> i32;
            fn js_sqlite_close(db: i64) -> i32;
        }

        let filename = js_string_from_bytes(b":memory:".as_ptr(), 8);
        let db = unsafe { js_sqlite_open(filename) };
        assert!(db >= 0, "in-memory database must open");

        let begin = short(b"BEGIN");
        let end = short(b"END");
        let expected_db = js_nanbox_pointer(db);
        assert_eq!(
            unsafe { dispatch_sqlite_db(db, "exec", &[begin]) }.to_bits(),
            expected_db.to_bits(),
            "exec keeps its database chaining result"
        );

        // A second BEGIN through the heap-string native entry fails only when
        // the first SSO BEGIN actually reached SQLite.
        let heap_begin = js_string_from_bytes(b"BEGIN".as_ptr(), 5);
        let heap_begin_value = perry_runtime::js_nanbox_string(heap_begin as i64);
        let heap_begin_ptr = js_ffi_arg_ptr(heap_begin_value) as *const perry_runtime::StringHeader;
        assert_eq!(unsafe { js_sqlite_exec(db, heap_begin_ptr) }, 0);

        let prepared = unsafe { dispatch_sqlite_db(db, "prepare", &[begin]) };
        assert_ne!(
            prepared.to_bits(),
            JSValue::undefined().bits(),
            "a prepared BEGIN from an SSO SQL string returns a statement handle"
        );

        assert_eq!(
            unsafe { dispatch_sqlite_db(db, "exec", &[end]) }.to_bits(),
            expected_db.to_bits(),
            "SSO END commits and preserves the chaining result"
        );
        assert_eq!(
            unsafe { js_sqlite_exec(db, heap_begin_ptr) },
            1,
            "a heap-string BEGIN succeeds after the SSO END closed the transaction"
        );
        unsafe { dispatch_sqlite_db(db, "exec", &[end]) };
        unsafe { js_sqlite_close(db) };
    }
}
