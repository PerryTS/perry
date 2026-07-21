//! Module-level `events.once(...)` helpers and its listener closures.
//!
//! Moved verbatim from the `events.rs` trunk during the file split.

use super::*;

use perry_runtime::{
    js_array_alloc, js_array_length, js_array_push_f64, js_nanbox_get_pointer, js_nanbox_pointer,
    js_nanbox_string, js_promise_new, js_promise_reject, js_promise_resolve, js_string_from_bytes,
    ArrayHeader, ClosureHeader, JSValue, ObjectHeader, Promise, StringHeader,
};

use crate::common::{get_handle_mut, Handle};

extern "C" fn events_once_abort_listener(closure: *const ClosureHeader) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
    let promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;

    let pending = get_handle_mut::<EventEmitterHandle>(handle)
        .and_then(|emitter| remove_pending_once_promise(emitter, promise));
    if let Some(pending) = pending {
        unsafe {
            cleanup_pending_abort_listener(&pending);
            if !pending.promise.is_null() {
                js_promise_reject(pending.promise, perry_runtime::url::js_abort_error_value());
            }
        }
    }

    undefined_value()
}

extern "C" fn events_once_stream_resolve_listener(closure: *const ClosureHeader, rest: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let handle = js_closure_get_capture_ptr(closure, 1) as Handle;
    let error_listener = js_closure_get_capture_ptr(closure, 2);
    let error_event_ptr = js_closure_get_capture_ptr(closure, 3);
    if promise.is_null() {
        return undefined_value();
    }
    if handle != 0 && error_listener != 0 && error_event_ptr != 0 {
        let error_event = js_nanbox_string(error_event_ptr);
        let error_listener_value = js_nanbox_pointer(error_listener);
        let _ = perry_runtime::node_stream::js_node_stream_method_remove_listener(
            handle,
            error_event,
            error_listener_value,
        );
    }
    js_promise_resolve(promise, rest_array_or_empty(rest));
    undefined_value()
}

extern "C" fn events_once_stream_reject_listener(closure: *const ClosureHeader, rest: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let handle = js_closure_get_capture_ptr(closure, 1) as Handle;
    let event_name_ptr = js_closure_get_capture_ptr(closure, 2);
    let resolve_listener = js_closure_get_capture_ptr(closure, 3);
    if handle != 0 && event_name_ptr != 0 && resolve_listener != 0 {
        let event = js_nanbox_string(event_name_ptr);
        let resolve_listener_value = js_nanbox_pointer(resolve_listener);
        let _ = perry_runtime::node_stream::js_node_stream_method_remove_listener(
            handle,
            event,
            resolve_listener_value,
        );
    }
    if !promise.is_null() {
        js_promise_reject(promise, first_rest_arg_or_undefined(rest));
    }
    undefined_value()
}

fn rest_array_or_empty(rest: f64) -> f64 {
    if JSValue::from_bits(rest.to_bits()).is_pointer() {
        rest
    } else {
        js_nanbox_pointer(js_array_alloc(0) as i64)
    }
}

fn first_rest_arg_or_undefined(rest: f64) -> f64 {
    if !JSValue::from_bits(rest.to_bits()).is_pointer() {
        return undefined_value();
    }
    let arr = js_nanbox_get_pointer(rest) as *const ArrayHeader;
    if arr.is_null() || js_array_length(arr) == 0 {
        undefined_value()
    } else {
        perry_runtime::array::js_array_get_f64(arr, 0)
    }
}

extern "C" fn events_once_event_target_listener(closure: *const ClosureHeader, arg0: f64) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let target = js_closure_get_capture_ptr(closure, 1) as *mut ObjectHeader;
    let event_name_ptr = js_closure_get_capture_ptr(closure, 2) as *const StringHeader;
    unsafe {
        if !target.is_null() && !event_name_ptr.is_null() {
            perry_runtime::event_target::js_event_target_remove_event_listener(
                target,
                event_name_ptr,
                closure as i64,
            );
        }
        if !promise.is_null() {
            let mut args = js_array_alloc(0);
            args = js_array_push_f64(args, arg0);
            js_promise_resolve(promise, js_nanbox_pointer(args as i64));
        }
    }
    undefined_value()
}

/// `events.once(emitter, eventName[, options])` — returns a Promise that resolves
/// to an array of the args fired by the next `emit(eventName, ...)`.
///
/// Node returns the *full* args array (e.g. `emit('x', 1, 2)` resolves
/// to `[1, 2]`). Perry's emit FFI today is single-arg, so the resolved
/// array is single-element. That's enough for the parity probe in
/// issue #850; multi-arg parity is a follow-up.
#[no_mangle]
pub unsafe extern "C" fn js_events_once(
    target_value: f64,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut Promise {
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    ensure_gc_scanner_registered();
    let promise = js_promise_new();
    let target = match event_helper_target(target_value) {
        Some(target) => target,
        None => {
            js_promise_reject(
                promise,
                invalid_arg_type_error(&invalid_instance_arg_message(
                    "emitter",
                    "EventEmitter",
                    target_value,
                )),
            );
            return promise;
        }
    };
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return promise,
    };
    let signal = match options_signal_result(options) {
        Ok(signal) => signal,
        Err(error) => {
            js_promise_reject(promise, error);
            return promise;
        }
    };
    if signal.is_some_and(signal_is_aborted) {
        js_promise_reject(promise, perry_runtime::url::js_abort_error_value());
        return promise;
    }
    if let EventHelperTarget::EventEmitter(handle) = target {
        let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) else {
            return promise;
        };
        let mut pending = PendingOnce {
            promise,
            signal: undefined_value(),
            abort_listener: 0,
        };
        if let Some(signal) = signal {
            if let Some(signal_ptr) = object_ptr_from_value(signal) {
                let abort_listener = js_closure_alloc(events_once_abort_listener as *const u8, 2);
                js_closure_set_capture_ptr(abort_listener, 0, handle);
                js_closure_set_capture_ptr(abort_listener, 1, promise as i64);
                perry_runtime::url::js_abort_signal_add_listener(
                    signal_ptr,
                    abort_event_value(),
                    js_nanbox_pointer(abort_listener as i64),
                );
                pending.signal = signal;
                pending.abort_listener = abort_listener as i64;
            }
        }
        emitter
            .pending_once_promises
            .entry(event_name)
            .or_default()
            .push(pending);
        return promise;
    }
    if let EventHelperTarget::EventTarget(target) = target {
        let listener = js_closure_alloc(events_once_event_target_listener as *const u8, 3);
        js_closure_set_capture_ptr(listener, 0, promise as i64);
        js_closure_set_capture_ptr(listener, 1, target as i64);
        js_closure_set_capture_ptr(listener, 2, event_name_ptr as i64);
        perry_runtime::event_target::js_event_target_add_event_listener(
            target,
            event_name_ptr,
            listener as i64,
        );
        return promise;
    }
    if let EventHelperTarget::Stream(handle) = target {
        perry_runtime::closure::js_register_closure_rest(
            events_once_stream_resolve_listener as *const u8,
            0,
        );
        perry_runtime::closure::js_register_closure_rest(
            events_once_stream_reject_listener as *const u8,
            0,
        );
        let listener = js_closure_alloc(events_once_stream_resolve_listener as *const u8, 4);
        js_closure_set_capture_ptr(listener, 0, promise as i64);
        js_closure_set_capture_ptr(listener, 1, handle);
        js_closure_set_capture_ptr(listener, 2, 0);
        js_closure_set_capture_ptr(listener, 3, 0);
        let event_value = js_nanbox_string(event_name_ptr as i64);
        let listener_value = js_nanbox_pointer(listener as i64);
        if event_name != "error" {
            let error_event_name = b"error";
            let error_event_ptr =
                js_string_from_bytes(error_event_name.as_ptr(), error_event_name.len() as u32);
            let reject_listener =
                js_closure_alloc(events_once_stream_reject_listener as *const u8, 4);
            js_closure_set_capture_ptr(reject_listener, 0, promise as i64);
            js_closure_set_capture_ptr(reject_listener, 1, handle);
            js_closure_set_capture_ptr(reject_listener, 2, event_name_ptr as i64);
            js_closure_set_capture_ptr(reject_listener, 3, listener as i64);
            js_closure_set_capture_ptr(listener, 2, reject_listener as i64);
            js_closure_set_capture_ptr(listener, 3, error_event_ptr as i64);
            let error_event = js_nanbox_string(error_event_ptr as i64);
            let reject_listener_value = js_nanbox_pointer(reject_listener as i64);
            let _ = perry_runtime::node_stream::js_node_stream_method_once(
                handle,
                error_event,
                reject_listener_value,
            );
        }
        let _ = perry_runtime::node_stream::js_node_stream_method_once(
            handle,
            event_value,
            listener_value,
        );
    }
    promise
}
