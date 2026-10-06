use super::*;
use perry_runtime::{
    buffer::{is_any_array_buffer, is_data_view, is_registered_buffer},
    js_array_alloc, js_array_get, js_array_length, js_array_push, js_get_string_pointer_unified,
    js_object_alloc_null_proto, js_object_get_field_by_name, js_object_set_field,
    js_object_set_keys, js_string_from_bytes, ArrayHeader, BigIntHeader, JSValue, ObjectHeader,
    StringHeader,
};
use rusqlite::{ffi, types::Value as SqliteValue, Connection};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};

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
            // fetch, no crypto) would otherwise fail to resolve `perry_hex::`
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
    sqlite_errmsg_raw(conn.handle())
}

pub(crate) unsafe fn sqlite_errmsg_raw(db: *mut ffi::sqlite3) -> String {
    CStr::from_ptr(ffi::sqlite3_errmsg(db))
        .to_string_lossy()
        .into_owned()
}

/// Throw `ERR_SQLITE_ERROR` for a raw connection's current error state.
pub(crate) unsafe fn throw_sqlite_error_from_db(db: *mut ffi::sqlite3) -> ! {
    let errcode = ffi::sqlite3_extended_errcode(db);
    let message = sqlite_errmsg_raw(db);
    throw_sqlite_error_ext(&message, errcode)
}

pub(crate) unsafe fn prepare_node_raw_statement(conn: &Connection, sql: &str) -> RawNodeStatement {
    let c_sql = CString::new(sql)
        .unwrap_or_else(|_| throw_sqlite_error("SQL string must not contain null bytes"));
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

/// `sqlite3_expanded_sql` as an owned string (empty when unavailable).
pub(crate) unsafe fn expanded_sql_of(raw_stmt: *mut ffi::sqlite3_stmt) -> String {
    let expanded = ffi::sqlite3_expanded_sql(raw_stmt);
    if expanded.is_null() {
        String::new()
    } else {
        let text = CStr::from_ptr(expanded).to_string_lossy().into_owned();
        ffi::sqlite3_free(expanded.cast::<c_void>());
        text
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

/// Why a parameter could not be bound, as plain data: the caller finalizes
/// its statement before throwing ([`BindError::throw`]).
pub(crate) enum BindError {
    Type(String),
    ArgValue(String),
    InvalidState(String),
    Sqlite { message: String, code: i32 },
}

impl BindError {
    pub(crate) unsafe fn throw(self) -> ! {
        match self {
            BindError::Type(message) => throw_type(&message),
            BindError::ArgValue(message) => throw_arg_value(&message),
            BindError::InvalidState(message) => throw_invalid_state(&message),
            BindError::Sqlite { message, code } => throw_sqlite_error_ext(&message, code),
        }
    }
}

unsafe fn bind_rc(db: *mut ffi::sqlite3, rc: c_int) -> Result<(), BindError> {
    if rc == ffi::SQLITE_OK {
        Ok(())
    } else {
        Err(BindError::Sqlite {
            message: sqlite_errmsg_raw(db),
            code: ffi::sqlite3_extended_errcode(db),
        })
    }
}

pub(crate) unsafe fn bind_node_sqlite_value(
    db: *mut ffi::sqlite3,
    raw_stmt: *mut ffi::sqlite3_stmt,
    index: c_int,
    value: f64,
) -> Result<(), BindError> {
    let js = value_from_f64(value);
    let rc = if js.is_null() {
        ffi::sqlite3_bind_null(raw_stmt, index)
    } else if js.is_undefined() || js.is_bool() {
        return Err(BindError::Type(format!(
            "Provided value cannot be bound to SQLite parameter {}.",
            index
        )));
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
            return Err(BindError::ArgValue(
                "BigInt value is too large to bind.".to_string(),
            ));
        };
        ffi::sqlite3_bind_int64(raw_stmt, index, value)
    } else if js.is_number() {
        ffi::sqlite3_bind_double(raw_stmt, index, js.as_number())
    } else {
        let raw = raw_addr_from_value(value);
        let status = perry_runtime::buffer::bytes::no_gc(|scope| {
            let value = f64::from_bits(JSValue::pointer(raw as *const u8).bits());
            let bytes = perry_runtime::buffer::bytes::bytes(value, scope).ok()?;
            Some(if bytes.is_empty() {
                ffi::sqlite3_bind_zeroblob(raw_stmt, index, 0)
            } else {
                ffi::sqlite3_bind_blob(
                    raw_stmt,
                    index,
                    bytes.as_ptr().cast(),
                    bytes.len() as c_int,
                    ffi::SQLITE_TRANSIENT(),
                )
            })
        });
        if let Some(status) = status {
            status
        } else {
            return Err(BindError::Type(format!(
                "Provided value cannot be bound to SQLite parameter {}.",
                index
            )));
        }
    };
    bind_rc(db, rc)
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
    raw >= 0x1000
        && !is_registered_buffer(raw)
        && perry_runtime::typedarray::lookup_typed_array_kind(raw).is_none()
        && unsafe { perry_runtime::symbol::js_is_symbol(value) == 0 }
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
    flags: StmtFlags,
    db: *mut ffi::sqlite3,
    raw_stmt: *mut ffi::sqlite3_stmt,
    args_arr: *const ArrayHeader,
) -> Result<(), BindError> {
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
        let allow_bare = flags.allow_bare_named_parameters;
        let allow_unknown = flags.allow_unknown_named_parameters;
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
                            return Err(BindError::InvalidState(format!(
                                "Cannot create bare named parameter '{}' because of conflicting names '{}' and '{}'.",
                                bare, fulls[0], fulls[1]
                            )));
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
                    return Err(BindError::InvalidState(format!(
                        "Unknown named parameter '{}'",
                        key
                    )));
                };
                let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let value = js_object_get_field_by_name(obj, key_ptr);
                bind_node_sqlite_value(db, raw_stmt, index, f64_from_jsvalue(value))?;
            }
        }
    }

    let positional_count = args.len().saturating_sub(positional_start);
    let positional_indices: Vec<c_int> = if named_params.is_some() {
        anonymous_indices
    } else if named_indices
        .keys()
        .any(|name| has_sqlite_parameter_prefix(name))
    {
        Vec::new()
    } else {
        (1..=param_count).collect()
    };
    if positional_count > positional_indices.len() {
        // Node raises ERR_SQLITE_ERROR with errcode 25 (SQLITE_RANGE) when
        // more anonymous values are supplied than the statement has
        // anonymous parameters (#6561).
        return Err(BindError::Sqlite {
            message: "column index out of range".to_string(),
            code: ffi::SQLITE_RANGE,
        });
    }
    for (offset, index) in positional_indices.into_iter().enumerate() {
        if let Some(value) = args.get(positional_start + offset).copied() {
            bind_node_sqlite_value(db, raw_stmt, index, value)?;
        }
    }
    Ok(())
}

pub(crate) unsafe fn bind_node_sqlite_positional_params(
    db: *mut ffi::sqlite3,
    raw_stmt: *mut ffi::sqlite3_stmt,
    values: &[f64],
) -> Result<(), BindError> {
    let param_count = ffi::sqlite3_bind_parameter_count(raw_stmt).max(0) as usize;
    for (offset, value) in values.iter().take(param_count).enumerate() {
        bind_node_sqlite_value(db, raw_stmt, (offset + 1) as c_int, *value)?;
    }
    Ok(())
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
            let ptr = ffi::sqlite3_column_blob(raw_stmt, index);
            let input = if len > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr as *const u8, len)
            } else {
                &[]
            };
            JSValue::from_bits(
                perry_runtime::buffer::bytes::from_slice(
                    perry_runtime::buffer::bytes::Brand::Buffer,
                    input,
                )
                .to_bits(),
            )
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

pub(crate) unsafe fn node_sqlite_blob_like_bytes(value: f64) -> Option<Vec<u8>> {
    let raw = raw_addr_from_value(value);
    if raw < 0x1000 {
        return None;
    }
    if is_registered_buffer(raw) && is_any_array_buffer(raw) && !is_data_view(raw) {
        return None;
    }
    perry_runtime::buffer::bytes::no_gc(|scope| {
        let value = f64::from_bits(JSValue::pointer(raw as *const u8).bits());
        perry_runtime::buffer::bytes::bytes(value, scope)
            .ok()
            .map(<[u8]>::to_vec)
    })
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
    flags: StmtFlags,
    raw_stmt: *mut ffi::sqlite3_stmt,
) -> JSValue {
    node_sqlite_row_value_with_mode(flags, raw_stmt, flags.return_arrays)
}

pub(crate) unsafe fn node_sqlite_row_value_with_mode(
    flags: StmtFlags,
    raw_stmt: *mut ffi::sqlite3_stmt,
    return_arrays: bool,
) -> JSValue {
    let column_count = ffi::sqlite3_column_count(raw_stmt);
    let read_bigints = flags.read_bigints;
    if return_arrays {
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

#[cfg(test)]
mod tests {
    use super::is_named_parameter_object;

    #[test]
    fn symbols_are_not_named_parameter_objects() {
        let symbol = unsafe { perry_runtime::symbol::js_symbol_new_empty() };
        assert!(!is_named_parameter_object(symbol));
    }

    #[test]
    fn typed_arrays_are_not_named_parameter_objects() {
        let typed_array = perry_runtime::typedarray::js_typed_array_new_empty(
            perry_runtime::typedarray::KIND_UINT8 as i32,
            1,
        );
        let value = perry_runtime::value::js_nanbox_pointer(typed_array as i64);
        assert!(!is_named_parameter_object(value));
    }
}
