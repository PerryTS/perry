//! `process.loadEnvFile()`, `process.chdir()` and `process.execve()` — the
//! entry points that read a `.env` file off disk, move the process's working
//! directory, or replace the process image (argv + environ) outright.
//!
//! Split verbatim out of the sibling [`super::env_misc`] module to keep that
//! file under the 2000-line gate. Pure code move — no behavior change.

use super::*;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

/// process.loadEnvFile(path?) — read a `.env`-formatted file from disk and
/// merge its `KEY=value` entries into `process.env`. Node 20.12+. With no
/// path, the default is `.env` in the current working directory. Throws a
/// Node-shaped `Error` (`code: "ENOENT"`, `syscall: "open"`) when the file
/// can't be opened. #2135 (#1399 follow-through): previously a no-op that
/// returned undefined so probe-and-call sites didn't crash; with
/// `process.env.X = v` now persisting via std::env (#1344), eager loading
/// is meaningful.
#[no_mangle]
pub extern "C" fn js_process_load_env_file(path_value: f64) {
    let target = load_env_file_path(path_value);
    let contents = match std::fs::read_to_string(&target) {
        Ok(s) => s,
        Err(err) => unsafe {
            throw_load_env_file_open_error(&err, &target);
        },
    };
    for (key, value) in crate::util_parse_env::parse_env(&contents) {
        if std::env::var_os(&key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

fn load_env_file_path(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return ".env".to_string();
    }
    unsafe {
        validate_load_env_file_url(value);
        crate::fs::decode_path_value(value)
            .unwrap_or_else(|| crate::fs::validate::throw_invalid_path_arg("path", value))
    }
}

unsafe fn validate_load_env_file_url(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return;
    }
    let obj = jv.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
    if obj.is_null() || !crate::url::is_url_object_shape(obj) {
        return;
    }
    let protocol = crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_PROTOCOL,
    ));
    if protocol != "file:" {
        throw_invalid_load_env_file_url_scheme();
    }
    let pathname = crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_PATHNAME,
    ));
    if has_encoded_forward_slash(&pathname) {
        crate::fs::validate::throw_type_error_with_code(
            "File URL path must not include encoded / characters",
            "ERR_INVALID_FILE_URL_PATH",
        );
    }
}

fn has_encoded_forward_slash(pathname: &str) -> bool {
    let bytes = pathname.as_bytes();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        if bytes[i] == b'%' && bytes[i + 1] == b'2' && (bytes[i + 2] | 0x20) == b'f' {
            return true;
        }
        i += 1;
    }
    false
}

fn throw_invalid_load_env_file_url_scheme() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The URL must be of scheme file",
        "ERR_INVALID_URL_SCHEME",
    )
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

// Issue #2013 — process-arg-validation helpers shared by `js_process_chdir`
// and `js_process_hrtime`. Sited here (not os.rs) so the process surface's
// validation logic stays under the 2000-line file gate as the os.rs splits
// progress.

/// `process.chdir(value)` entry point that takes the full NaN-boxed
/// value. Throws `TypeError [ERR_INVALID_ARG_TYPE]` for any non-string
/// (matching Node), then re-dispatches to `js_process_chdir` with the
/// extracted `StringHeader`. The codegen now emits this entry instead
/// of the bare string-only one so a `process.chdir(123)` call throws
/// the right error code instead of garbage-deref'ing to an `ENOENT`
/// based on whatever bytes the numeric value masqueraded as.
#[no_mangle]
pub unsafe extern "C" fn js_process_chdir_jsv(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        let message = format!(
            "The \"directory\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    crate::os::js_process_chdir(ptr);
}

fn execve_throw_invalid_arg_type(name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be {}. Received {}",
        name,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn execve_received_value(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if crate::fs::validate::is_numeric(jv) {
        let n = if jv.is_int32() {
            jv.as_int32() as f64
        } else {
            jv.as_number()
        };
        return crate::fs::validate::format_received_number(n);
    }
    if let Some(value) = module_value_to_string(value) {
        return format!("'{}'", value);
    }
    crate::fs::validate::describe_received(value)
}

fn execve_env_received(value: f64) -> String {
    let Some(obj) = module_object_ptr(value) else {
        return crate::fs::validate::describe_received(value);
    };
    let keys = crate::object::js_object_keys(obj);
    let len = crate::array::js_array_length(keys);
    let mut parts = Vec::new();
    for i in 0..len.min(3) {
        let key_value = crate::array::js_array_get_f64(keys, i);
        let key = module_value_to_string(key_value).unwrap_or_default();
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let field = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
        parts.push(format!("{}: {}", key, execve_received_value(field)));
    }
    if len > 3 {
        parts.push("...".to_string());
    }
    format!("{{ {} }}", parts.join(", "))
}

fn execve_throw_invalid_arg_value(name: &str, received: String) -> ! {
    let message = format!(
        "The argument '{}' must be a string without null bytes. Received {}",
        name, received
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn execve_throw_invalid_env(value: f64) -> ! {
    let message = format!(
        "The argument 'env' must be an object with string keys and values without null bytes. Received {}",
        execve_env_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

fn execve_parse_args(args: f64) -> Vec<String> {
    let args_value = JSValue::from_bits(args.to_bits());
    if args_value.is_undefined() {
        return Vec::new();
    }
    if !is_array_value(args_value) {
        execve_throw_invalid_arg_type("args", "an instance of Array", args);
    }
    let arr = args_value.as_pointer::<crate::array::ArrayHeader>();
    let len = crate::array::js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let value = crate::array::js_array_get_f64(arr, i);
        let Some(item) = module_value_to_string(value) else {
            execve_throw_invalid_arg_value(&format!("args[{i}]"), execve_received_value(value));
        };
        if item.as_bytes().contains(&0) {
            execve_throw_invalid_arg_value(&format!("args[{i}]"), execve_received_value(value));
        }
        out.push(item);
    }
    out
}

fn execve_parse_env(env: f64) -> Vec<(String, String)> {
    let env_value = JSValue::from_bits(env.to_bits());
    if env_value.is_undefined() {
        return std::env::vars().collect();
    }
    let Some(obj) = module_object_ptr(env) else {
        execve_throw_invalid_arg_type("env", "of type object", env);
    };
    let keys = crate::object::js_object_keys(obj);
    let len = crate::array::js_array_length(keys);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let key_value = crate::array::js_array_get_f64(keys, i);
        let Some(key) = module_value_to_string(key_value) else {
            execve_throw_invalid_env(env);
        };
        if key.as_bytes().contains(&0) {
            execve_throw_invalid_env(env);
        }
        let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let value = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
        let Some(value_string) = module_value_to_string(value) else {
            execve_throw_invalid_env(env);
        };
        if value_string.as_bytes().contains(&0) {
            execve_throw_invalid_env(env);
        }
        out.push((key, value_string));
    }
    out
}

#[no_mangle]
pub extern "C" fn js_process_execve(exec_path: f64, args: f64, env: f64) -> f64 {
    let Some(path) = module_value_to_string(exec_path) else {
        execve_throw_invalid_arg_type("execPath", "of type string", exec_path);
    };
    if path.as_bytes().contains(&0) {
        execve_throw_invalid_arg_value("execPath", execve_received_value(exec_path));
    }
    let argv = execve_parse_args(args);
    let env_pairs = execve_parse_env(env);

    #[cfg(unix)]
    {
        let path_c = match std::ffi::CString::new(path.as_str()) {
            Ok(path_c) => path_c,
            Err(_) => execve_throw_invalid_arg_value("execPath", execve_received_value(exec_path)),
        };
        let argv_c: Vec<std::ffi::CString> = argv
            .iter()
            .map(|arg| std::ffi::CString::new(arg.as_str()).unwrap())
            .collect();
        let env_c: Vec<std::ffi::CString> = env_pairs
            .iter()
            .map(|(key, value)| std::ffi::CString::new(format!("{key}={value}")).unwrap())
            .collect();
        let mut argv_ptrs: Vec<*const libc::c_char> =
            argv_c.iter().map(|arg| arg.as_ptr()).collect();
        let mut env_ptrs: Vec<*const libc::c_char> =
            env_c.iter().map(|entry| entry.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());
        env_ptrs.push(std::ptr::null());
        unsafe {
            libc::execve(path_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
            libc::abort();
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (path, argv, env_pairs);
        crate::fs::validate::throw_type_error_with_code(
            "process.execve() is unavailable on this platform",
            "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
        )
    }
}
