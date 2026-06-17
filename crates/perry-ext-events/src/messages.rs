//! `ERR_INVALID_ARG_TYPE` message builders and the `describe_received`
//! value-formatter shared by the EventEmitter / EventTarget validation paths.
//! Extracted from `lib.rs` to keep that file under the file-size gate.

use super::*;

fn js_bool_to_string(value: JsValue) -> &'static str {
    if value.to_bool() {
        "true"
    } else {
        "false"
    }
}

pub(super) fn describe_received(value: f64) -> String {
    let jsval = JsValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        return "undefined".to_string();
    }
    if jsval.is_null() {
        return "null".to_string();
    }
    if jsval.is_bool() {
        return format!("type boolean ({})", js_bool_to_string(jsval));
    }
    if jsval.is_number() {
        return format!("type number ({})", jsval.to_number());
    }
    if jsval.is_string() {
        let text = unsafe { read_string(JsString::from_raw(jsval.as_string_ptr())) };
        return match text {
            Some(text) => format!("type string ('{text}')"),
            None => "type string".to_string(),
        };
    }
    if jsval.is_pointer() {
        if unsafe { js_array_is_array(value).to_bits() == TAG_TRUE_F64_BITS } {
            return "an instance of Array".to_string();
        }
        return "an instance of Object".to_string();
    }
    "unknown".to_string()
}

pub(super) fn invalid_instance_arg_message(name: &str, expected: &str, value: f64) -> String {
    format!(
        "The \"{name}\" argument must be an instance of {expected}. Received {}",
        describe_received(value)
    )
}

pub(super) fn invalid_instance_property_message(name: &str, expected: &str, value: f64) -> String {
    format!(
        "The \"{name}\" property must be an instance of {expected}. Received {}",
        describe_received(value)
    )
}

pub(super) fn invalid_type_arg_message(name: &str, expected: &str, value: f64) -> String {
    format!(
        "The \"{name}\" argument must be of type {expected}. Received {}",
        describe_received(value)
    )
}

pub(super) fn invalid_arg_type_error(message: &str) -> f64 {
    f64::from_bits(
        error_value_with_code(message, "ERR_INVALID_ARG_TYPE", ErrorKind::TypeError).bits(),
    )
}

pub(super) fn throw_invalid_arg_type(message: &str) -> ! {
    throw_with_code(message, "ERR_INVALID_ARG_TYPE", ErrorKind::TypeError)
}

pub(super) fn throw_invalid_emitter(value: f64) -> ! {
    throw_invalid_arg_type(&invalid_instance_arg_message(
        "emitter",
        "EventEmitter",
        value,
    ))
}
