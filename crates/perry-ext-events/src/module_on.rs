use super::*;

#[no_mangle]
pub unsafe extern "C" fn js_events_on(
    target_value: f64,
    event_name_ptr: *const StringHeader,
    options: f64,
) -> *mut ArrayHeader {
    ensure_gc_scanner_registered();
    let root_scope = TransientRootScope::enter();
    let target_root = root_scope.root_nanbox(target_value);
    let event_name_root = root_scope.root_nanbox(f64::from_bits(nanbox_string_bits(
        string_header_ptr_from_arg(event_name_ptr) as *mut StringHeader,
    )));
    let _ = event_helper_target(target_root.get())
        .unwrap_or_else(|| throw_invalid_emitter(target_root.get()));
    let queue = js_array_alloc(0);
    let queue_root = root_scope.root_nanbox(nanbox_pointer_bits(queue as i64));
    let state = events_on_state_new();
    let state_root = root_scope.root_nanbox(nanbox_pointer_bits(state as i64));
    events_on_install_async_iterator(
        (queue_root.get().to_bits() & POINTER_MASK) as *mut ArrayHeader,
        (state_root.get().to_bits() & POINTER_MASK) as *mut ArrayHeader,
    );
    let Some(event_name) = event_name_from_bits(event_name_root.get().to_bits() as i64) else {
        return (queue_root.get().to_bits() & POINTER_MASK) as *mut ArrayHeader;
    };
    let event_name_ptr = (event_name_root.get().to_bits() & POINTER_MASK) as *const StringHeader;
    let signal = options_signal_or_throw(options);
    if signal.is_some_and(signal_is_aborted) {
        js_throw(js_abort_error_value());
    }
    let listener = js_closure_alloc(events_on_queue_listener as *const u8, 1);
    js_closure_set_capture_ptr(
        listener,
        0,
        (state_root.get().to_bits() & POINTER_MASK) as i64,
    );
    let listener_root = root_scope.root_addr(listener as i64);
    let target = event_helper_target(target_root.get())
        .unwrap_or_else(|| throw_invalid_emitter(target_root.get()));
    let (handle, cleanup_target, cleanup_kind) = match target {
        EventHelperTarget::EventEmitter(handle) => {
            if let Some(emitter) = get_event_emitter_mut(handle) {
                emitter.add_listener(handle, &event_name, listener_root.get(), false, false);
            }
            (handle, handle as f64, EVENTS_ON_EVENT_EMITTER)
        }
        EventHelperTarget::EventTarget(target) => {
            if !event_name_ptr.is_null() {
                js_event_target_add_event_listener(target, event_name_ptr, listener_root.get());
            }
            (
                target as Handle,
                nanbox_pointer_bits(target as i64),
                EVENTS_ON_EVENT_TARGET,
            )
        }
        EventHelperTarget::NetSocket(handle) | EventHelperTarget::NativeHandle(handle) => {
            if !event_name_ptr.is_null() {
                let event = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
                let listener_value = nanbox_pointer_bits(listener_root.get());
                let _ = call_net_socket_method(handle, "on", &[event, listener_value]);
            }
            (handle, handle as f64, EVENTS_ON_NET_HANDLE)
        }
        EventHelperTarget::Stream(handle) => {
            if !event_name_ptr.is_null() {
                let event = f64::from_bits(nanbox_string_bits(event_name_ptr as *mut StringHeader));
                let listener_value = nanbox_pointer_bits(listener_root.get());
                let _ = js_node_stream_method_on(handle, event, listener_value);
            }
            (handle, handle as f64, EVENTS_ON_STREAM)
        }
    };
    events_on_state_set_target(
        (state_root.get().to_bits() & POINTER_MASK) as *mut ArrayHeader,
        cleanup_target,
        listener_root.get() as *mut RawClosureHeader,
        event_name_root.get(),
        cleanup_kind,
    );
    if let Some(close) = get_object_property(options, b"close") {
        if js_array_is_array(close).to_bits() == TAG_TRUE_F64_BITS {
            let close_array = (close.to_bits() & POINTER_MASK) as *mut ArrayHeader;
            if !close_array.is_null() {
                for index in 0..js_array_length(close_array) {
                    let close_value = f64::from_bits(js_array_get(close_array, index).bits());
                    let Some(close_name) = event_name_from_bits(close_value.to_bits() as i64)
                    else {
                        continue;
                    };
                    let close_ptr =
                        js_string_from_bytes(close_name.as_ptr(), close_name.len() as u32);
                    let close_listener = js_closure_alloc(events_on_close_listener as *const u8, 1);
                    js_closure_set_capture_ptr(
                        close_listener,
                        0,
                        (state_root.get().to_bits() & POINTER_MASK) as i64,
                    );
                    let close_listener_root = root_scope.root_addr(close_listener as i64);
                    match target {
                        EventHelperTarget::EventEmitter(emitter_handle) => {
                            if let Some(emitter) = get_event_emitter_mut(emitter_handle) {
                                emitter.add_listener(
                                    emitter_handle,
                                    &close_name,
                                    close_listener_root.get(),
                                    true,
                                    false,
                                );
                            }
                        }
                        EventHelperTarget::EventTarget(event_target) => {
                            js_event_target_add_event_listener(
                                event_target,
                                close_ptr,
                                close_listener_root.get(),
                            );
                        }
                        EventHelperTarget::NetSocket(handle)
                        | EventHelperTarget::NativeHandle(handle) => {
                            let event = f64::from_bits(nanbox_string_bits(close_ptr));
                            let callback = nanbox_pointer_bits(close_listener_root.get());
                            let _ = call_net_socket_method(handle, "once", &[event, callback]);
                        }
                        EventHelperTarget::Stream(handle) => {
                            let event = f64::from_bits(nanbox_string_bits(close_ptr));
                            let callback = nanbox_pointer_bits(close_listener_root.get());
                            let _ = js_node_stream_method_once(handle, event, callback);
                        }
                    }
                }
            }
        }
    }
    if let Some(signal) = signal {
        if let Some(signal_ptr) = object_ptr_from_value(signal) {
            let abort_listener = js_closure_alloc(events_on_abort_listener as *const u8, 5);
            js_closure_set_capture_ptr(abort_listener, 0, handle);
            js_closure_set_capture_ptr(abort_listener, 1, listener_root.get());
            js_closure_set_capture_ptr(abort_listener, 2, signal_ptr as i64);
            js_closure_set_capture_ptr(
                abort_listener,
                3,
                (state_root.get().to_bits() & POINTER_MASK) as i64,
            );
            js_closure_set_capture_ptr(abort_listener, 4, event_name_ptr as i64);
            js_abort_signal_add_listener(
                signal_ptr as *mut u8,
                abort_event_value(),
                nanbox_pointer_bits(abort_listener as i64),
            );
        }
    }
    (queue_root.get().to_bits() & POINTER_MASK) as *mut ArrayHeader
}

extern "C" fn events_abort_listener_dispose(closure: *const RawClosureHeader) -> f64 {
    unsafe {
        let signal_ptr = js_closure_get_capture_ptr(closure, 0);
        let callback_ptr = js_closure_get_capture_ptr(closure, 1);
        if signal_ptr != 0 && callback_ptr != 0 {
            js_abort_signal_remove_listener(
                signal_ptr as *mut u8,
                abort_event_value(),
                nanbox_pointer_bits(callback_ptr),
            );
        }
    }
    undefined_value()
}

#[no_mangle]
pub unsafe extern "C" fn js_events_add_abort_listener(signal: f64, listener: f64) -> i64 {
    let root_scope = TransientRootScope::enter();
    let signal_root = root_scope.root_nanbox(signal);
    let listener_root = root_scope.root_nanbox(listener);
    let signal = validate_abort_signal_arg(signal_root.get(), "signal");
    let signal_ptr = object_ptr_from_value(signal_root.get()).unwrap_or_else(|| {
        throw_invalid_arg_type(&invalid_instance_arg_message(
            "signal",
            "AbortSignal",
            signal,
        ))
    });
    let callback_ptr = validate_event_listener(listener_root.get().to_bits() as i64);
    js_abort_signal_add_listener(
        signal_ptr as *mut u8,
        abort_event_value(),
        nanbox_pointer_bits(callback_ptr),
    );
    let dispose_closure = js_closure_alloc(events_abort_listener_dispose as *const u8, 2);
    let dispose_closure_root = root_scope.root_addr(dispose_closure as i64);
    let signal_ptr = object_ptr_from_value(signal_root.get())
        .expect("validated AbortSignal remained a rooted object");
    let callback_ptr = validate_event_listener(listener_root.get().to_bits() as i64);
    js_closure_set_capture_ptr(
        dispose_closure_root.get() as *mut RawClosureHeader,
        0,
        signal_ptr as i64,
    );
    js_closure_set_capture_ptr(
        dispose_closure_root.get() as *mut RawClosureHeader,
        1,
        callback_ptr,
    );
    let disposable = js_object_alloc(0, 0);
    let disposable_root = root_scope.root_addr(disposable as i64);
    let dispose_key = b"@@__perry_wk_dispose";
    let dispose_key_ptr = js_string_from_bytes(dispose_key.as_ptr(), dispose_key.len() as u32);
    let dispose_sym_val = js_symbol_for(f64::from_bits(nanbox_string_bits(dispose_key_ptr)));
    js_object_set_symbol_property(
        nanbox_pointer_bits(disposable_root.get()),
        dispose_sym_val,
        nanbox_pointer_bits(dispose_closure_root.get()),
    );
    disposable_root.get()
}
