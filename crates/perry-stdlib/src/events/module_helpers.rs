//! Module-level `events.*` helper aliases.
//!
//! `events.addAbortListener`, `events.getEventListeners`,
//! `events.listenerCount`, `events.getMaxListeners`, `events.setMaxListeners`,
//! and the legacy `events.init()` no-op. Moved verbatim from the `events.rs`
//! trunk during the file split.

use super::*;

use perry_runtime::{
    js_array_alloc, js_array_length, js_array_push_f64, js_closure_call0, js_closure_call1,
    js_closure_call2, js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, js_object_alloc,
    js_object_get_field_by_name_f64, js_promise_new, js_promise_reject, js_promise_resolve,
    js_string_from_bytes, ArrayHeader, ClosureHeader, JSValue, ObjectHeader, Promise, StringHeader,
};
use std::collections::{HashMap, HashSet};

use crate::common::{for_each_handle_mut_of, get_handle, get_handle_mut, Handle};

extern "C" fn events_abort_listener_dispose(closure: *const ClosureHeader) -> f64 {
    use perry_runtime::closure::js_closure_get_capture_ptr;

    let signal_ptr = js_closure_get_capture_ptr(closure, 0);
    let callback_ptr = js_closure_get_capture_ptr(closure, 1);
    if signal_ptr != 0 && callback_ptr != 0 {
        let event_name = b"abort";
        let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
        let event_val = js_nanbox_string(event_str as i64);
        let listener_val = js_nanbox_pointer(callback_ptr);
        perry_runtime::url::js_abort_signal_remove_listener(
            signal_ptr as *mut perry_runtime::ObjectHeader,
            event_val,
            listener_val,
        );
    }

    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

/// `events.addAbortListener(signal, listener)` — attach listener to AbortSignal
/// and return a disposable-shaped object whose `Symbol.dispose` unregisters it.
#[no_mangle]
pub unsafe extern "C" fn js_events_add_abort_listener(signal: f64, listener: f64) -> i64 {
    use perry_runtime::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    let signal = validate_abort_signal_arg(signal, "signal");
    let signal_ptr = object_ptr_from_value(signal).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "signal",
            "AbortSignal",
            signal,
        ))
    });
    let callback_ptr = validate_listener_arg(listener, "listener");

    let event_name = b"abort";
    let event_str = js_string_from_bytes(event_name.as_ptr(), event_name.len() as u32);
    let event_val = js_nanbox_string(event_str as i64);
    let listener_val = js_nanbox_pointer(callback_ptr);
    perry_runtime::url::js_abort_signal_add_listener(signal_ptr, event_val, listener_val);

    let dispose_closure = js_closure_alloc(events_abort_listener_dispose as *const u8, 2);
    js_closure_set_capture_ptr(dispose_closure, 0, signal_ptr as i64);
    js_closure_set_capture_ptr(dispose_closure, 1, callback_ptr);
    let dispose_val = js_nanbox_pointer(dispose_closure as i64);

    let disposable = js_object_alloc(0, 0);
    let disposable_val = js_nanbox_pointer(disposable as i64);
    let dispose_sym = perry_runtime::symbol::well_known_symbol("dispose");
    let dispose_sym_val = js_nanbox_pointer(dispose_sym as i64);
    perry_runtime::symbol::js_object_set_symbol_property(
        disposable_val,
        dispose_sym_val,
        dispose_val,
    );
    disposable as i64
}

/// `events.getEventListeners(emitter, eventName)` — alias for
/// `emitter.listeners(eventName)`.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_event_listeners(
    target_value: f64,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    // AbortSignal is an EventTarget in Node, but Perry represents it as its
    // own native object (url/abort.rs) that `event_helper_target` doesn't
    // recognize. A signal only ever tracks "abort" listeners.
    let signal_ptr = perry_runtime::url::abort::js_abort_signal_resolve_ptr(target_value);
    if !signal_ptr.is_null() {
        if string_from_header(event_name_ptr).as_deref() == Some("abort") {
            return perry_runtime::url::abort::js_abort_signal_listeners_copy(signal_ptr);
        }
        return js_array_alloc(0);
    }
    match event_helper_target(target_value).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "emitter",
            "EventEmitter or EventTarget",
            target_value,
        ))
    }) {
        EventHelperTarget::EventEmitter(handle) => {
            js_event_emitter_listeners(handle, event_bits_from_string_ptr(event_name_ptr))
        }
        EventHelperTarget::EventTarget(target) => {
            perry_runtime::event_target::js_event_target_get_event_listeners(target, event_name_ptr)
        }
        EventHelperTarget::Stream(handle) => {
            stream_listeners_for_heap_object(handle, event_name_ptr)
                .unwrap_or_else(|| js_array_alloc(0))
        }
    }
}

/// `events.listenerCount(emitter, eventName)` — alias for
/// `emitter.listenerCount(eventName)`.
#[no_mangle]
pub unsafe extern "C" fn js_events_listener_count(
    target_value: f64,
    event_name_ptr: *const StringHeader,
) -> f64 {
    // AbortSignal: see `js_events_get_event_listeners`.
    let signal_ptr = perry_runtime::url::abort::js_abort_signal_resolve_ptr(target_value);
    if !signal_ptr.is_null() {
        if string_from_header(event_name_ptr).as_deref() == Some("abort") {
            return perry_runtime::url::abort::js_abort_signal_listener_count(signal_ptr);
        }
        return 0.0;
    }
    match event_helper_target(target_value).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "emitter",
            "EventEmitter or EventTarget",
            target_value,
        ))
    }) {
        EventHelperTarget::EventEmitter(handle) => js_event_emitter_listener_count(
            handle,
            event_bits_from_string_ptr(event_name_ptr),
            undefined_bits(),
        ),
        EventHelperTarget::EventTarget(target) => event_target_array_len(target, event_name_ptr),
        EventHelperTarget::Stream(handle) => {
            let event = js_nanbox_string(event_name_ptr as i64);
            perry_runtime::node_stream::js_node_stream_method_listener_count(handle, event)
        }
    }
}

/// `events.getMaxListeners(emitter)` — alias.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_max_listeners(target_value: f64) -> f64 {
    // AbortSignal: Node's default EventTarget listener cap. Perry stores no
    // per-signal override (`setMaxListeners` below is an accepted no-op), so
    // the default is always reported.
    if !perry_runtime::url::abort::js_abort_signal_resolve_ptr(target_value).is_null() {
        return 10.0;
    }
    match event_helper_target(target_value).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "emitter",
            "EventEmitter or EventTarget",
            target_value,
        ))
    }) {
        EventHelperTarget::EventEmitter(handle) => js_event_emitter_get_max_listeners(handle),
        EventHelperTarget::EventTarget(target) => {
            perry_runtime::event_target::js_event_target_get_max_listeners(target)
        }
        EventHelperTarget::Stream(handle) => {
            perry_runtime::node_stream::js_node_stream_method_get_max_listeners(handle)
        }
    }
}

/// `events.setMaxListeners(n, ...targets)` — codegen passes the varargs
/// target list as a Perry array of EventEmitter handles and EventTarget
/// object pointers.
#[no_mangle]
pub unsafe extern "C" fn js_events_set_max_listeners(
    n: f64,
    handles_ptr: *const ArrayHeader,
) -> f64 {
    let n = validate_max_listeners(n);
    if !handles_ptr.is_null() {
        let len = js_array_length(handles_ptr);
        for i in 0..len {
            let value = perry_runtime::array::js_array_get_f64(handles_ptr, i);
            // AbortSignal is an EventTarget in Node — SDKs routinely call
            // `events.setMaxListeners(n, controller.signal)` to raise the
            // MaxListenersExceededWarning threshold on a shared signal. Perry
            // represents signals as their own native object that
            // `event_helper_target` doesn't recognize, so this threw
            // ERR_INVALID_ARG_TYPE and rejected the caller's whole request
            // path. Accept the signal; the warning threshold is the call's
            // only Node-observable effect and Perry never emits that warning
            // for signals, so accepting is a faithful no-op.
            if !perry_runtime::url::abort::js_abort_signal_resolve_ptr(value).is_null() {
                continue;
            }
            match event_helper_target(value).unwrap_or_else(|| {
                throw_invalid_arg_type(&invalid_instance_arg_message(
                    "eventTargets",
                    "EventEmitter or EventTarget",
                    value,
                ))
            }) {
                EventHelperTarget::EventEmitter(handle) => {
                    if let Some(emitter) = get_handle_mut::<EventEmitterHandle>(handle) {
                        emitter.max_listeners = n;
                    }
                }
                EventHelperTarget::EventTarget(target) => {
                    let _ =
                        perry_runtime::event_target::js_event_target_set_max_listeners(target, n);
                }
                EventHelperTarget::Stream(handle) => {
                    let _ = perry_runtime::node_stream::js_node_stream_method_set_max_listeners(
                        handle, n,
                    );
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

/// Legacy `events.init()` no-op export retained for Node surface parity.
#[no_mangle]
pub extern "C" fn js_events_init() -> f64 {
    undefined_value()
}
