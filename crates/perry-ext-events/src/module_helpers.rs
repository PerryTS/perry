//! Module-level EventEmitter introspection helpers and indirect dispatcher.

use super::*;

extern "C" {
    fn js_native_call_method_str_key(
        object: f64,
        name_handle: i64,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64;
}

pub(super) unsafe fn call_net_socket_method(handle: Handle, name: &str, args: &[f64]) -> f64 {
    let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_native_call_method_str_key(
        nanbox_pointer_bits(handle),
        name_ptr as i64,
        args.as_ptr(),
        args.len(),
    )
}

/// `events.getEventListeners(emitter, eventName)` — alias for
/// `emitter.listeners(eventName)`.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_event_listeners(
    target_value: f64,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    let signal_ptr = js_abort_signal_resolve_ptr(target_value);
    if !signal_ptr.is_null() {
        if string_from_header(event_name_ptr).as_deref() == Some("abort") {
            return js_abort_signal_listeners_copy(signal_ptr);
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
            js_event_emitter_listeners(handle, event_name_ptr as i64)
        }
        EventHelperTarget::EventTarget(target) => {
            js_event_target_get_event_listeners(target, event_name_ptr)
        }
        EventHelperTarget::NetSocket(handle) | EventHelperTarget::NativeHandle(handle) => {
            let event = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
            let value = call_net_socket_method(handle, "listeners", &[event]);
            (value.to_bits() & POINTER_MASK) as *mut ArrayHeader
        }
        EventHelperTarget::Stream(handle) => {
            stream_listeners_for_heap_object(handle, event_name_ptr)
                .unwrap_or_else(|| js_array_alloc(0))
        }
    }
}

/// `events.listenerCount(emitter, eventName)` — alias.
///
/// # Safety
///
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_events_listener_count(
    target_value: f64,
    event_name_ptr: *const StringHeader,
) -> f64 {
    let signal_ptr = js_abort_signal_resolve_ptr(target_value);
    if !signal_ptr.is_null() {
        if string_from_header(event_name_ptr).as_deref() == Some("abort") {
            return js_abort_signal_listener_count(signal_ptr);
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
            event_name_ptr as i64,
            TAG_UNDEFINED_F64_BITS as i64,
        ),
        EventHelperTarget::EventTarget(target) => event_target_array_len(target, event_name_ptr),
        EventHelperTarget::NetSocket(handle) | EventHelperTarget::NativeHandle(handle) => {
            let event = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
            call_net_socket_method(handle, "listenerCount", &[event])
        }
        EventHelperTarget::Stream(handle) => {
            stream_array_len(handle, event_name_ptr).unwrap_or(0.0)
        }
    }
}

/// `events.getMaxListeners(emitter)` — alias.
#[no_mangle]
pub unsafe extern "C" fn js_events_get_max_listeners(target_value: f64) -> f64 {
    if !js_abort_signal_resolve_ptr(target_value).is_null() {
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
        EventHelperTarget::EventTarget(target) => js_event_target_get_max_listeners(target),
        EventHelperTarget::NetSocket(_) | EventHelperTarget::NativeHandle(_) => 10.0,
        EventHelperTarget::Stream(handle) => js_node_stream_method_get_max_listeners(handle),
    }
}

/// `events.setMaxListeners(n, ...emitters)` — Perry FFI takes a single
/// array of target handles from the codegen varargs lowering.
#[no_mangle]
pub unsafe extern "C" fn js_events_set_max_listeners(
    n: f64,
    handles_ptr: *const ArrayHeader,
) -> f64 {
    let n = validate_max_listeners(n);
    if !handles_ptr.is_null() {
        let len = (*handles_ptr).length;
        for i in 0..len {
            let value = f64::from_bits(js_array_get(handles_ptr, i).bits());
            if !js_abort_signal_resolve_ptr(value).is_null() {
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
                    if let Some(emitter) = get_event_emitter_mut(handle) {
                        emitter.max_listeners = n;
                    }
                }
                EventHelperTarget::EventTarget(target) => {
                    let _ = js_event_target_set_max_listeners(target, n);
                }
                EventHelperTarget::NetSocket(_) | EventHelperTarget::NativeHandle(_) => {}
                EventHelperTarget::Stream(handle) => {
                    let _ = js_node_stream_method_set_max_listeners(handle, n);
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED_F64_BITS)
}

unsafe fn event_name_header(value: f64) -> *const StringHeader {
    js_jsvalue_to_string(value)
}

/// Indirect/captured `events.*` helper dispatcher for the external events
/// implementation. CommonJS destructuring resolves through this hook.
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
    let arg = |index: usize| {
        if !args.is_null() && index < args_len {
            *args.add(index)
        } else {
            undefined
        }
    };
    match name {
        "listenerCount" => js_events_listener_count(arg(0), event_name_header(arg(1))),
        "getMaxListeners" => js_events_get_max_listeners(arg(0)),
        "getEventListeners" => {
            let array = js_events_get_event_listeners(arg(0), event_name_header(arg(1)));
            if array.is_null() {
                undefined
            } else {
                nanbox_pointer_bits(array as i64)
            }
        }
        "once" => {
            let promise = js_events_once(arg(0), event_name_header(arg(1)), arg(2));
            if promise.is_null() {
                undefined
            } else {
                nanbox_pointer_bits(promise as i64)
            }
        }
        "on" => {
            let iterator = js_events_on(arg(0), event_name_header(arg(1)), arg(2));
            if iterator.is_null() {
                undefined
            } else {
                nanbox_pointer_bits(iterator as i64)
            }
        }
        "addAbortListener" => {
            let disposable = js_events_add_abort_listener(arg(0), arg(1));
            if disposable == 0 {
                undefined
            } else {
                nanbox_pointer_bits(disposable)
            }
        }
        "setMaxListeners" => {
            let scope = perry_ffi::TransientRootScope::enter();
            let target_values = (1..args_len)
                .map(|index| scope.root_nanbox(arg(index)))
                .collect::<Vec<_>>();
            let targets = js_array_alloc(target_values.len() as u32);
            let targets = scope.root_nanbox(nanbox_pointer_bits(targets as i64));
            for target in target_values {
                let current = (targets.get().to_bits() & POINTER_MASK) as *mut ArrayHeader;
                let _ = js_array_push_f64(current, target.get());
            }
            let targets = (targets.get().to_bits() & POINTER_MASK) as *mut ArrayHeader;
            js_events_set_max_listeners(arg(0), targets)
        }
        _ => undefined,
    }
}
