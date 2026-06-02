//! `mkdtempDisposableSync` / `mkdtempDisposable` (#3814) — the explicit-
//! resource-management `Symbol.dispose`/`Symbol.asyncDispose` temp-dir wrapper
//! objects, split out of `fs/mod.rs` to keep it under the 2k limit.
//! `use super::*` pulls in `mkdtemp_bytes_result`, `fs_encoding_option`, the
//! closure plumbing, and the promise helpers.

use super::*;

fn mkdtemp_disposable_buffer_encoding_error() -> ! {
    validate::throw_type_error_with_code(
        "The \"paths[1]\" argument must be of type string. Received an instance of Buffer",
        "ERR_INVALID_ARG_TYPE",
    )
}

fn remove_temp_dir_result(path_value: f64) -> Result<(), f64> {
    unsafe {
        let path = match decode_path_value_named(path_value, "path") {
            Some(path) => path,
            None => return Ok(()),
        };
        match fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(build_fs_error_value(&err, "rm", &path)),
        }
    }
}

fn resolved_promise(value: f64) -> f64 {
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_resolve(promise, value);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

fn rejected_promise(reason: f64) -> f64 {
    let promise = crate::promise::js_promise_rejected(reason);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

extern "C" fn mkdtemp_disposable_remove_impl(closure: *const ClosureHeader) -> f64 {
    let path_value = crate::closure::js_closure_get_capture_f64(closure, 0);
    match remove_temp_dir_result(path_value) {
        Ok(()) => f64::from_bits(crate::value::TAG_UNDEFINED),
        Err(err) => crate::exception::js_throw(err),
    }
}

extern "C" fn mkdtemp_disposable_async_remove_impl(closure: *const ClosureHeader) -> f64 {
    let path_value = crate::closure::js_closure_get_capture_f64(closure, 0);
    match remove_temp_dir_result(path_value) {
        Ok(()) => resolved_promise(f64::from_bits(crate::value::TAG_UNDEFINED)),
        Err(err) => rejected_promise(err),
    }
}

fn mkdtemp_disposable_method(
    path_value: f64,
    func: extern "C" fn(*const ClosureHeader) -> f64,
) -> f64 {
    crate::closure::js_register_closure_arity(func as *const u8, 0);
    let closure = crate::closure::js_closure_alloc(func as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, path_value);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

fn set_object_field(obj: *mut crate::object::ObjectHeader, name: &'static [u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

fn build_mkdtemp_disposable_object(
    actual_path_bytes: Vec<u8>,
    options_value: f64,
    async_remove: bool,
) -> f64 {
    let actual_path = encoded_string_ptr(&actual_path_bytes, "utf8");
    let actual_path_value = f64::from_bits(crate::value::JSValue::string_ptr(actual_path).bits());
    let display_encoding = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let display_path = encoded_string_ptr(&actual_path_bytes, &display_encoding);
    let display_path_value = f64::from_bits(crate::value::JSValue::string_ptr(display_path).bits());
    let remove_func = if async_remove {
        mkdtemp_disposable_async_remove_impl as extern "C" fn(*const ClosureHeader) -> f64
    } else {
        mkdtemp_disposable_remove_impl as extern "C" fn(*const ClosureHeader) -> f64
    };
    let remove_method = mkdtemp_disposable_method(actual_path_value, remove_func);
    let symbol_method = mkdtemp_disposable_method(actual_path_value, remove_func);
    let obj = crate::object::js_object_alloc(0, 3);
    set_object_field(obj, b"path", display_path_value);
    set_object_field(obj, b"remove", remove_method);
    let obj_value = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
    let symbol_name = if async_remove {
        "asyncDispose"
    } else {
        "dispose"
    };
    let symbol = crate::symbol::well_known_symbol(symbol_name);
    if !symbol.is_null() {
        let symbol_value =
            f64::from_bits(crate::value::JSValue::pointer(symbol as *const u8).bits());
        unsafe {
            crate::symbol::js_object_set_symbol_property(obj_value, symbol_value, symbol_method);
        }
    }
    obj_value
}

pub(crate) fn js_fs_mkdtemp_disposable_object(
    prefix_value: f64,
    options_value: f64,
    async_remove: bool,
) -> f64 {
    validate::validate_path("prefix", prefix_value);
    validate::validate_string_or_object_options("options", options_value);
    if fs_encoding_option(options_value).as_deref() == Some("buffer") {
        mkdtemp_disposable_buffer_encoding_error();
    }
    let bytes = match mkdtemp_bytes_result(prefix_value) {
        Ok(bytes) => bytes,
        Err(err) => crate::exception::js_throw(err),
    };
    build_mkdtemp_disposable_object(bytes, options_value, async_remove)
}
