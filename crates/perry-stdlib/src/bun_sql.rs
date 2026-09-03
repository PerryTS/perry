//! Bun's unified `SQL` tagged-template surface.
//!
//! The first adapter is SQLite, backed by the same rusqlite handles as
//! `bun:sqlite` and `node:sqlite`.  The public value is deliberately a closure:
//! Bun SQL clients are both callable tags (`sql`SELECT ... ${value}``) and
//! objects with lifecycle/transaction methods.

use crate::common::Handle;
use crate::sqlite::*;
use perry_runtime::{
    closure::{
        closure_set_dynamic_prop, js_closure_alloc, js_closure_call1, js_closure_call2,
        js_closure_get_capture_f64, js_closure_set_capture_f64, js_register_closure_arity,
        js_register_closure_rest, ClosureHeader,
    },
    exception::{js_call_catching, js_throw},
    gc::RuntimeHandleScope,
    js_array_get, js_array_is_array, js_array_length, js_nanbox_get_pointer, js_promise_resolved,
    promise::{js_promise_reject, js_promise_resolved_catching, js_promise_then},
    ArrayHeader, JSValue, Promise,
};
use std::sync::Once;

const CLIENT_HANDLE_CAPTURE: u32 = 0;
const METHOD_HANDLE_CAPTURE: u32 = 0;
const METHOD_CLIENT_CAPTURE: u32 = 1;
const METHOD_ONCLOSE_CAPTURE: u32 = 2;

fn promise_value(promise: *mut Promise) -> f64 {
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

fn rejected_promise(reason: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let reason = scope.root_nanbox_f64(reason);
    let promise = scope.root_raw_mut_ptr(perry_runtime::promise::js_promise_new());
    js_promise_reject(promise.get_raw_mut_ptr(), reason.get_nanbox_f64());
    promise_value(promise.get_raw_mut_ptr())
}

fn resolved_promise(value: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let promise = scope.root_raw_mut_ptr(js_promise_resolved(value.get_nanbox_f64()));
    promise_value(promise.get_raw_mut_ptr())
}

fn boxed_closure(closure: *const ClosureHeader) -> f64 {
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

unsafe fn captured_handle(closure: *const ClosureHeader) -> Handle {
    js_closure_get_capture_f64(closure, METHOD_HANDLE_CAPTURE) as Handle
}

unsafe fn captured_client(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, METHOD_CLIENT_CAPTURE)
}

unsafe fn template_array(value: f64) -> Option<*const ArrayHeader> {
    let is_array = JSValue::from_bits(js_array_is_array(value).to_bits());
    if !is_array.is_bool() || !is_array.as_bool() {
        return None;
    }
    let ptr = js_nanbox_get_pointer(value) as *const ArrayHeader;
    (!ptr.is_null()).then_some(ptr)
}

unsafe fn build_sql(strings_value: f64, params_value: f64) -> String {
    let strings = template_array(strings_value)
        .unwrap_or_else(|| throw_plain_type("SQL queries must be called as tagged templates"));
    let params = template_array(params_value)
        .unwrap_or_else(|| throw_plain_type("SQL tagged-template parameters must be an array"));
    let string_count = js_array_length(strings);
    let param_count = js_array_length(params);
    if string_count != param_count.saturating_add(1) {
        throw_plain_type("Invalid SQL tagged-template strings array");
    }

    let mut sql = String::new();
    for index in 0..string_count {
        let quasi = js_array_get(strings, index);
        let Some(text) = string_key_from_js_value(quasi) else {
            throw_plain_type("SQL tagged-template strings must contain only strings");
        };
        sql.push_str(&text);
        if index < param_count {
            // Values stay in `params` and are bound by SQLite.  Never append a
            // rendered JS value to this string: this is the injection boundary.
            sql.push('?');
        }
    }
    sql
}

unsafe fn execute_sqlite_query(db_handle: Handle, strings_value: f64, params_value: f64) -> f64 {
    let outcome = js_call_catching(|| {
        let scope = RuntimeHandleScope::new();
        let strings = scope.root_nanbox_f64(strings_value);
        let params = scope.root_nanbox_f64(params_value);
        let sql = build_sql(strings.get_nanbox_f64(), params.get_nanbox_f64());
        let sql_value = scope.root_nanbox_f64(f64_from_jsvalue(string_value(&sql)));
        let statement = js_bun_sqlite_database_query(db_handle, sql_value.get_nanbox_f64());
        let params_ptr = template_array(params.get_nanbox_f64())
            .expect("the rooted tagged-template parameter array remains an array");
        let rows = js_call_catching(|| {
            let rows = js_node_sqlite_statement_sync_all(statement, params_ptr);
            f64::from_bits(JSValue::array_ptr(rows).bits())
        });
        finalize_node_sqlite_statement_handle(statement);
        match rows {
            Ok(rows) => rows,
            Err(error) => js_throw(error),
        }
    });

    match outcome {
        Ok(rows) => resolved_promise(rows),
        Err(error) => rejected_promise(error),
    }
}

extern "C" fn bun_sql_query_tag(
    closure: *const ClosureHeader,
    strings_value: f64,
    params_value: f64,
) -> f64 {
    unsafe { execute_sqlite_query(captured_handle(closure), strings_value, params_value) }
}

fn transaction_sql(nested: bool, success: bool) -> &'static str {
    match (nested, success) {
        (false, true) => "COMMIT",
        (false, false) => "ROLLBACK",
        (true, true) => "RELEASE perry_bun_sql",
        (true, false) => "ROLLBACK TO perry_bun_sql; RELEASE perry_bun_sql",
    }
}

unsafe fn finish_transaction(db_handle: Handle, nested: bool, success: bool) {
    let sql = transaction_sql(nested, success);
    with_open_node_connection(db_handle, |conn| {
        if let Err((message, code)) = node_sqlite_exec_batch(conn, sql) {
            throw_sqlite_error_ext(&message, code);
        }
    });
}

extern "C" fn bun_sql_transaction_fulfilled(closure: *const ClosureHeader, value: f64) -> f64 {
    unsafe {
        let handle = captured_handle(closure);
        let nested = js_closure_get_capture_f64(closure, 1) != 0.0;
        finish_transaction(handle, nested, true);
        value
    }
}

extern "C" fn bun_sql_transaction_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let reason = scope.root_nanbox_f64(reason);
        let handle = captured_handle(closure);
        let nested = js_closure_get_capture_f64(closure, 1) != 0.0;
        // Preserve the callback's rejection even if a best-effort rollback
        // itself fails; it is the user-observable transaction failure.
        let _ = js_call_catching(|| {
            finish_transaction(handle, nested, false);
            undefined_f64()
        });
        js_throw(reason.get_nanbox_f64())
    }
}

extern "C" fn bun_sql_begin(closure: *const ClosureHeader, callback_value: f64) -> f64 {
    unsafe {
        if closure_ptr_from_value(callback_value).is_none() {
            let error = js_call_catching(|| throw_plain_type("SQL.begin expects a callback"))
                .expect_err("throw_plain_type always throws");
            return rejected_promise(error);
        };
        let scope = RuntimeHandleScope::new();
        let callback = scope.root_nanbox_f64(callback_value);
        let client = scope.root_nanbox_f64(captured_client(closure));
        let db_handle = captured_handle(closure);
        let nested = with_open_node_connection(db_handle, |conn| !conn.is_autocommit());
        let begin = if nested {
            "SAVEPOINT perry_bun_sql"
        } else {
            "BEGIN"
        };
        let started = js_call_catching(|| {
            with_open_node_connection(db_handle, |conn| {
                if let Err((message, code)) = node_sqlite_exec_batch(conn, begin) {
                    throw_sqlite_error_ext(&message, code);
                }
            });
            undefined_f64()
        });
        if let Err(error) = started {
            return rejected_promise(error);
        }

        let callback_ptr = closure_ptr_from_value(callback.get_nanbox_f64())
            .expect("the rooted transaction callback remains callable");
        let callback_result =
            match js_call_catching(|| js_closure_call1(callback_ptr, client.get_nanbox_f64())) {
                Ok(value) => value,
                Err(error) => {
                    let error = scope.root_nanbox_f64(error);
                    let _ = js_call_catching(|| {
                        finish_transaction(db_handle, nested, false);
                        undefined_f64()
                    });
                    return rejected_promise(error.get_nanbox_f64());
                }
            };
        let callback_scope = RuntimeHandleScope::new();
        let callback_result = callback_scope.root_nanbox_f64(callback_result);
        let callback_promise = match js_promise_resolved_catching(callback_result.get_nanbox_f64())
        {
            Ok(promise) => promise,
            Err(error) => {
                let error = scope.root_nanbox_f64(error);
                let _ = js_call_catching(|| {
                    finish_transaction(db_handle, nested, false);
                    undefined_f64()
                });
                return rejected_promise(error.get_nanbox_f64());
            }
        };

        let callback_promise = scope.root_raw_mut_ptr(callback_promise);
        let fulfilled = scope.root_raw_mut_ptr(js_closure_alloc(
            bun_sql_transaction_fulfilled as *const u8,
            2,
        ));
        js_closure_set_capture_f64(fulfilled.get_raw_mut_ptr(), 0, db_handle as f64);
        js_closure_set_capture_f64(
            fulfilled.get_raw_mut_ptr(),
            1,
            if nested { 1.0 } else { 0.0 },
        );
        let rejected = scope.root_raw_mut_ptr(js_closure_alloc(
            bun_sql_transaction_rejected as *const u8,
            2,
        ));
        js_closure_set_capture_f64(rejected.get_raw_mut_ptr(), 0, db_handle as f64);
        js_closure_set_capture_f64(
            rejected.get_raw_mut_ptr(),
            1,
            if nested { 1.0 } else { 0.0 },
        );

        promise_value(js_promise_then(
            callback_promise.get_raw_mut_ptr(),
            fulfilled.get_raw_const_ptr(),
            rejected.get_raw_const_ptr(),
        ))
    }
}

extern "C" fn bun_sql_reserve(closure: *const ClosureHeader, _options: f64) -> f64 {
    unsafe { resolved_promise(captured_client(closure)) }
}

extern "C" fn bun_sql_release(_closure: *const ClosureHeader) -> f64 {
    undefined_f64()
}

unsafe fn call_onclose(callback_value: f64, client: f64, error: f64) -> Result<(), f64> {
    let scope = RuntimeHandleScope::new();
    let callback_value = scope.root_nanbox_f64(callback_value);
    let client = scope.root_nanbox_f64(client);
    let error = scope.root_nanbox_f64(error);
    let Some(callback) = closure_ptr_from_value(callback_value.get_nanbox_f64()) else {
        return Ok(());
    };
    js_call_catching(|| js_closure_call2(callback, client.get_nanbox_f64(), error.get_nanbox_f64()))
        .map(|_| ())
}

extern "C" fn bun_sql_close(closure: *const ClosureHeader, _options: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let db_handle = captured_handle(closure);
        let client = scope.root_nanbox_f64(captured_client(closure));
        let onclose =
            scope.root_nanbox_f64(js_closure_get_capture_f64(closure, METHOD_ONCLOSE_CAPTURE));
        match js_call_catching(|| {
            js_node_sqlite_database_sync_close(db_handle);
            undefined_f64()
        }) {
            Ok(_) => match call_onclose(
                onclose.get_nanbox_f64(),
                client.get_nanbox_f64(),
                undefined_f64(),
            ) {
                Ok(()) => resolved_promise(undefined_f64()),
                Err(error) => rejected_promise(error),
            },
            Err(error) => {
                let error = scope.root_nanbox_f64(error);
                let _ = call_onclose(
                    onclose.get_nanbox_f64(),
                    client.get_nanbox_f64(),
                    error.get_nanbox_f64(),
                );
                rejected_promise(error.get_nanbox_f64())
            }
        }
    }
}

static BUN_SQL_CLOSURES_REGISTERED: Once = Once::new();

fn register_closure_shapes() {
    BUN_SQL_CLOSURES_REGISTERED.call_once(|| {
        js_register_closure_rest(bun_sql_query_tag as *const u8, 1);
        js_register_closure_arity(bun_sql_begin as *const u8, 1);
        js_register_closure_arity(bun_sql_reserve as *const u8, 1);
        js_register_closure_arity(bun_sql_release as *const u8, 0);
        js_register_closure_arity(bun_sql_close as *const u8, 1);
        js_register_closure_arity(bun_sql_transaction_fulfilled as *const u8, 1);
        js_register_closure_arity(bun_sql_transaction_rejected as *const u8, 1);
    });
}

unsafe fn make_client(handle: Handle, onclose: f64) -> f64 {
    register_closure_shapes();
    let scope = RuntimeHandleScope::new();
    let onclose = scope.root_nanbox_f64(onclose);
    let client = scope.root_raw_mut_ptr(js_closure_alloc(bun_sql_query_tag as *const u8, 1));
    js_closure_set_capture_f64(
        client.get_raw_mut_ptr(),
        CLIENT_HANDLE_CAPTURE,
        handle as f64,
    );

    for (name, function) in [
        ("begin", bun_sql_begin as *const u8),
        ("reserve", bun_sql_reserve as *const u8),
        ("release", bun_sql_release as *const u8),
        ("close", bun_sql_close as *const u8),
    ] {
        // Allocate first, then re-read every rooted pointer/value. The method
        // allocation itself can move the client and the onclose callback.
        let method = scope.root_raw_mut_ptr(js_closure_alloc(function, 3));
        js_closure_set_capture_f64(
            method.get_raw_mut_ptr(),
            METHOD_HANDLE_CAPTURE,
            handle as f64,
        );
        js_closure_set_capture_f64(
            method.get_raw_mut_ptr(),
            METHOD_CLIENT_CAPTURE,
            boxed_closure(client.get_raw_const_ptr()),
        );
        js_closure_set_capture_f64(
            method.get_raw_mut_ptr(),
            METHOD_ONCLOSE_CAPTURE,
            onclose.get_nanbox_f64(),
        );
        closure_set_dynamic_prop(
            client.get_raw_const_ptr::<ClosureHeader>() as usize,
            name,
            boxed_closure(method.get_raw_const_ptr()),
        );
    }

    boxed_closure(client.get_raw_const_ptr())
}

fn sqlite_config_from_url(input: &str) -> (String, Option<BunSqliteOpenMode>) {
    for prefix in ["sqlite://", "sqlite:", "file://", "file:"] {
        if let Some(url) = input.strip_prefix(prefix) {
            let (path, query) = url
                .split_once('?')
                .map_or((url, None), |(path, query)| (path, Some(query)));
            let mode = query
                .and_then(|query| {
                    query.split('&').find_map(|pair| {
                        pair.split_once('=')
                            .filter(|(name, _)| *name == "mode")
                            .map(|(_, value)| value)
                    })
                })
                .map(|mode| match mode {
                    "ro" => BunSqliteOpenMode::ReadOnly,
                    "rw" => BunSqliteOpenMode::ReadWrite,
                    "rwc" => BunSqliteOpenMode::ReadWriteCreate,
                    _ => throw_plain_type("SQLite URL mode must be one of: ro, rw, rwc"),
                });
            return (
                if path.is_empty() {
                    ":memory:".to_string()
                } else {
                    path.to_string()
                },
                mode,
            );
        }
    }
    // A plain filename selected via `{ adapter: "sqlite" }` is not a URL;
    // preserve literal `?` characters rather than interpreting them as a
    // query string.
    (input.to_string(), None)
}

fn normalize_sqlite_config(input: &str) -> (String, Option<BunSqliteOpenMode>) {
    if input == ":memory:" {
        (input.to_string(), None)
    } else {
        sqlite_config_from_url(input)
    }
}

/// `new SQL(config?, options?)` / `SQL(config?, options?)` from `"bun"`.
#[no_mangle]
pub unsafe extern "C" fn js_bun_sql_new(config_value: f64, options_value: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let config = scope.root_nanbox_f64(config_value);
    let explicit_options = scope.root_nanbox_f64(options_value);
    let config_js = JSValue::from_bits(config.get_nanbox_f64().to_bits());

    let (path, url_mode, effective_options) = if config_js.is_any_string() {
        let input = string_from_value(config.get_nanbox_f64(), "config");
        let adapter =
            if !JSValue::from_bits(explicit_options.get_nanbox_f64().to_bits()).is_undefined() {
                string_option(explicit_options.get_nanbox_f64(), "adapter", None)
            } else {
                None
            };
        let is_sqlite = input == ":memory:"
            || input.starts_with("sqlite:")
            || input.starts_with("file:")
            || adapter.as_deref() == Some("sqlite");
        if !is_sqlite {
            throw_plain_type("Bun.SQL currently requires a SQLite connection string");
        }
        let (path, url_mode) = normalize_sqlite_config(&input);
        (path, url_mode, explicit_options.get_nanbox_f64())
    } else if is_object_like(config.get_nanbox_f64()) {
        let adapter = string_option(config.get_nanbox_f64(), "adapter", None);
        if adapter.as_deref() != Some("sqlite") {
            throw_plain_type("Bun.SQL currently requires adapter: \"sqlite\"");
        }
        let path = string_option(config.get_nanbox_f64(), "filename", Some(":memory:"))
            .unwrap_or_else(|| ":memory:".to_string());
        let (path, url_mode) = normalize_sqlite_config(&path);
        (path, url_mode, config.get_nanbox_f64())
    } else {
        throw_plain_type("Bun.SQL requires a connection string or options object");
    };

    let effective_options = scope.root_nanbox_f64(effective_options);
    let onclose = function_option(effective_options.get_nanbox_f64(), "onclose")
        .unwrap_or_else(undefined_f64);
    let onclose = scope.root_nanbox_f64(onclose);
    let onconnect = function_option(effective_options.get_nanbox_f64(), "onconnect");
    let onconnect = onconnect.map(|value| scope.root_nanbox_f64(value));
    let handle = open_bun_sqlite_database(path, effective_options.get_nanbox_f64(), url_mode);
    let client = scope.root_nanbox_f64(make_client(handle, onclose.get_nanbox_f64()));

    if let Some(onconnect) = onconnect {
        if let Some(callback) = closure_ptr_from_value(onconnect.get_nanbox_f64()) {
            js_closure_call1(callback, client.get_nanbox_f64());
        }
    }
    client.get_nanbox_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_urls_are_normalized_without_touching_sql_text() {
        assert_eq!(normalize_sqlite_config(":memory:").0, ":memory:");
        assert_eq!(normalize_sqlite_config("sqlite://:memory:").0, ":memory:");
        assert_eq!(
            normalize_sqlite_config("sqlite:///tmp/app.db").0,
            "/tmp/app.db"
        );
        assert_eq!(
            normalize_sqlite_config("file:relative.db?mode=rwc").0,
            "relative.db"
        );
        assert_eq!(
            normalize_sqlite_config("plain?mode=ro.db").0,
            "plain?mode=ro.db"
        );
        assert_eq!(
            normalize_sqlite_config("sqlite:///tmp/app.db?cache=shared&mode=ro").1,
            Some(BunSqliteOpenMode::ReadOnly)
        );
        assert_eq!(
            normalize_sqlite_config("file:relative.db?mode=rw").1,
            Some(BunSqliteOpenMode::ReadWrite)
        );
        assert_eq!(
            normalize_sqlite_config("sqlite:created.db?mode=rwc").1,
            Some(BunSqliteOpenMode::ReadWriteCreate)
        );
    }
}
