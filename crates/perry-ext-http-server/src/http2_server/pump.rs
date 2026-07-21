//! The session event-pump: inbound HTTP/2 request handling and the
//! main-thread drain that fires queued events to JS listeners.

use super::*;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, Request, Response};
use perry_ffi::{
    alloc_buffer, get_handle, get_handle_mut, iter_handles_of, register_handle, JsClosure,
    RawClosureHeader,
};
use tokio::sync::{mpsc, oneshot};

use crate::request::{
    alloc_incoming_message, handle_to_pointer_f64, with_implicit_this, IncomingMessage,
};
use crate::response::{alloc_server_response_for_request, HyperResponseShape, ResponseBody};
use crate::server::{synthesize_default_response_if_needed, HttpPendingRequest};
use crate::types::{js_promise_run_microtasks, POINTER_TAG, PTR_MASK, TAG_UNDEFINED};

pub(crate) async fn handle_h2_request(
    server_handle: i64,
    session_handle: i64,
    peer: SocketAddr,
    req: Request<Incoming>,
    request_tx: Arc<mpsc::Sender<HttpPendingRequest>>,
) -> Result<Response<ResponseBody>, hyper::Error> {
    let method = req.method().to_string();
    let uri = req.uri();
    let url = match uri.query() {
        Some(q) => format!("{}?{}", uri.path(), q),
        None => uri.path().to_string(),
    };
    let mut headers_lower = HashMap::new();
    let mut raw_headers = Vec::new();
    headers_lower.insert(":method".to_string(), method.clone());
    headers_lower.insert(":path".to_string(), url.clone());
    headers_lower.insert(":scheme".to_string(), "http".to_string());
    if let Some(authority) = uri.authority() {
        headers_lower.insert(":authority".to_string(), authority.to_string());
    }
    for (n, v) in req.headers() {
        if let Ok(vs) = v.to_str() {
            headers_lower.insert(n.to_string().to_lowercase(), vs.to_string());
            raw_headers.push((n.to_string(), vs.to_string()));
        }
    }
    let stream_headers = headers_lower.clone();
    let body = match req.collect().await {
        Ok(c) => c.to_bytes().to_vec(),
        Err(_) => Vec::new(),
    };
    let mut im = IncomingMessage::new(
        method,
        url,
        headers_lower,
        raw_headers,
        body,
        peer.ip().to_string(),
        peer.port(),
    );
    im.http_version = "2.0".to_string();
    let im_handle = alloc_incoming_message(im);
    let (response_tx, response_rx) = oneshot::channel::<HyperResponseShape>();
    let (request_listeners, stream_listeners, handler) =
        match get_handle::<Http2SecureServer>(server_handle) {
            Some(s) => (
                s.base.listeners.get("request").cloned().unwrap_or_default(),
                s.base.listeners.get("stream").cloned().unwrap_or_default(),
                s.handler,
            ),
            None => (Vec::new(), Vec::new(), 0),
        };
    let has_stream_listener = !stream_listeners.is_empty();
    let (sr_handle, h2_stream_handle, h2_stream_headers) = if has_stream_listener {
        let (dummy_tx, _dummy_rx) = oneshot::channel::<HyperResponseShape>();
        let stream_handle = register_handle(Http2StreamHandle {
            session_handle,
            id: next_stream_id(),
            pending: false,
            closed: false,
            destroyed: false,
            aborted: false,
            rst_code: 0,
            headers_sent: false,
            sent_headers: Vec::new(),
            request_headers: stream_headers.clone(),
            listeners: HashMap::new(),
            encoding: None,
            response_tx: Some(response_tx),
            response_status: 200,
            response_headers: Vec::new(),
        });
        let headers_vec = stream_headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>();
        (
            alloc_server_response_for_request(dummy_tx, im_handle),
            stream_handle,
            headers_vec,
        )
    } else {
        (
            alloc_server_response_for_request(response_tx, im_handle),
            0,
            Vec::new(),
        )
    };
    let pending = HttpPendingRequest {
        server_handle,
        request_handle: im_handle,
        response_handle: sr_handle,
        skip_default_response: has_stream_listener,
        h2_stream_handle,
        h2_stream_headers,
        request_listeners,
        handler,
        check_continue_listeners: Vec::new(),
        is_check_continue: false,
    };
    if request_tx.send(pending).await.is_err() {
        return Ok(Response::builder()
            .status(503)
            .body(Full::new(Bytes::from("Server unavailable")).boxed())
            .unwrap());
    }
    perry_ffi::notify_main_thread();
    match response_rx.await {
        Ok(shape) => Ok(shape.into_hyper()),
        Err(_) => Ok(Response::builder()
            .status(500)
            .body(Full::new(Bytes::from("Handler error")).boxed())
            .unwrap()),
    }
}

/// Non-blocking try_recv for HTTP/2 pending requests. Called by
/// `js_node_http_server_process_pending` in `server.rs` each tick.
pub(crate) fn try_recv_pending_h2_nonblocking(server_handle: i64) -> Option<HttpPendingRequest> {
    if let Some(s) = get_handle_mut::<Http2SecureServer>(server_handle) {
        if let Some(rx) = s.base.request_rx.as_mut() {
            return rx.try_recv().ok();
        }
    }
    None
}

/// Dispatch one HTTP/2 pending request. Per the issue #604
/// architectural change, we no longer block on the handler-returned
/// Promise.
pub(crate) fn process_pending_h2(pending: HttpPendingRequest) {
    let req_f64 = handle_to_pointer_f64(pending.request_handle);
    let res_f64 = handle_to_pointer_f64(pending.response_handle);
    // #6710 — clear a possibly-recycled handle id's per-handle JS side tables
    // before the handler observes req/res (see process_pending in server.rs).
    // The HTTP/2 stream handle is recycled from the same pool, so clear it too
    // (no-op when `h2_stream_handle == 0`).
    unsafe {
        crate::types::js_handle_clear_side_tables(pending.request_handle);
        crate::types::js_handle_clear_side_tables(pending.response_handle);
        crate::types::js_handle_clear_side_tables(pending.h2_stream_handle);
    }
    // #4903 — Node invokes `'request'` listeners (and the `createServer`
    // handler, which is one) with `this` bound to the server.
    let server_this = handle_to_pointer_f64(pending.server_handle);
    for cb in &pending.request_listeners {
        if *cb == 0 {
            continue;
        }
        unsafe {
            let raw = *cb as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                with_implicit_this(server_this, || {
                    let _ = closure.call2(req_f64, res_f64);
                });
            }
            js_promise_run_microtasks();
        }
    }
    if pending.handler != 0 {
        unsafe {
            let raw = pending.handler as *const RawClosureHeader;
            let closure = JsClosure::from_raw(raw);
            if !closure.is_null() {
                with_implicit_this(server_this, || {
                    let _ = closure.call2(req_f64, res_f64);
                });
            }
            js_promise_run_microtasks();
        }
    }
    if pending.h2_stream_handle != 0 {
        let stream_f64 = handle_to_pointer_f64(pending.h2_stream_handle);
        let headers_f64 = pairs_to_js_object(&pending.h2_stream_headers);
        let stream_listeners = get_handle::<Http2SecureServer>(pending.server_handle)
            .and_then(|s| s.base.listeners.get("stream").cloned())
            .unwrap_or_default();
        for cb in &stream_listeners {
            if *cb == 0 {
                continue;
            }
            unsafe {
                let raw = *cb as *const RawClosureHeader;
                let closure = JsClosure::from_raw(raw);
                if !closure.is_null() {
                    let _ = closure.call2(stream_f64, headers_f64);
                }
                js_promise_run_microtasks();
            }
        }
        synthesize_default_h2_stream_response(pending.h2_stream_handle);
    }
    if !pending.skip_default_response {
        synthesize_default_response_if_needed(pending.response_handle);
    }
    perry_ffi::drop_handle(pending.request_handle);
    perry_ffi::drop_handle(pending.response_handle);
}

fn synthesize_default_h2_stream_response(stream_handle: i64) {
    if let Some(stream) = get_handle_mut::<Http2StreamHandle>(stream_handle) {
        if stream.response_tx.is_none() {
            return;
        }
        stream.headers_sent = true;
        stream.closed = true;
        stream.destroyed = true;
        let mut headers = stream.response_headers.clone();
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        {
            headers.push(("Content-Length".to_string(), "0".to_string()));
        }
        let shape = HyperResponseShape {
            status: stream.response_status,
            status_message: None,
            headers,
            trailers: Vec::new(),
            body: crate::response::ShapeBody::Full(Vec::new()),
        };
        if let Some(tx) = stream.response_tx.take() {
            let _ = tx.send(shape);
        }
    }
}

pub(crate) fn has_pending_h2_events() -> bool {
    H2_PENDING_EVENTS
        .lock()
        .map(|q| !q.is_empty())
        .unwrap_or(false)
}

pub(crate) fn has_active_h2_clients() -> bool {
    if has_pending_h2_events() {
        return true;
    }
    let mut active = false;
    iter_handles_of::<Http2SessionHandle, _>(|session| {
        if session.session_type == 1 && !session.closed && !session.destroyed {
            active = true;
        }
    });
    active
}

pub(crate) fn process_pending_h2_events() -> i32 {
    let mut events: Vec<Http2PendingEvent> = match H2_PENDING_EVENTS.lock() {
        Ok(mut q) => q.drain(..).collect(),
        Err(_) => return 0,
    };
    // Causally, the server creates its session and sends its SETTINGS frame
    // before a client can complete its connect handshake, so the server-side
    // `session` event fires BEFORE the client-side `connect` (Node on Linux:
    // `server>client`). Drain `Session` first so a single-process loopback
    // observes `session` then `connect`, matching the causal/Linux ordering.
    events.sort_by_key(|event| match event {
        Http2PendingEvent::Session { .. } => 0,
        Http2PendingEvent::ClientConnect { .. } => 1,
        _ => 2,
    });
    let count = events.len() as i32;
    for event in events {
        match event {
            Http2PendingEvent::Session {
                server_handle,
                session_handle,
            } => {
                let listeners = get_handle::<Http2SecureServer>(server_handle)
                    .and_then(|s| s.base.listeners.get("session").cloned())
                    .unwrap_or_default();
                let arg = handle_to_pointer_f64(session_handle);
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                    session.session_event_emitted = true;
                }
                for cb in listeners {
                    call1(cb, arg);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientConnect { session_handle } => {
                if !local_client_connect_ready(session_handle) {
                    push_h2_event(Http2PendingEvent::ClientConnect { session_handle });
                    continue;
                }
                let listeners = get_handle::<Http2SessionHandle>(session_handle)
                    .and_then(|s| s.listeners.get("connect").cloned())
                    .unwrap_or_default();
                for cb in listeners {
                    call0(cb);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientResponse {
                stream_handle,
                headers,
            } => {
                let listeners = get_handle::<Http2StreamHandle>(stream_handle)
                    .and_then(|s| s.listeners.get("response").cloned())
                    .unwrap_or_default();
                let arg = map_to_js_object(&headers);
                for cb in listeners {
                    call1(cb, arg);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientData {
                stream_handle,
                body,
            } => {
                let (listeners, encoding) = get_handle::<Http2StreamHandle>(stream_handle)
                    .map(|s| {
                        (
                            s.listeners.get("data").cloned().unwrap_or_default(),
                            s.encoding.clone(),
                        )
                    })
                    .unwrap_or_default();
                if !listeners.is_empty() && !body.is_empty() {
                    let arg = match encoding.as_deref() {
                        Some(_) => string_value(&String::from_utf8_lossy(&body)),
                        None => {
                            let buf = alloc_buffer(&body);
                            if buf.is_null() {
                                f64::from_bits(TAG_UNDEFINED)
                            } else {
                                f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
                            }
                        }
                    };
                    if arg.to_bits() != TAG_UNDEFINED {
                        for cb in listeners {
                            call1(cb, arg);
                            unsafe {
                                js_promise_run_microtasks();
                            }
                        }
                    }
                }
            }
            Http2PendingEvent::ClientEnd { stream_handle } => {
                let listeners = get_handle::<Http2StreamHandle>(stream_handle)
                    .and_then(|s| s.listeners.get("end").cloned())
                    .unwrap_or_default();
                for cb in listeners {
                    call0(cb);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientClose {
                session_handle,
                callback,
            } => {
                let listeners = get_handle::<Http2SessionHandle>(session_handle)
                    .and_then(|s| s.listeners.get("close").cloned())
                    .unwrap_or_default();
                for cb in listeners {
                    call0(cb);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
                call0(callback);
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                    session.close_callbacks.retain(|cb| *cb != callback);
                }
            }
            Http2PendingEvent::SessionSettingsEvent {
                session_handle,
                event,
                settings,
            } => {
                let listeners = get_handle::<Http2SessionHandle>(session_handle)
                    .and_then(|s| s.listeners.get(event).cloned())
                    .unwrap_or_default();
                let arg = settings_value(&settings);
                for cb in listeners {
                    call1(cb, arg);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::SessionSettingsCallback {
                session_handle,
                callback,
                settings,
            } => {
                call2(callback, null_value(), settings_value(&settings));
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                    session.pending_callbacks.retain(|cb| *cb != callback);
                    session.pending_settings_ack = false;
                }
                unsafe {
                    js_promise_run_microtasks();
                }
            }
            Http2PendingEvent::SessionPingCallback {
                session_handle,
                callback,
                payload,
            } => {
                call3(
                    callback,
                    null_value(),
                    0.0,
                    buffer_value_from_bytes(&payload),
                );
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                    session.pending_callbacks.retain(|cb| *cb != callback);
                }
                unsafe {
                    js_promise_run_microtasks();
                }
            }
            Http2PendingEvent::SessionGoaway {
                session_handle,
                code,
                last_stream_id,
                opaque_data,
            } => {
                let listeners = get_handle::<Http2SessionHandle>(session_handle)
                    .and_then(|s| s.listeners.get("goaway").cloned())
                    .unwrap_or_default();
                let opaque = buffer_value_from_bytes(&opaque_data);
                for cb in listeners {
                    call3(cb, code, last_stream_id, opaque);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
            Http2PendingEvent::ClientError { handle, message } => {
                let listeners = get_handle::<Http2SessionHandle>(handle)
                    .and_then(|s| s.listeners.get("error").cloned())
                    .or_else(|| {
                        get_handle::<Http2StreamHandle>(handle)
                            .and_then(|s| s.listeners.get("error").cloned())
                    })
                    .unwrap_or_default();
                let arg = string_value(&message);
                for cb in listeners {
                    call1(cb, arg);
                    unsafe {
                        js_promise_run_microtasks();
                    }
                }
            }
        }
    }
    count
}
