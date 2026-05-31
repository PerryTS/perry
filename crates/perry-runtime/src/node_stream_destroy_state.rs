use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::value::JSValue;

use super::{
    get_hidden_value, has_truthy_hidden, hidden_error_key, hidden_key, set_hidden_value,
    stream_value_from_handle, this_value, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED,
};

pub(super) extern "C" fn ns_destroy_error_microtask(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let err = js_closure_get_capture_f64(closure, 1);
    let bits = err.to_bits();
    if bits != TAG_UNDEFINED && bits != TAG_NULL {
        set_hidden_value(stream, hidden_error_key(), err);
        let error = super::string_value(b"error");
        if super::event_emitter::stream_listener_count_for_event(stream, error) > 0 {
            let _ = super::event_emitter::emit_stream_event(stream, error, &[err]);
        }
    }
    super::mark_stream_closed(stream);
    let _ = super::event_emitter::emit_stream_event(stream, super::string_value(b"close"), &[]);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_destroy_option_done(closure: *const ClosureHeader, err: f64) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let original_err = js_closure_get_capture_f64(closure, 1);
    let destroy_err = if err.to_bits() == TAG_UNDEFINED || err.to_bits() == TAG_NULL {
        original_err
    } else {
        err
    };
    queue_destroy_events(stream, destroy_err);
    f64::from_bits(TAG_UNDEFINED)
}

fn queue_destroy_events(stream: f64, err: f64) {
    let closure = js_closure_alloc(ns_destroy_error_microtask as *const u8, 2);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    js_closure_set_capture_f64(closure, 1, err);
    crate::builtins::js_queue_microtask(closure as i64);
}

pub(super) fn destroy_stream(stream: f64, err: f64) {
    if has_truthy_hidden(stream, hidden_key(b"destroyed")) {
        return;
    }
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_TRUE));
    super::refresh_readable_aborted_flag(stream);
    if let Some(destroy) = get_hidden_value(stream, hidden_key(b"__perryStreamDestroy")) {
        if super::is_callable_value(destroy) {
            crate::closure::js_register_closure_arity(ns_destroy_option_done as *const u8, 1);
            let cb = js_closure_alloc(ns_destroy_option_done as *const u8, 2);
            js_closure_set_capture_f64(cb, 0, stream);
            js_closure_set_capture_f64(cb, 1, err);
            let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
            let destroy_arg = if err.to_bits() == TAG_UNDEFINED {
                f64::from_bits(TAG_NULL)
            } else {
                err
            };
            let args = [destroy_arg, cb_value];
            let prev_this = crate::object::js_implicit_this_set(stream);
            unsafe {
                let _ = crate::closure::js_native_call_value(destroy, args.as_ptr(), args.len());
            }
            crate::object::js_implicit_this_set(prev_this);
            return;
        }
    }
    queue_destroy_events(stream, err);
}

pub(super) extern "C" fn ns_destroy1(closure: *const ClosureHeader, err: f64) -> f64 {
    let stream = this_value(closure);
    destroy_stream(stream, err);
    stream
}

/// `stream.destroyed` property getter on typed stream instances.
#[no_mangle]
pub extern "C" fn js_node_stream_method_destroyed(stream_handle: i64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    get_hidden_value(stream, hidden_key(b"destroyed")).unwrap_or(f64::from_bits(TAG_FALSE))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_destroy(stream_handle: i64, err: f64) -> f64 {
    let stream = stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_destroy_value(stream) {
        return result;
    }
    destroy_stream(stream, err);
    stream
}
