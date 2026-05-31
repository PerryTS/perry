//! `process.loadEnvFile` dotenv parsing (extracted from process.rs to keep it
//! under the 2000-line cap). `use super::*` preserves the parent visibility.
use super::*;

/// process.loadEnvFile(path?) — read a `.env`-formatted file from disk and
/// merge its `KEY=value` entries into `process.env`. Node 20.12+. With no
/// path, the default is `.env` in the current working directory. Throws a
/// Node-shaped `Error` (`code: "ENOENT"`, `syscall: "open"`) when the file
/// can't be opened. #2135 (#1399 follow-through): previously a no-op that
/// returned undefined so probe-and-call sites didn't crash; with
/// `process.env.X = v` now persisting via std::env (#1344), eager loading
/// is meaningful.
#[no_mangle]
pub extern "C" fn js_process_load_env_file(path_ptr: *const StringHeader) {
    let target = unsafe {
        if path_ptr.is_null() {
            ".env".to_string()
        } else {
            let len = (*path_ptr).byte_len as usize;
            let data = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            match std::str::from_utf8(bytes) {
                Ok(s) => s.to_string(),
                Err(_) => return,
            }
        }
    };
    load_env_file_at(&target)
}

/// Full-value entry point for process.loadEnvFile(path?). It preserves the
/// nullish-default behavior while accepting the path-like forms Node supports.
#[no_mangle]
pub extern "C" fn js_process_load_env_file_value(path: f64) {
    let target = match load_env_file_target(path) {
        Some(target) => target,
        None => ".env".to_string(),
    };
    load_env_file_at(&target);
}

fn load_env_file_at(target: &str) {
    let contents = match std::fs::read_to_string(target) {
        Ok(s) => s,
        Err(err) => unsafe {
            throw_load_env_file_open_error(&err, target);
        },
    };
    for (key, value) in parse_dotenv_entries(&contents) {
        if std::env::var_os(&key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

fn load_env_file_target(path: f64) -> Option<String> {
    let jv = crate::value::JSValue::from_bits(path.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return None;
    }
    if jv.is_any_string() {
        return Some(read_js_string(path));
    }
    if crate::buffer::js_buffer_is_buffer(path.to_bits() as i64) == 1 {
        let data = crate::buffer::js_native_buffer_data_ptr(path);
        let len = crate::buffer::js_native_buffer_byte_len(path);
        if data.is_null() {
            return Some(String::new());
        }
        return Some(unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
        });
    }
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
        if !obj.is_null() && crate::url::is_url_object_shape(obj) {
            let path_value = crate::url::js_url_file_url_to_path(path);
            return Some(read_js_string(path_value));
        }
    }
    crate::fs::validate::throw_invalid_path_arg("path", path);
}

fn read_js_string(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn parse_dotenv_entries(contents: &str) -> Vec<(String, String)> {
    let bytes = contents.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        i = skip_line_leading_space(bytes, i);
        if i >= bytes.len() {
            break;
        }
        if is_line_break(bytes[i]) {
            i = skip_line_break(bytes, i);
            continue;
        }
        if bytes[i] == b'#' {
            i = skip_to_next_line(bytes, i);
            continue;
        }
        if bytes[i..].starts_with(b"export")
            && bytes
                .get(i + "export".len())
                .copied()
                .is_some_and(is_space_or_tab)
        {
            i += "export".len();
            i = skip_spaces_and_tabs(bytes, i);
        }

        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' && !is_line_break(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            i = skip_to_next_line(bytes, i);
            continue;
        }
        let raw_key = &contents[key_start..i];
        let key = raw_key.trim().to_string();
        i += 1;
        if key.is_empty() {
            i = skip_to_next_line(bytes, i);
            continue;
        }

        i = skip_spaces_and_tabs(bytes, i);
        let (value, next) = parse_dotenv_value(contents, i);
        out.push((key, value));
        i = next;
    }
    out
}

fn parse_dotenv_value(contents: &str, mut i: usize) -> (String, usize) {
    let bytes = contents.as_bytes();
    if i >= bytes.len() {
        return (String::new(), i);
    }
    if matches!(bytes[i], b'"' | b'\'') {
        let quote = bytes[i];
        i += 1;
        let value_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        let value = String::from_utf8_lossy(&bytes[value_start..i]).into_owned();
        if i < bytes.len() && bytes[i] == quote {
            i += 1;
        }
        return (value, skip_to_next_line(bytes, i));
    }

    let value_start = i;
    while i < bytes.len() && !is_line_break(bytes[i]) {
        i += 1;
    }
    let mut raw_end = i;
    let mut scan = value_start;
    while scan < i {
        if bytes[scan] == b'#' {
            raw_end = scan;
            break;
        }
        scan += 1;
    }
    let value = String::from_utf8_lossy(&bytes[value_start..raw_end])
        .trim_end()
        .to_string();
    (value, skip_line_break(bytes, i))
}

fn skip_line_leading_space(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && is_space_or_tab(bytes[i]) {
        i += 1;
    }
    i
}

fn skip_spaces_and_tabs(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && is_space_or_tab(bytes[i]) {
        i += 1;
    }
    i
}

fn skip_to_next_line(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && !is_line_break(bytes[i]) {
        i += 1;
    }
    skip_line_break(bytes, i)
}

fn skip_line_break(bytes: &[u8], mut i: usize) -> usize {
    if i < bytes.len() && bytes[i] == b'\r' {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'\n' {
        i += 1;
    }
    i
}

fn is_space_or_tab(b: u8) -> bool {
    matches!(b, b' ' | b'\t')
}

fn is_line_break(b: u8) -> bool {
    matches!(b, b'\n' | b'\r')
}

unsafe fn throw_load_env_file_open_error(err: &std::io::Error, target: &str) -> ! {
    use std::io::ErrorKind;
    let code: &'static str = match err.kind() {
        ErrorKind::NotFound => "ENOENT",
        ErrorKind::PermissionDenied => "EACCES",
        _ => "EIO",
    };
    let desc = match code {
        "ENOENT" => "no such file or directory",
        "EACCES" => "permission denied",
        _ => "i/o error",
    };
    let message = format!("{code}: {desc}, open '{target}'");
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, code);
    crate::node_submodules::register_error_syscall(msg_ptr, "open");
    crate::node_submodules::register_error_path(msg_ptr, target.to_string());
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
}
