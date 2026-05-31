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
    let contents = match std::fs::read_to_string(&target) {
        Ok(s) => s,
        Err(err) => unsafe {
            throw_load_env_file_open_error(&err, &target);
        },
    };
    for line in contents.lines() {
        let trimmed = line.trim_start();
        // Comments and blank lines.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }
        // Strip a matched surrounding quote pair on the trimmed value;
        // otherwise keep the trimmed text verbatim (so unquoted spaces
        // around `=` are dropped but inner `=` survives — see Node's
        // built-in `.env` parser).
        let value_trimmed = raw_value.trim();
        let value = strip_matched_quotes(value_trimmed);
        std::env::set_var(key, value);
    }
}

fn strip_matched_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return &s[1..s.len() - 1];
        }
    }
    s
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
