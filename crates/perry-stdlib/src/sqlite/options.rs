use super::*;
use perry_runtime::{
    closure::{is_closure_ptr, ClosureHeader},
    js_get_string_pointer_unified, js_nanbox_pointer, js_object_get_field_by_name,
    js_string_from_bytes, JSValue, ObjectHeader, StringHeader,
};
use rusqlite::{ffi, limits::Limit, Connection};
use std::ffi::{CStr, CString};

/// Helper to extract string from StringHeader pointer
pub(crate) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

pub(crate) fn undefined_f64() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

pub(crate) fn null_f64() -> f64 {
    f64::from_bits(TAG_NULL_BITS)
}

pub(crate) fn bool_f64(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

pub(crate) fn value_from_f64(value: f64) -> JSValue {
    JSValue::from_bits(value.to_bits())
}

pub(crate) fn throw_type(message: &str) -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

pub(crate) fn throw_plain_type(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::exception::js_throw(js_nanbox_pointer(err as i64))
}

pub(crate) fn throw_plain_range(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_rangeerror_new(msg);
    perry_runtime::exception::js_throw(js_nanbox_pointer(err as i64))
}

pub(crate) fn throw_construct_required() -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(
        "Class constructor DatabaseSync cannot be invoked without 'new'",
        "ERR_CONSTRUCT_CALL_REQUIRED",
    )
}

pub(crate) fn throw_range(message: &str) -> ! {
    perry_runtime::fs::validate::throw_range_error_with_code(message)
}

pub(crate) fn throw_invalid_state(message: &str) -> ! {
    perry_runtime::fs::validate::throw_error_with_code(message, "ERR_INVALID_STATE")
}

pub(crate) fn throw_sqlite_error(message: &str) -> ! {
    perry_runtime::fs::validate::throw_error_with_code(message, "ERR_SQLITE_ERROR")
}

pub(crate) fn throw_arg_value(message: &str) -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_VALUE")
}

pub(crate) fn throw_illegal_constructor() -> ! {
    perry_runtime::fs::validate::throw_error_with_code(
        "Illegal constructor",
        "ERR_ILLEGAL_CONSTRUCTOR",
    )
}

pub(crate) fn throw_load_sqlite_extension(message: &str) -> ! {
    perry_runtime::fs::validate::throw_error_with_code(message, "ERR_LOAD_SQLITE_EXTENSION")
}

/// Run a multi-statement SQL batch. On failure returns the error message
/// plus the extended result code so callers can raise Node-shaped
/// `ERR_SQLITE_ERROR`s carrying `errcode`/`errstr` (#6561).
pub(crate) unsafe fn node_sqlite_exec_batch(
    conn: &Connection,
    sql: &str,
) -> Result<(), (String, i32)> {
    let c_sql = CString::new(sql).map_err(|_| {
        (
            "SQL string must not contain null bytes".to_string(),
            ffi::SQLITE_MISUSE,
        )
    })?;
    let mut error_message = std::ptr::null_mut();
    let rc = ffi::sqlite3_exec(
        conn.handle(),
        c_sql.as_ptr(),
        None,
        std::ptr::null_mut(),
        &mut error_message,
    );
    if rc == ffi::SQLITE_OK {
        return Ok(());
    }

    let errcode = ffi::sqlite3_extended_errcode(conn.handle());
    let message = if error_message.is_null() {
        CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
            .to_string_lossy()
            .into_owned()
    } else {
        let message = CStr::from_ptr(error_message).to_string_lossy().into_owned();
        ffi::sqlite3_free(error_message.cast());
        message
    };
    Err((message, errcode))
}

pub(crate) unsafe fn string_from_value(value: f64, name: &str) -> String {
    let js = value_from_f64(value);
    if !js.is_any_string() {
        throw_type(&format!("The \"{}\" argument must be of type string", name));
    }
    let ptr = js_get_string_pointer_unified(value) as *const StringHeader;
    let s = string_from_header(ptr).unwrap_or_else(|| {
        throw_type(&format!("The \"{}\" argument must be of type string", name))
    });
    if s.as_bytes().contains(&0) {
        throw_type(&format!(
            "The \"{}\" argument must not contain null bytes",
            name
        ));
    }
    s
}

pub(crate) fn is_object_like(value: f64) -> bool {
    value_from_f64(value).is_pointer()
}

pub(crate) unsafe fn object_field(object_value: f64, name: &str) -> JSValue {
    if !is_object_like(object_value) {
        return JSValue::undefined();
    }
    let obj_ptr = value_from_f64(object_value).as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() || (obj_ptr as usize) < 0x1000 {
        return JSValue::undefined();
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_get_field_by_name(obj_ptr, key)
}

pub(crate) fn raw_addr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    if (0x7FF8..=0x7FFF).contains(&top16) {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && bits >= 0x1000 {
        bits as usize
    } else {
        0
    }
}

pub(crate) fn closure_ptr_from_value(value: f64) -> Option<*const ClosureHeader> {
    let ptr = raw_addr_from_value(value);
    if ptr >= 0x10000 && is_closure_ptr(ptr) {
        Some(ptr as *const ClosureHeader)
    } else {
        None
    }
}

pub(crate) unsafe fn function_option(options_value: f64, name: &str) -> Option<f64> {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return None;
    }
    let value_f64 = f64::from_bits(value.bits());
    if closure_ptr_from_value(value_f64).is_none() {
        throw_type(&format!(
            "The \"options.{}\" argument must be a function.",
            name
        ));
    }
    Some(value_f64)
}

pub(crate) unsafe fn string_option(
    options_value: f64,
    name: &str,
    default: Option<&str>,
) -> Option<String> {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default.map(ToOwned::to_owned);
    }
    if !value.is_any_string() {
        throw_type(&format!(
            "The \"options.{}\" argument must be a string.",
            name
        ));
    }
    Some(string_from_value(
        f64::from_bits(value.bits()),
        &format!("options.{}", name),
    ))
}

pub(crate) unsafe fn validate_optional_object(options_value: f64) {
    let js = value_from_f64(options_value);
    if js.is_undefined() {
        return;
    }
    if js.is_null() || !is_object_like(options_value) {
        throw_type("The \"options\" argument must be an object.");
    }
}

pub(crate) unsafe fn bool_option(options_value: f64, name: &str, default: bool) -> bool {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    if !value.is_bool() {
        throw_type(&format!("The \"{}\" option must be of type boolean", name));
    }
    value.as_bool()
}

pub(crate) fn non_negative_i32_value(value: JSValue, name: &str, allow_infinity: bool) -> i32 {
    let number = if value.is_int32() {
        value.as_int32() as f64
    } else if value.is_number() {
        value.as_number()
    } else {
        throw_type(&format!("The \"{}\" option must be a number", name));
    };

    if allow_infinity && number == f64::INFINITY {
        return i32::MAX;
    }
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > i32::MAX as f64 {
        throw_range(&format!(
            "The value of \"{}\" is out of range. It must be a non-negative integer.",
            name
        ));
    }
    number as i32
}

pub(crate) unsafe fn non_negative_i32_option(options_value: f64, name: &str, default: i32) -> i32 {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    non_negative_i32_value(value, name, false)
}

pub(crate) fn node_sqlite_limit(name: &str) -> Option<(usize, Limit)> {
    match name {
        "length" => Some((0, Limit::SQLITE_LIMIT_LENGTH)),
        "sqlLength" => Some((1, Limit::SQLITE_LIMIT_SQL_LENGTH)),
        "column" => Some((2, Limit::SQLITE_LIMIT_COLUMN)),
        "exprDepth" => Some((3, Limit::SQLITE_LIMIT_EXPR_DEPTH)),
        "compoundSelect" => Some((4, Limit::SQLITE_LIMIT_COMPOUND_SELECT)),
        "vdbeOp" => Some((5, Limit::SQLITE_LIMIT_VDBE_OP)),
        "functionArg" => Some((6, Limit::SQLITE_LIMIT_FUNCTION_ARG)),
        "attach" => Some((7, Limit::SQLITE_LIMIT_ATTACHED)),
        "likePatternLength" => Some((8, Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH)),
        "variableNumber" => Some((9, Limit::SQLITE_LIMIT_VARIABLE_NUMBER)),
        "triggerDepth" => Some((10, Limit::SQLITE_LIMIT_TRIGGER_DEPTH)),
        _ => None,
    }
}

pub(crate) unsafe fn parse_node_sqlite_options(options_value: f64) -> NodeSqliteOptions {
    let mut options = NodeSqliteOptions::default();
    let js = value_from_f64(options_value);
    if js.is_undefined() {
        return options;
    }
    if js.is_null() || !is_object_like(options_value) {
        throw_type("The \"options\" argument must be an object.");
    }

    options.open = bool_option(options_value, "open", options.open);
    options.read_only = bool_option(options_value, "readOnly", options.read_only);
    options.enable_foreign_keys = bool_option(
        options_value,
        "enableForeignKeyConstraints",
        options.enable_foreign_keys,
    );
    options.enable_dqs = bool_option(
        options_value,
        "enableDoubleQuotedStringLiterals",
        options.enable_dqs,
    );
    options.timeout_ms = non_negative_i32_option(options_value, "timeout", options.timeout_ms);
    options.read_bigints = bool_option(options_value, "readBigInts", options.read_bigints);
    options.return_arrays = bool_option(options_value, "returnArrays", options.return_arrays);
    options.allow_bare_named_parameters = bool_option(
        options_value,
        "allowBareNamedParameters",
        options.allow_bare_named_parameters,
    );
    options.allow_unknown_named_parameters = bool_option(
        options_value,
        "allowUnknownNamedParameters",
        options.allow_unknown_named_parameters,
    );
    options.allow_extension = bool_option(options_value, "allowExtension", options.allow_extension);
    options.defensive = bool_option(options_value, "defensive", options.defensive);

    let limits = object_field(options_value, "limits");
    if !limits.is_undefined() {
        let limits_value = f64::from_bits(limits.bits());
        if limits.is_null() || !is_object_like(limits_value) {
            throw_type("The \"options.limits\" argument must be an object.");
        }
        for name in [
            "length",
            "sqlLength",
            "column",
            "exprDepth",
            "compoundSelect",
            "vdbeOp",
            "functionArg",
            "attach",
            "likePatternLength",
            "variableNumber",
            "triggerDepth",
        ] {
            if let Some((idx, _)) = node_sqlite_limit(name) {
                let value = object_field(limits_value, name);
                if !value.is_undefined() {
                    options.initial_limits[idx] = Some(non_negative_i32_value(value, name, false));
                }
            }
        }
    }

    options
}
