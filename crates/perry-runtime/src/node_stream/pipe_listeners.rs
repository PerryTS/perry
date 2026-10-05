//! The `pipe()` destination listeners: closures capturing (src, dest, and their siblings) that unpipe, forward errors and finish the destination.

use super::*;

// Native destinations own their emitter. Subscribe through their public
// methods so write(false) can resume the source when their drain fires.
pub(super) fn destination_listener(dest: f64, event: &'static [u8], listener: f64, remove: bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let dest = scope.root_nanbox_f64(dest);
    let listener = scope.root_nanbox_f64(listener);
    let event = scope.root_nanbox_f64(literal_string_value(event));
    if is_classic_stream_instance_value(dest.get_nanbox_f64()) {
        if remove {
            remove_stream_listener_for_event(
                dest.get_nanbox_f64(),
                event.get_nanbox_f64(),
                listener.get_nanbox_f64(),
            );
        } else {
            add_stream_listener_for_event(
                dest.get_nanbox_f64(),
                event.get_nanbox_f64(),
                listener.get_nanbox_f64(),
            );
        }
    } else {
        let method = if remove {
            b"removeListener".as_slice()
        } else {
            b"on".as_slice()
        };
        let args = [event.get_nanbox_f64(), listener.get_nanbox_f64()];
        unsafe {
            crate::object::js_native_call_method(
                dest.get_nanbox_f64(),
                method.as_ptr().cast(),
                method.len(),
                args.as_ptr(),
                args.len(),
            );
        }
    }
}

fn destination_error_listener_count(dest: f64) -> usize {
    let scope = crate::gc::RuntimeHandleScope::new();
    let dest = scope.root_nanbox_f64(dest);
    let event = scope.root_nanbox_f64(literal_string_value(b"error"));
    if is_classic_stream_instance_value(dest.get_nanbox_f64()) {
        stream_listener_count_for_event(dest.get_nanbox_f64(), event.get_nanbox_f64())
    } else {
        let args = [event.get_nanbox_f64()];
        let count = unsafe {
            crate::object::js_native_call_method(
                dest.get_nanbox_f64(),
                b"listenerCount".as_ptr().cast(),
                13,
                args.as_ptr(),
                args.len(),
            )
        };
        count as usize
    }
}

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
    let scope = crate::gc::RuntimeHandleScope::new();
    let dest = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let listeners: Vec<_> = (2..6)
        .map(|i| scope.root_nanbox_f64(js_closure_get_capture_f64(closure, i)))
        .collect();
    for (event, listener) in [b"unpipe".as_slice(), b"error", b"close", b"finish"]
        .into_iter()
        .zip(&listeners)
    {
        destination_listener(
            dest.get_nanbox_f64(),
            event,
            listener.get_nanbox_f64(),
            true,
        );
    }
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_const_ptr(closure);
    let src = scope.root_nanbox_f64(js_closure_get_capture_f64(closure.get_raw_const_ptr(), 0));
    let dest = scope.root_nanbox_f64(js_closure_get_capture_f64(closure.get_raw_const_ptr(), 1));
    let err = scope.root_nanbox_f64(err);
    let _ = unpipe_destination(src.get_nanbox_f64(), dest.get_nanbox_f64());
    // Foreign destinations own their unpipe event; cleanup is idempotent.
    cleanup_pipe_listeners_from_closure(closure.get_raw_const_ptr());
    if destination_error_listener_count(dest.get_nanbox_f64()) == 0 {
        crate::exception::js_throw(err.get_nanbox_f64());
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_const_ptr(closure);
    let src = scope.root_nanbox_f64(js_closure_get_capture_f64(closure.get_raw_const_ptr(), 0));
    let dest = scope.root_nanbox_f64(js_closure_get_capture_f64(closure.get_raw_const_ptr(), 1));
    let _ = unpipe_destination(src.get_nanbox_f64(), dest.get_nanbox_f64());
    // Foreign destinations own their unpipe event; cleanup is idempotent.
    cleanup_pipe_listeners_from_closure(closure.get_raw_const_ptr());
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn pipe_finish_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_const_ptr(closure);
    let src = scope.root_nanbox_f64(js_closure_get_capture_f64(closure.get_raw_const_ptr(), 0));
    let dest = scope.root_nanbox_f64(js_closure_get_capture_f64(closure.get_raw_const_ptr(), 1));
    let _ = unpipe_destination(src.get_nanbox_f64(), dest.get_nanbox_f64());
    // Foreign destinations own their unpipe event; cleanup is idempotent.
    cleanup_pipe_listeners_from_closure(closure.get_raw_const_ptr());
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn pipe_drain_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let dest = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let listener = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 2));
    destination_listener(
        dest.get_nanbox_f64(),
        b"drain",
        listener.get_nanbox_f64(),
        true,
    );
    if pipe_destination_contains(src.get_nanbox_f64(), dest.get_nanbox_f64())
        && !stream_destroyed(src.get_nanbox_f64())
    {
        if stream_hidden_ended(src.get_nanbox_f64())
            && pending_readable_chunk_count(src.get_nanbox_f64()) == 0
        {
            set_readable_flowing(src.get_nanbox_f64(), f64::from_bits(TAG_TRUE));
            schedule_readable_end(src.get_nanbox_f64());
            return f64::from_bits(TAG_UNDEFINED);
        }
        let _ = resume_readable_stream_from_pipe(src.get_nanbox_f64());
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
