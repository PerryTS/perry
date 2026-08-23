//! Module-level `events.*` helper aliases.
//!
//! `events.addAbortListener`, `events.getEventListeners`,
//! `events.listenerCount`, `events.getMaxListeners`, `events.setMaxListeners`,
//! and the legacy `events.init()` no-op. Moved verbatim from the `events.rs`
//! trunk during the file split.

use super::*;

use perry_runtime::{
    js_array_alloc, js_array_length, js_nanbox_pointer, js_nanbox_string, js_object_alloc,
    js_string_from_bytes, ArrayHeader, ClosureHeader, StringHeader,
};

use crate::common::get_handle_mut;

pub(super) unsafe fn call_net_socket_method(handle: i64, name: &str, args: &[f64]) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let name = scope.root_string_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
    let args = args
        .iter()
        .map(|value| scope.root_nanbox_f64(*value))
        .collect::<Vec<_>>();
    let args = args
        .iter()
        .map(|value| value.get_nanbox_f64())
        .collect::<Vec<_>>();
    perry_runtime::object::js_native_call_method_str_key(
        js_nanbox_pointer(handle),
        name.get_raw_const_ptr::<StringHeader>() as i64,
        args.as_ptr(),
        args.len(),
    )
}

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
        EventHelperTarget::NetSocket(handle) => {
            if event_name_ptr.is_null() {
                return js_array_alloc(0);
            }
            let result = call_net_socket_method(
                handle,
                "listeners",
                &[js_nanbox_string(event_name_ptr as i64)],
            );
            let value = JSValue::from_bits(result.to_bits());
            if value.is_pointer() {
                value.as_pointer::<ArrayHeader>() as *mut ArrayHeader
            } else {
                js_array_alloc(0)
            }
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
        EventHelperTarget::NetSocket(handle) => call_net_socket_method(
            handle,
            "listenerCount",
            &[js_nanbox_string(event_name_ptr as i64)],
        ),
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
        EventHelperTarget::NetSocket(handle) => {
            call_net_socket_method(handle, "getMaxListeners", &[])
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
                EventHelperTarget::NetSocket(handle) => {
                    let _ = call_net_socket_method(handle, "setMaxListeners", &[n]);
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

/// Coerce a NaN-boxed event-name argument to a heap string, the way the static
/// call's `NA_STR` marshalling does. A non-string goes through `ToString`
/// (Node keys listeners by the coerced name), and a missing argument becomes
/// `"undefined"` rather than a null pointer, so the helpers below see exactly
/// what the static path would hand them.
unsafe fn event_name_header(value: f64) -> *const StringHeader {
    let materialized = perry_runtime::string::js_string_materialize_to_heap(value);
    if !materialized.is_null() {
        return materialized;
    }
    perry_runtime::value::js_jsvalue_to_string(value)
}

/// Runtime bridge for the module-level `events.*` helpers reached INDIRECTLY —
/// a captured value (`const c = events.listenerCount; c(e, "x")`), a type-erased
/// receiver (`(events as any).listenerCount(e, "x")`), or a spread call
/// (`events.listenerCount(...args)`).
///
/// All of these route through `dispatch_native_module_method` →
/// `nm_dispatch_events` in perry-runtime, which cannot name these functions
/// (perry-stdlib depends on perry-runtime, not the reverse) and so answered
/// `undefined` for every one of them while the statically dispatched call went
/// straight to the same FFI and was correct. Registered in
/// `common/dispatch/init.rs`; mirrors the zlib / querystring / domain bridges.
///
/// The name list must stay in sync with the `has_receiver: false` `events` rows
/// of perry-codegen's `NativeModSig` table (`lower_call/native_table/net_events.rs`)
/// and the arm in `nm_dispatch_events` that calls this. `init` and
/// `EventEmitterAsyncResource` are deliberately absent — perry-runtime answers
/// those itself without needing stdlib.
#[no_mangle]
pub unsafe extern "C" fn js_events_native_dispatch(
    method: *const u8,
    method_len: usize,
    args: *const f64,
    args_len: usize,
) -> f64 {
    let undefined = f64::from_bits(TAG_UNDEFINED_F64_BITS);
    if method.is_null() || method_len == 0 {
        return undefined;
    }
    let name = std::str::from_utf8(std::slice::from_raw_parts(method, method_len)).unwrap_or("");
    let arg = |i: usize| -> f64 {
        if i < args_len && !args.is_null() {
            *args.add(i)
        } else {
            undefined
        }
    };
    match name {
        "listenerCount" => js_events_listener_count(arg(0), event_name_header(arg(1))),
        "getMaxListeners" => js_events_get_max_listeners(arg(0)),
        "getEventListeners" => {
            let arr = js_events_get_event_listeners(arg(0), event_name_header(arg(1)));
            if arr.is_null() {
                undefined
            } else {
                js_nanbox_pointer(arr as i64)
            }
        }
        "once" => {
            let promise = crate::events::js_events_once(arg(0), event_name_header(arg(1)), arg(2));
            if promise.is_null() {
                undefined
            } else {
                js_nanbox_pointer(promise as i64)
            }
        }
        "on" => {
            let iter = crate::events::js_events_on(arg(0), event_name_header(arg(1)), arg(2));
            if iter.is_null() {
                undefined
            } else {
                js_nanbox_pointer(iter as i64)
            }
        }
        "addAbortListener" => {
            let disposable = js_events_add_abort_listener(arg(0), arg(1));
            if disposable == 0 {
                undefined
            } else {
                js_nanbox_pointer(disposable)
            }
        }
        // `setMaxListeners(n, ...targets)`: the static path hands the trailing
        // targets over as ONE Perry array (`NA_VARARGS`), so rebuild that array
        // from the remaining arguments rather than passing them positionally.
        "setMaxListeners" => {
            let mut targets = js_array_alloc(args_len.saturating_sub(1) as u32);
            for i in 1..args_len {
                targets = perry_runtime::array::js_array_push_f64(targets, arg(i));
            }
            js_events_set_max_listeners(arg(0), targets)
        }
        _ => undefined,
    }
}
