//! `node:util.getCallSites([frameCount][, options])`.
//!
//! Perry does not maintain V8-style JavaScript stack metadata yet, so the
//! returned records are synthetic. They match Node's public object shape and
//! argument validation, which is enough for consumers that probe the helper or
//! use it for bounded diagnostics without depending on exact source locations.

use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::url::create_string_f64;
use crate::value::JSValue;

const DEFAULT_FRAME_COUNT: i32 = 10;
const MAX_FRAME_COUNT: i64 = 200;

fn f64_from_js(value: JSValue) -> f64 {
    f64::from_bits(value.bits())
}

fn js_undefined() -> f64 {
    f64_from_js(JSValue::undefined())
}

fn key(bytes: &'static [u8]) -> *mut StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn pointer_value<T>(ptr: *const T) -> f64 {
    f64_from_js(JSValue::pointer(ptr as *const u8))
}

fn gc_type_of(value: f64) -> Option<u8> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null()
        || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000
        || !crate::object::is_valid_obj_ptr(ptr)
    {
        return None;
    }
    unsafe {
        let header = &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader);
        Some(header.obj_type)
    }
}

fn is_object_like_for_overload(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    jv.is_null()
        || matches!(
            gc_type_of(value),
            Some(crate::gc::GC_TYPE_OBJECT)
                | Some(crate::gc::GC_TYPE_ARRAY)
                | Some(crate::gc::GC_TYPE_MAP)
                | Some(crate::gc::GC_TYPE_SET)
                | Some(crate::gc::GC_TYPE_ERROR)
                | Some(crate::gc::GC_TYPE_DATE_CELL)
        )
}

fn throw_invalid_arg_type(arg_name: &str, expectation: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be {}. Received {}",
        arg_name,
        expectation,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_source_map(value: f64) -> ! {
    let message = format!(
        "The \"options.sourceMap\" property must be of type boolean. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn validate_options(options: f64) {
    let jv = JSValue::from_bits(options.to_bits());
    if jv.is_undefined() {
        return;
    }
    let Some(kind) = gc_type_of(options) else {
        throw_invalid_arg_type("options", "of type object", options);
    };
    if matches!(kind, crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_CLOSURE) {
        throw_invalid_arg_type("options", "of type object", options);
    }
    if kind != crate::gc::GC_TYPE_OBJECT {
        return;
    }

    let obj = jv.as_pointer::<crate::object::ObjectHeader>();
    let source_map = js_object_get_field_by_name_f64(obj, key(b"sourceMap"));
    let source_map_value = JSValue::from_bits(source_map.to_bits());
    if !source_map_value.is_undefined() && !source_map_value.is_bool() {
        throw_invalid_source_map(source_map);
    }
}

fn frame_count_value(frame_count: f64) -> i32 {
    crate::fs::validate::validate_int32(frame_count, "frameCount", 1, MAX_FRAME_COUNT);
    let jv = JSValue::from_bits(frame_count.to_bits());
    if jv.is_int32() {
        jv.as_int32()
    } else {
        jv.as_number() as i32
    }
}

fn resolve_args(frame_count_or_options: f64, options: f64) -> (i32, f64) {
    let first = JSValue::from_bits(frame_count_or_options.to_bits());
    let second = JSValue::from_bits(options.to_bits());

    if second.is_undefined()
        && (first.is_undefined() || is_object_like_for_overload(frame_count_or_options))
    {
        let resolved_options = if first.is_undefined() {
            js_undefined()
        } else {
            frame_count_or_options
        };
        validate_options(resolved_options);
        return (DEFAULT_FRAME_COUNT, resolved_options);
    }

    validate_options(options);
    if first.is_undefined() {
        (DEFAULT_FRAME_COUNT, options)
    } else {
        (frame_count_value(frame_count_or_options), options)
    }
}

fn set_string(obj: *mut crate::object::ObjectHeader, name: &'static [u8], value: &str) {
    js_object_set_field_by_name(obj, key(name), create_string_f64(value));
}

fn set_number(obj: *mut crate::object::ObjectHeader, name: &'static [u8], value: f64) {
    js_object_set_field_by_name(obj, key(name), value);
}

fn call_site_object(index: i32) -> *mut crate::object::ObjectHeader {
    let obj = js_object_alloc(0, 6);
    let line = 1.0 + index as f64;
    set_string(obj, b"functionName", "");
    set_string(obj, b"scriptId", "0");
    set_string(obj, b"scriptName", "");
    set_number(obj, b"lineNumber", line);
    set_number(obj, b"columnNumber", 1.0);
    set_number(obj, b"column", 1.0);
    obj
}

#[no_mangle]
pub extern "C" fn js_util_get_call_sites(frame_count_or_options: f64, options: f64) -> f64 {
    let (frame_count, _options) = resolve_args(frame_count_or_options, options);
    let mut out = crate::array::js_array_alloc(frame_count as u32);
    for i in 0..frame_count {
        out = crate::array::js_array_push_f64(out, pointer_value(call_site_object(i)));
    }
    pointer_value(out)
}
