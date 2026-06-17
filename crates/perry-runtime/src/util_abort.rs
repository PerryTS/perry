//! `node:util` AbortSignal helper surface.

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::string::js_string_from_bytes;
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_UNDEFINED};

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn string_value(bytes: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn set_field(obj: *mut crate::object::ObjectHeader, key: &[u8], value: f64) {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(obj, key_ptr, value);
}

fn type_error_value(message: &str, code: &'static str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    js_nanbox_pointer(err as i64)
}

fn invalid_signal_error(signal: f64) -> f64 {
    type_error_value(
        &format!(
            "The \"signal\" argument must be an instance of AbortSignal. Received {}",
            crate::fs::validate::describe_received(signal)
        ),
        "ERR_INVALID_ARG_TYPE",
    )
}

fn invalid_aborted_signal_error() -> f64 {
    type_error_value("signal is not of type AbortSignal.", "ERR_INVALID_ARG_TYPE")
}

fn invalid_resource_error(resource: f64) -> f64 {
    type_error_value(
        &format!(
            "The \"resource\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(resource)
        ),
        "ERR_INVALID_ARG_TYPE",
    )
}

fn rejected_promise(reason: f64) -> f64 {
    js_nanbox_pointer(crate::promise::js_promise_rejected(reason) as i64)
}

fn resolved_promise(value: f64) -> f64 {
    js_nanbox_pointer(crate::promise::js_promise_resolved(value) as i64)
}

fn is_object_resource(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_pointer()
}

fn abort_event(signal: f64) -> f64 {
    let event = js_object_alloc(0, 2);
    set_field(event, b"type", string_value(b"abort"));
    set_field(event, b"target", signal);
    js_nanbox_pointer(event as i64)
}

fn promise_from_capture(closure: *const ClosureHeader, index: u32) -> *mut crate::promise::Promise {
    let bits = js_closure_get_capture_ptr(closure, index) as u64;
    js_nanbox_get_pointer(f64::from_bits(bits)) as *mut crate::promise::Promise
}

extern "C" fn aborted_resolve_listener(closure: *const ClosureHeader) -> f64 {
    let promise = promise_from_capture(closure, 0);
    let signal = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    if !promise.is_null() {
        crate::promise::js_promise_schedule_resolve(promise, abort_event(signal));
    }
    undefined()
}

#[no_mangle]
pub extern "C" fn js_util_aborted(signal: f64, resource: f64) -> f64 {
    let signal_ptr = match crate::url::abort::abort_signal_ptr_from_value(signal) {
        Some(signal_ptr) => signal_ptr,
        None => return rejected_promise(invalid_aborted_signal_error()),
    };
    if !is_object_resource(resource) {
        return rejected_promise(invalid_resource_error(resource));
    }
    if crate::url::abort::js_abort_signal_is_aborted(signal_ptr) != 0 {
        return resolved_promise(undefined());
    }

    let promise = crate::promise::js_promise_new();
    let promise_value = js_nanbox_pointer(promise as i64);
    let listener_func = aborted_resolve_listener as *const u8;
    js_register_closure_arity(listener_func, 0);
    let listener = js_closure_alloc(listener_func, 2);
    js_closure_set_capture_ptr(listener, 0, promise_value.to_bits() as i64);
    js_closure_set_capture_ptr(listener, 1, signal.to_bits() as i64);
    crate::url::js_abort_signal_add_listener(
        signal_ptr,
        string_value(b"abort"),
        js_nanbox_pointer(listener as i64),
    );
    promise_value
}

#[no_mangle]
pub extern "C" fn js_util_transferable_abort_controller() -> f64 {
    js_nanbox_pointer(crate::url::js_abort_controller_new() as i64)
}

#[no_mangle]
pub extern "C" fn js_util_transferable_abort_signal(signal: f64) -> f64 {
    if !crate::url::abort::is_abort_signal_value(signal) {
        crate::exception::js_throw(invalid_signal_error(signal));
    }
    signal
}
