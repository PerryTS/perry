//! SQLite module (better-sqlite3 compatible)
//!
//! Native implementation of the 'better-sqlite3' npm package using rusqlite.
//! Provides synchronous SQLite database operations.

use crate::common::{get_handle, register_handle, Handle};
use perry_runtime::{
    buffer::{buffer_alloc, buffer_data, buffer_data_mut, is_registered_buffer, BufferHeader},
    closure::{is_closure_ptr, js_closure_call1, ClosureHeader},
    js_array_alloc, js_array_get, js_array_length, js_array_push, js_array_push_f64,
    js_get_string_pointer_unified, js_nanbox_pointer, js_object_alloc, js_object_alloc_null_proto,
    js_object_alloc_with_shape, js_object_get_field_by_name, js_object_set_field,
    js_object_set_field_by_name, js_object_set_keys, js_promise_rejected, js_promise_resolved,
    js_string_from_bytes, ArrayHeader, BigIntHeader, JSValue, ObjectHeader, Promise, StringHeader,
};
use rusqlite::{ffi, limits::Limit, types::Value as SqliteValue, Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

fn undefined_f64() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

fn null_f64() -> f64 {
    f64::from_bits(TAG_NULL_BITS)
}

fn bool_f64(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn value_from_f64(value: f64) -> JSValue {
    JSValue::from_bits(value.to_bits())
}

fn throw_type(message: &str) -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

fn throw_construct_required() -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(
        "Class constructor DatabaseSync cannot be invoked without 'new'",
        "ERR_CONSTRUCT_CALL_REQUIRED",
    )
}

fn throw_range(message: &str) -> ! {
    perry_runtime::fs::validate::throw_range_error_with_code(message)
}

fn throw_invalid_state(message: &str) -> ! {
    perry_runtime::fs::validate::throw_error_with_code(message, "ERR_INVALID_STATE")
}

fn throw_sqlite_error(message: &str) -> ! {
    perry_runtime::fs::validate::throw_error_with_code(message, "ERR_SQLITE_ERROR")
}

fn throw_arg_value(message: &str) -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_VALUE")
}

fn throw_illegal_constructor() -> ! {
    perry_runtime::fs::validate::throw_error_with_code(
        "Illegal constructor",
        "ERR_ILLEGAL_CONSTRUCTOR",
    )
}

fn throw_load_sqlite_extension(message: &str) -> ! {
    perry_runtime::fs::validate::throw_error_with_code(message, "ERR_LOAD_SQLITE_EXTENSION")
}

unsafe fn node_sqlite_exec_batch(conn: &Connection, sql: &str) -> Result<(), String> {
    let c_sql =
        CString::new(sql).map_err(|_| "SQL string must not contain null bytes".to_string())?;
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

    let message = if error_message.is_null() {
        CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
            .to_string_lossy()
            .into_owned()
    } else {
        let message = CStr::from_ptr(error_message).to_string_lossy().into_owned();
        ffi::sqlite3_free(error_message.cast());
        message
    };
    Err(message)
}

unsafe fn string_from_value(value: f64, name: &str) -> String {
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

fn is_object_like(value: f64) -> bool {
    value_from_f64(value).is_pointer()
}

unsafe fn object_field(object_value: f64, name: &str) -> JSValue {
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

fn raw_addr_from_value(value: f64) -> usize {
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

fn closure_ptr_from_value(value: f64) -> Option<*const ClosureHeader> {
    let ptr = raw_addr_from_value(value);
    if ptr >= 0x10000 && is_closure_ptr(ptr) {
        Some(ptr as *const ClosureHeader)
    } else {
        None
    }
}

unsafe fn function_option(options_value: f64, name: &str) -> Option<f64> {
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

unsafe fn string_option(options_value: f64, name: &str, default: Option<&str>) -> Option<String> {
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

unsafe fn bool_option(options_value: f64, name: &str, default: bool) -> bool {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    if !value.is_bool() {
        throw_type(&format!("The \"{}\" option must be of type boolean", name));
    }
    value.as_bool()
}

fn non_negative_i32_value(value: JSValue, name: &str, allow_infinity: bool) -> i32 {
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

unsafe fn non_negative_i32_option(options_value: f64, name: &str, default: i32) -> i32 {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    non_negative_i32_value(value, name, false)
}

fn node_sqlite_limit(name: &str) -> Option<(usize, Limit)> {
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

unsafe fn parse_node_sqlite_options(options_value: f64) -> NodeSqliteOptions {
    let mut options = NodeSqliteOptions::default();
    let js = value_from_f64(options_value);
    if js.is_undefined() {
        return options;
    }
    if js.is_null() || !is_object_like(options_value) {
        throw_type("The \"options\" argument must be an object");
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
            throw_type("The \"limits\" option must be an object");
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

struct NodeSqliteBackupOptions {
    source: String,
    target: String,
    rate: i32,
    progress: Option<*const ClosureHeader>,
}

impl Default for NodeSqliteBackupOptions {
    fn default() -> Self {
        Self {
            source: "main".to_string(),
            target: "main".to_string(),
            rate: 100,
            progress: None,
        }
    }
}

struct NodeSqliteBackupError {
    message: String,
    errcode: Option<i32>,
    errstr: Option<String>,
}

fn sqlite_errstr(code: i32) -> String {
    unsafe {
        CStr::from_ptr(ffi::sqlite3_errstr(code))
            .to_string_lossy()
            .into_owned()
    }
}

unsafe fn sqlite_error_from_db(db: *mut ffi::sqlite3) -> NodeSqliteBackupError {
    if db.is_null() {
        return NodeSqliteBackupError {
            message: "SQLite error".to_string(),
            errcode: None,
            errstr: None,
        };
    }
    let code = ffi::sqlite3_extended_errcode(db);
    let errstr = sqlite_errstr(code);
    let message = CStr::from_ptr(ffi::sqlite3_errmsg(db))
        .to_string_lossy()
        .into_owned();
    NodeSqliteBackupError {
        message: if message.is_empty() {
            errstr.clone()
        } else {
            message
        },
        errcode: Some(code),
        errstr: Some(errstr),
    }
}

fn sqlite_error_from_code(code: i32) -> NodeSqliteBackupError {
    let errstr = sqlite_errstr(code);
    NodeSqliteBackupError {
        message: errstr.clone(),
        errcode: Some(code),
        errstr: Some(errstr),
    }
}

fn sqlite_error_from_rusqlite(err: rusqlite::Error) -> NodeSqliteBackupError {
    match err {
        rusqlite::Error::SqliteFailure(error, message) => {
            let code = error.extended_code;
            let errstr = sqlite_errstr(code);
            NodeSqliteBackupError {
                message: message.unwrap_or_else(|| errstr.clone()),
                errcode: Some(code),
                errstr: Some(errstr),
            }
        }
        other => NodeSqliteBackupError {
            message: other.to_string(),
            errcode: None,
            errstr: None,
        },
    }
}

unsafe fn sqlite_error_value(error: NodeSqliteBackupError) -> f64 {
    let msg = js_string_from_bytes(error.message.as_ptr(), error.message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, "ERR_SQLITE_ERROR");
    let err = perry_runtime::error::js_error_new_with_message(msg);
    let err_obj = err as *mut ObjectHeader;

    if let Some(errcode) = error.errcode {
        let key = js_string_from_bytes(b"errcode".as_ptr(), "errcode".len() as u32);
        js_object_set_field_by_name(err_obj, key, f64::from_bits(JSValue::int32(errcode).bits()));
    }
    if let Some(errstr) = error.errstr {
        let key = js_string_from_bytes(b"errstr".as_ptr(), "errstr".len() as u32);
        let value = js_string_from_bytes(errstr.as_ptr(), errstr.len() as u32);
        js_object_set_field_by_name(
            err_obj,
            key,
            f64::from_bits(JSValue::string_ptr(value).bits()),
        );
    }

    js_nanbox_pointer(err as i64)
}

fn backup_path_type_error(name: &str, value: f64) -> ! {
    let received = perry_runtime::fs::validate::describe_received(value);
    throw_type(&format!(
        "The \"{}\" argument must be of type string or an instance of Buffer or URL. Received {}",
        name, received
    ));
}

unsafe fn string_from_jsvalue(value: JSValue) -> Option<String> {
    if !value.is_any_string() {
        return None;
    }
    let ptr = js_get_string_pointer_unified(f64::from_bits(value.bits())) as *const StringHeader;
    string_from_header(ptr)
}

fn percent_decode_pathname(pathname: &str) -> String {
    fn hex(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        }
    }

    let bytes = pathname.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                decoded.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

unsafe fn bytes_from_path_like(value: f64) -> Option<Vec<u8>> {
    let raw = raw_addr_from_value(value);
    if raw < 0x1000 {
        return None;
    }
    if is_registered_buffer(raw) {
        let buffer = raw as *const BufferHeader;
        let bytes = std::slice::from_raw_parts(buffer_data(buffer), (*buffer).length as usize);
        return Some(bytes.to_vec());
    }
    if perry_runtime::typedarray::lookup_typed_array_kind(raw)
        == Some(perry_runtime::typedarray::KIND_UINT8)
    {
        let bytes = perry_runtime::typedarray::typed_array_bytes(
            raw as *const perry_runtime::typedarray::TypedArrayHeader,
        )?;
        return Some(bytes.to_vec());
    }
    None
}

unsafe fn path_like_from_value(value: f64, name: &str) -> String {
    let js = value_from_f64(value);
    let path = if js.is_any_string() {
        string_from_value(value, name)
    } else if let Some(bytes) = bytes_from_path_like(value) {
        if bytes.contains(&0) {
            throw_type(&format!(
                "The \"{}\" argument must not contain null bytes",
                name
            ));
        }
        String::from_utf8_lossy(&bytes).into_owned()
    } else if js.is_pointer() {
        let protocol = object_field(value, "protocol");
        let protocol = string_from_jsvalue(protocol).unwrap_or_default();
        if protocol != "file:" {
            backup_path_type_error(name, value);
        }
        let pathname = object_field(value, "pathname");
        let pathname = string_from_jsvalue(pathname).unwrap_or_default();
        if pathname.is_empty() {
            backup_path_type_error(name, value);
        }
        percent_decode_pathname(&pathname)
    } else {
        backup_path_type_error(name, value);
    };

    if path.as_bytes().contains(&0) {
        throw_type(&format!(
            "The \"{}\" argument must not contain null bytes",
            name
        ));
    }
    path
}

fn int32_option_value(value: JSValue, name: &str) -> i32 {
    if value.is_int32() {
        return value.as_int32();
    }
    if value.is_number() {
        let number = value.as_number();
        if number.is_finite()
            && number.fract() == 0.0
            && number >= i32::MIN as f64
            && number <= i32::MAX as f64
        {
            return number as i32;
        }
    }
    throw_type(&format!(
        "The \"options.{}\" argument must be an integer.",
        name
    ));
}

unsafe fn int32_option(options_value: f64, name: &str, default: i32) -> i32 {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    int32_option_value(value, name)
}

unsafe fn parse_node_sqlite_backup_options(options_value: f64) -> NodeSqliteBackupOptions {
    let mut options = NodeSqliteBackupOptions::default();
    let js = value_from_f64(options_value);
    if js.is_undefined() {
        return options;
    }
    if js.is_null() || !is_object_like(options_value) {
        throw_type("The \"options\" argument must be an object.");
    }

    options.rate = int32_option(options_value, "rate", options.rate);
    options.source = string_option(options_value, "source", Some("main")).unwrap();
    options.target = string_option(options_value, "target", Some("main")).unwrap();
    options.progress = function_option(options_value, "progress").and_then(closure_ptr_from_value);
    options
}

unsafe fn database_handle_from_backup_source(value: f64) -> Handle {
    let js = value_from_f64(value);
    if !js.is_pointer() {
        throw_type("The \"sourceDb\" argument must be an object.");
    }
    let handle = raw_addr_from_value(value) as Handle;
    if get_handle::<NodeSqliteDbHandle>(handle).is_none() {
        throw_type("The \"sourceDb\" argument must be an instance of DatabaseSync.");
    }
    handle
}

unsafe fn call_backup_progress(
    progress: *const ClosureHeader,
    total_pages: i32,
    remaining_pages: i32,
) {
    let info = js_object_alloc(0, 2);
    let total_key = js_string_from_bytes(b"totalPages".as_ptr(), "totalPages".len() as u32);
    let remaining_key =
        js_string_from_bytes(b"remainingPages".as_ptr(), "remainingPages".len() as u32);
    js_object_set_field_by_name(
        info,
        total_key,
        f64::from_bits(JSValue::int32(total_pages).bits()),
    );
    js_object_set_field_by_name(
        info,
        remaining_key,
        f64::from_bits(JSValue::int32(remaining_pages).bits()),
    );
    js_closure_call1(
        progress,
        f64::from_bits(JSValue::object_ptr(info as *mut u8).bits()),
    );
}

unsafe fn perform_node_sqlite_backup(
    source_conn: &Connection,
    path: &str,
    options: &NodeSqliteBackupOptions,
) -> Result<i32, NodeSqliteBackupError> {
    let destination = Connection::open_with_flags(
        resolve_sqlite_path(path),
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sqlite_error_from_rusqlite)?;

    let source_name = CString::new(options.source.as_str()).map_err(|_| NodeSqliteBackupError {
        message: "The \"options.source\" argument must not contain null bytes".to_string(),
        errcode: None,
        errstr: None,
    })?;
    let target_name = CString::new(options.target.as_str()).map_err(|_| NodeSqliteBackupError {
        message: "The \"options.target\" argument must not contain null bytes".to_string(),
        errcode: None,
        errstr: None,
    })?;

    let backup = ffi::sqlite3_backup_init(
        destination.handle(),
        target_name.as_ptr(),
        source_conn.handle(),
        source_name.as_ptr(),
    );
    if backup.is_null() {
        return Err(sqlite_error_from_db(destination.handle()));
    }

    let step_pages = if options.rate == 0 { -1 } else { options.rate };
    let mut total_pages;
    let mut result = Ok(());

    loop {
        let rc = ffi::sqlite3_backup_step(backup, step_pages);
        total_pages = ffi::sqlite3_backup_pagecount(backup);
        let remaining_pages = ffi::sqlite3_backup_remaining(backup);

        if remaining_pages != 0 {
            if let Some(progress) = options.progress {
                call_backup_progress(progress, total_pages, remaining_pages);
            }
        }

        if rc == ffi::SQLITE_DONE {
            break;
        }
        if rc == ffi::SQLITE_OK || rc == ffi::SQLITE_BUSY || rc == ffi::SQLITE_LOCKED {
            continue;
        }
        result = Err(sqlite_error_from_code(rc));
        break;
    }

    let finish_rc = ffi::sqlite3_backup_finish(backup);
    if let Err(err) = result {
        return Err(err);
    }
    if finish_rc != ffi::SQLITE_OK {
        return Err(sqlite_error_from_db(destination.handle()));
    }
    Ok(total_pages)
}

fn resolve_sqlite_path(filename: &str) -> String {
    if filename == ":memory:" || filename.starts_with('/') || filename.starts_with(':') {
        return filename.to_string();
    }
    #[cfg(target_os = "ios")]
    {
        extern "C" {
            fn getenv(name: *const i8) -> *const i8;
        }
        unsafe {
            let home = getenv(b"HOME\0".as_ptr() as *const i8);
            if !home.is_null() {
                let home_str = std::ffi::CStr::from_ptr(home).to_str().unwrap_or("");
                let docs = format!("{}/Documents", home_str);
                let _ = std::fs::create_dir_all(&docs);
                return format!("{}/{}", docs, filename);
            }
        }
    }
    filename.to_string()
}

fn open_node_sqlite_connection(db: &NodeSqliteDbHandle) -> rusqlite::Result<Connection> {
    let flags = if db.read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let conn = if db.path == ":memory:" {
        Connection::open_in_memory_with_flags(flags)?
    } else {
        Connection::open_with_flags(resolve_sqlite_path(&db.path), flags)?
    };

    if db.timeout_ms > 0 {
        conn.busy_timeout(Duration::from_millis(db.timeout_ms as u64))?;
    }

    conn.execute_batch(if db.enable_foreign_keys {
        "PRAGMA foreign_keys = ON"
    } else {
        "PRAGMA foreign_keys = OFF"
    })?;

    for (idx, value) in db.initial_limits.iter().enumerate() {
        if let Some(value) = value {
            if let Some(limit) = [
                Limit::SQLITE_LIMIT_LENGTH,
                Limit::SQLITE_LIMIT_SQL_LENGTH,
                Limit::SQLITE_LIMIT_COLUMN,
                Limit::SQLITE_LIMIT_EXPR_DEPTH,
                Limit::SQLITE_LIMIT_COMPOUND_SELECT,
                Limit::SQLITE_LIMIT_VDBE_OP,
                Limit::SQLITE_LIMIT_FUNCTION_ARG,
                Limit::SQLITE_LIMIT_ATTACHED,
                Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH,
                Limit::SQLITE_LIMIT_VARIABLE_NUMBER,
                Limit::SQLITE_LIMIT_TRIGGER_DEPTH,
            ]
            .get(idx)
            {
                conn.set_limit(*limit, *value);
            }
        }
    }

    Ok(conn)
}

unsafe fn configure_node_sqlite_load_extension(
    conn: &Connection,
    enable: bool,
) -> Result<(), String> {
    let mut current = 0;
    let rc = ffi::sqlite3_db_config(
        conn.handle(),
        ffi::SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION,
        if enable { 1 } else { 0 },
        &mut current,
    );
    if rc == ffi::SQLITE_OK {
        return Ok(());
    }
    Err(CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
        .to_string_lossy()
        .into_owned())
}

unsafe fn with_sqlite_connection<R, F>(db_handle: Handle, f: F) -> Option<R>
where
    F: FnOnce(&Connection) -> R,
{
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return Some(f(&conn));
        }
    }
    if let Some(db) = get_handle::<NodeSqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            if let Some(conn) = conn.as_ref() {
                return Some(f(conn));
            }
        }
    }
    None
}

unsafe fn with_open_node_connection<R, F>(db_handle: Handle, f: F) -> R
where
    F: FnOnce(&Connection) -> R,
{
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    let conn_ptr = {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
        if let Some(conn) = conn.as_ref() {
            conn as *const Connection
        } else {
            drop(conn);
            throw_invalid_state("Database is not open")
        }
    };
    f(&*conn_ptr)
}

unsafe fn ensure_open_node_database(db_handle: Handle) {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    let conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    if conn.is_none() {
        drop(conn);
        throw_invalid_state("Database is not open");
    }
}

unsafe fn finalize_node_sqlite_statements(db: &NodeSqliteDbHandle) {
    let handles: Vec<Handle> = db
        .statements
        .lock()
        .map(|mut statements| statements.drain().collect())
        .unwrap_or_default();

    for handle in handles {
        if let Some(stmt) = get_handle::<NodeSqliteStmtHandle>(handle) {
            stmt.finalized.store(true, Ordering::Relaxed);
        }
    }
}

/// SQLite database handle
pub struct SqliteDbHandle {
    pub conn: Mutex<Connection>,
}

/// Node `node:sqlite` DatabaseSync handle.
///
/// Kept separate from `SqliteDbHandle` so the historical better-sqlite3
/// close/exec/prepare behavior remains unchanged.
pub struct NodeSqliteDbHandle {
    pub conn: Mutex<Option<Connection>>,
    pub path: String,
    pub read_only: bool,
    pub enable_foreign_keys: bool,
    pub enable_dqs: bool,
    pub timeout_ms: i32,
    pub read_bigints: bool,
    pub return_arrays: bool,
    pub allow_bare_named_parameters: bool,
    pub allow_unknown_named_parameters: bool,
    pub allow_load_extension: bool,
    pub enable_load_extension: AtomicBool,
    pub defensive: bool,
    pub initial_limits: [Option<i32>; NODE_SQLITE_LIMIT_COUNT],
    pub limits_handle: Mutex<Option<Handle>>,
    pub statements: Mutex<HashSet<Handle>>,
}

pub struct NodeSqliteLimitsHandle {
    pub db_handle: Handle,
}

pub struct NodeSqliteStmtHandle {
    pub db_handle: Handle,
    pub sql: String,
    pub finalized: AtomicBool,
    pub read_bigints: AtomicBool,
    pub return_arrays: AtomicBool,
    pub allow_bare_named_parameters: AtomicBool,
    pub allow_unknown_named_parameters: AtomicBool,
    pub expanded_sql: Mutex<String>,
}

struct NodeSqliteStmtOptions {
    read_bigints: bool,
    return_arrays: bool,
    allow_bare_named_parameters: bool,
    allow_unknown_named_parameters: bool,
}

#[derive(Clone)]
struct NodeSqliteOptions {
    open: bool,
    read_only: bool,
    enable_foreign_keys: bool,
    enable_dqs: bool,
    timeout_ms: i32,
    read_bigints: bool,
    return_arrays: bool,
    allow_bare_named_parameters: bool,
    allow_unknown_named_parameters: bool,
    allow_extension: bool,
    defensive: bool,
    initial_limits: [Option<i32>; NODE_SQLITE_LIMIT_COUNT],
}

impl Default for NodeSqliteOptions {
    fn default() -> Self {
        Self {
            open: true,
            read_only: false,
            enable_foreign_keys: true,
            enable_dqs: false,
            timeout_ms: 0,
            read_bigints: false,
            return_arrays: false,
            allow_bare_named_parameters: true,
            allow_unknown_named_parameters: false,
            allow_extension: false,
            defensive: true,
            initial_limits: [None; NODE_SQLITE_LIMIT_COUNT],
        }
    }
}

const NODE_SQLITE_LIMIT_COUNT: usize = 11;
const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL_BITS: u64 = 0x7FFC_0000_0000_0002;
const JS_SAFE_INTEGER_MAX: i64 = 9_007_199_254_740_991;
const JS_SAFE_INTEGER_MIN: i64 = -9_007_199_254_740_991;

/// SQLite statement handle
pub struct SqliteStmtHandle {
    pub sql: String,
    pub db_handle: Handle,
    /// Per-statement raw mode flag — `stmt.raw([toggle])` enables this.
    /// In raw mode, `stmt.all(...)` returns array-of-arrays (one inner
    /// array per row, column values in declared order) and
    /// `stmt.get(...)` returns a single column-value array. drizzle's
    /// `PreparedQuery.values()` chains `this.stmt.raw().all(...)` to
    /// feed `mapResultRow(fields, row, joinsNotNullableMap)`. Without
    /// this method `stmt.raw` is undefined and the call surfaces as
    /// `(number).all is not a function` deeper in the chain. Refs #643.
    pub raw_mode: AtomicBool,
}

/// Convert SQLite value to JSValue
unsafe fn sqlite_value_to_jsvalue(value: &SqliteValue) -> JSValue {
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

struct RawNodeStatement {
    ptr: *mut ffi::sqlite3_stmt,
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

fn f64_from_jsvalue(value: JSValue) -> f64 {
    f64::from_bits(value.bits())
}

fn string_value(value: &str) -> JSValue {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    JSValue::string_ptr(ptr)
}

unsafe fn sqlite_c_string_value(ptr: *const c_char) -> JSValue {
    if ptr.is_null() {
        return JSValue::null();
    }
    let value = CStr::from_ptr(ptr).to_string_lossy();
    string_value(&value)
}

unsafe fn sqlite_error_message(conn: &Connection) -> String {
    CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
        .to_string_lossy()
        .into_owned()
}

unsafe fn prepare_node_raw_statement(conn: &Connection, sql: &str) -> RawNodeStatement {
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
        throw_sqlite_error(&sqlite_error_message(conn));
    }
    RawNodeStatement { ptr: raw }
}

unsafe fn update_node_expanded_sql(stmt: &NodeSqliteStmtHandle, raw_stmt: *mut ffi::sqlite3_stmt) {
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

fn bigint_to_i64(ptr: *const BigIntHeader) -> Option<i64> {
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

unsafe fn node_sqlite_bind_error(conn: &Connection, rc: c_int) {
    if rc != ffi::SQLITE_OK {
        throw_sqlite_error(&sqlite_error_message(conn));
    }
}

unsafe fn bind_node_sqlite_value(
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
        ffi::sqlite3_bind_int64(raw_stmt, index, js.as_int32() as i64)
    } else if js.is_bigint() {
        let Some(value) = bigint_to_i64(js.as_bigint_ptr()) else {
            throw_arg_value("BigInt value is too large to bind.");
        };
        ffi::sqlite3_bind_int64(raw_stmt, index, value)
    } else if js.is_number() {
        let number = js.as_number();
        if number.is_finite()
            && number.fract() == 0.0
            && number >= i64::MIN as f64
            && number <= i64::MAX as f64
        {
            ffi::sqlite3_bind_int64(raw_stmt, index, number as i64)
        } else {
            ffi::sqlite3_bind_double(raw_stmt, index, number)
        }
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

unsafe fn node_args_from_array(args_arr: *const ArrayHeader) -> Vec<f64> {
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

fn is_named_parameter_object(value: f64) -> bool {
    let js = value_from_f64(value);
    if !js.is_pointer() {
        return false;
    }
    let raw = raw_addr_from_value(value);
    raw >= 0x1000 && !is_registered_buffer(raw)
}

unsafe fn string_key_from_js_value(value: JSValue) -> Option<String> {
    if !value.is_any_string() {
        return None;
    }
    let ptr = js_get_string_pointer_unified(f64_from_jsvalue(value)) as *const StringHeader;
    string_from_header(ptr)
}

fn strip_sqlite_parameter_prefix(name: &str) -> &str {
    name.strip_prefix(':')
        .or_else(|| name.strip_prefix('@'))
        .or_else(|| name.strip_prefix('$'))
        .unwrap_or(name)
}

fn has_sqlite_parameter_prefix(name: &str) -> bool {
    name.starts_with(':') || name.starts_with('@') || name.starts_with('$')
}

unsafe fn bind_node_sqlite_params(
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
        throw_sqlite_error("column index out of range");
    }
    for (offset, index) in anonymous_indices.into_iter().enumerate() {
        if let Some(value) = args.get(positional_start + offset).copied() {
            bind_node_sqlite_value(conn, raw_stmt, index, value);
        }
    }
}

unsafe fn node_sqlite_integer_value(value: i64, read_bigints: bool) -> JSValue {
    if read_bigints {
        return JSValue::bigint_ptr(perry_runtime::bigint::js_bigint_from_i64(value));
    }
    if !(JS_SAFE_INTEGER_MIN..=JS_SAFE_INTEGER_MAX).contains(&value) {
        throw_range(&format!(
            "Value is too large to be represented as a JavaScript number: {}",
            value
        ));
    }
    if (i32::MIN as i64..=i32::MAX as i64).contains(&value) {
        JSValue::int32(value as i32)
    } else {
        JSValue::number(value as f64)
    }
}

unsafe fn node_sqlite_column_value(
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

unsafe fn set_object_keys_from_names(obj: *mut ObjectHeader, names: &[String]) {
    let mut keys = js_array_alloc(names.len() as u32);
    for name in names {
        let ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        keys = js_array_push(keys, JSValue::string_ptr(ptr));
    }
    js_object_set_keys(obj, keys);
}

unsafe fn make_null_proto_object(names: &[String], values: &[JSValue]) -> *mut ObjectHeader {
    let obj = js_object_alloc_null_proto(0, names.len() as u32);
    set_object_keys_from_names(obj, names);
    for (idx, value) in values.iter().enumerate() {
        js_object_set_field(obj, idx as u32, *value);
    }
    obj
}

unsafe fn node_sqlite_row_value(
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

unsafe fn with_node_sqlite_statement<R, F>(
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
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    let conn_ptr = {
        let conn_guard = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
        if let Some(conn) = conn_guard.as_ref() {
            conn as *const Connection
        } else {
            drop(conn_guard);
            throw_invalid_state("Database is not open");
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

/// Build packed keys (null-separated) and a shape_id from column names.
fn build_packed_keys(column_names: &[String]) -> (Vec<u8>, u32) {
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

/// new Database(filename) -> Database
///
/// Open or create a SQLite database.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_open(filename_ptr: *const StringHeader) -> Handle {
    let filename = match string_from_header(filename_ptr) {
        Some(f) => f,
        None => return -1,
    };

    let conn = if filename == ":memory:" {
        Connection::open_in_memory()
    } else {
        // On iOS/Android, resolve relative paths to a writable directory
        // (the CWD is typically the read-only app bundle on mobile platforms)
        let resolved = if !filename.starts_with('/') && !filename.starts_with(':') {
            #[cfg(target_os = "ios")]
            {
                extern "C" {
                    fn getenv(name: *const i8) -> *const i8;
                }
                let home = getenv(b"HOME\0".as_ptr() as *const i8);
                if !home.is_null() {
                    let home_str = std::ffi::CStr::from_ptr(home).to_str().unwrap_or("");
                    let docs = format!("{}/Documents", home_str);
                    let _ = std::fs::create_dir_all(&docs);
                    format!("{}/{}", docs, filename)
                } else {
                    filename.clone()
                }
            }
            #[cfg(not(target_os = "ios"))]
            {
                filename.clone()
            }
        } else {
            filename.clone()
        };
        Connection::open(&resolved)
    };

    match conn {
        Ok(c) => register_handle(SqliteDbHandle {
            conn: Mutex::new(c),
        }),
        Err(_) => -1,
    }
}

/// db.exec(sql) -> Database
///
/// Execute one or more SQL statements.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_exec(db_handle: Handle, sql_ptr: *const StringHeader) -> i32 {
    let sql = match string_from_header(sql_ptr) {
        Some(s) => s,
        None => return 0,
    };

    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute_batch(&sql).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// db.prepare(sql) -> Statement
///
/// Create a prepared statement.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_prepare(
    db_handle: Handle,
    sql_ptr: *const StringHeader,
) -> Handle {
    let sql = match string_from_header(sql_ptr) {
        Some(s) => s,
        None => return -1,
    };

    // Verify the SQL is valid
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            if conn.prepare(&sql).is_ok() {
                return register_handle(SqliteStmtHandle {
                    sql,
                    db_handle,
                    raw_mode: AtomicBool::new(false),
                });
            }
        }
    }
    -1
}

/// stmt.raw([toggle]) -> stmt
///
/// Toggle raw mode on the statement and return the same handle so
/// `stmt.raw().all(...)` chains. Raw mode makes subsequent `.all()` /
/// `.get()` return rows as arrays of column values (in declared
/// column order) instead of objects keyed by column name.
///
/// drizzle's `PreparedQuery.values()` chains
/// `this.stmt.raw().all(...params)` to get back row arrays it then
/// hands to `mapResultRow(fields, row, joinsNotNullableMap)`. Without
/// this method `stmt.raw` is undefined and the call surfaces as
/// `(number).all is not a function` deeper in the chain because perry
/// returns a number sentinel when calling `undefined()` instead of
/// throwing immediately. Refs #643.
///
/// Argument handling: drizzle only ever uses the no-arg form. Real
/// better-sqlite3 also accepts `.raw(false)` to disable. We don't
/// thread the toggle through the codegen's NativeMethodCall dispatch
/// yet (it would need an `NA_F64` slot), so the no-arg form is the
/// only path. Conservative: always enable on call.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_raw(stmt_handle: Handle) -> Handle {
    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        stmt.raw_mode.store(true, Ordering::Relaxed);
    }
    stmt_handle
}

/// Extract SQLite parameters from a NaN-boxed array
unsafe fn params_from_array(arr_ptr: *const ArrayHeader) -> Vec<Box<dyn rusqlite::ToSql>> {
    if arr_ptr.is_null() {
        return vec![];
    }
    // Codegen pads omitted-arg slots with TAG_UNDEFINED bits when a stmt
    // method is called with no params (e.g. `stmt.run()` / `stmt.all()`).
    // Those bits look like a non-null pointer but actually carry the
    // 0x7FFC NaN-box tag in the high 16; dereferencing as ArrayHeader is
    // UB and reads a garbage `length` that crashes the loop below.
    // Treat any value with non-zero upper-16 as "no params".
    let upper16 = (arr_ptr as usize as u64) >> 48;
    if upper16 != 0 {
        return vec![];
    }
    let len = (*arr_ptr).length as usize;
    let elements = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(len);

    for i in 0..len {
        let val = *elements.add(i);
        let bits = val.to_bits();

        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
        const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
        const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
        const STRING_TAG: u64 = 0x7FFF;
        const INT32_TAG: u64 = 0x7FFE;

        let top16 = bits >> 48;

        if bits == TAG_NULL || bits == TAG_UNDEFINED {
            params.push(Box::new(rusqlite::types::Null));
        } else if bits == TAG_TRUE {
            params.push(Box::new(1i64));
        } else if bits == TAG_FALSE {
            params.push(Box::new(0i64));
        } else if top16 == STRING_TAG {
            // String: extract pointer
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            if let Some(s) = string_from_header(ptr) {
                params.push(Box::new(s));
            } else {
                params.push(Box::new(rusqlite::types::Null));
            }
        } else if top16 == INT32_TAG {
            let n = (bits & 0xFFFF_FFFF) as i32;
            params.push(Box::new(n as i64));
        } else {
            // Regular f64 number
            if val.fract() == 0.0 && val >= i64::MIN as f64 && val <= i64::MAX as f64 {
                params.push(Box::new(val as i64));
            } else {
                params.push(Box::new(val));
            }
        }
    }

    params
}

/// stmt.run(...params) -> RunResult
///
/// Execute a prepared statement with parameters.
/// Returns { changes: number, lastInsertRowid: number }
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_run(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    let sqlite_params = params_from_array(params_arr);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        if let Some(result) = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(changes) = conn.execute(&stmt.sql, param_refs.as_slice()) {
                let last_id = conn.last_insert_rowid();
                let keys = vec!["changes".to_string(), "lastInsertRowid".to_string()];
                let (packed_keys, shape_id) = build_packed_keys(&keys);
                let result = js_object_alloc_with_shape(
                    shape_id,
                    2,
                    packed_keys.as_ptr(),
                    packed_keys.len() as u32,
                );
                js_object_set_field(result, 0, JSValue::number(changes as f64));
                js_object_set_field(result, 1, JSValue::number(last_id as f64));
                return result;
            }
            std::ptr::null_mut()
        }) {
            return result;
        }
    }

    std::ptr::null_mut()
}

/// stmt.get(...params) -> Row | undefined
///
/// Get a single row from a query. Returns f64 (NaN-boxed bits) instead
/// of JSValue to avoid SysV AMD64 ABI mismatch on x86_64 (JSValue's
/// `#[repr(transparent)] u64` returns in RAX but LLVM reads from XMM0
/// when the call site declares a `double` return).
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    let sqlite_params = params_from_array(params_arr);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        if let Some(result) = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                let column_names: Vec<String> = prepared
                    .column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                let mut rows = prepared.query(param_refs.as_slice());
                if let Ok(ref mut rows) = rows {
                    if let Ok(Some(row)) = rows.next() {
                        if raw {
                            let row_arr = js_array_alloc(0);
                            for (idx, _) in column_names.iter().enumerate() {
                                let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                                js_array_push(row_arr, sqlite_value_to_jsvalue(&value));
                            }
                            return f64::from_bits(JSValue::object_ptr(row_arr as *mut u8).bits());
                        }
                        let (packed_keys, shape_id) = build_packed_keys(&column_names);
                        let obj = js_object_alloc_with_shape(
                            shape_id,
                            column_names.len() as u32,
                            packed_keys.as_ptr(),
                            packed_keys.len() as u32,
                        );

                        for (idx, _name) in column_names.iter().enumerate() {
                            let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                            js_object_set_field(obj, idx as u32, sqlite_value_to_jsvalue(&value));
                        }

                        return f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits());
                    }
                }
            }
            f64::from_bits(JSValue::undefined().bits())
        }) {
            return result;
        }
    }

    f64::from_bits(JSValue::undefined().bits())
}

/// stmt.all(...params) -> Row[]
///
/// Get all rows from a query.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_stmt_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    let sqlite_params = params_from_array(params_arr);
    let result_array = js_array_alloc(0);

    if let Some(stmt) = get_handle::<SqliteStmtHandle>(stmt_handle) {
        let raw = stmt.raw_mode.load(Ordering::Relaxed);
        let _ = with_sqlite_connection(stmt.db_handle, |conn| {
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                sqlite_params.iter().map(|p| p.as_ref()).collect();

            if let Ok(mut prepared) = conn.prepare(&stmt.sql) {
                let column_names: Vec<String> = prepared
                    .column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();

                // Only build the per-row object shape in non-raw
                // mode. In raw mode each row is its own array of
                // column values; no per-row object shape needed.
                let object_shape = if raw {
                    None
                } else {
                    Some(build_packed_keys(&column_names))
                };

                let mut rows = prepared.query(param_refs.as_slice());
                if let Ok(ref mut rows) = rows {
                    while let Ok(Some(row)) = rows.next() {
                        if raw {
                            let row_arr = js_array_alloc(0);
                            for (idx, _) in column_names.iter().enumerate() {
                                let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                                js_array_push(row_arr, sqlite_value_to_jsvalue(&value));
                            }
                            js_array_push(result_array, JSValue::object_ptr(row_arr as *mut u8));
                            continue;
                        }
                        let (packed_keys, shape_id) = object_shape.as_ref().unwrap();
                        let obj = js_object_alloc_with_shape(
                            *shape_id,
                            column_names.len() as u32,
                            packed_keys.as_ptr(),
                            packed_keys.len() as u32,
                        );

                        for (idx, _name) in column_names.iter().enumerate() {
                            let value: SqliteValue = row.get(idx).unwrap_or(SqliteValue::Null);
                            js_object_set_field(obj, idx as u32, sqlite_value_to_jsvalue(&value));
                        }

                        js_array_push(result_array, JSValue::object_ptr(obj as *mut u8));
                    }
                }
            }
        });
    }

    result_array
}

/// db.pragma(pragma, value?) -> any
///
/// Execute a PRAGMA statement.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_pragma(
    db_handle: Handle,
    pragma_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut StringHeader {
    let pragma = match string_from_header(pragma_ptr) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };

    let value = string_from_header(value_ptr);

    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            let sql = if let Some(v) = value {
                format!("PRAGMA {} = {}", pragma, v)
            } else {
                format!("PRAGMA {}", pragma)
            };

            if let Ok(mut stmt) = conn.prepare(&sql) {
                let mut rows = stmt.query([]);
                if let Ok(ref mut rows) = rows {
                    if let Ok(Some(row)) = rows.next() {
                        let result: String = row.get(0).unwrap_or_default();
                        return js_string_from_bytes(result.as_ptr(), result.len() as u32);
                    }
                }
            }
        }
    }

    std::ptr::null_mut()
}

/// The transaction wrapper function — called when the returned closure is invoked.
/// Captures: [0] = db_handle (as f64), [1] = original closure ptr (as i64)
unsafe extern "C" fn sqlite_tx_wrapper(
    wrapper_closure: *const perry_runtime::ClosureHeader,
    arg0: f64,
) -> f64 {
    use perry_runtime::closure::{
        js_closure_call1, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    };

    let db_handle_f64 = js_closure_get_capture_f64(wrapper_closure, 0);
    let db_handle = db_handle_f64 as i64;
    let original_closure =
        js_closure_get_capture_ptr(wrapper_closure, 1) as *const perry_runtime::ClosureHeader;

    // BEGIN
    js_sqlite_begin_transaction(db_handle);

    // Call original closure with argument
    let result = js_closure_call1(original_closure, arg0);

    // COMMIT
    js_sqlite_commit(db_handle);

    result
}

/// db.transaction(fn) -> wrapping closure
///
/// Returns a closure that wraps fn in BEGIN/COMMIT.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_transaction(
    db_handle: Handle,
    closure_ptr: i64,
) -> *mut perry_runtime::ClosureHeader {
    use perry_runtime::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };

    let wrapper = js_closure_alloc(sqlite_tx_wrapper as *const u8, 2);
    js_closure_set_capture_f64(wrapper, 0, db_handle as f64);
    js_closure_set_capture_ptr(wrapper, 1, closure_ptr);

    wrapper
}

/// Begin a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_begin_transaction(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("BEGIN TRANSACTION", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// Commit a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_commit(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("COMMIT", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// Rollback a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_rollback(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            return if conn.execute("ROLLBACK", []).is_ok() {
                1
            } else {
                0
            };
        }
    }
    0
}

/// db.close() -> void
///
/// Close the database connection.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_close(db_handle: Handle) -> i32 {
    // The connection will be closed when the handle is dropped
    // For now, we just verify the handle is valid
    if get_handle::<SqliteDbHandle>(db_handle).is_some() {
        1
    } else {
        0
    }
}

/// db.inTransaction -> boolean
///
/// Check if currently in a transaction.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_in_transaction(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<SqliteDbHandle>(db_handle) {
        if let Ok(conn) = db.conn.lock() {
            // SQLite's autocommit mode is off when in a transaction
            return if !conn.is_autocommit() { 1 } else { 0 };
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_call(
    _path_value: f64,
    _options_value: f64,
) -> Handle {
    throw_construct_required()
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

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_new(
    path_value: f64,
    options_value: f64,
) -> Handle {
    let path = string_from_value(path_value, "path");
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
        defensive: options.defensive,
        initial_limits: options.initial_limits,
        limits_handle: Mutex::new(None),
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
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
        if conn.is_some() {
            drop(conn);
            throw_invalid_state("Database is already open");
        }
    }
    let opened = match open_node_sqlite_connection(db) {
        Ok(opened) => opened,
        Err(err) => throw_sqlite_error(&err.to_string()),
    };
    if let Err(err) = configure_node_sqlite_load_extension(
        &opened,
        db.enable_load_extension.load(Ordering::Relaxed),
    ) {
        throw_sqlite_error(&err);
    }
    let mut conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    *conn = Some(opened);
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_close(db_handle: Handle) -> i32 {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
        if conn.is_none() {
            drop(conn);
            throw_invalid_state("Database is not open");
        }
    }
    finalize_node_sqlite_statements(db);
    let mut conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    if conn.is_some() {
        *conn = None;
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_dispose(db_handle: Handle) -> i32 {
    if let Some(db) = get_handle::<NodeSqliteDbHandle>(db_handle) {
        finalize_node_sqlite_statements(db);
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
        Err(err) => throw_sqlite_error(&err),
    }
}

unsafe fn parse_statement_options(
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
        throw_type("The \"options\" argument must be an object");
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
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
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

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_call(_arg0: f64, _arg1: f64) -> Handle {
    throw_illegal_constructor()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_new(_arg0: f64, _arg1: f64) -> Handle {
    throw_illegal_constructor()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_run(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ObjectHeader {
    with_node_sqlite_statement(stmt_handle, params_arr, |conn, stmt, raw_stmt| {
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
pub unsafe extern "C" fn js_node_sqlite_statement_sync_get(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    with_node_sqlite_statement(stmt_handle, params_arr, |conn, stmt, raw_stmt| {
        match ffi::sqlite3_step(raw_stmt) {
            ffi::SQLITE_ROW => f64_from_jsvalue(node_sqlite_row_value(stmt, raw_stmt)),
            ffi::SQLITE_DONE => undefined_f64(),
            _ => throw_sqlite_error(&sqlite_error_message(conn)),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_all(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> *mut ArrayHeader {
    with_node_sqlite_statement(stmt_handle, params_arr, |conn, stmt, raw_stmt| {
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
pub unsafe extern "C" fn js_node_sqlite_statement_sync_iterate(
    stmt_handle: Handle,
    params_arr: *const ArrayHeader,
) -> f64 {
    let rows = js_node_sqlite_statement_sync_all(stmt_handle, params_arr);
    perry_runtime::array::array_values_iter(f64_from_jsvalue(JSValue::array_ptr(rows)))
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_columns(
    stmt_handle: Handle,
) -> *mut ArrayHeader {
    with_node_sqlite_statement(stmt_handle, std::ptr::null(), |_conn, _stmt, raw_stmt| {
        let column_count = ffi::sqlite3_column_count(raw_stmt);
        let mut result = js_array_alloc(column_count as u32);
        let keys = vec![
            "column".to_string(),
            "database".to_string(),
            "name".to_string(),
            "table".to_string(),
            "type".to_string(),
        ];
        for index in 0..column_count {
            let values = vec![
                sqlite_c_string_value(ffi::sqlite3_column_origin_name(raw_stmt, index)),
                sqlite_c_string_value(ffi::sqlite3_column_database_name(raw_stmt, index)),
                sqlite_c_string_value(ffi::sqlite3_column_name(raw_stmt, index)),
                sqlite_c_string_value(ffi::sqlite3_column_table_name(raw_stmt, index)),
                sqlite_c_string_value(ffi::sqlite3_column_decltype(raw_stmt, index)),
            ];
            let obj = make_null_proto_object(&keys, &values);
            result = js_array_push(result, JSValue::object_ptr(obj as *mut u8));
        }
        result
    })
}

unsafe fn set_node_statement_bool_option(
    stmt_handle: Handle,
    value: f64,
    field: &AtomicBool,
) -> i32 {
    if get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .map(|stmt| stmt.finalized.load(Ordering::Relaxed))
        .unwrap_or(true)
    {
        throw_invalid_state("statement has been finalized");
    }
    let js = value_from_f64(value);
    if !js.is_bool() {
        throw_type("The \"enabled\" argument must be a boolean");
    }
    field.store(js.as_bool(), Ordering::Relaxed);
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_set_read_bigints(
    stmt_handle: Handle,
    value: f64,
) -> i32 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    set_node_statement_bool_option(stmt_handle, value, &stmt.read_bigints)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_set_return_arrays(
    stmt_handle: Handle,
    value: f64,
) -> i32 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    set_node_statement_bool_option(stmt_handle, value, &stmt.return_arrays)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_set_allow_bare_named_parameters(
    stmt_handle: Handle,
    value: f64,
) -> i32 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    set_node_statement_bool_option(stmt_handle, value, &stmt.allow_bare_named_parameters)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_set_allow_unknown_named_parameters(
    stmt_handle: Handle,
    value: f64,
) -> i32 {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    set_node_statement_bool_option(stmt_handle, value, &stmt.allow_unknown_named_parameters)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_source_sql(
    stmt_handle: Handle,
) -> *mut StringHeader {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    if stmt.finalized.load(Ordering::Relaxed) {
        throw_invalid_state("statement has been finalized");
    }
    js_string_from_bytes(stmt.sql.as_ptr(), stmt.sql.len() as u32)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_statement_sync_expanded_sql(
    stmt_handle: Handle,
) -> *mut StringHeader {
    let stmt = get_handle::<NodeSqliteStmtHandle>(stmt_handle)
        .unwrap_or_else(|| throw_invalid_state("statement has been finalized"));
    if stmt.finalized.load(Ordering::Relaxed) {
        throw_invalid_state("statement has been finalized");
    }
    let expanded = stmt
        .expanded_sql
        .lock()
        .map(|sql| sql.clone())
        .unwrap_or_default();
    js_string_from_bytes(expanded.as_ptr(), expanded.len() as u32)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_enable_load_extension(
    db_handle: Handle,
    allow_value: f64,
) -> i32 {
    let allow = {
        let js = value_from_f64(allow_value);
        if !js.is_bool() {
            throw_type("The \"allow\" argument must be a boolean");
        }
        js.as_bool()
    };

    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    if allow && !db.allow_load_extension {
        throw_invalid_state(
            "Cannot enable extension loading because it was disabled at database creation.",
        );
    }

    let conn = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    let config_error = conn
        .as_ref()
        .and_then(|conn| configure_node_sqlite_load_extension(conn, allow).err());
    drop(conn);
    if let Some(err) = config_error {
        throw_sqlite_error(&err);
    }
    db.enable_load_extension.store(allow, Ordering::Relaxed);
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_load_extension(
    db_handle: Handle,
    path_value: f64,
) -> i32 {
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    {
        let conn = db
            .conn
            .lock()
            .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
        if conn.is_none() {
            drop(conn);
            throw_invalid_state("Database is not open");
        }
    }

    if !db.allow_load_extension || !db.enable_load_extension.load(Ordering::Relaxed) {
        throw_invalid_state("extension loading is not allowed");
    }

    let path = string_from_value(path_value, "path");
    let c_path = CString::new(path)
        .unwrap_or_else(|_| throw_type("The \"path\" argument must not contain null bytes"));
    let conn_guard = db
        .conn
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    let Some(conn) = conn_guard.as_ref() else {
        drop(conn_guard);
        throw_invalid_state("Database is not open");
    };
    let mut error_message = std::ptr::null_mut();
    let rc = ffi::sqlite3_load_extension(
        conn.handle(),
        c_path.as_ptr(),
        std::ptr::null(),
        &mut error_message,
    );
    if rc == ffi::SQLITE_OK {
        return 1;
    }

    let message = if error_message.is_null() {
        CStr::from_ptr(ffi::sqlite3_errmsg(conn.handle()))
            .to_string_lossy()
            .into_owned()
    } else {
        let message = CStr::from_ptr(error_message).to_string_lossy().into_owned();
        ffi::sqlite3_free(error_message.cast());
        message
    };
    drop(conn_guard);
    throw_load_sqlite_extension(&message)
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_location(
    db_handle: Handle,
    db_name_value: f64,
) -> f64 {
    ensure_open_node_database(db_handle);
    let db_name = if value_from_f64(db_name_value).is_undefined() {
        "main".to_string()
    } else {
        string_from_value(db_name_value, "dbName")
    };
    let c_name = CString::new(db_name)
        .unwrap_or_else(|_| throw_type("The \"dbName\" argument must not contain null bytes"));
    with_open_node_connection(db_handle, |conn| {
        let filename =
            unsafe { rusqlite::ffi::sqlite3_db_filename(conn.handle(), c_name.as_ptr()) };
        if filename.is_null() {
            return null_f64();
        }
        let filename = unsafe { CStr::from_ptr(filename) }.to_str().unwrap_or("");
        if filename.is_empty() {
            null_f64()
        } else {
            let ptr = js_string_from_bytes(filename.as_ptr(), filename.len() as u32);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_limits(db_handle: Handle) -> Handle {
    ensure_open_node_database(db_handle);
    let db = get_handle::<NodeSqliteDbHandle>(db_handle)
        .unwrap_or_else(|| throw_invalid_state("Database is not open"));
    let mut limits_handle = db
        .limits_handle
        .lock()
        .unwrap_or_else(|_| throw_invalid_state("Database is not open"));
    if let Some(handle) = *limits_handle {
        return handle;
    }
    let handle = register_handle(NodeSqliteLimitsHandle { db_handle });
    *limits_handle = Some(handle);
    handle
}

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

unsafe fn packed_args_array(args: &[f64]) -> *mut ArrayHeader {
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
        JSValue::int32(conn.limit(limit))
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

/// Returns `1` if `handle` currently resolves to a `SqliteDbHandle` in
/// this crate's handle registry, `0` otherwise. Used by the V8 bridge
/// in `perry-jsruntime::bridge::native_object_to_v8` to decide whether
/// to materialize a `v8::Object` proxy with `prepare`/`exec`/etc.
/// method callbacks when a sqlite Database crosses the native→V8
/// boundary (drizzle's `BetterSQLiteSession` does
/// `this.client.prepare(query.sql)` from session.js — refs #1022).
///
/// Mirrors `perry-ext-better-sqlite3::js_sqlite_is_db_handle`. The
/// duplicate-symbol resolution at link time picks one impl; whichever
/// crate's `js_sqlite_open` registered the handle is the same impl
/// whose `is_db_handle` answers the membership check (each crate
/// keeps its own registry).
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_is_db_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteDbHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Returns `1` if `handle` currently resolves to a `SqliteStmtHandle`
/// in this crate's handle registry, `0` otherwise. Mirror of
/// `js_sqlite_is_db_handle` for the Statement side — drizzle's
/// PreparedQuery calls `stmt.run(...)` / `stmt.all(...)` /
/// `stmt.get(...)` / `stmt.raw().all(...)` on the handle returned from
/// `client.prepare(...)`. Refs #1022.
#[no_mangle]
pub unsafe extern "C" fn js_sqlite_is_stmt_handle(handle: Handle) -> i32 {
    if get_handle::<SqliteStmtHandle>(handle).is_some() {
        1
    } else {
        0
    }
}
