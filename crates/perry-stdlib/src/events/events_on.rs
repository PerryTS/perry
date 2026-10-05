//! `events.on(...)` async-iterator machinery.
//!
//! Moved verbatim from the `events.rs` trunk during the file split. Node's
//! `on()` returns an async iterator that buffers emitted events and blocks
//! `next()` until one arrives.

use super::*;

use perry_runtime::{
    js_array_alloc, js_array_length, js_array_push_f64, js_nanbox_get_pointer, js_nanbox_pointer,
    js_promise_new, js_promise_reject, js_promise_resolve, ArrayHeader, ClosureHeader, JSValue,
    Promise, StringHeader,
};

// `events.on(...)` async-iterator state. Node's `on()` returns an async
// iterator that buffers emitted events and blocks `next()` until one arrives.
// The shared state lives in a GC-rooted JS array (the returned handle keeps it
// reachable) with this fixed layout:
//   [0] buffer        — FIFO of `[arg]` arrays awaiting consumption
//   [1] pending       — FIFO of `next()` Promises blocked on a future event
//   [2] done          — bool: iteration ended (return() / abort)
//   [3] abort_reason  — the AbortError to reject `next()` with, or undefined
//   [4] emitter       — the emitter / EventTarget value (for listener removal)
//   [5] listener      — the queue listener closure (for removal on return)
//   [6] event         — the event name value
const EVENTS_ON_BUFFER: u32 = 0;
const EVENTS_ON_PENDING: u32 = 1;
const EVENTS_ON_DONE: u32 = 2;
const EVENTS_ON_ABORT: u32 = 3;
const EVENTS_ON_EMITTER: u32 = 4;
const EVENTS_ON_LISTENER: u32 = 5;
const EVENTS_ON_EVENT: u32 = 6;
const EVENTS_ON_ITER_SHAPE_ID: u32 = 0x7FFF_FF60;

unsafe fn events_on_state_new() -> *mut ArrayHeader {
    let state = js_array_alloc(7);
    let buffer = js_array_alloc(0);
    let pending = js_array_alloc(0);
    let _ = js_array_push_f64(state, js_nanbox_pointer(buffer as i64));
    let _ = js_array_push_f64(state, js_nanbox_pointer(pending as i64));
    let _ = js_array_push_f64(state, TAG_FALSE_F64);
    for _ in EVENTS_ON_ABORT..=EVENTS_ON_EVENT {
        let _ = js_array_push_f64(state, f64::from_bits(TAG_UNDEFINED_F64_BITS));
    }
    state
}

unsafe fn events_on_state_array(state: *mut ArrayHeader, idx: u32) -> *mut ArrayHeader {
    js_nanbox_get_pointer(perry_runtime::array::js_array_get_f64(state, idx)) as *mut ArrayHeader
}

unsafe fn events_on_state_set(state: *mut ArrayHeader, idx: u32, value: f64) {
    perry_runtime::array::js_array_set_f64_unchecked(state, idx, value);
}

/// Build a `{ value, done }` iterator-result object.
fn events_iter_result(value: f64, done: bool) -> f64 {
    let packed = b"value\0done\0";
    let obj = perry_runtime::object::js_object_alloc_with_shape(
        EVENTS_ON_ITER_SHAPE_ID,
        2,
        packed.as_ptr(),
        packed.len() as u32,
    );
    perry_runtime::object::js_object_set_field(obj, 0, JSValue::from_bits(value.to_bits()));
    perry_runtime::object::js_object_set_field(obj, 1, JSValue::bool(done));
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// A Promise already resolved with `{ value, done }`.
fn events_resolved_iter_promise(value: f64, done: bool) -> f64 {
    let p = perry_runtime::promise::js_promise_resolved(events_iter_result(value, done));
    f64::from_bits(JSValue::pointer(p as *const u8).bits())
}

fn register_events_on_arities() {}

/// The queue listener fired for each emitted event. Resolves a blocked `next()`
/// Promise immediately if one is waiting, otherwise buffers the args array.
extern "C" fn events_on_queue_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    rest: f64,
) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let state = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
    if state.is_null() {
        return f64::from_bits(TAG_UNDEFINED_F64_BITS);
    }
    unsafe {
        let args_val = if JSValue::from_bits(rest.to_bits()).is_pointer() {
            rest
        } else {
            js_nanbox_pointer(js_array_alloc(0) as i64)
        };

        let pending = events_on_state_array(state, EVENTS_ON_PENDING);
        if !pending.is_null() && js_array_length(pending) > 0 {
            let promise = js_nanbox_get_pointer(perry_runtime::array::js_array_shift_f64(pending))
                as *mut Promise;
            if !promise.is_null() {
                js_promise_resolve(promise, events_iter_result(args_val, false));
            }
        } else {
            let buffer = events_on_state_array(state, EVENTS_ON_BUFFER);
            if !buffer.is_null() {
                let _ = js_array_push_f64(buffer, args_val);
            }
        }
    }

    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

/// `next()` — drain a buffered event, reject on abort, finish when done, or
/// return a pending Promise the listener will resolve on the next event.
extern "C" fn events_on_next(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let state = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
    if state.is_null() {
        return events_resolved_iter_promise(f64::from_bits(TAG_UNDEFINED_F64_BITS), true);
    }
    unsafe {
        let buffer = events_on_state_array(state, EVENTS_ON_BUFFER);
        if !buffer.is_null() && js_array_length(buffer) > 0 {
            let args_val = perry_runtime::array::js_array_shift_f64(buffer);
            return events_resolved_iter_promise(args_val, false);
        }
        let abort = perry_runtime::array::js_array_get_f64(state, EVENTS_ON_ABORT);
        if abort.to_bits() != TAG_UNDEFINED_F64_BITS {
            let p = js_promise_new();
            js_promise_reject(p, abort);
            return f64::from_bits(JSValue::pointer(p as *const u8).bits());
        }
        let done = perry_runtime::array::js_array_get_f64(state, EVENTS_ON_DONE);
        if done.to_bits() == TAG_TRUE_F64.to_bits() {
            return events_resolved_iter_promise(f64::from_bits(TAG_UNDEFINED_F64_BITS), true);
        }
        // No event ready yet: hand back a pending Promise; the listener resolves
        // it (or the abort listener rejects it) when the next event lands.
        let pending = events_on_state_array(state, EVENTS_ON_PENDING);
        let p = js_promise_new();
        if !pending.is_null() {
            let _ = js_array_push_f64(pending, js_nanbox_pointer(p as i64));
        }
        f64::from_bits(JSValue::pointer(p as *const u8).bits())
    }
}

/// `return()` — end iteration: mark done, detach the listener, settle any
/// blocked `next()` with `{ done: true }`.
extern "C" fn events_on_return(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let state = js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
    if state.is_null() {
        return events_resolved_iter_promise(f64::from_bits(TAG_UNDEFINED_F64_BITS), true);
    }
    unsafe {
        events_on_state_set(state, EVENTS_ON_DONE, TAG_TRUE_F64);
        // Detach the queue listener from the emitter so no further events queue.
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let state_handle = scope.root_raw_mut_ptr(state);
        detach_queue_listener(state);
        let state = state_handle.get_raw_mut_ptr::<ArrayHeader>();
        // Resolve any blocked `next()` with completion.
        let pending = events_on_state_array(state, EVENTS_ON_PENDING);
        if !pending.is_null() {
            while js_array_length(pending) > 0 {
                let promise =
                    js_nanbox_get_pointer(perry_runtime::array::js_array_shift_f64(pending))
                        as *mut Promise;
                if !promise.is_null() {
                    js_promise_resolve(
                        promise,
                        events_iter_result(f64::from_bits(TAG_UNDEFINED_F64_BITS), true),
                    );
                }
            }
        }
    }
    events_resolved_iter_promise(f64::from_bits(TAG_UNDEFINED_F64_BITS), true)
}

extern "C" fn events_on_aiter_self(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    perry_runtime::closure::js_closure_get_capture_f64(closure, 0)
}

/// `queue[Symbol.asyncIterator]()` — build a fresh `{ next, return }` iterator
/// object bound to the shared state.
extern "C" fn events_on_async_iterator(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    let state = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as *mut ArrayHeader;
    register_events_on_arities();

    let packed = b"next\0return\0";
    let obj = perry_runtime::object::js_object_alloc_with_shape(
        EVENTS_ON_ITER_SHAPE_ID + 1,
        2,
        packed.as_ptr(),
        packed.len() as u32,
    );
    let next_cl = js_closure_alloc(
        perry_runtime::fn_info!(events_on_next, 0; with_declared(0)),
        1,
    );
    js_closure_set_capture_ptr(next_cl, 0, state as i64);
    perry_runtime::object::js_object_set_field(obj, 0, JSValue::pointer(next_cl as *const u8));
    let ret_cl = js_closure_alloc(
        perry_runtime::fn_info!(events_on_return, 0; with_declared(0)),
        1,
    );
    js_closure_set_capture_ptr(ret_cl, 0, state as i64);
    perry_runtime::object::js_object_set_field(obj, 1, JSValue::pointer(ret_cl as *const u8));

    let iter_val = f64::from_bits(JSValue::pointer(obj as *const u8).bits());
    let async_iterator = perry_runtime::symbol::well_known_symbol("asyncIterator");
    if !async_iterator.is_null() {
        let self_cl = js_closure_alloc(
            perry_runtime::fn_info!(events_on_aiter_self, 0; with_declared(0)),
            1,
        );
        perry_runtime::closure::js_closure_set_capture_f64(self_cl, 0, iter_val);
        unsafe {
            perry_runtime::symbol::js_object_set_symbol_property(
                iter_val,
                js_nanbox_pointer(async_iterator as i64),
                js_nanbox_pointer(self_cl as i64),
            );
        }
    }
    iter_val
}

unsafe fn install_events_on_async_iterator(queue: *mut ArrayHeader, state: *mut ArrayHeader) {
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    register_events_on_arities();
    let async_iterator = perry_runtime::symbol::well_known_symbol("asyncIterator");
    if async_iterator.is_null() {
        return;
    }
    let closure = js_closure_alloc(
        perry_runtime::fn_info!(events_on_async_iterator, 0; with_declared(0)),
        1,
    );
    js_closure_set_capture_ptr(closure, 0, state as i64);
    perry_runtime::symbol::js_object_set_symbol_property(
        js_nanbox_pointer(queue as i64),
        js_nanbox_pointer(async_iterator as i64),
        js_nanbox_pointer(closure as i64),
    );
}

/// Remove the queue listener recorded in `state` from its emitter.
unsafe fn detach_queue_listener(state: *mut ArrayHeader) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let emitter = scope.root_nanbox_f64(perry_runtime::array::js_array_get_f64(
        state,
        EVENTS_ON_EMITTER,
    ));
    let listener = scope.root_nanbox_f64(perry_runtime::array::js_array_get_f64(
        state,
        EVENTS_ON_LISTENER,
    ));
    let event = scope.root_nanbox_f64(perry_runtime::array::js_array_get_f64(
        state,
        EVENTS_ON_EVENT,
    ));
    if emitter.get_nanbox_u64() == TAG_UNDEFINED_F64_BITS
        || listener.get_nanbox_u64() == TAG_UNDEFINED_F64_BITS
    {
        return;
    }
    if let Some(target) = event_target_ptr(emitter.get_nanbox_f64()) {
        let event_ptr = perry_runtime::value::js_get_string_pointer_unified(event.get_nanbox_f64())
            as *const StringHeader;
        perry_runtime::event_target::js_event_target_remove_event_listener(
            target,
            event_ptr,
            js_nanbox_get_pointer(listener.get_nanbox_f64()),
        );
    } else {
        let _ = call_emitter_method(
            emitter.get_nanbox_f64(),
            "removeListener",
            &[event.get_nanbox_f64(), listener.get_nanbox_f64()],
        );
    }
}

/// The signal's `abort`: detach, then end the iteration with an AbortError.
/// Captures `[state, signal]`.
extern "C" fn events_on_abort_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_f64;

    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let state = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let signal = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let this_listener = scope.root_raw_mut_ptr(closure as *mut ClosureHeader);
    unsafe {
        detach_queue_listener(js_nanbox_get_pointer(state.get_nanbox_f64()) as *mut ArrayHeader);
        remove_abort_listener(
            signal.get_nanbox_f64(),
            js_nanbox_pointer(this_listener.get_raw_mut_ptr::<ClosureHeader>() as i64),
        );
        // Mark the iterator aborted and reject any blocked `next()`. Buffered
        // events drained before the abort still surface; only once the buffer is
        // empty does `next()` observe the stored AbortError (matching Node).
        let abort_err = scope.root_nanbox_f64(perry_runtime::url::js_abort_error_value());
        let state = js_nanbox_get_pointer(state.get_nanbox_f64()) as *mut ArrayHeader;
        if !state.is_null() {
            let abort_err = abort_err.get_nanbox_f64();
            events_on_state_set(state, EVENTS_ON_ABORT, abort_err);
            events_on_state_set(state, EVENTS_ON_DONE, TAG_TRUE_F64);
            let pending = events_on_state_array(state, EVENTS_ON_PENDING);
            if !pending.is_null() {
                while js_array_length(pending) > 0 {
                    let promise =
                        js_nanbox_get_pointer(perry_runtime::array::js_array_shift_f64(pending))
                            as *mut Promise;
                    if !promise.is_null() {
                        js_promise_reject(promise, abort_err);
                    }
                }
            }
        }
    }

    undefined_value()
}

/// `events.on(emitter, eventName[, options])` — returns a Node-style async
/// iterator. `[Symbol.asyncIterator]()` builds a `{ next, return }` object bound
/// to shared state: emitted events are buffered as `[arg]` arrays, `next()`
/// drains the buffer (or blocks on a Promise the listener resolves on the next
/// event), and an `AbortSignal` makes a buffer-empty `next()` reject.
#[no_mangle]
pub unsafe extern "C" fn js_events_on(
    target_value: f64,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut ArrayHeader {
    use perry_runtime::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };

    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let target_value = scope.root_nanbox_f64(target_value);
    let event_name = scope.root_string_ptr(event_name_ptr);
    let options = scope.root_nanbox_f64(options);
    let target = event_helper_target(target_value.get_nanbox_f64())
        .unwrap_or_else(|| throw_invalid_emitter(target_value.get_nanbox_f64()));
    // `queue` is the returned async-iterable handle; `state` holds the buffer /
    // pending / done / abort bookkeeping and is kept alive through the handle's
    // `Symbol.asyncIterator` closure capture.
    let queue = scope.root_raw_mut_ptr(js_array_alloc(0));
    let state = scope.root_raw_mut_ptr(events_on_state_new());
    install_events_on_async_iterator(queue.get_raw_mut_ptr(), state.get_raw_mut_ptr());
    if string_from_header(event_name.get_raw_const_ptr()).is_none() {
        return queue.get_raw_mut_ptr();
    }
    let signal =
        options_signal_or_throw(options.get_nanbox_f64()).map(|s| scope.root_nanbox_f64(s));
    if signal
        .as_ref()
        .is_some_and(|signal| signal_is_aborted(signal.get_nanbox_f64()))
    {
        perry_runtime::exception::js_throw(perry_runtime::url::js_abort_error_value());
    }

    let listener = scope.root_raw_mut_ptr(js_closure_alloc(
        perry_runtime::fn_info!(events_on_queue_listener, 1; with_rest(0)),
        1,
    ));
    js_closure_set_capture_ptr(
        listener.get_raw_mut_ptr(),
        0,
        state.get_raw_mut_ptr::<ArrayHeader>() as i64,
    );
    let event = scope.root_nanbox_f64(event_value(event_name.get_raw_const_ptr()));
    let listener_value = || js_nanbox_pointer(listener.get_raw_mut_ptr::<ClosureHeader>() as i64);

    match target {
        EventHelperTarget::EventTarget(target) => {
            perry_runtime::event_target::js_event_target_add_event_listener(
                target,
                event_name.get_raw_const_ptr(),
                listener.get_raw_mut_ptr::<ClosureHeader>() as i64,
            );
        }
        EventHelperTarget::Emitter(_) => {
            let _ = call_emitter_method(
                target_value.get_nanbox_f64(),
                "on",
                &[event.get_nanbox_f64(), listener_value()],
            );
        }
    }

    // Record the emitter, listener and event so `return()` / abort detach.
    let state_ptr = state.get_raw_mut_ptr::<ArrayHeader>();
    events_on_state_set(state_ptr, EVENTS_ON_EMITTER, target_value.get_nanbox_f64());
    events_on_state_set(state_ptr, EVENTS_ON_LISTENER, listener_value());
    events_on_state_set(state_ptr, EVENTS_ON_EVENT, event.get_nanbox_f64());

    if let Some(signal) = &signal {
        if object_ptr_from_value(signal.get_nanbox_f64()).is_some() {
            let abort_listener = scope.root_raw_mut_ptr(js_closure_alloc(
                perry_runtime::fn_info!(events_on_abort_listener, 0),
                2,
            ));
            js_closure_set_capture_f64(
                abort_listener.get_raw_mut_ptr(),
                0,
                js_nanbox_pointer(state.get_raw_mut_ptr::<ArrayHeader>() as i64),
            );
            js_closure_set_capture_f64(
                abort_listener.get_raw_mut_ptr(),
                1,
                signal.get_nanbox_f64(),
            );
            let abort_event = scope.root_nanbox_f64(abort_event_value());
            if let Some(signal_ptr) = object_ptr_from_value(signal.get_nanbox_f64()) {
                perry_runtime::url::js_abort_signal_add_listener(
                    signal_ptr,
                    abort_event.get_nanbox_f64(),
                    js_nanbox_pointer(abort_listener.get_raw_mut_ptr::<ClosureHeader>() as i64),
                );
            }
        }
    }

    queue.get_raw_mut_ptr()
}
