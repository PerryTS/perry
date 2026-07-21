use super::*;
use crate::common::{get_handle, Handle};
use perry_runtime::{
    buffer::{
        buffer_alloc, buffer_data, buffer_data_mut, is_any_array_buffer, is_data_view,
        is_registered_buffer, mark_as_uint8array, BufferHeader,
    },
    closure::js_closure_call_array,
    js_array_alloc, js_array_get, js_array_length, js_array_push, js_get_string_pointer_unified,
    js_object_alloc_null_proto, js_object_get_field_by_name, js_object_set_field,
    js_object_set_keys, js_string_from_bytes, ArrayHeader, BigIntHeader, JSValue, ObjectHeader,
    StringHeader,
};
use rusqlite::{ffi, types::Value as SqliteValue, Connection};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::atomic::Ordering;

/// Convert SQLite value to JSValue
pub(crate) unsafe fn sqlite_value_to_jsvalue(value: &SqliteValue) -> JSValue {
    match value {
        SqliteValue::Null => JSValue::null(),
        SqliteValue::Integer(n) => {
            if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                JSValue::int32(*n as i32)
            } else {
                JSValue::number(*n as f64)
            }
        }
        SqliteValue::Real(n) => JSValue::number(*n),
        SqliteValue::Text(s) => {
            let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            JSValue::string_ptr(ptr)
        }
        SqliteValue::Blob(b) => {
            // Return blob as hex string. Hand-rolled to avoid pulling in
            // the `hex` crate, which lives behind the `crypto` Cargo
            // feature — auto-optimize builds that enable only
            // `database-sqlite` (e.g. mango: better-sqlite3 + mongodb +
            // fetch, no crypto) would otherwise fail to resolve `hex::`
            // and fall back to the prebuilt full stdlib.
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut out = Vec::with_capacity(b.len() * 2);
            for &byte in b {
                out.push(HEX[(byte >> 4) as usize]);
                out.push(HEX[(byte & 0x0f) as usize]);
            }
            let ptr = js_string_from_bytes(out.as_ptr(), out.len() as u32);
            JSValue::string_ptr(ptr)
        }
    }
}

pub(crate) struct RawNodeStatement {
    pub(crate) ptr: *mut ffi::sqlite3_stmt,
}

impl Drop for RawNodeStatement {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                ffi::sqlite3_finalize(self.ptr);
            }
        }
    }
}

pub(crate) fn f64_from_jsvalue(value: JSValue) -> f64 {
    f64::from_bits(value.bits())
}

pub(crate) fn string_value(value: &str) -> JSValue {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    JSValue::string_ptr(ptr)
}

pub(crate) unsafe fn sqlite_c_string_value(ptr: *const c_char) -> JSValue {
    if ptr.is_null() {
        return JSValue::null();
    }
    let value = CStr::from_ptr(ptr).to_string_lossy();
    string_value(&value)
}

pub(crate) unsafe fn sqlite_error_message(conn: &Connection) -> String {
    CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
        .to_string_lossy()
        .into_owned()
}

pub(crate) unsafe fn prepare_node_raw_statement(conn: &Connection, sql: &str) -> RawNodeStatement {
    let c_sql = CString::new(sql)
        .unwrap_or_else(|_| throw_type("The \"sql\" argument must not contain null bytes"));
    let mut raw = std::ptr::null_mut();
    let rc = ffi::sqlite3_prepare_v2(
        conn.handle(),
        c_sql.as_ptr(),
        -1,
        &mut raw,
        std::ptr::null_mut(),
    );
    if rc != ffi::SQLITE_OK {
        throw_sqlite_error_from_conn(conn);
    }
    RawNodeStatement { ptr: raw }
}

pub(crate) unsafe fn update_node_expanded_sql(
    stmt: &NodeSqliteStmtHandle,
    raw_stmt: *mut ffi::sqlite3_stmt,
) {
    let expanded = ffi::sqlite3_expanded_sql(raw_stmt);
    let text = if expanded.is_null() {
        String::new()
    } else {
        let text = CStr::from_ptr(expanded).to_string_lossy().into_owned();
        ffi::sqlite3_free(expanded.cast::<c_void>());
        text
    };
    if let Ok(mut cached) = stmt.expanded_sql.lock() {
        *cached = text;
    }
}

pub(crate) fn bigint_to_i64(ptr: *const BigIntHeader) -> Option<i64> {
    if ptr.is_null() {
        return None;
    }
    let limbs = unsafe { (*ptr).limbs };
    let lo = limbs[0];
    let fill = if (lo >> 63) == 0 { 0 } else { u64::MAX };
    if limbs[1..].iter().all(|limb| *limb == fill) {
        Some(lo as i64)
    } else {
        None
    }
}

pub(crate) unsafe fn node_sqlite_bind_error(conn: &Connection, rc: c_int) {
    if rc != ffi::SQLITE_OK {
        throw_sqlite_error_from_conn(conn);
    }
}

pub(crate) unsafe fn bind_node_sqlite_value(
    conn: &Connection,
    raw_stmt: *mut ffi::sqlite3_stmt,
    index: c_int,
    value: f64,
) {
    let js = value_from_f64(value);
    let rc = if js.is_null() {
        ffi::sqlite3_bind_null(raw_stmt, index)
    } else if js.is_undefined() || js.is_bool() {
        throw_type(&format!(
            "Provided value cannot be bound to SQLite parameter {}.",
            index
        ));
    } else if js.is_any_string() {
        let ptr = js_get_string_pointer_unified(value) as *const StringHeader;
        if ptr.is_null() {
            ffi::sqlite3_bind_null(raw_stmt, index)
        } else {
            let len = (*ptr).byte_len as c_int;
            let data_ptr =
                (ptr as *const u8).add(std::mem::size_of::<StringHeader>()) as *const c_char;
            ffi::sqlite3_bind_text(raw_stmt, index, data_ptr, len, ffi::SQLITE_TRANSIENT())
        }
    } else if js.is_int32() {
        // Node binds every JS number via sqlite3_bind_double — even
        // integral values (a column with no affinity stores them as REAL,
        // and `stmt.expandedSQL` renders `5.0`). Match that instead of
        // promoting integral numbers to SQLite INTEGERs (#6561); only
        // BigInt binds as INTEGER.
        ffi::sqlite3_bind_double(raw_stmt, index, js.as_int32() as f64)
    } else if js.is_bigint() {
        let Some(value) = bigint_to_i64(js.as_bigint_ptr()) else {
            throw_arg_value("BigInt value is too large to bind.");
        };
        ffi::sqlite3_bind_int64(raw_stmt, index, value)
    } else if js.is_number() {
        ffi::sqlite3_bind_double(raw_stmt, index, js.as_number())
    } else {
        let raw = raw_addr_from_value(value);
        if raw != 0 && is_registered_buffer(raw) {
            let buffer = raw as *const BufferHeader;
            let len = (*buffer).length as usize;
            let data_ptr = if len == 0 {
                std::ptr::null()
            } else {
                buffer_data(buffer) as *const c_void
            };
            ffi::sqlite3_bind_blob(
                raw_stmt,
                index,
                data_ptr,
                len as c_int,
                ffi::SQLITE_TRANSIENT(),
            )
        } else {
            throw_type(&format!(
                "Provided value cannot be bound to SQLite parameter {}.",
                index
            ));
        }
    };
    node_sqlite_bind_error(conn, rc);
}

pub(crate) unsafe fn node_args_from_array(args_arr: *const ArrayHeader) -> Vec<f64> {
    if args_arr.is_null() || ((args_arr as usize as u64) >> 48) != 0 {
        return Vec::new();
    }
    let len = js_array_length(args_arr);
    let mut args = Vec::with_capacity(len as usize);
    for i in 0..len {
        args.push(f64_from_jsvalue(js_array_get(args_arr, i)));
    }
    args
}

pub(crate) fn is_named_parameter_object(value: f64) -> bool {
    let js = value_from_f64(value);
    if !js.is_pointer() {
        return false;
    }
    let raw = raw_addr_from_value(value);
    raw >= 0x1000 && !is_registered_buffer(raw)
}

pub(crate) unsafe fn string_key_from_js_value(value: JSValue) -> Option<String> {
    if !value.is_any_string() {
        return None;
    }
    let ptr = js_get_string_pointer_unified(f64_from_jsvalue(value)) as *const StringHeader;
    string_from_header(ptr)
}

pub(crate) fn strip_sqlite_parameter_prefix(name: &str) -> &str {
    name.strip_prefix(':')
        .or_else(|| name.strip_prefix('@'))
        .or_else(|| name.strip_prefix('$'))
        .unwrap_or(name)
}

pub(crate) fn has_sqlite_parameter_prefix(name: &str) -> bool {
    name.starts_with(':') || name.starts_with('@') || name.starts_with('$')
}

pub(crate) unsafe fn bind_node_sqlite_params(
    stmt: &NodeSqliteStmtHandle,
    conn: &Connection,
    raw_stmt: *mut ffi::sqlite3_stmt,
    args_arr: *const ArrayHeader,
) {
    let args = node_args_from_array(args_arr);
    let mut positional_start = 0usize;
    let mut named_params: Option<f64> = None;
    if let Some(first) = args.first().copied() {
        if is_named_parameter_object(first) {
            named_params = Some(first);
            positional_start = 1;
        }
    }

    let param_count = ffi::sqlite3_bind_parameter_count(raw_stmt);
    let mut anonymous_indices = Vec::new();
    let mut named_indices = HashMap::<String, c_int>::new();
    let mut bare_names = HashMap::<String, Vec<String>>::new();
    for index in 1..=param_count {
        let name_ptr = ffi::sqlite3_bind_parameter_name(raw_stmt, index);
        if name_ptr.is_null() {
            anonymous_indices.push(index);
        } else {
            let name = CStr::from_ptr(name_ptr).to_string_lossy().into_owned();
            named_indices.entry(name.clone()).or_insert(index);
            bare_names
                .entry(strip_sqlite_parameter_prefix(&name).to_string())
                .or_default()
                .push(name);
        }
    }

    if let Some(named_value) = named_params {
        let allow_bare = stmt.allow_bare_named_parameters.load(Ordering::Relaxed);
        let allow_unknown = stmt.allow_unknown_named_parameters.load(Ordering::Relaxed);
        if !closure_ptr_from_value(named_value).is_some() {
            let keys = perry_runtime::object::js_object_keys_value(named_value);
            let key_count = js_array_length(keys);
            let obj = value_from_f64(named_value).as_pointer::<ObjectHeader>();
            for i in 0..key_count {
                let Some(key) = string_key_from_js_value(js_array_get(keys, i)) else {
                    continue;
                };
                let bare = strip_sqlite_parameter_prefix(&key).to_string();
                if allow_bare {
                    if let Some(fulls) = bare_names.get(&bare) {
                        if fulls.len() > 1 {
                            throw_invalid_state(&format!(
                                "Cannot create bare named parameter '{}' because of conflicting names '{}' and '{}'.",
                                bare, fulls[0], fulls[1]
                            ));
                        }
                    }
                }
                let index = if has_sqlite_parameter_prefix(&key) {
                    named_indices.get(&key).copied()
                } else if allow_bare {
                    bare_names
                        .get(&bare)
                        .and_then(|fulls| fulls.first())
                        .and_then(|full| named_indices.get(full).copied())
                } else {
                    None
                };
                let Some(index) = index else {
                    if allow_unknown {
                        continue;
                    }
                    throw_invalid_state(&format!("Unknown named parameter '{}'", key));
                };
                let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let value = js_object_get_field_by_name(obj, key_ptr);
                bind_node_sqlite_value(conn, raw_stmt, index, f64_from_jsvalue(value));
            }
        }
    }

    let positional_count = args.len().saturating_sub(positional_start);
    if positional_count > anonymous_indices.len() {
        // Node raises ERR_SQLITE_ERROR with errcode 25 (SQLITE_RANGE) when
        // more anonymous values are supplied than the statement has
        // anonymous parameters (#6561).
        throw_sqlite_error_ext("column index out of range", ffi::SQLITE_RANGE);
    }
    for (offset, index) in anonymous_indices.into_iter().enumerate() {
        if let Some(value) = args.get(positional_start + offset).copied() {
            bind_node_sqlite_value(conn, raw_stmt, index, value);
        }
    }
}

pub(crate) unsafe fn bind_node_sqlite_positional_params(
    conn: &Connection,
    raw_stmt: *mut ffi::sqlite3_stmt,
    values: &[f64],
) {
    let param_count = ffi::sqlite3_bind_parameter_count(raw_stmt).max(0) as usize;
    for (offset, value) in values.iter().take(param_count).enumerate() {
        bind_node_sqlite_value(conn, raw_stmt, (offset + 1) as c_int, *value);
    }
}

pub(crate) unsafe fn node_sqlite_integer_value(value: i64, read_bigints: bool) -> JSValue {
    if read_bigints {
        return JSValue::bigint_ptr(perry_runtime::bigint::js_bigint_from_i64(value));
    }
    if !(JS_SAFE_INTEGER_MIN..=JS_SAFE_INTEGER_MAX).contains(&value) {
        throw_range(&format!(
            "Value is too large to be represented as a JavaScript number: {}",
            value
        ));
    }
    // Always hand out a NUMBER-tagged double, never an INT32-tagged value
    // (#6561). Node's node:sqlite returns plain JS numbers, and perry's
    // INT32 tag shares its storage shape with `Expr::ClassRef`
    // (`INT32_TAG | class_id`, see #618): when a small integer like a
    // rowid `1` escapes into an any-typed context, `js_value_typeof`
    // cannot tell it apart from a registered class id and reports
    // "function" instead of "number".
    JSValue::number(value as f64)
}

pub(crate) unsafe fn node_sqlite_column_value(
    raw_stmt: *mut ffi::sqlite3_stmt,
    index: c_int,
    read_bigints: bool,
) -> JSValue {
    match ffi::sqlite3_column_type(raw_stmt, index) {
        ffi::SQLITE_NULL => JSValue::null(),
        ffi::SQLITE_INTEGER => {
            node_sqlite_integer_value(ffi::sqlite3_column_int64(raw_stmt, index), read_bigints)
        }
        ffi::SQLITE_FLOAT => JSValue::number(ffi::sqlite3_column_double(raw_stmt, index)),
        ffi::SQLITE_TEXT => {
            let ptr = ffi::sqlite3_column_text(raw_stmt, index);
            if ptr.is_null() {
                return JSValue::null();
            }
            let len = ffi::sqlite3_column_bytes(raw_stmt, index) as usize;
            let bytes = std::slice::from_raw_parts(ptr, len);
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            JSValue::string_ptr(str_ptr)
        }
        ffi::SQLITE_BLOB => {
            let len = ffi::sqlite3_column_bytes(raw_stmt, index) as usize;
            let buf = buffer_alloc(len as u32);
            (*buf).length = len as u32;
            if len > 0 {
                let ptr = ffi::sqlite3_column_blob(raw_stmt, index);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(ptr as *const u8, buffer_data_mut(buf), len);
                }
            }
            JSValue::object_ptr(buf as *mut u8)
        }
        _ => JSValue::null(),
    }
}

pub(crate) unsafe fn node_sqlite_bool_option_exact(
    options_value: f64,
    name: &str,
    default: bool,
) -> bool {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    if !value.is_bool() {
        throw_type(&format!(
            "The \"options.{}\" argument must be a boolean.",
            name
        ));
    }
    value.as_bool()
}

pub(crate) unsafe fn node_sqlite_function_arg(value: f64, name: &str) -> f64 {
    if closure_ptr_from_value(value).is_none() {
        throw_type(&format!("The \"{}\" argument must be a function.", name));
    }
    value
}

pub(crate) unsafe fn node_sqlite_optional_callback_option(
    options_value: f64,
    name: &str,
    strict: bool,
) -> Option<f64> {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return None;
    }
    let value_f64 = f64::from_bits(value.bits());
    if closure_ptr_from_value(value_f64).is_none() {
        if strict {
            throw_type(&format!(
                "The \"options.{}\" argument must be a function.",
                name
            ));
        }
        return None;
    }
    Some(value_f64)
}

pub(crate) unsafe fn node_sqlite_closure_arity(callback: f64) -> c_int {
    let Some(closure) = closure_ptr_from_value(callback) else {
        return 0;
    };
    perry_runtime::closure::closure_arity(closure).unwrap_or(0) as c_int
}

pub(crate) unsafe fn node_sqlite_call_closure(callback: f64, args: &[f64]) -> f64 {
    let Some(closure) = closure_ptr_from_value(callback) else {
        throw_plain_type("value is not a function");
    };
    js_closure_call_array(
        closure as i64,
        if args.is_empty() {
            std::ptr::null()
        } else {
            args.as_ptr()
        },
        args.len() as i64,
    )
}

pub(crate) unsafe fn node_sqlite_value_arg(
    value: *mut ffi::sqlite3_value,
    use_bigints: bool,
) -> JSValue {
    if value.is_null() {
        return JSValue::null();
    }
    match ffi::sqlite3_value_type(value) {
        ffi::SQLITE_NULL => JSValue::null(),
        ffi::SQLITE_INTEGER => {
            node_sqlite_integer_value(ffi::sqlite3_value_int64(value), use_bigints)
        }
        ffi::SQLITE_FLOAT => JSValue::number(ffi::sqlite3_value_double(value)),
        ffi::SQLITE_TEXT => {
            let ptr = ffi::sqlite3_value_text(value);
            if ptr.is_null() {
                return JSValue::null();
            }
            let len = ffi::sqlite3_value_bytes(value) as usize;
            let bytes = std::slice::from_raw_parts(ptr, len);
            JSValue::string_ptr(js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32))
        }
        ffi::SQLITE_BLOB => {
            let len = ffi::sqlite3_value_bytes(value) as usize;
            let buf = buffer_alloc(len as u32);
            (*buf).length = len as u32;
            mark_as_uint8array(buf as usize);
            if len > 0 {
                let ptr = ffi::sqlite3_value_blob(value);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(ptr as *const u8, buffer_data_mut(buf), len);
                }
            }
            JSValue::object_ptr(buf as *mut u8)
        }
        _ => JSValue::null(),
    }
}

pub(crate) unsafe fn node_sqlite_callback_args(
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
    use_bigints: bool,
) -> Vec<f64> {
    let argc = argc.max(0) as usize;
    let mut args = Vec::with_capacity(argc);
    for index in 0..argc {
        let value = if argv.is_null() {
            std::ptr::null_mut()
        } else {
            *argv.add(index)
        };
        args.push(f64_from_jsvalue(node_sqlite_value_arg(value, use_bigints)));
    }
    args
}

pub(crate) unsafe fn node_sqlite_blob_like_bytes(value: f64) -> Option<Vec<u8>> {
    let raw = raw_addr_from_value(value);
    if raw < 0x1000 {
        return None;
    }
    if perry_runtime::typedarray::lookup_typed_array_kind(raw).is_some() {
        let ta = raw as *const perry_runtime::typedarray::TypedArrayHeader;
        if let Some(bytes) = perry_runtime::typedarray::typed_array_bytes(ta) {
            return Some(bytes.to_vec());
        }
    }
    if is_registered_buffer(raw) {
        if is_any_array_buffer(raw) && !is_data_view(raw) {
            return None;
        }
        let buf = raw as *const BufferHeader;
        let len = (*buf).length as usize;
        let data = buffer_data(buf);
        return Some(std::slice::from_raw_parts(data, len).to_vec());
    }
    None
}

pub(crate) unsafe fn sqlite_result_error(ctx: *mut ffi::sqlite3_context, message: &str) {
    let c_message = CString::new(message).unwrap_or_else(|_| CString::new("SQLite error").unwrap());
    ffi::sqlite3_result_error(ctx, c_message.as_ptr(), -1);
}

pub(crate) unsafe fn node_sqlite_result_value(ctx: *mut ffi::sqlite3_context, value: f64) {
    let js = value_from_f64(value);
    if js.is_null() || js.is_undefined() {
        ffi::sqlite3_result_null(ctx);
    } else if js.is_int32() {
        ffi::sqlite3_result_double(ctx, js.as_int32() as f64);
    } else if js.is_number() {
        ffi::sqlite3_result_double(ctx, js.as_number());
    } else if js.is_any_string() {
        let ptr = js_get_string_pointer_unified(value) as *const StringHeader;
        if ptr.is_null() {
            ffi::sqlite3_result_null(ctx);
            return;
        }
        let len = (*ptr).byte_len as c_int;
        let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>()) as *const c_char;
        ffi::sqlite3_result_text(ctx, data_ptr, len, ffi::SQLITE_TRANSIENT());
    } else if js.is_bigint() {
        let Some(value) = bigint_to_i64(js.as_bigint_ptr()) else {
            sqlite_result_error(ctx, "BigInt value is too large for SQLite");
            return;
        };
        ffi::sqlite3_result_int64(ctx, value);
    } else if let Some(bytes) = node_sqlite_blob_like_bytes(value) {
        let data_ptr = if bytes.is_empty() {
            std::ptr::null()
        } else {
            bytes.as_ptr() as *const c_void
        };
        ffi::sqlite3_result_blob(ctx, data_ptr, bytes.len() as c_int, ffi::SQLITE_TRANSIENT());
    } else {
        sqlite_result_error(
            ctx,
            "Returned JavaScript value cannot be converted to a SQLite value",
        );
    }
}

pub(crate) unsafe extern "C" fn node_sqlite_scalar_callback(
    ctx: *mut ffi::sqlite3_context,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
) {
    let info = ffi::sqlite3_user_data(ctx) as *mut NodeSqliteCustomFunction;
    if info.is_null() {
        sqlite_result_error(ctx, "SQLite function is not available");
        return;
    }
    let args = node_sqlite_callback_args(argc, argv, (*info).use_bigint_arguments);
    let result = node_sqlite_call_closure((*info).callback, &args);
    node_sqlite_result_value(ctx, result);
}

pub(crate) unsafe extern "C" fn node_sqlite_scalar_destroy(data: *mut c_void) {
    let info = data as *mut NodeSqliteCustomFunction;
    unregister_node_sqlite_custom_function(info);
    if !info.is_null() {
        drop(Box::from_raw(info));
    }
}

pub(crate) unsafe fn node_sqlite_aggregate_start(aggregate: &NodeSqliteCustomAggregate) -> f64 {
    if closure_ptr_from_value(aggregate.start).is_some() {
        node_sqlite_call_closure(aggregate.start, &[])
    } else {
        aggregate.start
    }
}

pub(crate) unsafe fn node_sqlite_aggregate_state(
    ctx: *mut ffi::sqlite3_context,
    aggregate: &NodeSqliteCustomAggregate,
    create: bool,
) -> Option<*mut NodeSqliteAggregateState> {
    let slot = ffi::sqlite3_aggregate_context(
        ctx,
        if create {
            std::mem::size_of::<*mut NodeSqliteAggregateState>() as c_int
        } else {
            0
        },
    ) as *mut *mut NodeSqliteAggregateState;
    if slot.is_null() {
        if create {
            ffi::sqlite3_result_error_nomem(ctx);
        }
        return None;
    }
    if (*slot).is_null() && create {
        let initial = node_sqlite_aggregate_start(aggregate);
        perry_runtime::gc::js_write_barrier_root_nanbox(initial.to_bits());
        let state = Box::into_raw(Box::new(NodeSqliteAggregateState { state: initial }));
        register_node_sqlite_aggregate_state(state);
        *slot = state;
    }
    if (*slot).is_null() {
        None
    } else {
        Some(*slot)
    }
}

pub(crate) unsafe fn node_sqlite_aggregate_apply(
    ctx: *mut ffi::sqlite3_context,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
    callback: f64,
) {
    let aggregate = ffi::sqlite3_user_data(ctx) as *mut NodeSqliteCustomAggregate;
    if aggregate.is_null() {
        sqlite_result_error(ctx, "SQLite aggregate is not available");
        return;
    }
    let Some(state) = node_sqlite_aggregate_state(ctx, &*aggregate, true) else {
        return;
    };
    let mut args = Vec::with_capacity(argc.max(0) as usize + 1);
    args.push((*state).state);
    args.extend(node_sqlite_callback_args(
        argc,
        argv,
        (*aggregate).use_bigint_arguments,
    ));
    let next = node_sqlite_call_closure(callback, &args);
    perry_runtime::gc::js_write_barrier_root_nanbox(next.to_bits());
    (*state).state = next;
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_step(
    ctx: *mut ffi::sqlite3_context,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
) {
    let aggregate = ffi::sqlite3_user_data(ctx) as *mut NodeSqliteCustomAggregate;
    if aggregate.is_null() {
        sqlite_result_error(ctx, "SQLite aggregate is not available");
        return;
    }
    node_sqlite_aggregate_apply(ctx, argc, argv, (*aggregate).step);
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_inverse(
    ctx: *mut ffi::sqlite3_context,
    argc: c_int,
    argv: *mut *mut ffi::sqlite3_value,
) {
    let aggregate = ffi::sqlite3_user_data(ctx) as *mut NodeSqliteCustomAggregate;
    if aggregate.is_null() {
        sqlite_result_error(ctx, "SQLite aggregate is not available");
        return;
    }
    let Some(inverse) = (*aggregate).inverse else {
        sqlite_result_error(ctx, "SQLite aggregate inverse is not available");
        return;
    };
    node_sqlite_aggregate_apply(ctx, argc, argv, inverse);
}

pub(crate) unsafe fn node_sqlite_aggregate_emit(ctx: *mut ffi::sqlite3_context, finalize: bool) {
    let aggregate = ffi::sqlite3_user_data(ctx) as *mut NodeSqliteCustomAggregate;
    if aggregate.is_null() {
        sqlite_result_error(ctx, "SQLite aggregate is not available");
        return;
    }
    let Some(state) = node_sqlite_aggregate_state(ctx, &*aggregate, true) else {
        return;
    };
    let value = if let Some(result) = (*aggregate).result {
        node_sqlite_call_closure(result, &[(*state).state])
    } else {
        (*state).state
    };
    node_sqlite_result_value(ctx, value);
    if finalize {
        let slot = ffi::sqlite3_aggregate_context(ctx, 0) as *mut *mut NodeSqliteAggregateState;
        if !slot.is_null() && !(*slot).is_null() {
            let state_ptr = *slot;
            unregister_node_sqlite_aggregate_state(state_ptr);
            drop(Box::from_raw(state_ptr));
            *slot = std::ptr::null_mut();
        }
    }
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_final(ctx: *mut ffi::sqlite3_context) {
    node_sqlite_aggregate_emit(ctx, true);
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_value(ctx: *mut ffi::sqlite3_context) {
    node_sqlite_aggregate_emit(ctx, false);
}

pub(crate) unsafe extern "C" fn node_sqlite_aggregate_destroy(data: *mut c_void) {
    let aggregate = data as *mut NodeSqliteCustomAggregate;
    unregister_node_sqlite_custom_aggregate(aggregate);
    if !aggregate.is_null() {
        drop(Box::from_raw(aggregate));
    }
}

pub(crate) unsafe fn set_object_keys_from_names(obj: *mut ObjectHeader, names: &[String]) {
    let mut keys = js_array_alloc(names.len() as u32);
    for name in names {
        let ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        keys = js_array_push(keys, JSValue::string_ptr(ptr));
    }
    js_object_set_keys(obj, keys);
}

pub(crate) unsafe fn make_null_proto_object(
    names: &[String],
    values: &[JSValue],
) -> *mut ObjectHeader {
    let obj = js_object_alloc_null_proto(0, names.len() as u32);
    set_object_keys_from_names(obj, names);
    for (idx, value) in values.iter().enumerate() {
        js_object_set_field(obj, idx as u32, *value);
    }
    obj
}

pub(crate) unsafe fn node_sqlite_row_value(
    stmt: &NodeSqliteStmtHandle,
    raw_stmt: *mut ffi::sqlite3_stmt,
) -> JSValue {
    let column_count = ffi::sqlite3_column_count(raw_stmt);
    let read_bigints = stmt.read_bigints.load(Ordering::Relaxed);
    if stmt.return_arrays.load(Ordering::Relaxed) {
        let mut arr = js_array_alloc(column_count as u32);
        for index in 0..column_count {
            arr = js_array_push(arr, node_sqlite_column_value(raw_stmt, index, read_bigints));
        }
        return JSValue::array_ptr(arr);
    }

    let mut names = Vec::with_capacity(column_count as usize);
    let mut values = Vec::with_capacity(column_count as usize);
    for index in 0..column_count {
        let name_ptr = ffi::sqlite3_column_name(raw_stmt, index);
        let name = if name_ptr.is_null() {
            String::new()
        } else {
            CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
        };
        names.push(name);
        values.push(node_sqlite_column_value(raw_stmt, index, read_bigints));
    }
    JSValue::object_ptr(make_null_proto_object(&names, &values) as *mut u8)
}

pub(crate) unsafe fn with_node_sqlite_statement<R, F>(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
    action: F,
) -> R
where
    F: FnOnce(&Connection, &NodeSqliteStmtHandle, *mut ffi::sqlite3_stmt) -> R,
{
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    if stmt.finalized.load(Ordering::Relaxed) {
        throw_invalid_state("statement has been finalized");
    }
    let db = get_handle::<NodeSqliteDbHandle>(stmt.db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let conn_ptr = {
        let conn_guard = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("database is not open"));
        if let Some(conn) = conn_guard.as_ref() {
            conn as *const Connection
        } else {
            drop(conn_guard);
            throw_invalid_state("database is not open");
        }
    };
    let conn = &*conn_ptr;
    let raw = prepare_node_raw_statement(conn, &stmt.sql);
    let raw_ptr = raw.ptr;
    bind_node_sqlite_params(stmt, conn, raw_ptr, params_arr);
    update_node_expanded_sql(stmt, raw_ptr);
    let result = action(conn, stmt, raw_ptr);
    drop(raw);
    result
}

pub(crate) unsafe fn with_node_sqlite_statement_positional<R, F>(
    stmt_handle: Handle,
    values: &[f64],
    action: F,
) -> R
where
    F: FnOnce(&Connection, &NodeSqliteStmtHandle, *mut ffi::sqlite3_stmt) -> R,
{
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    if stmt.finalized.load(Ordering::Relaxed) {
        throw_invalid_state("statement has been finalized");
    }
    let db = get_handle::<NodeSqliteDbHandle>(stmt.db_handle)
        .unwrap_or_else(|| throw_invalid_state("database is not open"));
    let conn_ptr = {
        let conn_guard = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("database is not open"));
        if let Some(conn) = conn_guard.as_ref() {
            conn as *const Connection
        } else {
            drop(conn_guard);
            throw_invalid_state("database is not open");
        }
    };
    let conn = &*conn_ptr;
    let raw = prepare_node_raw_statement(conn, &stmt.sql);
    let raw_ptr = raw.ptr;
    bind_node_sqlite_positional_params(conn, raw_ptr, values);
    update_node_expanded_sql(stmt, raw_ptr);
    let result = action(conn, stmt, raw_ptr);
    drop(raw);
    result
}

/// Build packed keys (null-separated) and a shape_id from column names.
pub(crate) fn build_packed_keys(column_names: &[String]) -> (Vec<u8>, u32) {
    let mut packed = Vec::new();
    let mut shape_id: u32 = 0x5143_0000; // "SQ" prefix
    for (i, name) in column_names.iter().enumerate() {
        if i > 0 {
            packed.push(0u8);
        }
        packed.extend_from_slice(name.as_bytes());
        // Simple hash for shape_id
        for &b in name.as_bytes() {
            shape_id = shape_id.wrapping_mul(31).wrapping_add(b as u32);
        }
    }
    shape_id = shape_id.wrapping_add(column_names.len() as u32);
    (packed, shape_id)
}
