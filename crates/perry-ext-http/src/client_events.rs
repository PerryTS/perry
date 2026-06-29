//! #4905 / #4909 — client-request event helpers for the pending-event
//! drain loop: response/error/timeout/flush handling, no-arg/error
//! listener firing, and transport-error → Node-coded Error mapping.

use super::*;

extern "C" {
    /// perry-runtime method dispatch by string key (`obj[name](args)`,
    /// binding `this = obj`). Used to drive `dest.write(chunk)` / `dest.end()`
    /// on a `.pipe(dest)` destination.
    fn js_native_call_method_str_key(
        object: f64,
        name_handle: i64,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64;
}

/// Forward a body chunk to a `.pipe(dest)` destination: `dest.write(chunk)`.
/// `dest_bits` is the NaN-boxed destination value captured by `pipe()`.
unsafe fn forward_pipe_write(dest_bits: u64, bytes: &[u8], encoding: Option<&str>) {
    if bytes.is_empty() {
        return;
    }
    let name = alloc_string("write");
    let name_ptr = name.as_raw();
    if name_ptr.is_null() {
        return;
    }
    let chunk = body_chunk_value(bytes, encoding);
    if chunk.to_bits() == TAG_UNDEFINED {
        return;
    }
    let args = [chunk];
    js_native_call_method_str_key(f64::from_bits(dest_bits), name_ptr as i64, args.as_ptr(), 1);
}

/// Finish a `.pipe(dest)` destination once the response body ends: `dest.end()`.
unsafe fn forward_pipe_end(dest_bits: u64) {
    let name = alloc_string("end");
    let name_ptr = name.as_raw();
    if name_ptr.is_null() {
        return;
    }
    js_native_call_method_str_key(
        f64::from_bits(dest_bits),
        name_ptr as i64,
        std::ptr::null(),
        0,
    );
}

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

/// Fire `'close'` exactly once per request (#4909 — the response, error,
/// timeout and destroy paths can each reach the close edge; Node emits it
/// a single time).
pub(crate) fn fire_request_close_once(request_handle: Handle) {
    let fire = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        if req.close_emitted {
            false
        } else {
            req.close_emitted = true;
            true
        }
    })
    .unwrap_or(false);
    if fire {
        unsafe {
            fire_request_event_listeners(request_handle, "close");
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

/// Drain handler for `PendingHttpEvent::Response`: build the
/// IncomingMessage handle, call the factory callback and `'response'`
/// listeners, deliver `'data'`/`'end'`, then `'close'` on the request.
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_response_event(
    request_handle: Handle,
    status: u16,
    status_message: String,
    headers: Vec<(String, String)>,
    trailers: Vec<(String, String)>,
    body: Vec<u8>,
) {
    // #4909 — a destroyed request delivers nothing (Node tears the
    // exchange down); `completed` also suppresses any late timeout timer.
    let already_done = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        let was = req.completed;
        req.completed = true;
        was
    })
    .unwrap_or(false);
    if already_done {
        return;
    }

    let response_callback = get_handle_mut::<ClientRequestHandle>(request_handle)
        .map(|r| r.response_callback)
        .unwrap_or(0);

    let mut trailers_map = HashMap::new();
    for (k, v) in trailers {
        trailers_map.insert(k, v);
    }

    let body_clone = body.clone();
    let incoming = register_handle(IncomingMessageHandle {
        status_code: status,
        status_message,
        headers,
        trailers: trailers_map,
        body,
        listeners: HashMap::new(),
        encoding: None,
        pipes: Vec::new(),
    });

    // Hand the IncomingMessage handle to the user's `(res) => { ... }`
    // callback. POINTER_TAG so the closure-arg unboxer extracts the i64.
    let arg = f64::from_bits(POINTER_TAG | (incoming as u64 & PTR_MASK));
    if response_callback != 0 {
        let closure = JsClosure::from_raw(response_callback as *const RawClosureHeader);
        let _ = closure.call1(arg);
    }
    // #4909 — `.on('response', cb)` listeners fire too (the factory
    // callback is just Node's pre-registered once-listener).
    let response_listeners = get_handle_mut::<ClientRequestHandle>(request_handle)
        .and_then(|r| r.listeners.get("response").cloned())
        .unwrap_or_default();
    for cb in response_listeners {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call1(arg);
        }
    }

    // `'data'` listeners — body is delivered as a single chunk. True
    // streaming requires a cooperative spawn_async perry-ffi surface
    // (v0.6.0 followup).
    //
    // Issue #1124: bytes cross the FFI boundary as a JS Buffer
    // (`alloc_buffer`), not a lossily-decoded string — unless
    // `res.setEncoding(enc)` asked for Readable's string-chunk behavior.
    let (data_listeners, encoding) = get_handle_mut::<IncomingMessageHandle>(incoming)
        .map(|r| {
            (
                r.listeners.get("data").cloned().unwrap_or_default(),
                r.encoding.clone(),
            )
        })
        .unwrap_or_default();
    if !data_listeners.is_empty() && !body_clone.is_empty() {
        let arg = body_chunk_value(&body_clone, encoding.as_deref());
        if arg.to_bits() != TAG_UNDEFINED {
            for cb in data_listeners {
                if cb != 0 {
                    let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
                    let _ = closure.call1(arg);
                }
            }
        }
    }

    // `'end'` listeners — fire after data.
    let end_listeners = get_handle_mut::<IncomingMessageHandle>(incoming)
        .and_then(|r| r.listeners.get("end").cloned())
        .unwrap_or_default();
    for cb in end_listeners {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call0();
        }
    }

    // Forward the buffered body to any `.pipe(dest)` destinations registered
    // during the response callback above (node-fetch: `res.pipe(new
    // PassThrough())`). Deliver the whole body as one write, then end.
    let pipes = get_handle_mut::<IncomingMessageHandle>(incoming)
        .map(|r| r.pipes.clone())
        .unwrap_or_default();
    for dest in pipes {
        forward_pipe_write(dest, &body_clone, encoding.as_deref());
        forward_pipe_end(dest);
    }

    // Node emits `'close'` on the request once the response has fully
    // ended (#4905).
    fire_request_close_once(request_handle);
}

/// Drain handler for `PendingHttpEvent::ResponseHead` (streaming path):
/// build the IncomingMessage handle with an empty body, remember it on the
/// request, and fire the factory callback + `'response'` listeners. Body
/// chunks and the end edge arrive as separate events.
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_response_head_event(
    request_handle: Handle,
    status: u16,
    status_message: String,
    headers: Vec<(String, String)>,
) {
    // A destroyed request delivers nothing.
    let destroyed =
        with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| req.completed)
            .unwrap_or(true);
    if destroyed {
        return;
    }

    let incoming = register_handle(IncomingMessageHandle {
        status_code: status,
        status_message,
        headers,
        trailers: HashMap::new(),
        body: Vec::new(),
        listeners: HashMap::new(),
        encoding: None,
        pipes: Vec::new(),
    });
    let response_callback = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        req.incoming_handle = incoming;
        req.response_callback
    })
    .unwrap_or(0);

    let arg = f64::from_bits(POINTER_TAG | (incoming as u64 & PTR_MASK));
    if response_callback != 0 {
        let closure = JsClosure::from_raw(response_callback as *const RawClosureHeader);
        let _ = closure.call1(arg);
    }
    let response_listeners = get_handle_mut::<ClientRequestHandle>(request_handle)
        .and_then(|r| r.listeners.get("response").cloned())
        .unwrap_or_default();
    for cb in response_listeners {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call1(arg);
        }
    }
}

/// Drain handler for `PendingHttpEvent::ResponseChunk`: deliver to the
/// message's `'data'` listeners, or buffer until `'end'` when none are
/// registered yet (listeners typically attach inside the response
/// callback, which has already run by the time chunks drain).
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_response_chunk_event(request_handle: Handle, chunk: Bytes) {
    let (incoming, done) = get_handle_mut::<ClientRequestHandle>(request_handle)
        .map(|r| (r.incoming_handle, r.completed))
        .unwrap_or((0, true));
    // `completed` mid-stream means the request was destroyed — chunks
    // never arrive after the end edge, so this only suppresses delivery
    // into a torn-down exchange.
    if incoming == 0 || done {
        return;
    }
    let (data_listeners, encoding, pipes) = get_handle_mut::<IncomingMessageHandle>(incoming)
        .map(|r| {
            (
                r.listeners.get("data").cloned().unwrap_or_default(),
                r.encoding.clone(),
                r.pipes.clone(),
            )
        })
        .unwrap_or_default();
    // Forward to `.pipe(dest)` destinations (node-fetch streams the body out
    // this way and registers no `'data'` listener).
    for dest in &pipes {
        forward_pipe_write(*dest, chunk.as_ref(), encoding.as_deref());
    }
    if data_listeners.is_empty() {
        // Buffer for a late `'data'` listener only when the body isn't being
        // piped out (a pipe consumes it eagerly above).
        if pipes.is_empty() {
            if let Some(im) = get_handle_mut::<IncomingMessageHandle>(incoming) {
                im.body.extend_from_slice(&chunk);
            }
        }
        return;
    }
    let arg = body_chunk_value(&chunk, encoding.as_deref());
    if arg.to_bits() == TAG_UNDEFINED {
        return;
    }
    for cb in data_listeners {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call1(arg);
        }
    }
}

/// Drain handler for `PendingHttpEvent::ResponseEnd`: flush any buffered
/// chunks to late-registered `'data'` listeners, fire `'end'` on the
/// message, then `'close'` on the request.
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_response_end_event(request_handle: Handle) {
    let (incoming, was_done) =
        with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
            let was = req.completed;
            req.completed = true;
            (req.incoming_handle, was)
        })
        .unwrap_or((0, true));
    // `was_done` means the request was destroyed mid-stream — the
    // teardown already emitted its own error/close edges.
    if incoming == 0 || was_done {
        return;
    }

    let (data_listeners, encoding, buffered) = get_handle_mut::<IncomingMessageHandle>(incoming)
        .map(|r| {
            (
                r.listeners.get("data").cloned().unwrap_or_default(),
                r.encoding.clone(),
                std::mem::take(&mut r.body),
            )
        })
        .unwrap_or_default();
    if !data_listeners.is_empty() && !buffered.is_empty() {
        let arg = body_chunk_value(&buffered, encoding.as_deref());
        if arg.to_bits() != TAG_UNDEFINED {
            for cb in data_listeners {
                if cb != 0 {
                    let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
                    let _ = closure.call1(arg);
                }
            }
        }
    } else if !buffered.is_empty() {
        // Nobody consumed the body — keep it on the handle for any
        // late reader.
        if let Some(im) = get_handle_mut::<IncomingMessageHandle>(incoming) {
            im.body = buffered;
        }
    }

    let end_listeners = get_handle_mut::<IncomingMessageHandle>(incoming)
        .and_then(|r| r.listeners.get("end").cloned())
        .unwrap_or_default();
    for cb in end_listeners {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call0();
        }
    }

    // End any `.pipe(dest)` destinations now that the body is complete (the
    // body chunks were forwarded as they arrived).
    let pipes = get_handle_mut::<IncomingMessageHandle>(incoming)
        .map(|r| r.pipes.clone())
        .unwrap_or_default();
    for dest in pipes {
        forward_pipe_end(dest);
    }

    fire_request_close_once(request_handle);
}

/// Drain handler for `PendingHttpEvent::Error`: `'error'` listeners then
/// `'close'`, suppressed entirely once the request already completed
/// (e.g. a `req.destroy()` raced the transport failure).
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_error_event(request_handle: Handle, error_message: &str) {
    let already_done = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        let was = req.completed;
        req.completed = true;
        was
    })
    .unwrap_or(false);
    if already_done {
        return;
    }
    fire_request_error_listeners(request_handle, error_event_arg(error_message));
    // Node emits `'close'` on the request after `'error'` (#4905).
    fire_request_close_once(request_handle);
}

/// Drain handler for `PendingHttpEvent::TransportError`: fire `'error'`
/// listeners with a real Node-coded `Error` (`.code` / `.syscall` / `.errno`)
/// then `'close'`. Suppressed once the request already completed (same race
/// guard as [`handle_error_event`]).
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_transport_error_event(
    request_handle: Handle,
    message: &str,
    code: &str,
    syscall: &str,
    errno: i64,
) {
    let already_done = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        let was = req.completed;
        req.completed = true;
        was
    })
    .unwrap_or(false);
    if already_done {
        return;
    }
    let err = perry_ffi::system_error_value(message, code, syscall, errno);
    fire_request_error_listeners(request_handle, f64::from_bits(err.bits()));
    fire_request_close_once(request_handle);
}

/// #4905 / #4909 — drain handler for `PendingHttpEvent::Timeout`.
///
/// `'timeout'` fires at most once per request and never after the
/// response/error completed it. For an in-flight exchange our transport
/// deadline has already aborted the request, so when nobody listens the
/// legacy error surface (+ `'close'`) keeps existing waiters finishing;
/// a request that was never dispatched just gets the event (Node doesn't
/// tear anything down on `'timeout'` — the canonical handler calls
/// `req.destroy()`, which emits its own coded ECONNRESET + `'close'`).
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_timeout_event(request_handle: Handle) {
    let (fire, ended) = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        if req.completed || req.timeout_fired {
            (false, req.ended)
        } else {
            req.timeout_fired = true;
            (true, req.ended)
        }
    })
    .unwrap_or((false, false));
    if !fire {
        return;
    }

    let timeout_listeners = get_handle_mut::<ClientRequestHandle>(request_handle)
        .and_then(|r| r.listeners.get("timeout").cloned())
        .unwrap_or_default();
    if timeout_listeners.is_empty() {
        if ended {
            // In-flight exchange aborted by the transport deadline with no
            // `'timeout'` listener — keep the legacy error surface.
            fire_request_error_listeners(request_handle, error_event_arg("request timed out"));
            fire_request_close_once(request_handle);
        }
        return;
    }
    for cb in timeout_listeners {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call0();
        }
    }
    // The transport deadline killed an in-flight exchange; if the handler
    // didn't destroy the request (destroy emits its own error + close),
    // fire `'close'` so waiters still finish — nothing else will arrive.
    if ended && !client_request_surface::request_destroyed(request_handle) {
        fire_request_close_once(request_handle);
    }
}

/// #4909 — drain handler for `PendingHttpEvent::Flushed`: the body was
/// handed to the transport at `end()`. Node's flush ordering: queued
/// `write(chunk, cb)` callbacks (in order) → `'finish'` listeners → the
/// `end(..., cb)` callback.
///
/// # Safety
///
/// Same listener-liveness contract as [`fire_request_event_listeners`].
pub(crate) unsafe fn handle_flushed_event(request_handle: Handle) {
    let (write_cbs, end_cb) = with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        (
            std::mem::take(&mut req.pending_write_callbacks),
            std::mem::replace(&mut req.end_callback, 0),
        )
    })
    .unwrap_or_default();
    for cb in write_cbs {
        if cb != 0 {
            let closure = JsClosure::from_raw(cb as *const RawClosureHeader);
            let _ = closure.call0();
        }
    }
    fire_request_event_listeners(request_handle, "finish");
    if end_cb != 0 {
        let closure = JsClosure::from_raw(end_cb as *const RawClosureHeader);
        let _ = closure.call0();
    }
}
