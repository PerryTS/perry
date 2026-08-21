use super::*;
use crate::common::{get_handle, Handle};
use perry_runtime::{
    closure::{
        js_closure_alloc, js_closure_call_array, js_closure_get_capture_f64,
        js_closure_get_capture_ptr, js_closure_set_capture_f64, js_closure_set_capture_ptr,
        js_register_closure_rest, ClosureHeader,
    },
    js_array_alloc, js_array_get, js_array_length, js_array_push, js_nanbox_get_pointer,
    js_nanbox_pointer, js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader, StringHeader,
};
use rusqlite::ffi;
use std::sync::{atomic::Ordering, Once};

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_call(
    _path_value: f64,
    _options_value: f64,
) -> Handle {
    throw_plain_type("Cannot call a class constructor Database without |new|")
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_new(path_value: f64, options_value: f64) -> Handle {
    let path = if value_from_f64(path_value).is_undefined() {
        ":memory:".to_string()
    } else {
        let value = node_sqlite_database_path(path_value);
        if value.is_empty() {
            ":memory:".to_string()
        } else {
            value
        }
    };

    let mut options = NodeSqliteOptions::default();
    let explicit_options = !value_from_f64(options_value).is_undefined();
    if explicit_options {
        validate_optional_object(options_value);
        options.read_only = bool_option(options_value, "readonly", false);
        options.read_write = bool_option(options_value, "readwrite", false);
        options.create = bool_option(options_value, "create", false);
        if options.read_only && options.read_write {
            throw_plain_type(
                "flags must not include both SQLITE_OPEN_READONLY and SQLITE_OPEN_READWRITE",
            );
        }
        if !options.read_only && !options.read_write && !options.create {
            throw_sqlite_error("flags must include SQLITE_OPEN_READONLY or SQLITE_OPEN_READWRITE");
        }
        // Bun's strict mode accepts bare named keys and rejects unknown ones.
        let strict = bool_option(options_value, "strict", false);
        options.allow_bare_named_parameters = strict;
        options.allow_unknown_named_parameters = !strict;
        options.read_bigints = bool_option(options_value, "safeIntegers", false);
    }
    options.enable_foreign_keys = false;
    options.allow_extension = true;
    options.defensive = false;
    register_node_sqlite_database(path, options, "bun:sqlite")
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_query(db_handle: Handle, sql_value: f64) -> Handle {
    js_node_sqlite_database_sync_prepare(db_handle, sql_value, undefined_f64())
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_run(
    db_handle: Handle,
    sql_value: f64,
    params: *const ArrayHeader,
) -> *mut ObjectHeader {
    let statement = js_bun_sqlite_database_query(db_handle, sql_value);
    let result = js_node_sqlite_statement_sync_run(statement, params);
    finalize_node_sqlite_statement_handle(statement);
    result
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_filename(db_handle: Handle) -> *mut StringHeader {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    js_string_from_bytes(db.path.as_ptr(), db.path.len() as u32)
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_statement_values(
    stmt_handle: Handle,
    params: *const ArrayHeader,
) -> *mut ArrayHeader {
    with_node_sqlite_statement(stmt_handle, params, |conn, stmt, raw_stmt| {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let rows = js_array_alloc(0);
        let rows_handle = scope.root_raw_mut_ptr(rows);
        let row_handle = scope.root_nanbox_u64(JSValue::undefined().bits());
        loop {
            match ffi::sqlite3_step(raw_stmt) {
                ffi::SQLITE_ROW => {
                    let row = node_sqlite_row_value_with_mode(stmt, raw_stmt, true);
                    row_handle.set_nanbox_u64(row.bits());
                    let rows = js_array_push(
                        rows_handle.get_raw_mut_ptr(),
                        JSValue::from_bits(row_handle.get_nanbox_u64()),
                    );
                    rows_handle.set_raw_mut_ptr(rows);
                }
                ffi::SQLITE_DONE => break,
                _ => throw_sqlite_error_from_conn(conn),
            }
        }
        rows_handle.get_raw_mut_ptr()
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_statement_safe_integers(
    stmt_handle: Handle,
    enabled_value: f64,
) -> f64 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    if stmt.finalized.load(Ordering::Relaxed) {
        throw_invalid_state("statement has been finalized");
    }
    let enabled = value_from_f64(enabled_value);
    if enabled.is_undefined() {
        return bool_f64(stmt.read_bigints.load(Ordering::Relaxed));
    }
    stmt.read_bigints
        .store(enabled.to_bool(), Ordering::Relaxed);
    js_nanbox_pointer(stmt_handle)
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_statement_finalize(stmt_handle: Handle) {
    finalize_node_sqlite_statement_handle(stmt_handle);
}

unsafe extern "C" fn bun_sqlite_transaction_wrapper(
    wrapper: *const ClosureHeader,
    rest_value: f64,
) -> f64 {
    let db_handle = js_closure_get_capture_f64(wrapper, 0) as Handle;
    let callback = js_closure_get_capture_ptr(wrapper, 1) as *const ClosureHeader;
    let rest = js_nanbox_get_pointer(rest_value) as *const ArrayHeader;
    let arg_count = js_array_length(rest);
    let args: Vec<f64> = (0..arg_count)
        .map(|index| f64_from_jsvalue(js_array_get(rest, index)))
        .collect();
    let nested = with_open_node_connection(db_handle, |conn| !conn.is_autocommit());
    let begin = if nested {
        "SAVEPOINT `bun:sqlite transaction`"
    } else {
        "BEGIN"
    };
    with_open_node_connection(db_handle, |conn| {
        if let Err((message, code)) = node_sqlite_exec_batch(conn, begin) {
            throw_sqlite_error_ext(&message, code);
        }
    });

    match perry_runtime::exception::js_call_catching(|| {
        js_closure_call_array(
            callback as i64,
            if args.is_empty() {
                std::ptr::null()
            } else {
                args.as_ptr()
            },
            args.len() as i64,
        )
    }) {
        Ok(value) => {
            let finish = if nested {
                "RELEASE `bun:sqlite transaction`"
            } else {
                "COMMIT"
            };
            with_open_node_connection(db_handle, |conn| {
                if let Err((message, code)) = node_sqlite_exec_batch(conn, finish) {
                    throw_sqlite_error_ext(&message, code);
                }
            });
            value
        }
        Err(error) => {
            let rollback = if nested {
                "ROLLBACK TO `bun:sqlite transaction`; RELEASE `bun:sqlite transaction`"
            } else {
                "ROLLBACK"
            };
            with_open_node_connection(db_handle, |conn| {
                let _ = node_sqlite_exec_batch(conn, rollback);
            });
            perry_runtime::exception::js_throw(error)
        }
    }
}

static BUN_SQLITE_TRANSACTION_WRAPPER_REGISTERED: Once = Once::new();

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_database_transaction(
    db_handle: Handle,
    callback_value: f64,
) -> *mut ClosureHeader {
    let callback = closure_ptr_from_value(callback_value)
        .unwrap_or_else(|| throw_plain_type("Expected a function"));
    BUN_SQLITE_TRANSACTION_WRAPPER_REGISTERED.call_once(|| {
        // The wrapper has no fixed arguments and receives every invocation
        // argument in the synthetic rest array. This preserves Bun's
        // `transaction(fn)(...args)` forwarding for arbitrary callback arity.
        js_register_closure_rest(bun_sqlite_transaction_wrapper as *const u8, 0);
    });
    let wrapper = js_closure_alloc(bun_sqlite_transaction_wrapper as *const u8, 2);
    js_closure_set_capture_f64(wrapper, 0, db_handle as f64);
    js_closure_set_capture_ptr(wrapper, 1, callback as i64);
    wrapper
}
