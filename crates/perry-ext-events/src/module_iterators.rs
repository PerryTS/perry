use super::*;

pub(super) extern "C" fn events_once_event_target_listener(
    closure: *const RawClosureHeader,
    arg0: f64,
) -> f64 {
    unsafe {
        let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
        let target = js_closure_get_capture_ptr(closure, 1) as *mut u8;
        let event_name_ptr = js_closure_get_capture_ptr(closure, 2) as *const StringHeader;
        if !target.is_null() && !event_name_ptr.is_null() {
            js_event_target_remove_event_listener(target, event_name_ptr, closure as i64);
        }
        if !promise.is_null() {
            let mut args = js_array_alloc(0);
            args = js_array_push_f64(args, arg0);
            js_promise_resolve(promise, nanbox_pointer_bits(args as i64));
            js_native_async_drop_promise_token(promise);
        }
    }
    undefined_value()
}

pub(super) extern "C" fn events_once_abort_listener(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
        let pending = get_event_emitter_mut(handle)
            .and_then(|emitter| remove_pending_once_promise(emitter, promise));
        if let Some(pending) = pending {
            cleanup_pending_abort_listener(&pending);
            if !pending.promise.is_null() {
                js_promise_reject(pending.promise, js_abort_error_value());
                js_native_async_drop_promise_token(pending.promise);
            }
        }
    }
    undefined_value()
}

pub(super) extern "C" fn events_once_stream_resolve_listener(
    closure: *const RawClosureHeader,
    rest: f64,
) -> f64 {
    unsafe {
        let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
        let handle = js_closure_get_capture_ptr(closure, 1) as Handle;
        let error_listener = js_closure_get_capture_ptr(closure, 2);
        let error_event_ptr = js_closure_get_capture_ptr(closure, 3);
        if promise.is_null() {
            return undefined_value();
        }
        if handle != 0 && error_listener != 0 && error_event_ptr != 0 {
            let error_event =
                f64::from_bits(nanbox_string_bits(error_event_ptr as *mut StringHeader));
            let error_listener_value = nanbox_pointer_bits(error_listener);
            let _ =
                js_node_stream_method_remove_listener(handle, error_event, error_listener_value);
        }
        js_promise_resolve(promise, rest_array_or_empty(rest));
        js_native_async_drop_promise_token(promise);
    }
    undefined_value()
}

pub(super) extern "C" fn events_once_stream_reject_listener(
    closure: *const RawClosureHeader,
    rest: f64,
) -> f64 {
    unsafe {
        let promise = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
        let handle = js_closure_get_capture_ptr(closure, 1) as Handle;
        let event_name_ptr = js_closure_get_capture_ptr(closure, 2);
        let resolve_listener = js_closure_get_capture_ptr(closure, 3);
        if handle != 0 && event_name_ptr != 0 && resolve_listener != 0 {
            let event = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
            let resolve_listener_value = nanbox_pointer_bits(resolve_listener);
            let _ = js_node_stream_method_remove_listener(handle, event, resolve_listener_value);
        }
        if !promise.is_null() {
            js_promise_reject(promise, first_rest_arg_or_undefined(rest));
            js_native_async_drop_promise_token(promise);
        }
    }
    undefined_value()
}

pub(super) fn rest_array_or_empty(rest: f64) -> f64 {
    if JsValue::from_bits(rest.to_bits()).is_pointer() {
        rest
    } else {
        nanbox_pointer_bits(unsafe { js_array_alloc(0) } as i64)
    }
}

pub(super) unsafe fn first_rest_arg_or_undefined(rest: f64) -> f64 {
    let value = JsValue::from_bits(rest.to_bits());
    if !value.is_pointer() {
        return undefined_value();
    }
    let arr = value.as_pointer::<ArrayHeader>();
    if arr.is_null() || (*arr).length == 0 {
        undefined_value()
    } else {
        f64::from_bits(js_array_get(arr, 0).bits())
    }
}

/// Queue listener for `events.on(...)` — captures the queue array in
/// slot 0 and pushes `[arg]` onto it for each emitted event. The
/// `for await (... of iter)` loop pulls items off the array as the
/// stream produces them.
pub(super) extern "C" fn events_on_queue_listener(
    closure: *const RawClosureHeader,
    arg0: f64,
) -> f64 {
    unsafe {
        let queue = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
        let abort_promise = js_closure_get_capture_ptr(closure, 1) as *mut Promise;
        if !queue.is_null() {
            let mut args = js_array_alloc(0);
            args = js_array_push_f64(args, arg0);
            let args_val = nanbox_pointer_bits(args as i64);
            if abort_promise.is_null() {
                let _ = js_array_push_f64(queue, args_val);
            } else {
                let abort_val = nanbox_pointer_bits(abort_promise as i64);
                let len = (*queue).length;
                if len == 0 {
                    let _ = js_array_push_f64(queue, args_val);
                    let _ = js_array_push_f64(queue, abort_val);
                } else {
                    js_array_set(queue, len - 1, JsValue::from_bits(args_val.to_bits()));
                    let _ = js_array_push_f64(queue, abort_val);
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

pub(super) extern "C" fn events_on_abort_listener(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let data_listener = js_closure_get_capture_ptr(closure, 1);
        let signal_ptr = js_closure_get_capture_ptr(closure, 2) as *mut u8;
        let abort_promise = js_closure_get_capture_ptr(closure, 3) as *mut Promise;
        let event_name_ptr = js_closure_get_capture_ptr(closure, 4) as *const StringHeader;

        if let Some(emitter) = get_event_emitter_mut(handle) {
            remove_listener_by_callback(emitter, data_listener);
        }
        if !event_name_ptr.is_null() {
            if let Some(target) = event_target_ptr(handle) {
                js_event_target_remove_event_listener(target, event_name_ptr, data_listener);
            } else if stream_value_from_handle(handle).is_some() {
                let event = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
                let listener = nanbox_pointer_bits(data_listener);
                let _ = js_node_stream_method_remove_listener(handle, event, listener);
            }
        }
        if !signal_ptr.is_null() {
            js_abort_signal_remove_listener(
                signal_ptr,
                abort_event_value(),
                nanbox_pointer_bits(closure as i64),
            );
        }
        if !abort_promise.is_null() {
            js_promise_reject(abort_promise, js_abort_error_value());
        }
    }
    undefined_value()
}
