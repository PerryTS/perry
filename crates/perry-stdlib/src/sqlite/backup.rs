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

pub(crate) struct NodeSqliteBackupOptions {
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

pub(crate) struct NodeSqliteBackupError {
    message: String,
    errcode: Option<i32>,
    errstr: Option<String>,
}

pub(crate) fn sqlite_errstr(code: i32) -> String {
    unsafe {
        CStr::from_ptr(ffi::sqlite3_errstr(code))
            .to_string_lossy()
            .into_owned()
    }
}

pub(crate) unsafe fn sqlite_error_from_db(db: *mut ffi::sqlite3) -> NodeSqliteBackupError {
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

pub(crate) fn sqlite_error_from_code(code: i32) -> NodeSqliteBackupError {
    let errstr = sqlite_errstr(code);
    NodeSqliteBackupError {
        message: errstr.clone(),
        errcode: Some(code),
        errstr: Some(errstr),
    }
}

pub(crate) fn sqlite_error_from_rusqlite(err: rusqlite::Error) -> NodeSqliteBackupError {
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

pub(crate) unsafe fn sqlite_error_value(error: NodeSqliteBackupError) -> f64 {
    let msg = js_string_from_bytes(error.message.as_ptr(), error.message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, "ERR_SQLITE_ERROR");
    let err = perry_runtime::error::js_error_new_with_message(msg);
    let err_obj = err as *mut ObjectHeader;

    if let Some(errcode) = error.errcode {
        let key = js_string_from_bytes(b"errcode".as_ptr(), "errcode".len() as u32);
        // NUMBER-tagged, not INT32-tagged: small extended result codes can
        // collide with registered class ids, which makes `typeof e.errcode`
        // report "function" (see node_sqlite_integer_value, #6561).
        js_object_set_field_by_name(
            err_obj,
            key,
            f64::from_bits(JSValue::number(errcode as f64).bits()),
        );
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

/// Throw a synchronous `ERR_SQLITE_ERROR` carrying Node's `errcode`
/// (extended result code) and `errstr` (`sqlite3_errstr` text) own
/// properties, matching `node:sqlite`'s error shape (#6561). The plain
/// [`throw_sqlite_error`] path only sets `code`; Node also exposes the
/// SQLite result code on every SQL-level failure.
pub(crate) unsafe fn throw_sqlite_error_ext(message: &str, errcode: i32) -> ! {
    let error = NodeSqliteBackupError {
        message: message.to_string(),
        errcode: Some(errcode),
        errstr: Some(sqlite_errstr(errcode)),
    };
    perry_runtime::exception::js_throw(sqlite_error_value(error))
}

/// Throw `ERR_SQLITE_ERROR` for the connection's current error state:
/// `sqlite3_errmsg` message + `sqlite3_extended_errcode` code (#6561).
pub(crate) unsafe fn throw_sqlite_error_from_conn(conn: &Connection) -> ! {
    let errcode = ffi::sqlite3_extended_errcode(conn.handle());
    let message = sqlite_error_message(conn);
    throw_sqlite_error_ext(&message, errcode)
}

pub(crate) fn backup_path_type_error(name: &str, value: f64) -> ! {
    let received = perry_runtime::fs::validate::describe_received(value);
    throw_type(&format!(
        "The \"{}\" argument must be of type string or an instance of Buffer or URL. Received {}",
        name, received
    ));
}

pub(crate) unsafe fn string_from_jsvalue(value: JSValue) -> Option<String> {
    if !value.is_any_string() {
        return None;
    }
    let ptr = js_get_string_pointer_unified(f64::from_bits(value.bits())) as *const StringHeader;
    string_from_header(ptr)
}

pub(crate) fn percent_decode_pathname(pathname: &str) -> String {
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

pub(crate) unsafe fn bytes_from_path_like(value: f64) -> Option<Vec<u8>> {
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

pub(crate) unsafe fn path_like_from_value(value: f64, name: &str) -> String {
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

pub(crate) fn int32_option_value(value: JSValue, name: &str) -> i32 {
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

pub(crate) unsafe fn int32_option(options_value: f64, name: &str, default: i32) -> i32 {
    let value = object_field(options_value, name);
    if value.is_undefined() {
        return default;
    }
    int32_option_value(value, name)
}

pub(crate) unsafe fn parse_node_sqlite_backup_options(
    options_value: f64,
) -> NodeSqliteBackupOptions {
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

pub(crate) unsafe fn database_handle_from_backup_source(value: f64) -> Handle {
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

pub(crate) unsafe fn call_backup_progress(
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
        f64::from_bits(JSValue::number(total_pages as f64).bits()),
    );
    js_object_set_field_by_name(
        info,
        remaining_key,
        f64::from_bits(JSValue::number(remaining_pages as f64).bits()),
    );
    js_closure_call1(
        progress,
        f64::from_bits(JSValue::object_ptr(info as *mut u8).bits()),
    );
}

pub(crate) unsafe fn perform_node_sqlite_backup(
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

pub(crate) fn resolve_sqlite_path(filename: &str) -> String {
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
