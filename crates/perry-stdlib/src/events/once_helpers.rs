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

unsafe fn remove_stream_or_socket_once_listener(
    handle: Handle,
    event_name_ptr: i64,
    listener: i64,
) {
    if matches!(
        event_helper_target(js_nanbox_pointer(handle)),
        Some(EventHelperTarget::NetSocket(_))
    ) {
        let _ = super::module_helpers::call_net_socket_method(
            handle,
            "removeListener",
            &[
                js_nanbox_string(event_name_ptr),
                js_nanbox_pointer(listener),
            ],
        );
    } else {
        let _ = perry_runtime::node_stream::js_node_stream_method_remove_listener(
            handle,
            js_nanbox_string(event_name_ptr),
            js_nanbox_pointer(listener),
        );
    }
}

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
        unsafe {
            remove_stream_or_socket_once_listener(handle, error_event_ptr, error_listener);
        }
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
        unsafe {
            remove_stream_or_socket_once_listener(handle, event_name_ptr, resolve_listener);
        }
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
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let target_value = scope.root_nanbox_f64(target_value);
    let event_name_handle = scope.root_string_ptr(event_name_ptr);
    let options = scope.root_nanbox_f64(options);
    let promise = scope.root_raw_mut_ptr(js_promise_new());
    let target = match event_helper_target(target_value.get_nanbox_f64()) {
        Some(target) => target,
        None => {
            let error = invalid_arg_type_error(&invalid_instance_arg_message(
                "emitter",
                "EventEmitter",
                target_value.get_nanbox_f64(),
            ));
            js_promise_reject(promise.get_raw_mut_ptr(), error);
            return promise.get_raw_mut_ptr();
        }
    };
    let event_name = match string_from_header(event_name_handle.get_raw_const_ptr()) {
        Some(name) => name,
        None => return promise.get_raw_mut_ptr(),
    };
    let signal = match options_signal_result(options.get_nanbox_f64()) {
        Ok(signal) => signal,
        Err(error) => {
            js_promise_reject(promise.get_raw_mut_ptr(), error);
            return promise.get_raw_mut_ptr();
        }
    };
    let signal = signal.map(|value| scope.root_nanbox_f64(value));
    if signal
        .as_ref()
        .is_some_and(|value| signal_is_aborted(value.get_nanbox_f64()))
    {
        let error = perry_runtime::url::js_abort_error_value();
        js_promise_reject(promise.get_raw_mut_ptr(), error);
        return promise.get_raw_mut_ptr();
    }
    if let EventHelperTarget::EventEmitter(handle) = target {
        let mut pending = PendingOnce {
            promise: promise.get_raw_mut_ptr(),
            signal: undefined_value(),
            abort_listener: 0,
        };
        if let Some(signal) = &signal {
            if object_ptr_from_value(signal.get_nanbox_f64()).is_some() {
                let abort_listener = js_closure_alloc(events_once_abort_listener as *const u8, 2);
                let abort_listener = scope.root_raw_mut_ptr(abort_listener);
                js_closure_set_capture_ptr(abort_listener.get_raw_mut_ptr(), 0, handle);
                js_closure_set_capture_ptr(
                    abort_listener.get_raw_mut_ptr(),
                    1,
                    promise.get_raw_mut_ptr::<Promise>() as i64,
                );
                let abort_event = scope.root_nanbox_f64(abort_event_value());
                perry_runtime::url::js_abort_signal_add_listener(
                    object_ptr_from_value(signal.get_nanbox_f64()).unwrap_or(std::ptr::null_mut()),
                    abort_event.get_nanbox_f64(),
                    js_nanbox_pointer(abort_listener.get_raw_mut_ptr::<ClosureHeader>() as i64),
                );
                pending.signal = signal.get_nanbox_f64();
                pending.abort_listener = abort_listener.get_raw_mut_ptr::<ClosureHeader>() as i64;
            }
        }
        let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) else {
            return promise.get_raw_mut_ptr();
        };
        emitter
            .pending_once_promises
            .entry(event_name)
            .or_default()
            .push(pending);
        return promise.get_raw_mut_ptr();
    }
    if let EventHelperTarget::EventTarget(_) = target {
        let listener = js_closure_alloc(events_once_event_target_listener as *const u8, 3);
        let listener = scope.root_raw_mut_ptr(listener);
        let target =
            object_ptr_from_value(target_value.get_nanbox_f64()).unwrap_or(std::ptr::null_mut());
        js_closure_set_capture_ptr(
            listener.get_raw_mut_ptr(),
            0,
            promise.get_raw_mut_ptr::<Promise>() as i64,
        );
        js_closure_set_capture_ptr(listener.get_raw_mut_ptr(), 1, target as i64);
        js_closure_set_capture_ptr(
            listener.get_raw_mut_ptr(),
            2,
            event_name_handle.get_raw_const_ptr::<StringHeader>() as i64,
        );
        perry_runtime::event_target::js_event_target_add_event_listener(
            target,
            event_name_handle.get_raw_const_ptr(),
            listener.get_raw_mut_ptr::<ClosureHeader>() as i64,
        );
        return promise.get_raw_mut_ptr();
    }
    if let EventHelperTarget::NetSocket(handle) = target {
        let listens_for_error = event_name == "error";
        perry_runtime::closure::js_register_closure_rest(
            events_once_stream_resolve_listener as *const u8,
            0,
        );
        perry_runtime::closure::js_register_closure_rest(
            events_once_stream_reject_listener as *const u8,
            0,
        );
        let listener = js_closure_alloc(events_once_stream_resolve_listener as *const u8, 4);
        let listener = scope.root_raw_mut_ptr(listener);
        js_closure_set_capture_ptr(
            listener.get_raw_mut_ptr(),
            0,
            promise.get_raw_mut_ptr::<Promise>() as i64,
        );
        js_closure_set_capture_ptr(listener.get_raw_mut_ptr(), 1, handle);
        js_closure_set_capture_ptr(listener.get_raw_mut_ptr(), 2, 0);
        js_closure_set_capture_ptr(listener.get_raw_mut_ptr(), 3, 0);
        if !listens_for_error {
            let error_event_ptr = js_string_from_bytes(b"error".as_ptr(), 5);
            let error_event = scope.root_string_ptr(error_event_ptr);
            let reject_listener =
                js_closure_alloc(events_once_stream_reject_listener as *const u8, 4);
            let reject_listener = scope.root_raw_mut_ptr(reject_listener);
            js_closure_set_capture_ptr(
                reject_listener.get_raw_mut_ptr(),
                0,
                promise.get_raw_mut_ptr::<Promise>() as i64,
            );
            js_closure_set_capture_ptr(reject_listener.get_raw_mut_ptr(), 1, handle);
            js_closure_set_capture_ptr(
                reject_listener.get_raw_mut_ptr(),
                2,
                event_name_handle.get_raw_const_ptr::<StringHeader>() as i64,
            );
            js_closure_set_capture_ptr(
                reject_listener.get_raw_mut_ptr(),
                3,
                listener.get_raw_mut_ptr::<ClosureHeader>() as i64,
            );
            js_closure_set_capture_ptr(
                listener.get_raw_mut_ptr(),
                2,
                reject_listener.get_raw_mut_ptr::<ClosureHeader>() as i64,
            );
            js_closure_set_capture_ptr(
                listener.get_raw_mut_ptr(),
                3,
                error_event.get_raw_const_ptr::<StringHeader>() as i64,
            );
            let _ = super::module_helpers::call_net_socket_method(
                handle,
                "once",
                &[
                    js_nanbox_string(error_event.get_raw_const_ptr::<StringHeader>() as i64),
                    js_nanbox_pointer(reject_listener.get_raw_mut_ptr::<ClosureHeader>() as i64),
                ],
            );
        }
        let _ = super::module_helpers::call_net_socket_method(
            handle,
            "once",
            &[
                js_nanbox_string(event_name_handle.get_raw_const_ptr::<StringHeader>() as i64),
                js_nanbox_pointer(listener.get_raw_mut_ptr::<ClosureHeader>() as i64),
            ],
        );
        return promise.get_raw_mut_ptr();
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
        let listener = scope.root_raw_mut_ptr(listener);
        js_closure_set_capture_ptr(
            listener.get_raw_mut_ptr(),
            0,
            promise.get_raw_mut_ptr::<Promise>() as i64,
        );
        js_closure_set_capture_ptr(listener.get_raw_mut_ptr(), 1, handle);
        js_closure_set_capture_ptr(listener.get_raw_mut_ptr(), 2, 0);
        js_closure_set_capture_ptr(listener.get_raw_mut_ptr(), 3, 0);
        if event_name != "error" {
            let error_event_name = b"error";
            let error_event_ptr =
                js_string_from_bytes(error_event_name.as_ptr(), error_event_name.len() as u32);
            let error_event = scope.root_string_ptr(error_event_ptr);
            let reject_listener =
                js_closure_alloc(events_once_stream_reject_listener as *const u8, 4);
            let reject_listener = scope.root_raw_mut_ptr(reject_listener);
            js_closure_set_capture_ptr(
                reject_listener.get_raw_mut_ptr(),
                0,
                promise.get_raw_mut_ptr::<Promise>() as i64,
            );
            js_closure_set_capture_ptr(reject_listener.get_raw_mut_ptr(), 1, handle);
            js_closure_set_capture_ptr(
                reject_listener.get_raw_mut_ptr(),
                2,
                event_name_handle.get_raw_const_ptr::<StringHeader>() as i64,
            );
            js_closure_set_capture_ptr(
                reject_listener.get_raw_mut_ptr(),
                3,
                listener.get_raw_mut_ptr::<ClosureHeader>() as i64,
            );
            js_closure_set_capture_ptr(
                listener.get_raw_mut_ptr(),
                2,
                reject_listener.get_raw_mut_ptr::<ClosureHeader>() as i64,
            );
            js_closure_set_capture_ptr(
                listener.get_raw_mut_ptr(),
                3,
                error_event.get_raw_const_ptr::<StringHeader>() as i64,
            );
            let _ = perry_runtime::node_stream::js_node_stream_method_once(
                handle,
                js_nanbox_string(error_event.get_raw_const_ptr::<StringHeader>() as i64),
                js_nanbox_pointer(reject_listener.get_raw_mut_ptr::<ClosureHeader>() as i64),
            );
        }
        let _ = perry_runtime::node_stream::js_node_stream_method_once(
            handle,
            js_nanbox_string(event_name_handle.get_raw_const_ptr::<StringHeader>() as i64),
            js_nanbox_pointer(listener.get_raw_mut_ptr::<ClosureHeader>() as i64),
        );
        return promise.get_raw_mut_ptr();
    }
    promise.get_raw_mut_ptr()
}
