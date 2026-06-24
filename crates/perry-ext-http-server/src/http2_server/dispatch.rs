//! Stream-lifecycle surface: the `js_ext_http2_*` session/stream
//! method+property dispatchers and the `js_node_http2_server_*` JS API.

use super::*;

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, RawClosureHeader,
    StringHeader,
};
use std::collections::HashMap;

use crate::request::{emit_no_arg_to_listeners, handle_to_pointer_f64};
use crate::response::HyperResponseShape;
use crate::types::{
    jsvalue_to_body_bytes, jsvalue_to_owned_string, read_string_header, POINTER_TAG, PTR_MASK,
    TAG_UNDEFINED,
};

#[no_mangle]
pub extern "C" fn js_ext_http2_session_is_handle(handle: i64) -> i32 {
    if get_handle::<Http2SessionHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_ext_http2_stream_is_handle(handle: i64) -> i32 {
    if get_handle::<Http2StreamHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http2_session_dispatch_method(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let method =
        String::from_utf8_lossy(std::slice::from_raw_parts(method_ptr, method_len)).into_owned();
    let args = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    let self_ref = handle_to_pointer_f64(handle);
    match method.as_str() {
        "request" => {
            let headers = args.first().copied().unwrap_or(undef);
            let request_headers = parse_headers_object(headers);
            let stream_handle = register_handle(Http2StreamHandle {
                session_handle: handle,
                id: next_stream_id(),
                pending: false,
                closed: false,
                destroyed: false,
                aborted: false,
                rst_code: 0,
                headers_sent: false,
                sent_headers: Vec::new(),
                request_headers,
                listeners: HashMap::new(),
                encoding: None,
                response_tx: None,
                response_status: 200,
                response_headers: Vec::new(),
            });
            handle_to_pointer_f64(stream_handle)
        }
        "on" | "addListener" if args.len() >= 2 => {
            if let Some(event) = raw_event_name(args[0]) {
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                    session
                        .listeners
                        .entry(event)
                        .or_default()
                        .push(closure_arg(Some(args[1])));
                }
            }
            self_ref
        }
        "close" => {
            let callback = closure_arg(args.first().copied());
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                session.closed = true;
                session.destroyed = true;
                if let Ok(mut slot) = session.sender.lock() {
                    *slot = None;
                }
                if callback != 0 {
                    session.close_callbacks.push(callback);
                }
            }
            push_h2_event(Http2PendingEvent::ClientClose {
                session_handle: handle,
                callback,
            });
            self_ref
        }
        "destroy" => {
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                session.closed = true;
                session.destroyed = true;
                if let Ok(mut slot) = session.sender.lock() {
                    *slot = None;
                }
            }
            self_ref
        }
        "ref" | "unref" => undef,
        "setLocalWindowSize" => {
            if let Some(window_size) = args.first().and_then(|v| numeric_value(*v)) {
                if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                    session.local_window_size = window_size as i64;
                }
            }
            undef
        }
        "setTimeout" => {
            let callback = args
                .get(1)
                .copied()
                .map(|v| closure_arg(Some(v)))
                .unwrap_or(0);
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(handle) {
                session.timeout_callback = callback;
            }
            self_ref
        }
        "ping" => queue_session_ping(handle, args),
        "settings" => queue_session_settings(handle, args),
        "goaway" => queue_session_goaway(handle, args),
        _ => undef,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http2_session_dispatch_property(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let property = String::from_utf8_lossy(std::slice::from_raw_parts(property_ptr, property_len))
        .into_owned();
    match property.as_str() {
        "request" => bind_handle_method(handle, b"request"),
        "on" => bind_handle_method(handle, b"on"),
        "addListener" => bind_handle_method(handle, b"addListener"),
        "close" => bind_handle_method(handle, b"close"),
        "destroy" => bind_handle_method(handle, b"destroy"),
        "ref" => bind_handle_method(handle, b"ref"),
        "unref" => bind_handle_method(handle, b"unref"),
        "setTimeout" => bind_handle_method(handle, b"setTimeout"),
        "setLocalWindowSize" => bind_handle_method(handle, b"setLocalWindowSize"),
        "ping" => bind_handle_method(handle, b"ping"),
        "settings" => bind_handle_method(handle, b"settings"),
        "goaway" => bind_handle_method(handle, b"goaway"),
        "type" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| s.session_type as f64)
            .unwrap_or(0.0),
        "encrypted" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| {
                if s.connected {
                    bool_value(s.encrypted)
                } else {
                    undef
                }
            })
            .unwrap_or(undef),
        "connecting" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.connecting)
                .unwrap_or(false),
        ),
        "closed" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.closed)
                .unwrap_or(false),
        ),
        "destroyed" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.destroyed)
                .unwrap_or(false),
        ),
        "alpnProtocol" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| {
                if s.connected {
                    string_value(&s.alpn_protocol)
                } else {
                    undef
                }
            })
            .unwrap_or(undef),
        "pendingSettingsAck" => bool_value(
            get_handle::<Http2SessionHandle>(handle)
                .map(|s| s.pending_settings_ack)
                .unwrap_or(false),
        ),
        "localSettings" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| settings_value(&s.local_settings))
            .unwrap_or_else(empty_object_value),
        "remoteSettings" => get_handle::<Http2SessionHandle>(handle)
            .map(|s| settings_value(&s.remote_settings))
            .unwrap_or_else(empty_object_value),
        "state" => get_handle::<Http2SessionHandle>(handle)
            .map(session_state_value)
            .unwrap_or_else(empty_object_value),
        "socket" => empty_object_value(),
        _ => undef,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http2_stream_dispatch_method(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let method =
        String::from_utf8_lossy(std::slice::from_raw_parts(method_ptr, method_len)).into_owned();
    let args = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    let self_ref = handle_to_pointer_f64(handle);
    match method.as_str() {
        "on" | "addListener" if args.len() >= 2 => {
            if let Some(event) = raw_event_name(args[0]) {
                if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
                    stream
                        .listeners
                        .entry(event)
                        .or_default()
                        .push(closure_arg(Some(args[1])));
                }
            }
            self_ref
        }
        "setEncoding" if !args.is_empty() => {
            if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
                stream.encoding = jsvalue_to_owned_string(args[0]);
            }
            self_ref
        }
        "respond" if !args.is_empty() => {
            let headers = parse_headers_object(args[0]);
            if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
                stream.headers_sent = true;
                stream.sent_headers = headers
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                stream.response_status = headers
                    .get(":status")
                    .and_then(|status| status.parse::<u16>().ok())
                    .unwrap_or(200);
                stream.response_headers = headers
                    .iter()
                    .filter(|(name, _)| !name.starts_with(':'))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
            }
            self_ref
        }
        "end" => {
            let body = args
                .first()
                .copied()
                .and_then(jsvalue_to_body_bytes)
                .unwrap_or_default();
            let is_server_stream = get_handle::<Http2StreamHandle>(handle)
                .and_then(|stream| {
                    get_handle::<Http2SessionHandle>(stream.session_handle)
                        .map(|session| session.session_type == 0)
                })
                .unwrap_or(false);
            if is_server_stream {
                end_server_h2_stream(handle, body);
            } else {
                start_client_request(handle, body);
            }
            self_ref
        }
        "close" => {
            if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
                stream.closed = true;
                stream.destroyed = true;
            }
            self_ref
        }
        "setTimeout" | "priority" | "additionalHeaders" | "pushStream" | "respondWithFD"
        | "respondWithFile" | "sendTrailers" => self_ref,
        _ => undef,
    }
}

fn end_server_h2_stream(handle: i64, body: Vec<u8>) {
    if let Some(stream) = get_handle_mut::<Http2StreamHandle>(handle) {
        stream.closed = true;
        stream.destroyed = true;
        stream.headers_sent = true;
        let mut headers = stream.response_headers.clone();
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        {
            headers.push(("Content-Length".to_string(), body.len().to_string()));
        }
        let shape = HyperResponseShape {
            status: stream.response_status,
            status_message: None,
            headers,
            trailers: Vec::new(),
            body: crate::response::ShapeBody::Full(body),
        };
        if let Some(tx) = stream.response_tx.take() {
            let _ = tx.send(shape);
        }
    }
}

/// `http2SecureServer.address()`.
#[no_mangle]
pub extern "C" fn js_node_http2_server_address_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<Http2SecureServer>(handle)
        .map(|s| {
            if !s.base.listening {
                "null".to_string()
            } else {
                let family = if s.base.bound_host.contains(':') {
                    "IPv6"
                } else {
                    "IPv4"
                };
                serde_json::json!({
                    "port": s.base.bound_port,
                    "address": s.base.bound_host,
                    "family": family,
                })
                .to_string()
            }
        })
        .unwrap_or_else(|| "null".to_string());
    alloc_string(&s).as_raw()
}

/// `http2SecureServer.close(cb?)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http2_server_close(handle: i64, callback: i64) {
    let close_listeners;
    if let Some(s) = get_handle_mut::<Http2SecureServer>(handle) {
        s.base.listening = false;
        s.base.connections_checking_interval_destroyed = true;
        s.base.shutdown_tx.take();
        close_listeners = s.base.listeners.get("close").cloned().unwrap_or_default();
    } else {
        close_listeners = Vec::new();
    }
    mark_server_sessions_closed(handle);
    emit_no_arg_to_listeners(&close_listeners);
    if callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
}

/// `http2SecureServer.on(event, cb)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http2_server_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    if let Some(s) = get_handle_mut::<Http2SecureServer>(handle) {
        s.base.listeners.entry(event).or_default().push(callback);
    }
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}
