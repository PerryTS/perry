use super::*;
use crate::common::{for_each_handle_mut_of, get_handle, register_handle, Handle};
use perry_runtime::{
    buffer::{
        buffer_alloc, buffer_data, buffer_data_mut, is_any_array_buffer, is_data_view,
        is_registered_buffer, mark_as_uint8array, BufferHeader,
    },
    closure::{is_closure_ptr, js_closure_call1, js_closure_call_array, ClosureHeader},
    js_array_alloc, js_array_get, js_array_is_array, js_array_length, js_array_push,
    js_array_push_f64, js_get_string_pointer_unified, js_nanbox_pointer, js_object_alloc,
    js_object_alloc_null_proto, js_object_alloc_with_shape, js_object_get_field_by_name,
    js_object_set_field, js_object_set_field_by_name, js_object_set_keys, js_promise_rejected,
    js_promise_resolved, js_string_from_bytes, ArrayHeader, BigIntHeader, JSValue, ObjectHeader,
    Promise, StringHeader,
};
use rusqlite::{ffi, limits::Limit, types::Value as SqliteValue, Connection, OpenFlags};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;

pub(crate) fn node_sqlite_tag_store_capacity(value: f64) -> usize {
    let js = value_from_f64(value);
    let number = if js.is_int32() {
        js.as_int32() as f64
    } else if js.is_number() {
        js.as_number()
    } else {
        return 1000;
    };

    if !number.is_finite() {
        return if number.is_sign_positive() {
            i32::MAX as usize
        } else {
            0
        };
    }
    let truncated = number.trunc();
    if truncated <= 0.0 {
        0
    } else if truncated >= i32::MAX as f64 {
        i32::MAX as usize
    } else {
        truncated as usize
    }
}

pub(crate) unsafe fn node_sqlite_tag_store_template_args(
    args_arr: *const ArrayHeader,
) -> (String, Vec<f64>) {
    let args = node_args_from_array(args_arr);
    let strings_value = args.first().copied().unwrap_or_else(undefined_f64);
    let is_array = value_from_f64(js_array_is_array(strings_value));
    if !is_array.is_bool() || !is_array.as_bool() {
        throw_type("First argument must be an array of strings (template literal).");
    }

    let strings_ptr = raw_addr_from_value(strings_value) as *const ArrayHeader;
    if strings_ptr.is_null() {
        throw_type("First argument must be an array of strings (template literal).");
    }

    let strings_len = js_array_length(strings_ptr);
    let mut sql = String::new();
    for index in 0..strings_len {
        let Some(part) = string_key_from_js_value(js_array_get(strings_ptr, index)) else {
            throw_type("Template literal parts must be strings.");
        };
        sql.push_str(&part);
        if index + 1 < strings_len {
            sql.push('?');
        }
    }

    (sql, args.into_iter().skip(1).collect())
}

pub(crate) unsafe fn prepare_node_sqlite_tag_store_statement(
    db_handle: Handle,
    sql: &str,
) -> Handle {
    let sql_ptr = js_string_from_bytes(sql.as_ptr(), sql.len() as u32);
    js_node_sqlite_database_sync_prepare(
        db_handle,
        f64_from_jsvalue(JSValue::string_ptr(sql_ptr)),
        undefined_f64(),
    )
}

pub(crate) unsafe fn node_sqlite_tag_store_statement(
    tag_store_handle: Handle,
    args_arr: *const ArrayHeader,
) -> (Handle, Vec<f64>, bool) {
    let store = get_handle::<NodeSqliteTagStoreHandle>(tag_store_handle)
        .unwrap_or_else(|| throw_invalid_state("SQLTagStore is not open"));
    ensure_open_node_database(store.db_handle);

    let (sql, values) = node_sqlite_tag_store_template_args(args_arr);
    if store.capacity == 0 {
        let stmt = prepare_node_sqlite_tag_store_statement(store.db_handle, &sql);
        return (stmt, values, true);
    }

    {
        let mut cache = store
            .cache
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("SQLTagStore is not open"));
        if let Some(stmt_handle) = cache.get(&sql) {
            let finalized = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
                .map(|stmt| stmt.finalized.load(Ordering::Relaxed))
                .unwrap_or(true);
            if !finalized {
                return (stmt_handle, values, false);
            }
            cache.remove(&sql);
        }
    }

    let stmt_handle = prepare_node_sqlite_tag_store_statement(store.db_handle, &sql);
    let evicted = {
        let mut cache = store
            .cache
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("SQLTagStore is not open"));
        cache.put(sql, stmt_handle, store.capacity)
    };
    for handle in evicted {
        if handle != stmt_handle {
            finalize_node_sqlite_statement_handle(handle);
        }
    }
    (stmt_handle, values, false)
}

pub(crate) unsafe fn with_node_sqlite_tag_store_statement<R, F>(
    tag_store_handle: Handle,
    args_arr: *const ArrayHeader,
    action: F,
) -> R
where
    F: FnOnce(&Connection, &NodeSqliteStmtHandle, *mut ffi::sqlite3_stmt) -> R,
{
    let (stmt_handle, values, temporary) =
        node_sqlite_tag_store_statement(tag_store_handle, args_arr);
    let result = with_node_sqlite_statement_positional(stmt_handle, &values, action);
    if temporary {
        finalize_node_sqlite_statement_handle(stmt_handle);
    }
    result
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_create_tag_store(
    db_handle: Handle,
    max_size_value: f64,
) -> Handle {
    ensure_open_node_database(db_handle);
    register_handle(NodeSqliteTagStoreHandle {
        db_handle,
        capacity: node_sqlite_tag_store_capacity(max_size_value),
        cache: Mutex::new(NodeSqliteTagStoreCache::new()),
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_sql_tag_store_run(
    tag_store_handle: Handle,
    args_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    with_node_sqlite_tag_store_statement(tag_store_handle, args_arr, |conn, stmt, raw_stmt| {
        loop {
            let rc = ffi::sqlite3_step(raw_stmt);
            match rc {
                ffi::SQLITE_ROW => continue,
                ffi::SQLITE_DONE => break,
                _ => throw_sqlite_error(&sqlite_error_message(conn)),
            }
        }
        let read_bigints = stmt.read_bigints.load(Ordering::Relaxed);
        let changes = ffi::sqlite3_changes64(conn.handle());
        let last_insert_rowid = ffi::sqlite3_last_insert_rowid(conn.handle());
        let keys = vec!["changes".to_string(), "lastInsertRowid".to_string()];
        let (packed_keys, shape_id) = build_packed_keys(&keys);
        let obj =
            js_object_alloc_with_shape(shape_id, 2, packed_keys.as_ptr(), packed_keys.len() as u32);
        let changes_value = if read_bigints {
            JSValue::bigint_ptr(perry_runtime::bigint::js_bigint_from_i64(changes))
        } else {
            node_sqlite_integer_value(changes, false)
        };
        let rowid_value = if read_bigints {
            JSValue::bigint_ptr(perry_runtime::bigint::js_bigint_from_i64(last_insert_rowid))
        } else {
            node_sqlite_integer_value(last_insert_rowid, false)
        };
        js_object_set_field(obj, 0, changes_value);
        js_object_set_field(obj, 1, rowid_value);
        obj
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_sql_tag_store_get(
    tag_store_handle: Handle,
    args_arr: *const ArrayHeader,
) -> f64 {
    with_node_sqlite_tag_store_statement(tag_store_handle, args_arr, |conn, stmt, raw_stmt| {
        match ffi::sqlite3_step(raw_stmt) {
            ffi::SQLITE_ROW => f64_from_jsvalue(node_sqlite_row_value(stmt, raw_stmt)),
            ffi::SQLITE_DONE => undefined_f64(),
            _ => throw_sqlite_error(&sqlite_error_message(conn)),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_sql_tag_store_all(
    tag_store_handle: Handle,
    args_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    with_node_sqlite_tag_store_statement(tag_store_handle, args_arr, |conn, stmt, raw_stmt| {
        let mut rows = js_array_alloc(0);
        loop {
            match ffi::sqlite3_step(raw_stmt) {
                ffi::SQLITE_ROW => {
                    rows = js_array_push(rows, node_sqlite_row_value(stmt, raw_stmt));
                }
                ffi::SQLITE_DONE => break,
                _ => throw_sqlite_error(&sqlite_error_message(conn)),
            }
        }
        rows
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_sql_tag_store_iterate(
    tag_store_handle: Handle,
    args_arr: *const ArrayHeader,
) -> f64 {
    let rows = js_node_sqlite_sql_tag_store_all(tag_store_handle, args_arr);
    perry_runtime::array::array_values_iter(f64_from_jsvalue(JSValue::array_ptr(rows)))
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_sql_tag_store_clear(tag_store_handle: Handle) -> i32 {
    let store = get_handle::<NodeSqliteTagStoreHandle>(tag_store_handle)
        .unwrap_or_else(|| throw_invalid_state("SQLTagStore is not open"));
    let handles = store
        .cache
        .lock()
        .map(|mut cache| cache.clear())
        .unwrap_or_default();
    for handle in handles {
        finalize_node_sqlite_statement_handle(handle);
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_sql_tag_store_size(tag_store_handle: Handle) -> f64 {
    let size = get_handle::<NodeSqliteTagStoreHandle>(tag_store_handle)
        .and_then(|store| store.cache.lock().ok().map(|cache| cache.len()))
        .unwrap_or(0);
    f64_from_jsvalue(JSValue::number(size as f64))
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_sql_tag_store_capacity(tag_store_handle: Handle) -> f64 {
    let capacity = get_handle::<NodeSqliteTagStoreHandle>(tag_store_handle)
        .map(|store| store.capacity)
        .unwrap_or(0);
    f64_from_jsvalue(JSValue::number(capacity as f64))
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_sql_tag_store_db(tag_store_handle: Handle) -> Handle {
    get_handle::<NodeSqliteTagStoreHandle>(tag_store_handle)
        .map(|store| store.db_handle)
        .unwrap_or(-1)
}
