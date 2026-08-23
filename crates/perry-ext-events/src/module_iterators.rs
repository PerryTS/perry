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
            if matches!(
                event_helper_target(nanbox_pointer_bits(handle)),
                Some(EventHelperTarget::NetSocket(_) | EventHelperTarget::NativeHandle(_))
            ) {
                let _ = call_net_socket_method(
                    handle,
                    "removeListener",
                    &[error_event, error_listener_value],
                );
            } else {
                let _ = js_node_stream_method_remove_listener(
                    handle,
                    error_event,
                    error_listener_value,
                );
            }
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
            if matches!(
                event_helper_target(nanbox_pointer_bits(handle)),
                Some(EventHelperTarget::NetSocket(_) | EventHelperTarget::NativeHandle(_))
            ) {
                let _ = call_net_socket_method(
                    handle,
                    "removeListener",
                    &[event, resolve_listener_value],
                );
            } else {
                let _ =
                    js_node_stream_method_remove_listener(handle, event, resolve_listener_value);
            }
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

// `events.on()` state lives in a GC-traced Array captured by the listener and
// iterator closures. Keeping all JS pointers inside that array means the
// extension's existing listener scanner is sufficient across moving GC.
const EVENTS_ON_BUFFER: u32 = 0;
const EVENTS_ON_PENDING: u32 = 1;
const EVENTS_ON_DONE: u32 = 2;
const EVENTS_ON_ABORT: u32 = 3;
const EVENTS_ON_HANDLE: u32 = 4;
const EVENTS_ON_LISTENER: u32 = 5;
const EVENTS_ON_ITER_SHAPE_ID: u32 = 0x7FFF_FF60;

pub(super) unsafe fn events_on_state_new() -> *mut ArrayHeader {
    let scope = TransientRootScope::enter();
    let state = js_array_alloc(6);
    let state_root = scope.root_nanbox(nanbox_pointer_bits(state as i64));
    let buffer = js_array_alloc(0);
    let buffer_root = scope.root_nanbox(nanbox_pointer_bits(buffer as i64));
    let pending = js_array_alloc(0);
    let pending_root = scope.root_nanbox(nanbox_pointer_bits(pending as i64));
    let state_ptr = || (state_root.get().to_bits() & POINTER_MASK) as *mut ArrayHeader;
    let _ = js_array_push_f64(state_ptr(), buffer_root.get());
    let _ = js_array_push_f64(state_ptr(), pending_root.get());
    let _ = js_array_push_f64(state_ptr(), f64::from_bits(0x7FFC_0000_0000_0003));
    let _ = js_array_push_f64(state_ptr(), undefined_value());
    let _ = js_array_push_f64(state_ptr(), undefined_value());
    let _ = js_array_push_f64(state_ptr(), undefined_value());
    state_ptr()
}

unsafe fn events_on_state_array(state: *mut ArrayHeader, index: u32) -> *mut ArrayHeader {
    let value = f64::from_bits(js_array_get(state, index).bits());
    (value.to_bits() & POINTER_MASK) as *mut ArrayHeader
}

unsafe fn events_on_state_set(state: *mut ArrayHeader, index: u32, value: f64) {
    js_array_set(state, index, JsValue::from_bits(value.to_bits()));
}

pub(super) unsafe fn events_on_state_set_target(
    state: *mut ArrayHeader,
    handle: Handle,
    listener: *mut RawClosureHeader,
) {
    events_on_state_set(state, EVENTS_ON_HANDLE, handle as f64);
    events_on_state_set(
        state,
        EVENTS_ON_LISTENER,
        nanbox_pointer_bits(listener as i64),
    );
}

fn events_on_iter_result(value: f64, done: bool) -> f64 {
    let scope = TransientRootScope::enter();
    let value_root = scope.root_nanbox(value);
    let packed = b"value\0done\0";
    let object = unsafe {
        js_object_alloc_with_shape(
            EVENTS_ON_ITER_SHAPE_ID,
            2,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    let object_root = scope.root_nanbox(nanbox_pointer_bits(object as i64));
    unsafe {
        let current = (object_root.get().to_bits() & POINTER_MASK) as *mut ObjectHeader;
        js_object_set_field(current, 0, JsValue::from_bits(value_root.get().to_bits()));
        js_object_set_field(current, 1, JsValue::from_bool(done));
    }
    object_root.get()
}

fn events_on_resolved(value: f64, done: bool) -> f64 {
    unsafe {
        let scope = TransientRootScope::enter();
        let result = scope.root_nanbox(events_on_iter_result(value, done));
        let promise = js_promise_new();
        let promise_root = scope.root_addr(promise as i64);
        js_promise_resolve(promise_root.get() as *mut Promise, result.get());
        nanbox_pointer_bits(promise_root.get())
    }
}

fn events_on_finish_pending(state: *mut ArrayHeader, reason: Option<f64>) {
    unsafe {
        let pending = events_on_state_array(state, EVENTS_ON_PENDING);
        if pending.is_null() {
            return;
        }
        while js_array_length(pending) > 0 {
            let promise = (js_array_shift_f64(pending).to_bits() & POINTER_MASK) as *mut Promise;
            if promise.is_null() {
                continue;
            }
            if let Some(reason) = reason {
                js_promise_reject(promise, reason);
            } else {
                js_promise_resolve(promise, events_on_iter_result(undefined_value(), true));
            }
        }
    }
}

/// Queue listener for `events.on(...)`. Resolve an already-blocked `next()`
/// immediately, otherwise retain the argument tuple in FIFO order.
pub(super) extern "C" fn events_on_queue_listener(
    closure: *const RawClosureHeader,
    arg0: f64,
) -> f64 {
    unsafe {
        let state = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
        if !state.is_null() {
            let scope = TransientRootScope::enter();
            let state_root = scope.root_nanbox(nanbox_pointer_bits(state as i64));
            let mut args = js_array_alloc(0);
            args = js_array_push_f64(args, arg0);
            let args_root = scope.root_nanbox(nanbox_pointer_bits(args as i64));
            let current_state = (state_root.get().to_bits() & POINTER_MASK) as *mut ArrayHeader;
            let pending = events_on_state_array(current_state, EVENTS_ON_PENDING);
            if !pending.is_null() && js_array_length(pending) > 0 {
                let promise =
                    (js_array_shift_f64(pending).to_bits() & POINTER_MASK) as *mut Promise;
                if !promise.is_null() {
                    let promise_root = scope.root_addr(promise as i64);
                    let result = scope.root_nanbox(events_on_iter_result(args_root.get(), false));
                    js_promise_resolve(promise_root.get() as *mut Promise, result.get());
                }
            } else {
                let current_state = (state_root.get().to_bits() & POINTER_MASK) as *mut ArrayHeader;
                let buffer = events_on_state_array(current_state, EVENTS_ON_BUFFER);
                if !buffer.is_null() {
                    let _ = js_array_push_f64(buffer, args_root.get());
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

extern "C" fn events_on_next(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let state = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
        if state.is_null() {
            return events_on_resolved(undefined_value(), true);
        }
        let buffer = events_on_state_array(state, EVENTS_ON_BUFFER);
        if !buffer.is_null() && js_array_length(buffer) > 0 {
            return events_on_resolved(js_array_shift_f64(buffer), false);
        }
        let abort = f64::from_bits(js_array_get(state, EVENTS_ON_ABORT).bits());
        if abort.to_bits() != TAG_UNDEFINED_F64_BITS {
            let promise = js_promise_new();
            js_promise_reject(promise, abort);
            return nanbox_pointer_bits(promise as i64);
        }
        let done = f64::from_bits(js_array_get(state, EVENTS_ON_DONE).bits());
        if done.to_bits() == TAG_TRUE_F64_BITS {
            return events_on_resolved(undefined_value(), true);
        }
        let promise = js_promise_new();
        let pending = events_on_state_array(state, EVENTS_ON_PENDING);
        if !pending.is_null() {
            let _ = js_array_push_f64(pending, nanbox_pointer_bits(promise as i64));
        }
        nanbox_pointer_bits(promise as i64)
    }
}

extern "C" fn events_on_return(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let state = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
        if state.is_null() {
            return events_on_resolved(undefined_value(), true);
        }
        events_on_state_set(state, EVENTS_ON_DONE, f64::from_bits(TAG_TRUE_F64_BITS));
        let handle = f64::from_bits(js_array_get(state, EVENTS_ON_HANDLE).bits());
        let listener = f64::from_bits(js_array_get(state, EVENTS_ON_LISTENER).bits());
        if handle.is_finite() && listener.to_bits() != TAG_UNDEFINED_F64_BITS {
            if let Some(emitter) = get_event_emitter_mut(handle as Handle) {
                remove_listener_by_callback(emitter, (listener.to_bits() & POINTER_MASK) as i64);
            }
        }
        events_on_finish_pending(state, None);
        events_on_resolved(undefined_value(), true)
    }
}

extern "C" fn events_on_iterator_self(closure: *const RawClosureHeader) -> f64 {
    unsafe { js_closure_get_capture_f64(closure, 0) }
}

extern "C" fn events_on_async_iterator(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let state = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
        let scope = TransientRootScope::enter();
        let state_root = scope.root_nanbox(nanbox_pointer_bits(state as i64));
        let packed = b"next\0return\0";
        let object = js_object_alloc_with_shape(
            EVENTS_ON_ITER_SHAPE_ID + 1,
            2,
            packed.as_ptr(),
            packed.len() as u32,
        );
        let object_root = scope.root_nanbox(nanbox_pointer_bits(object as i64));
        let next = js_closure_alloc(events_on_next as *const u8, 1);
        js_closure_set_capture_ptr(next, 0, (state_root.get().to_bits() & POINTER_MASK) as i64);
        let next_root = scope.root_addr(next as i64);
        js_object_set_field(
            (object_root.get().to_bits() & POINTER_MASK) as *mut ObjectHeader,
            0,
            JsValue::from_object_ptr(next_root.get() as *mut u8),
        );
        let return_fn = js_closure_alloc(events_on_return as *const u8, 1);
        js_closure_set_capture_ptr(
            return_fn,
            0,
            (state_root.get().to_bits() & POINTER_MASK) as i64,
        );
        let return_root = scope.root_addr(return_fn as i64);
        js_object_set_field(
            (object_root.get().to_bits() & POINTER_MASK) as *mut ObjectHeader,
            1,
            JsValue::from_object_ptr(return_root.get() as *mut u8),
        );

        let iterator = object_root.get();
        let iterator_root = scope.root_nanbox(iterator);
        let symbol = js_symbol_well_known_async_iterator();
        let self_fn = js_closure_alloc(events_on_iterator_self as *const u8, 1);
        js_closure_set_capture_f64(self_fn, 0, iterator_root.get());
        js_object_set_symbol_property(
            iterator_root.get(),
            symbol,
            nanbox_pointer_bits(self_fn as i64),
        );
        iterator_root.get()
    }
}

pub(super) unsafe fn events_on_install_async_iterator(
    queue: *mut ArrayHeader,
    state: *mut ArrayHeader,
) {
    let scope = TransientRootScope::enter();
    let queue_root = scope.root_nanbox(nanbox_pointer_bits(queue as i64));
    let state_root = scope.root_nanbox(nanbox_pointer_bits(state as i64));
    js_register_closure_arity(events_on_next as *const u8, 0);
    js_register_closure_arity(events_on_return as *const u8, 0);
    js_register_closure_arity(events_on_iterator_self as *const u8, 0);
    js_register_closure_arity(events_on_async_iterator as *const u8, 0);
    let closure = js_closure_alloc(events_on_async_iterator as *const u8, 1);
    js_closure_set_capture_ptr(
        closure,
        0,
        (state_root.get().to_bits() & POINTER_MASK) as i64,
    );
    js_object_set_symbol_property(
        queue_root.get(),
        js_symbol_well_known_async_iterator(),
        nanbox_pointer_bits(closure as i64),
    );
}

/// A configured close event ends the iterator after already-buffered events.
pub(super) extern "C" fn events_on_close_listener(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let state = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
        if !state.is_null() {
            events_on_state_set(state, EVENTS_ON_DONE, f64::from_bits(TAG_TRUE_F64_BITS));
            events_on_finish_pending(state, None);
        }
    }
    undefined_value()
}

pub(super) extern "C" fn events_on_abort_listener(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let handle = js_closure_get_capture_ptr(closure, 0) as Handle;
        let data_listener = js_closure_get_capture_ptr(closure, 1);
        let signal_ptr = js_closure_get_capture_ptr(closure, 2) as *mut u8;
        let state = js_closure_get_capture_ptr(closure, 3) as *mut ArrayHeader;
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
        if !state.is_null() {
            let reason = js_abort_error_value();
            events_on_state_set(state, EVENTS_ON_ABORT, reason);
            events_on_state_set(state, EVENTS_ON_DONE, f64::from_bits(TAG_TRUE_F64_BITS));
            events_on_finish_pending(state, Some(reason));
        }
    }
    undefined_value()
}
