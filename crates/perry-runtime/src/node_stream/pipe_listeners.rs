//! The `pipe()` destination listeners: closures capturing (src, dest, and their siblings) that unpipe, forward errors and finish the destination.

use super::*;

pub(super) fn pipe_listener_value(listener: *const ClosureHeader) -> f64 {
    box_pointer(listener as *const u8)
}

pub(super) fn set_pipe_listener_captures(
    listener: *mut ClosureHeader,
    src: f64,
    dest: f64,
    unpipe: f64,
    error: f64,
    close: f64,
    finish: f64,
) {
    js_closure_set_capture_f64(listener, 0, src);
    js_closure_set_capture_f64(listener, 1, dest);
    js_closure_set_capture_f64(listener, 2, unpipe);
    js_closure_set_capture_f64(listener, 3, error);
    js_closure_set_capture_f64(listener, 4, close);
    js_closure_set_capture_f64(listener, 5, finish);
}

fn cleanup_pipe_listeners_from_closure(closure: *const ClosureHeader) {
    if closure.is_null() {
        return;
    }
    let dest = js_closure_get_capture_f64(closure, 1);
    let unpipe = js_closure_get_capture_f64(closure, 2);
    let error = js_closure_get_capture_f64(closure, 3);
    let close = js_closure_get_capture_f64(closure, 4);
    let finish = js_closure_get_capture_f64(closure, 5);
    let _ = remove_stream_listener_for_event(dest, literal_string_value(b"unpipe"), unpipe);
    let _ = remove_stream_listener_for_event(dest, literal_string_value(b"error"), error);
    let _ = remove_stream_listener_for_event(dest, literal_string_value(b"close"), close);
    let _ = remove_stream_listener_for_event(dest, literal_string_value(b"finish"), finish);
}

pub(super) extern "C" fn pipe_unpipe_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    src: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let expected_src = js_closure_get_capture_f64(closure, 0);
    if src.to_bits() == expected_src.to_bits() {
        cleanup_pipe_listeners_from_closure(closure);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn pipe_error_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let src = js_closure_get_capture_f64(closure, 0);
    let dest = js_closure_get_capture_f64(closure, 1);
    if !unpipe_destination(src, dest) {
        cleanup_pipe_listeners_from_closure(closure);
    }
    if stream_listener_count_for_event(dest, literal_string_value(b"error")) == 0 {
        crate::exception::js_throw(err);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn pipe_close_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let src = js_closure_get_capture_f64(closure, 0);
    let dest = js_closure_get_capture_f64(closure, 1);
    if !unpipe_destination(src, dest) {
        cleanup_pipe_listeners_from_closure(closure);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn pipe_finish_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let src = js_closure_get_capture_f64(closure, 0);
    let dest = js_closure_get_capture_f64(closure, 1);
    if !unpipe_destination(src, dest) {
        cleanup_pipe_listeners_from_closure(closure);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn pipe_drain_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let src = js_closure_get_capture_f64(closure, 0);
    let dest = js_closure_get_capture_f64(closure, 1);
    let listener = js_closure_get_capture_f64(closure, 2);
    let _ = remove_stream_listener_for_event(dest, literal_string_value(b"drain"), listener);
    if pipe_destination_contains(src, dest) && !stream_destroyed(src) {
        if stream_hidden_ended(src) && pending_readable_chunk_count(src) == 0 {
            set_readable_flowing(src, f64::from_bits(TAG_TRUE));
            schedule_readable_end(src);
            return f64::from_bits(TAG_UNDEFINED);
        }
        let _ = resume_readable_stream_from_pipe(src);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn finish_pipe_destination(dest: f64) {
    if stream_destroyed(dest) || has_truthy_hidden(dest, hidden_finish_emitted_key()) {
        return;
    }
    if writable_length(dest) > 0.0 {
        set_hidden_value(
            dest,
            hidden_stream_pipe_end_pending_key(),
            f64::from_bits(TAG_TRUE),
        );
    } else {
        set_hidden_value(
            dest,
            hidden_stream_pipe_end_pending_key(),
            f64::from_bits(TAG_FALSE),
        );
        if !finish_transform_stream(dest, None) {
            finish_stream(dest, None);
        }
    }
}

pub(super) extern "C" fn pipe_finish_destination_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    finish_pipe_destination(js_closure_get_capture_f64(closure, 0));
    f64::from_bits(TAG_UNDEFINED)
}
