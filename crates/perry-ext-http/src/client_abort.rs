//! AbortSignal wiring for ClientRequest.

use super::*;

extern "C" {
    fn js_abort_signal_resolve_ptr(value: f64) -> *mut ObjectHeader;
    fn js_abort_signal_is_aborted(signal: *mut ObjectHeader) -> i32;
    fn js_abort_signal_add_listener(signal: *mut ObjectHeader, event_type: f64, listener: f64);
    fn js_abort_signal_remove_listener(signal: *mut ObjectHeader, event_type: f64, listener: f64);
    fn js_abort_error_value() -> f64;
}

fn abort_event_value() -> f64 {
    f64::from_bits(JsValue::from_string_ptr(alloc_string("abort").as_raw()).bits())
}

extern "C" fn request_signal_listener(closure: *const RawClosureHeader) -> f64 {
    let request_handle = unsafe { perry_ffi::closure_capture_f64(closure, 0) } as i64;
    push_event(PendingHttpEvent::SignalAbort { request_handle });
    f64::from_bits(TAG_UNDEFINED)
}

pub(crate) unsafe fn attach_request_signal(request_handle: Handle, options: f64) {
    let signal_value =
        perry_ffi::object_field_by_name(JsValue::from_bits(options.to_bits()), "signal");
    if signal_value.is_undefined() || signal_value.is_null() {
        return;
    }
    let signal_f64 = f64::from_bits(signal_value.bits());
    let signal = js_abort_signal_resolve_ptr(signal_f64);
    if signal.is_null() {
        return;
    }
    if js_abort_signal_is_aborted(signal) != 0 {
        push_event(PendingHttpEvent::SignalAbort { request_handle });
        return;
    }

    let listener = perry_ffi::alloc_closure(request_signal_listener as *const u8, 1);
    perry_ffi::set_closure_capture_f64(listener, 0, request_handle as f64);
    let listener_value = f64::from_bits(POINTER_TAG | (listener as u64 & PTR_MASK));
    js_abort_signal_add_listener(signal, abort_event_value(), listener_value);
    with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |request| {
        request.abort_signal_bits = signal_value.bits();
        request.abort_listener_bits = listener_value.to_bits();
    });
}

pub(crate) unsafe fn cleanup_request_signal(request_handle: Handle) {
    let Some((signal_bits, listener_bits)) =
        with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |request| {
            let values = (request.abort_signal_bits, request.abort_listener_bits);
            request.abort_signal_bits = 0;
            request.abort_listener_bits = 0;
            values
        })
    else {
        return;
    };
    if signal_bits == 0 || listener_bits == 0 {
        return;
    }
    let signal = js_abort_signal_resolve_ptr(f64::from_bits(signal_bits));
    if !signal.is_null() {
        js_abort_signal_remove_listener(signal, abort_event_value(), f64::from_bits(listener_bits));
    }
}

pub(crate) unsafe fn handle_request_signal_abort(request_handle: Handle) {
    let already_done = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |request| {
        let was = request.completed;
        request.completed = true;
        was
    })
    .unwrap_or(true);
    cleanup_request_signal(request_handle);
    if already_done {
        return;
    }
    client_events::fire_request_error_listeners(request_handle, js_abort_error_value());
    client_events::fire_request_close_once(request_handle);
    if let Some((agent_handle, socket)) = get_handle_mut::<ClientRequestHandle>(request_handle)
        .map(|request| (request.agent_handle, request.socket_handle))
    {
        if agent_handle == 0 && socket != 0 {
            perry_ext_net::js_ext_net_destroy_socket(socket);
        }
    }
    finish_agent_request(request_handle, false);
}
