use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_get_capture_ptr,
    js_closure_set_capture_f64, js_closure_set_capture_ptr, ClosureHeader,
};

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

pub(super) fn destroy_stream(stream: f64, err: f64) {
    if has_truthy_hidden(stream, hidden_key(b"destroyed")) {
        return;
    }
    set_hidden_value(stream, hidden_key(b"destroyed"), f64::from_bits(TAG_TRUE));
    super::refresh_readable_aborted_flag(stream);
    let closure = js_closure_alloc(ns_destroy_error_microtask as *const u8, 2);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    js_closure_set_capture_f64(closure, 1, err);
    crate::builtins::js_queue_microtask(closure as i64);
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
    destroy_stream(stream, err);
    stream
}
