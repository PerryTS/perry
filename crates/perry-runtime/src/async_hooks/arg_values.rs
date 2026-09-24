//! Argument conversion and Node-shaped error rendering for `node:async_hooks`
//! (split out of `async_hooks.rs` for the 2000-line cap, #10952).
//!
//! Everything here reads a JS value and either converts it (async ids, trigger
//! ids, strings) or renders it the way Node's `ERR_INVALID_ARG_TYPE` /
//! `Function.prototype.apply` messages do. None of it touches the hook or
//! resource registries, and none of it holds a GC root across an allocation.

use super::{execution_async_id_u64, object_field};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

#[inline]
pub(super) fn async_id_to_js_number(id: u64) -> f64 {
    if id == u64::MAX {
        -1.0
    } else {
        id as f64
    }
}

fn string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = crate::string::string_data(ptr);
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

pub(super) fn js_string_value_to_string(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    string_header_to_string(ptr)
}

fn symbol_to_string(value: f64) -> String {
    if unsafe { crate::symbol::js_is_symbol(value) == 0 } {
        return "Symbol()".to_string();
    }
    let ptr = unsafe { crate::symbol::js_symbol_to_string(value) } as *const StringHeader;
    string_header_to_string(ptr)
}

fn value_is_array(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    // The TRACKED reader: `value` is arbitrary user input, and a POINTER-tagged
    // value can still name a header-less native allocation (the held subclass
    // `__perryAsyncResourceBacking` field). It proves the allocator owns the
    // address before reading the header in front of it, where the hand-rolled
    // `< GC_HEADER_SIZE + 0x1000` floor this replaced rejected neither handle
    // bands nor foreign allocations.
    let addr = jv.as_pointer::<u8>() as usize;
    unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }
        .is_some_and(|header| unsafe { header.as_ref() }.obj_type == crate::gc::GC_TYPE_ARRAY)
}

pub(super) fn is_callable_value(value: f64) -> bool {
    !crate::fs::extract_closure_ptr(value).is_null()
}

fn describe_received_async_hooks(value: f64) -> String {
    if is_callable_value(value) {
        return "function ".to_string();
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return format!("type symbol ({})", symbol_to_string(value));
    }
    crate::fs::validate::describe_received(value)
}

pub(super) fn require_string_arg(arg_name: &str, value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        let message = format!(
            "The \"{}\" argument must be of type string. Received {}",
            arg_name,
            describe_received_async_hooks(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    js_string_value_to_string(value)
}

fn format_js_number_for_error(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value == f64::INFINITY {
        "Infinity".to_string()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

const MAX_SAFE_JS_INTEGER: f64 = 9_007_199_254_740_991.0;

fn trigger_async_id_value(value: f64) -> Option<u64> {
    let jv = JSValue::from_bits(value.to_bits());
    let id = if jv.is_int32() {
        jv.as_int32() as f64
    } else if jv.is_number() {
        jv.as_number()
    } else {
        return None;
    };

    if !id.is_finite() || id.fract() != 0.0 || !(-1.0..=MAX_SAFE_JS_INTEGER).contains(&id) {
        return None;
    }
    if id == -1.0 {
        Some(u64::MAX)
    } else {
        Some(id as u64)
    }
}

fn render_invalid_trigger_async_id(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return jv.as_bool().to_string();
    }
    if jv.is_any_string() {
        return js_string_value_to_string(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return symbol_to_string(value);
    }
    if jv.is_int32() {
        return jv.as_int32().to_string();
    }
    if jv.is_number() {
        return format_js_number_for_error(jv.as_number());
    }
    if value_is_array(value) {
        return "[]".to_string();
    }
    if jv.is_pointer() {
        return "{}".to_string();
    }
    "undefined".to_string()
}

fn trigger_async_id_or_throw(value: f64) -> u64 {
    if let Some(id) = trigger_async_id_value(value) {
        return id;
    }
    let message = format!(
        "Invalid triggerAsyncId value: {}",
        render_invalid_trigger_async_id(value)
    );
    crate::fs::validate::throw_range_error_named(&message, "ERR_INVALID_ASYNC_ID")
}

fn throw_null_trigger_async_id_options() -> ! {
    let message = b"Cannot read properties of null (reading 'triggerAsyncId')";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(super) fn trigger_id_from_options(options: f64) -> u64 {
    let options_value = JSValue::from_bits(options.to_bits());
    if options_value.is_undefined() {
        return execution_async_id_u64();
    }
    if options_value.is_int32() || options_value.is_number() {
        return trigger_async_id_or_throw(options);
    }
    if options_value.is_null() {
        throw_null_trigger_async_id_options();
    }

    // Node's constructor first validates the option and then consumes it,
    // making an accessor observable twice. Preserve that exact ordering; the
    // `requireManualDestroy` option is read after both trigger-id reads.
    let first_trigger_value = object_field(options, b"triggerAsyncId");
    if !JSValue::from_bits(first_trigger_value.to_bits()).is_undefined() {
        let _ = trigger_async_id_or_throw(first_trigger_value);
    }
    let trigger_value = object_field(options, b"triggerAsyncId");
    let trigger_value_kind = JSValue::from_bits(trigger_value.to_bits());
    let trigger_id = if trigger_value_kind.is_undefined() {
        execution_async_id_u64()
    } else {
        trigger_async_id_or_throw(trigger_value)
    };
    let _ = object_field(options, b"requireManualDestroy");
    trigger_id
}

fn render_apply_value(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return jv.as_bool().to_string();
    }
    if jv.is_any_string() {
        return js_string_value_to_string(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        return symbol_to_string(value);
    }
    if jv.is_int32() {
        return jv.as_int32().to_string();
    }
    if jv.is_number() {
        return format_js_number_for_error(jv.as_number());
    }
    if value_is_array(value) {
        return "[object Array]".to_string();
    }
    if jv.is_pointer() {
        return "#<Object>".to_string();
    }
    "undefined".to_string()
}

fn describe_apply_type(value: f64) -> &'static str {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        "undefined"
    } else if jv.is_null() {
        "null"
    } else if jv.is_bool() {
        "a boolean"
    } else if jv.is_any_string() {
        "a string"
    } else if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        "a symbol"
    } else if jv.is_int32() || jv.is_number() {
        "a number"
    } else {
        "an object"
    }
}

pub(super) fn throw_apply_not_function(value: f64) -> ! {
    let message = format!(
        "Function.prototype.apply was called on {}, which is {} and not a function",
        render_apply_value(value),
        describe_apply_type(value)
    );
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(super) fn validate_bind_callback(value: f64) {
    if is_callable_value(value) {
        return;
    }
    let message = format!(
        "The \"fn\" argument must be of type function. Received {}",
        describe_received_async_hooks(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}
