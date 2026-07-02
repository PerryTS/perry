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

pub unsafe fn dispatch_node_sqlite_database_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if js_node_sqlite_is_database_sync_handle(handle) == 0 {
        return None;
    }
    let arg0 = args.first().copied().unwrap_or_else(undefined_f64);
    let arg1 = args.get(1).copied().unwrap_or_else(undefined_f64);
    let arg2 = args.get(2).copied().unwrap_or_else(undefined_f64);
    match method {
        "open" => {
            js_node_sqlite_database_sync_open(handle);
            Some(undefined_f64())
        }
        "close" => {
            js_node_sqlite_database_sync_close(handle);
            Some(undefined_f64())
        }
        "__perry_dispose__" | "@@__perry_wk_dispose" => {
            js_node_sqlite_database_sync_dispose(handle);
            Some(undefined_f64())
        }
        "exec" => {
            js_node_sqlite_database_sync_exec(handle, arg0);
            Some(undefined_f64())
        }
        "prepare" => {
            let stmt = js_node_sqlite_database_sync_prepare(handle, arg0, arg1);
            Some(js_nanbox_pointer(stmt))
        }
        "function" => {
            js_node_sqlite_database_sync_function(handle, arg0, arg1, arg2);
            Some(undefined_f64())
        }
        "aggregate" => {
            js_node_sqlite_database_sync_aggregate(handle, arg0, arg1);
            Some(undefined_f64())
        }
        "enableDefensive" => {
            js_node_sqlite_database_sync_enable_defensive(handle, arg0);
            Some(undefined_f64())
        }
        "setAuthorizer" => {
            js_node_sqlite_database_sync_set_authorizer(handle, arg0);
            Some(undefined_f64())
        }
        "createTagStore" => {
            let store = js_node_sqlite_database_sync_create_tag_store(handle, arg0);
            Some(js_nanbox_pointer(store))
        }
        "createSession" => {
            let session = js_node_sqlite_database_sync_create_session(handle, arg0);
            Some(js_nanbox_pointer(session))
        }
        "applyChangeset" => Some(js_node_sqlite_database_sync_apply_changeset(
            handle, arg0, arg1,
        )),
        "enableLoadExtension" => {
            js_node_sqlite_database_sync_enable_load_extension(handle, arg0);
            Some(undefined_f64())
        }
        "loadExtension" => {
            js_node_sqlite_database_sync_load_extension(handle, arg0);
            Some(undefined_f64())
        }
        "location" => Some(js_node_sqlite_database_sync_location(handle, arg0)),
        _ => None,
    }
}

pub unsafe fn dispatch_node_sqlite_database_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    if js_node_sqlite_is_database_sync_handle(handle) == 0 {
        return None;
    }
    match property_name {
        "isOpen" => Some(js_node_sqlite_database_sync_is_open(handle)),
        "isTransaction" => Some(js_node_sqlite_database_sync_is_transaction(handle)),
        "limits" => Some(js_nanbox_pointer(js_node_sqlite_database_sync_limits(
            handle,
        ))),
        "open"
        | "close"
        | "exec"
        | "prepare"
        | "function"
        | "aggregate"
        | "enableDefensive"
        | "setAuthorizer"
        | "createTagStore"
        | "createSession"
        | "applyChangeset"
        | "enableLoadExtension"
        | "loadExtension"
        | "location"
        | "__perry_dispose__"
        | "@@__perry_wk_dispose" => {
            extern "C" {
                fn js_class_method_bind(
                    instance: f64,
                    method_name_ptr: *const u8,
                    method_name_len: usize,
                ) -> f64;
            }
            let instance = js_nanbox_pointer(handle);
            Some(js_class_method_bind(
                instance,
                property_name.as_ptr(),
                property_name.len(),
            ))
        }
        _ => None,
    }
}

pub(crate) extern "C" fn sql_tag_store_constructor_thunk(_closure: *const ClosureHeader) -> f64 {
    throw_illegal_constructor()
}

pub(crate) unsafe fn sql_tag_store_constructor_value() -> f64 {
    let func_ptr = sql_tag_store_constructor_thunk as *const u8;
    perry_runtime::closure::js_register_closure_arity(func_ptr, 0);
    let closure = perry_runtime::closure::js_closure_alloc_singleton(func_ptr);
    if closure.is_null() {
        return undefined_f64();
    }
    let ptr = js_string_from_bytes(b"SQLTagStore".as_ptr(), "SQLTagStore".len() as u32);
    perry_runtime::closure::closure_set_dynamic_prop(
        closure as usize,
        "name",
        f64_from_jsvalue(JSValue::string_ptr(ptr)),
    );
    js_nanbox_pointer(closure as i64)
}

pub unsafe fn dispatch_node_sqlite_tag_store_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if js_node_sqlite_is_tag_store_handle(handle) == 0 {
        return None;
    }
    let args_arr = packed_args_array(args);
    match method {
        "run" => Some(js_nanbox_pointer(
            js_node_sqlite_sql_tag_store_run(handle, args_arr) as i64,
        )),
        "get" => Some(js_node_sqlite_sql_tag_store_get(handle, args_arr)),
        "all" => Some(js_nanbox_pointer(
            js_node_sqlite_sql_tag_store_all(handle, args_arr) as i64,
        )),
        "iterate" => Some(js_node_sqlite_sql_tag_store_iterate(handle, args_arr)),
        "clear" => {
            js_node_sqlite_sql_tag_store_clear(handle);
            Some(undefined_f64())
        }
        _ => None,
    }
}

pub unsafe fn dispatch_node_sqlite_tag_store_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    if js_node_sqlite_is_tag_store_handle(handle) == 0 {
        return None;
    }
    match property_name {
        "size" => Some(js_node_sqlite_sql_tag_store_size(handle)),
        "capacity" => Some(js_node_sqlite_sql_tag_store_capacity(handle)),
        "db" => Some(js_nanbox_pointer(js_node_sqlite_sql_tag_store_db(handle))),
        "constructor" => Some(sql_tag_store_constructor_value()),
        "run" | "get" | "all" | "iterate" | "clear" => {
            extern "C" {
                fn js_class_method_bind(
                    instance: f64,
                    method_name_ptr: *const u8,
                    method_name_len: usize,
                ) -> f64;
            }
            Some(js_class_method_bind(
                js_nanbox_pointer(handle),
                property_name.as_ptr(),
                property_name.len(),
            ))
        }
        _ => None,
    }
}

pub unsafe fn dispatch_node_sqlite_session_method(
    handle: Handle,
    method: &str,
    _args: &[f64],
) -> Option<f64> {
    if js_node_sqlite_is_session_handle(handle) == 0 {
        return None;
    }
    match method {
        "changeset" => Some(js_nanbox_pointer(
            js_node_sqlite_session_changeset(handle) as i64
        )),
        "patchset" => Some(js_nanbox_pointer(
            js_node_sqlite_session_patchset(handle) as i64
        )),
        "close" => {
            js_node_sqlite_session_close(handle);
            Some(undefined_f64())
        }
        "__perry_dispose__" | "@@__perry_wk_dispose" => {
            js_node_sqlite_session_dispose(handle);
            Some(undefined_f64())
        }
        _ => None,
    }
}

pub unsafe fn dispatch_node_sqlite_session_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    if js_node_sqlite_is_session_handle(handle) == 0 {
        return None;
    }
    match property_name {
        "changeset" | "patchset" | "close" | "__perry_dispose__" | "@@__perry_wk_dispose" => {
            extern "C" {
                fn js_class_method_bind(
                    instance: f64,
                    method_name_ptr: *const u8,
                    method_name_len: usize,
                ) -> f64;
            }
            let instance = js_nanbox_pointer(handle);
            Some(js_class_method_bind(
                instance,
                property_name.as_ptr(),
                property_name.len(),
            ))
        }
        _ => None,
    }
}

pub(crate) unsafe fn packed_args_array(args: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(args.len() as u32);
    for value in args {
        arr = js_array_push_f64(arr, *value);
    }
    arr
}

pub unsafe fn dispatch_node_sqlite_statement_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if js_node_sqlite_is_statement_sync_handle(handle) == 0 {
        return None;
    }
    let args_arr = packed_args_array(args);
    match method {
        "run" => Some(js_nanbox_pointer(
            js_node_sqlite_statement_sync_run(handle, args_arr) as i64,
        )),
        "get" => Some(js_node_sqlite_statement_sync_get(handle, args_arr)),
        "all" => Some(js_nanbox_pointer(
            js_node_sqlite_statement_sync_all(handle, args_arr) as i64,
        )),
        "iterate" => Some(js_node_sqlite_statement_sync_iterate(handle, args_arr)),
        "columns" => Some(js_nanbox_pointer(
            js_node_sqlite_statement_sync_columns(handle) as i64,
        )),
        "setReadBigInts" => {
            js_node_sqlite_statement_sync_set_read_bigints(
                handle,
                args.first().copied().unwrap_or_else(undefined_f64),
            );
            Some(undefined_f64())
        }
        "setReturnArrays" => {
            js_node_sqlite_statement_sync_set_return_arrays(
                handle,
                args.first().copied().unwrap_or_else(undefined_f64),
            );
            Some(undefined_f64())
        }
        "setAllowBareNamedParameters" => {
            js_node_sqlite_statement_sync_set_allow_bare_named_parameters(
                handle,
                args.first().copied().unwrap_or_else(undefined_f64),
            );
            Some(undefined_f64())
        }
        "setAllowUnknownNamedParameters" => {
            js_node_sqlite_statement_sync_set_allow_unknown_named_parameters(
                handle,
                args.first().copied().unwrap_or_else(undefined_f64),
            );
            Some(undefined_f64())
        }
        _ => None,
    }
}

pub unsafe fn dispatch_node_sqlite_statement_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    if js_node_sqlite_is_statement_sync_handle(handle) == 0 {
        return None;
    }
    match property_name {
        "sourceSQL" => Some(f64_from_jsvalue(JSValue::string_ptr(
            js_node_sqlite_statement_sync_source_sql(handle),
        ))),
        "expandedSQL" => Some(f64_from_jsvalue(JSValue::string_ptr(
            js_node_sqlite_statement_sync_expanded_sql(handle),
        ))),
        "run"
        | "get"
        | "all"
        | "iterate"
        | "columns"
        | "setReadBigInts"
        | "setReturnArrays"
        | "setAllowBareNamedParameters"
        | "setAllowUnknownNamedParameters" => {
            extern "C" {
                fn js_class_method_bind(
                    instance: f64,
                    method_name_ptr: *const u8,
                    method_name_len: usize,
                ) -> f64;
            }
            Some(js_class_method_bind(
                js_nanbox_pointer(handle),
                property_name.as_ptr(),
                property_name.len(),
            ))
        }
        _ => None,
    }
}

pub unsafe fn dispatch_node_sqlite_limits_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    let limits = get_handle::<NodeSqliteLimitsHandle>(handle)?;
    let (_, limit) = node_sqlite_limit(property_name)?;
    Some(with_open_node_connection(limits.db_handle, |conn| {
        // rusqlite 0.37: Connection::limit is now fallible. The category was
        // already validated by node_sqlite_limit above, so it won't error here.
        JSValue::int32(conn.limit(limit).unwrap_or(0))
    }))
    .map(|value| f64::from_bits(value.bits()))
}

pub unsafe fn dispatch_node_sqlite_limits_set(
    handle: Handle,
    property_name: &str,
    value: f64,
) -> bool {
    let Some(limits) = get_handle::<NodeSqliteLimitsHandle>(handle) else {
        return false;
    };
    let Some((_, limit)) = node_sqlite_limit(property_name) else {
        return false;
    };
    let new_value = non_negative_i32_value(value_from_f64(value), property_name, true);
    with_open_node_connection(limits.db_handle, |conn| {
        conn.set_limit(limit, new_value);
    });
    true
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_is_database_sync_handle(handle: Handle) -> i32 {
    if get_handle::<NodeSqliteDbHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_is_limits_handle(handle: Handle) -> i32 {
    if get_handle::<NodeSqliteLimitsHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_is_statement_sync_handle(handle: Handle) -> i32 {
    if get_handle::<NodeSqliteStmtHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_is_tag_store_handle(handle: Handle) -> i32 {
    if get_handle::<NodeSqliteTagStoreHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_is_session_handle(handle: Handle) -> i32 {
    if get_handle::<NodeSqliteSessionHandle>(handle).is_some() {
        1
    } else {
        0
    }
}
