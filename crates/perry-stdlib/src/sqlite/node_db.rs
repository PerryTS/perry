use super::*;
use crate::common::{get_handle, register_handle, Handle};
use perry_runtime::{
    js_get_string_pointer_unified, js_nanbox_pointer, js_promise_rejected, js_promise_resolved,
    JSValue, Promise, StringHeader,
};
use rusqlite::{ffi, Connection};
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_call(
    _path_value: f64,
    _options_value: f64,
) -> Handle {
    throw_construct_required()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_native_dispatch(
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    construct: i32,
) -> f64 {
    let method_name = if method_name_ptr.is_null() || method_name_len == 0 {
        ""
    } else {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(method_name_ptr, method_name_len))
    };
    let arg = |index: usize| -> f64 {
        if index < args_len && !args_ptr.is_null() {
            *args_ptr.add(index)
        } else {
            undefined_f64()
        }
    };
    let arg0 = arg(0);
    let arg1 = arg(1);
    let arg2 = arg(2);

    match (method_name, construct != 0) {
        ("DatabaseSync", true) => js_nanbox_pointer(js_node_sqlite_database_sync_new(arg0, arg1)),
        ("DatabaseSync", false) => js_nanbox_pointer(js_node_sqlite_database_sync_call(arg0, arg1)),
        ("Session", true) => js_nanbox_pointer(js_node_sqlite_session_new(arg0, arg1)),
        ("Session", false) => js_nanbox_pointer(js_node_sqlite_session_call(arg0, arg1)),
        ("StatementSync", true) => js_nanbox_pointer(js_node_sqlite_statement_sync_new(arg0, arg1)),
        ("StatementSync", false) => {
            js_nanbox_pointer(js_node_sqlite_statement_sync_call(arg0, arg1))
        }
        ("backup", _) => js_nanbox_pointer(js_node_sqlite_backup(arg0, arg1, arg2) as i64),
        _ => undefined_f64(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_backup(
    source_db_value: f64,
    path_value: f64,
    options_value: f64,
) -> *mut Promise {
    let db_handle = database_handle_from_backup_source(source_db_value);
    let db = get_handle::<NodeSqliteDbHandle>(db_handle).unwrap_or_else(|| {
        throw_type("The \"sourceDb\" argument must be an instance of DatabaseSync.")
    });
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

    let path = path_like_from_value(path_value, "path");
    let options = parse_node_sqlite_backup_options(options_value);
    let result = {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("database is not open"));
        let Some(conn) = conn.as_ref() else {
            drop(conn);
            throw_invalid_state("database is not open");
        };
        perform_node_sqlite_backup(conn, &path, &options)
    };

    match result {
        Ok(total_pages) => {
            js_promise_resolved(f64::from_bits(JSValue::number(total_pages as f64).bits()))
        }
        Err(error) => js_promise_rejected(sqlite_error_value(error)),
    }
}

/// Validate the `DatabaseSync` `path` argument per Node: a string,
/// Uint8Array/Buffer, or `file:` URL, none of which may contain null
/// bytes — with Node's exact `ERR_INVALID_ARG_TYPE` message (#6561).
pub(crate) unsafe fn node_sqlite_database_path(value: f64) -> String {
    const PATH_TYPE_MSG: &str =
        "The \"path\" argument must be a string, Uint8Array, or URL without null bytes.";
    let js = value_from_f64(value);
    let path = if js.is_any_string() {
        let ptr = js_get_string_pointer_unified(value) as *const StringHeader;
        string_from_header(ptr).unwrap_or_else(|| throw_type(PATH_TYPE_MSG))
    } else if let Some(bytes) = bytes_from_path_like(value) {
        if bytes.contains(&0) {
            throw_type(PATH_TYPE_MSG);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    } else if js.is_pointer() {
        // URL object — accept `file:` URLs, decoding the percent-encoded
        // pathname like Node's `fileURLToPath`.
        let protocol = string_from_jsvalue(object_field(value, "protocol")).unwrap_or_default();
        let pathname = string_from_jsvalue(object_field(value, "pathname")).unwrap_or_default();
        if protocol != "file:" || pathname.is_empty() {
            throw_type(PATH_TYPE_MSG);
        }
        percent_decode_pathname(&pathname)
    } else {
        throw_type(PATH_TYPE_MSG);
    };
    if path.as_bytes().contains(&0) {
        throw_type(PATH_TYPE_MSG);
    }
    path
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_new(
    path_value: f64,
    options_value: f64,
) -> Handle {
    let path = node_sqlite_database_path(path_value);
    let options = parse_node_sqlite_options(options_value);
    let open = options.open;
    let handle = register_handle(NodeSqliteDbHandle {
        conn: Mutex::new(None),
        path,
        read_only: options.read_only,
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
        authorizer_callback: Mutex::new(None),
        initial_limits: options.initial_limits,
        limits_handle: Mutex::new(None),
        sessions: Mutex::new(HashSet::new()),
        statements: Mutex::new(HashSet::new()),
    });
    if open {
        js_node_sqlite_database_sync_open(handle);
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_open(db_handle: Handle) -> i32 {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
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
        // Carry the SQLite result code through (`errcode`/`errstr`), e.g.
        // errcode 14 "unable to open database file" — matching Node (#6561).
        Err(err) => {
            perry_runtime::exception::js_throw(sqlite_error_value(sqlite_error_from_rusqlite(err)))
        }
    };
    if let Err(err) = configure_node_sqlite_defensive(&opened, db.defensive.load(Ordering::Relaxed))
    {
        throw_sqlite_error(&err);
    }
    if let Err(err) = configure_node_sqlite_load_extension(
        &opened,
        db.enable_load_extension.load(Ordering::Relaxed),
    ) {
        throw_sqlite_error(&err);
    }
    let mut conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    *conn = Some(opened);
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_close(db_handle: Handle) -> i32 {
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
    finalize_node_sqlite_statements(db);
    delete_node_sqlite_sessions(db);
    if let Ok(mut callback) = db.authorizer_callback.lock() {
        *callback = None;
    }
    let mut conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    if conn.is_some() {
        *conn = None;
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_dispose(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<NodeSqliteDbHandle>(db_handle) {
        finalize_node_sqlite_statements(db);
        delete_node_sqlite_sessions(db);
        if let Ok(mut callback) = db.authorizer_callback.lock() {
            *callback = None;
        }
        if let Ok(mut conn) = db.conn.lock() {
            *conn = None;
        }
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_is_open(db_handle: Handle) -> f64 {
    let is_open = get_handle::<NodeSqliteDbHandle>(db_handle)
        .and_then(|db| db.conn.lock().ok().map(|conn| conn.is_some()))
        .unwrap_or(false);
    bool_f64(is_open)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_is_transaction(db_handle: Handle) -> f64 {
    with_open_node_connection(db_handle, |conn| bool_f64(!conn.is_autocommit()))
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_exec(
    db_handle: Handle,
    sql_value: f64,
) -> i32 {
    ensure_open_node_database(db_handle);
    let sql = string_from_value(sql_value, "sql");
    let result = with_open_node_connection(db_handle, |conn| node_sqlite_exec_batch(conn, &sql));
    match result {
        Ok(_) => 1,
        Err((message, errcode)) => throw_sqlite_error_ext(&message, errcode),
    }
}

pub(crate) unsafe fn parse_statement_options(
    db: &NodeSqliteDbHandle,
    options_value: f64,
) -> NodeSqliteStmtOptions {
    let js = value_from_f64(options_value);
    if js.is_undefined() {
        return NodeSqliteStmtOptions {
            read_bigints: db.read_bigints,
            return_arrays: db.return_arrays,
            allow_bare_named_parameters: db.allow_bare_named_parameters,
            allow_unknown_named_parameters: db.allow_unknown_named_parameters,
        };
    }
    if js.is_null() || !is_object_like(options_value) {
        throw_type("The \"options\" argument must be an object.");
    }
    NodeSqliteStmtOptions {
        read_bigints: bool_option(options_value, "readBigInts", db.read_bigints),
        return_arrays: bool_option(options_value, "returnArrays", db.return_arrays),
        allow_bare_named_parameters: bool_option(
            options_value,
            "allowBareNamedParameters",
            db.allow_bare_named_parameters,
        ),
        allow_unknown_named_parameters: bool_option(
            options_value,
            "allowUnknownNamedParameters",
            db.allow_unknown_named_parameters,
        ),
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_prepare(
    db_handle: Handle,
    sql_value: f64,
    options_value: f64,
) -> Handle {
    ensure_open_node_database(db_handle);
    let sql = string_from_value(sql_value, "sql");
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let options = parse_statement_options(db, options_value);
    let expanded_sql = with_open_node_connection(db_handle, |conn| {
        let raw = prepare_node_raw_statement(conn, &sql);
        let expanded = ffi::sqlite3_expanded_sql(raw.ptr);
        let expanded_sql = if expanded.is_null() {
            String::new()
        } else {
            let text = CStr::from_ptr(expanded).to_string_lossy().into_owned();
            ffi::sqlite3_free(expanded.cast::<c_void>());
            text
        };
        drop(raw);
        expanded_sql
    });
    let handle = register_handle(NodeSqliteStmtHandle {
        db_handle,
        sql,
        finalized: AtomicBool::new(false),
        read_bigints: AtomicBool::new(options.read_bigints),
        return_arrays: AtomicBool::new(options.return_arrays),
        allow_bare_named_parameters: AtomicBool::new(options.allow_bare_named_parameters),
        allow_unknown_named_parameters: AtomicBool::new(options.allow_unknown_named_parameters),
        expanded_sql: Mutex::new(expanded_sql),
    });
    if let Ok(mut statements) = db.statements.lock() {
        statements.insert(handle);
    }
    handle
}

pub(crate) fn sqlite_function_name(name: String) -> CString {
    let bytes = name.as_bytes();
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    CString::new(&bytes[..end]).unwrap_or_else(|_| CString::new("").unwrap())
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_function(
    db_handle: Handle,
    name_value: f64,
    options_or_function_value: f64,
    function_value: f64,
) -> i32 {
    ensure_open_node_database(db_handle);
    let name = sqlite_function_name(string_from_value(name_value, "name"));

    let (options_value, callback) = if closure_ptr_from_value(options_or_function_value).is_some() {
        (undefined_f64(), options_or_function_value)
    } else {
        let options_js = value_from_f64(options_or_function_value);
        if options_js.is_undefined() && value_from_f64(function_value).is_undefined() {
            node_sqlite_function_arg(options_or_function_value, "function");
        }
        if options_js.is_null()
            || options_js.is_undefined()
            || !is_object_like(options_or_function_value)
        {
            throw_type("The \"options\" argument must be an object.");
        }
        (
            options_or_function_value,
            node_sqlite_function_arg(function_value, "function"),
        )
    };

    let use_bigint_arguments =
        node_sqlite_bool_option_exact(options_value, "useBigIntArguments", false);
    let varargs = node_sqlite_bool_option_exact(options_value, "varargs", false);
    let deterministic = node_sqlite_bool_option_exact(options_value, "deterministic", false);
    let direct_only = node_sqlite_bool_option_exact(options_value, "directOnly", false);
    let argc = if varargs {
        -1
    } else {
        node_sqlite_closure_arity(callback)
    };

    let mut text_rep = ffi::SQLITE_UTF8;
    if deterministic {
        text_rep |= ffi::SQLITE_DETERMINISTIC;
    }
    if direct_only {
        text_rep |= ffi::SQLITE_DIRECTONLY;
    }

    perry_runtime::gc::js_write_barrier_root_nanbox(callback.to_bits());
    let info = Box::into_raw(Box::new(NodeSqliteCustomFunction {
        callback,
        use_bigint_arguments,
    }));
    register_node_sqlite_custom_function(info);
    let rc = with_open_node_connection(db_handle, |conn| {
        ffi::sqlite3_create_function_v2(
            conn.handle(),
            name.as_ptr(),
            argc,
            text_rep,
            info as *mut c_void,
            Some(node_sqlite_scalar_callback),
            None,
            None,
            Some(node_sqlite_scalar_destroy),
        )
    });
    if rc != ffi::SQLITE_OK {
        if unregister_node_sqlite_custom_function(info) {
            drop(Box::from_raw(info));
        }
        with_open_node_connection(db_handle, |conn| throw_sqlite_error_from_conn(conn))
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_aggregate(
    db_handle: Handle,
    name_value: f64,
    options_value: f64,
) -> i32 {
    ensure_open_node_database(db_handle);
    let name = sqlite_function_name(string_from_value(name_value, "name"));

    let start = object_field(options_value, "start");
    if start.is_undefined() {
        throw_type("The \"options.start\" argument must be a function or a primitive value.");
    }
    let step = node_sqlite_optional_callback_option(options_value, "step", true)
        .unwrap_or_else(|| throw_type("The \"options.step\" argument must be a function."));
    let result = node_sqlite_optional_callback_option(options_value, "result", false);
    let inverse = node_sqlite_optional_callback_option(options_value, "inverse", true);
    let use_bigint_arguments =
        node_sqlite_bool_option_exact(options_value, "useBigIntArguments", false);
    let varargs = node_sqlite_bool_option_exact(options_value, "varargs", false);
    let direct_only = node_sqlite_bool_option_exact(options_value, "directOnly", false);
    let argc = if varargs {
        -1
    } else {
        node_sqlite_closure_arity(step).saturating_sub(1)
    };

    let mut text_rep = ffi::SQLITE_UTF8;
    if direct_only {
        text_rep |= ffi::SQLITE_DIRECTONLY;
    }

    let start = f64::from_bits(start.bits());
    perry_runtime::gc::js_write_barrier_root_nanbox(start.to_bits());
    perry_runtime::gc::js_write_barrier_root_nanbox(step.to_bits());
    if let Some(result) = result {
        perry_runtime::gc::js_write_barrier_root_nanbox(result.to_bits());
    }
    if let Some(inverse) = inverse {
        perry_runtime::gc::js_write_barrier_root_nanbox(inverse.to_bits());
    }
    let aggregate = Box::into_raw(Box::new(NodeSqliteCustomAggregate {
        start,
        step,
        result,
        inverse,
        use_bigint_arguments,
    }));
    register_node_sqlite_custom_aggregate(aggregate);
    let has_inverse = inverse.is_some();
    let rc = with_open_node_connection(db_handle, |conn| {
        ffi::sqlite3_create_window_function(
            conn.handle(),
            name.as_ptr(),
            argc,
            text_rep,
            aggregate as *mut c_void,
            Some(node_sqlite_aggregate_step),
            Some(node_sqlite_aggregate_final),
            if has_inverse {
                Some(node_sqlite_aggregate_value)
            } else {
                None
            },
            if has_inverse {
                Some(node_sqlite_aggregate_inverse)
            } else {
                None
            },
            Some(node_sqlite_aggregate_destroy),
        )
    });
    if rc != ffi::SQLITE_OK {
        if unregister_node_sqlite_custom_aggregate(aggregate) {
            drop(Box::from_raw(aggregate));
        }
        with_open_node_connection(db_handle, |conn| throw_sqlite_error_from_conn(conn))
    }
    1
}

pub(crate) unsafe fn configure_node_sqlite_defensive(
    conn: &Connection,
    active: bool,
) -> Result<(), String> {
    let mut current = 0;
    let rc = ffi::sqlite3_db_config(
        conn.handle(),
        ffi::SQLITE_DBCONFIG_DEFENSIVE,
        if active { 1 } else { 0 },
        &mut current,
    );
    if rc == ffi::SQLITE_OK {
        return Ok(());
    }
    Err(CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
        .to_string_lossy()
        .into_owned())
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_enable_defensive(
    db_handle: Handle,
    active_value: f64,
) -> i32 {
    let js = value_from_f64(active_value);
    if !js.is_bool() {
        throw_type("The \"active\" argument must be a boolean.");
    }
    let active = js.as_bool();
    ensure_open_node_database(db_handle);
    let result = with_open_node_connection(db_handle, |conn| {
        configure_node_sqlite_defensive(conn, active)
    });
    if let Err(message) = result {
        throw_sqlite_error(&message);
    }
    if let Some(db) = get_handle::<NodeSqliteDbHandle>(db_handle) {
        db.defensive.store(active, Ordering::Relaxed);
    }
    1
}

pub(crate) unsafe extern "C" fn node_sqlite_authorizer_callback(
    user_data: *mut c_void,
    action_code: c_int,
    arg1: *const c_char,
    arg2: *const c_char,
    db_name: *const c_char,
    trigger_or_view: *const c_char,
) -> c_int {
    let db_handle = user_data as Handle;
    let Some(db) = get_handle::<NodeSqliteDbHandle>(db_handle) else {
        return ffi::SQLITE_OK;
    };
    let callback = db
        .authorizer_callback
        .lock()
        .ok()
        .and_then(|callback| *callback);
    let Some(callback) = callback else {
        return ffi::SQLITE_OK;
    };
    let args = [
        f64_from_jsvalue(JSValue::number(action_code as f64)),
        f64_from_jsvalue(sqlite_c_string_value(arg1)),
        f64_from_jsvalue(sqlite_c_string_value(arg2)),
        f64_from_jsvalue(sqlite_c_string_value(db_name)),
        f64_from_jsvalue(sqlite_c_string_value(trigger_or_view)),
    ];
    let result = value_from_f64(node_sqlite_call_closure(callback, &args));
    let code = if result.is_int32() {
        result.as_int32()
    } else if result.is_number() {
        let number = result.as_number();
        if !number.is_finite()
            || number.fract() != 0.0
            || number < c_int::MIN as f64
            || number > c_int::MAX as f64
        {
            throw_plain_type("Authorizer callback must return an integer authorization code");
        }
        number as c_int
    } else {
        throw_plain_type("Authorizer callback must return an integer authorization code");
    };
    match code {
        ffi::SQLITE_OK | ffi::SQLITE_DENY | ffi::SQLITE_IGNORE => code,
        _ => throw_plain_range("Authorizer callback returned a invalid authorization code"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_set_authorizer(
    db_handle: Handle,
    callback_value: f64,
) -> i32 {
    ensure_open_node_database(db_handle);
    ensure_node_sqlite_gc_scanner_registered();
    let js = value_from_f64(callback_value);
    let callback = if js.is_null() {
        None
    } else {
        if closure_ptr_from_value(callback_value).is_none() {
            throw_type("The \"callback\" argument must be a function or null.");
        }
        perry_runtime::gc::js_write_barrier_root_nanbox(callback_value.to_bits());
        Some(callback_value)
    };
    let rc = with_open_node_connection(db_handle, |conn| {
        ffi::sqlite3_set_authorizer(
            conn.handle(),
            if callback.is_some() {
                Some(node_sqlite_authorizer_callback)
            } else {
                None
            },
            if callback.is_some() {
                db_handle as *mut c_void
            } else {
                std::ptr::null_mut()
            },
        )
    });
    if rc != ffi::SQLITE_OK {
        with_open_node_connection(db_handle, |conn| throw_sqlite_error_from_conn(conn))
    }
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    if let Ok(mut stored) = db.authorizer_callback.lock() {
        *stored = callback;
    }
    1
}
