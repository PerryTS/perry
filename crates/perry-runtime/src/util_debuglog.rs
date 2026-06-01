//! `util.debuglog(section[, callback])` and `util.debug(section)`.
//!
//! Node snapshots `NODE_DEBUG` at process start; Perry reads the host
//! environment when the logger is created. The returned logger is a rest
//! closure so calls forward through `util.format` before writing to stderr.

use std::cell::Cell;

use crate::array::{js_array_length, ArrayHeader};
use crate::closure::{
    js_closure_alloc, js_closure_call1, js_closure_get_capture_f64, js_closure_set_capture_f64,
    js_register_closure_rest, ClosureHeader,
};
use crate::string::StringHeader;
use crate::value::{
    JSValue, POINTER_MASK, POINTER_TAG, TAG_FALSE, TAG_MASK, TAG_TRUE, TAG_UNDEFINED,
};

const TAG_UNDEFINED_F64: f64 = f64::from_bits(TAG_UNDEFINED);
const TAG_TRUE_F64: f64 = f64::from_bits(TAG_TRUE);
const TAG_FALSE_F64: f64 = f64::from_bits(TAG_FALSE);

fn boxed_string(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn nanbox_closure(closure: *const ClosureHeader) -> f64 {
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn is_callable_closure(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits & TAG_MASK) != POINTER_TAG {
        return false;
    }
    crate::closure::is_closure_ptr((bits & POINTER_MASK) as usize)
}

fn closure_ptr(value: f64) -> Option<*const ClosureHeader> {
    if !is_callable_closure(value) {
        return None;
    }
    Some((value.to_bits() & POINTER_MASK) as *const ClosureHeader)
}

fn string_header_content(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn string_from_value(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value);
    string_header_content(ptr)
}

fn throw_to_upper_case_nullish() -> ! {
    let message = "String.prototype.toUpperCase called on null or undefined";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn register_thunks_once() {
    thread_local! {
        static REGISTERED: Cell<bool> = const { Cell::new(false) };
    }
    REGISTERED.with(|flag| {
        if flag.get() {
            return;
        }
        js_register_closure_rest(logger_thunk as *const u8, 0);
        flag.set(true);
    });
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == text;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0usize;
    let mut start_part = 0usize;

    if !pattern.starts_with('*') {
        let first = parts.first().copied().unwrap_or("");
        if !text.starts_with(first) {
            return false;
        }
        pos = first.len();
        start_part = 1;
    }

    let end_part = if pattern.ends_with('*') {
        parts.len()
    } else {
        parts.len().saturating_sub(1)
    };

    for part in &parts[start_part..end_part] {
        if part.is_empty() {
            continue;
        }
        let Some(found) = text[pos..].find(part) else {
            return false;
        };
        pos += found + part.len();
    }

    if !pattern.ends_with('*') {
        let last = parts.last().copied().unwrap_or("");
        if !text[pos..].ends_with(last) {
            return false;
        }
    }

    true
}

fn node_debug_enabled(section_upper: &str) -> bool {
    let Ok(value) = std::env::var("NODE_DEBUG") else {
        return false;
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| wildcard_match(&part.to_ascii_uppercase(), section_upper))
}

fn rest_array_ptr(rest_value: f64) -> *const ArrayHeader {
    let bits = rest_value.to_bits();
    if (bits & TAG_MASK) == POINTER_TAG {
        (bits & POINTER_MASK) as *const ArrayHeader
    } else {
        std::ptr::null()
    }
}

fn call_enabled_callback_once(scope: &crate::gc::RuntimeHandleScope, closure: *mut ClosureHeader) {
    if closure.is_null() {
        return;
    }

    let called = js_closure_get_capture_f64(closure, 2);
    if called.to_bits() == TAG_TRUE {
        return;
    }
    js_closure_set_capture_f64(closure, 2, TAG_TRUE_F64);

    let callback = js_closure_get_capture_f64(closure, 1);
    let Some(callback_ptr) = closure_ptr(callback) else {
        return;
    };

    let logger_value = nanbox_closure(closure);
    let logger_handle = scope.root_nanbox_f64(logger_value);
    js_closure_call1(callback_ptr, logger_handle.get_nanbox_f64());
}

fn write_stderr_line(line: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stderr = crate::os::js_process_stderr();
    let stderr_handle = scope.root_nanbox_f64(stderr);
    let line_value = boxed_string(line);
    let line_handle = scope.root_nanbox_f64(line_value);

    let stderr_js = JSValue::from_bits(stderr_handle.get_nanbox_f64().to_bits());
    if stderr_js.is_pointer() {
        let stderr_obj = stderr_js.as_pointer::<crate::object::ObjectHeader>();
        let key = crate::string::js_string_from_bytes(b"write".as_ptr(), 5);
        let key_handle = scope.root_string_ptr(key);
        let write_value = crate::object::js_object_get_field_by_name(
            stderr_obj,
            key_handle.get_raw_const_ptr::<StringHeader>(),
        );
        let write_f64 = f64::from_bits(write_value.bits());
        if let Some(write_ptr) = closure_ptr(write_f64) {
            js_closure_call1(write_ptr, line_handle.get_nanbox_f64());
            return;
        }
    }

    eprint!("{line}");
}

extern "C" fn logger_thunk(closure: *const ClosureHeader, rest_value: f64) -> f64 {
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let closure_handle = scope.root_raw_mut_ptr(closure as *mut ClosureHeader);
    let rest_handle = scope.root_nanbox_f64(rest_value);

    let enabled = js_closure_get_capture_f64(closure_handle.get_raw_const_ptr(), 3);
    if enabled.to_bits() != TAG_TRUE {
        return TAG_UNDEFINED_F64;
    }

    call_enabled_callback_once(&scope, closure_handle.get_raw_mut_ptr());

    let section = string_from_value(js_closure_get_capture_f64(
        closure_handle.get_raw_const_ptr(),
        0,
    ));
    let rest_ptr = rest_array_ptr(rest_handle.get_nanbox_f64());
    let formatted = if rest_ptr.is_null() || js_array_length(rest_ptr) == 0 {
        String::new()
    } else {
        let value = crate::builtins::js_util_format(rest_ptr);
        string_from_value(value)
    };
    let line = format!("{} {}: {}\n", section, std::process::id(), formatted);
    write_stderr_line(&line);

    TAG_UNDEFINED_F64
}

/// `util.debuglog(section[, callback])` -> logger function.
#[no_mangle]
pub extern "C" fn js_util_debuglog(section: f64, callback: f64) -> f64 {
    let section_js = JSValue::from_bits(section.to_bits());
    if section_js.is_undefined() || section_js.is_null() {
        throw_to_upper_case_nullish();
    }

    register_thunks_once();

    let section_upper = string_from_value(section).to_uppercase();
    let enabled = node_debug_enabled(&section_upper);
    let section_value = boxed_string(&section_upper);

    let scope = crate::gc::RuntimeHandleScope::new();
    let section_handle = scope.root_nanbox_f64(section_value);
    let callback_handle = scope.root_nanbox_f64(callback);
    let closure = js_closure_alloc(logger_thunk as *const u8, 4);
    if closure.is_null() {
        return TAG_UNDEFINED_F64;
    }
    let closure_handle = scope.root_raw_mut_ptr(closure);

    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        0,
        section_handle.get_nanbox_f64(),
    );
    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        1,
        callback_handle.get_nanbox_f64(),
    );
    js_closure_set_capture_f64(closure_handle.get_raw_mut_ptr(), 2, TAG_FALSE_F64);
    js_closure_set_capture_f64(
        closure_handle.get_raw_mut_ptr(),
        3,
        if enabled { TAG_TRUE_F64 } else { TAG_FALSE_F64 },
    );

    crate::object::set_bound_native_closure_name(closure_handle.get_raw_mut_ptr(), "logger");
    crate::object::set_property_attrs(
        closure_handle.get_raw_const_ptr::<ClosureHeader>() as usize,
        "name".to_string(),
        crate::object::PropertyAttrs::new(false, false, true),
    );
    crate::closure::closure_set_dynamic_prop(
        closure_handle.get_raw_const_ptr::<ClosureHeader>() as usize,
        "enabled",
        if enabled { TAG_TRUE_F64 } else { TAG_FALSE_F64 },
    );

    nanbox_closure(closure_handle.get_raw_const_ptr::<ClosureHeader>())
}
