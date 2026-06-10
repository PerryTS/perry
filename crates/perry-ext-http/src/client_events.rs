//! #4905 — client-request event helpers for the pending-event drain
//! loop: no-arg/error listener firing, transport-error → Node-coded
//! Error mapping, and the `'timeout'` event flow.

use super::*;

/// Fire a client request's `event` listeners with no arguments.
///
/// # Safety
///
/// Listener entries are raw closure headers registered via `.on()`; they
/// stay live for the program's lifetime (GC scanner pins them).
pub(crate) unsafe fn fire_request_event_listeners(request_handle: Handle, event: &str) {
    let listeners = get_handle_mut::<ClientRequestHandle>(request_handle)
        .and_then(|r| r.listeners.get(event).cloned())
        .unwrap_or_default();
    for cb in listeners {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call0();
        }
    }
}

/// Fire a client request's `'error'` listeners with `arg`.
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn fire_request_error_listeners(request_handle: Handle, arg: f64) {
    let listeners = get_handle_mut::<ClientRequestHandle>(request_handle)
        .and_then(|r| r.listeners.get("error").cloned())
        .unwrap_or_default();
    for cb in listeners {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call1(arg);
        }
    }
}

/// #4905 — map a transport error message to the value handed to
/// `'error'` listeners. Recognized shapes become real Error objects
/// carrying the Node `.code` (corpus tests assert
/// `err.code === 'ECONNRESET'`); unrecognized messages keep the legacy
/// string argument so existing consumers are unaffected.
pub(crate) fn error_event_arg(error_message: &str) -> f64 {
    let lower = error_message.to_lowercase();
    let coded = if lower.contains("connection reset")
        || lower.contains("incompletemessage")
        || lower.contains("connection closed before")
    {
        Some(("socket hang up".to_string(), "ECONNRESET"))
    } else if lower.contains("connection refused") {
        Some((error_message.to_string(), "ECONNREFUSED"))
    } else {
        None
    };
    match coded {
        Some((msg, code)) => f64::from_bits(
            perry_ffi::error_value_with_code(&msg, code, perry_ffi::ErrorKind::Error).bits(),
        ),
        None => {
            let s = alloc_string(error_message);
            f64::from_bits(STRING_TAG | (s.as_raw() as u64 & PTR_MASK))
        }
    }
}

/// #4905 — drain handler for `PendingHttpEvent::Timeout`: fire
/// `'timeout'` listeners (falling back to the legacy error surface when
/// none are registered), honor an in-handler `req.destroy()` with the
/// coded ECONNRESET "socket hang up" error, then fire `'close'`.
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_timeout_event(request_handle: Handle) {
    let timeout_listeners = get_handle_mut::<ClientRequestHandle>(request_handle)
        .and_then(|r| r.listeners.get("timeout").cloned())
        .unwrap_or_default();
    if timeout_listeners.is_empty() {
        // No `'timeout'` listener — keep the legacy error surface.
        fire_request_error_listeners(request_handle, error_event_arg("request timed out"));
    } else {
        for cb in timeout_listeners {
            if cb != 0 {
                let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
                let _ = closure.call0();
            }
        }
        // Node's `'timeout'` doesn't tear the request down by itself, but
        // our transport deadline already aborted the exchange. The
        // canonical pattern destroys the request in the handler and
        // expects ECONNRESET "socket hang up"; honor that destroy with
        // the coded error.
        if client_request_surface::request_destroyed(request_handle) {
            fire_request_error_listeners(
                request_handle,
                f64::from_bits(
                    perry_ffi::error_value_with_code(
                        "socket hang up",
                        "ECONNRESET",
                        perry_ffi::ErrorKind::Error,
                    )
                    .bits(),
                ),
            );
        }
    }
    fire_request_event_listeners(request_handle, "close");
}
