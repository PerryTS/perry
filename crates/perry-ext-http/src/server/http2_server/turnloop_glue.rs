//! The seam between the turnloop HTTP/2 transport and the JS-visible handles.
//!
//! Everything here runs **inside the completion sink**, so none of it may call
//! JS. Each function either mutates an `Http2SessionHandle` /
//! `Http2StreamHandle` record, or pushes an `Http2PendingEvent` onto the queue
//! `process_pending_h2_events` drains on the main thread's own tick — which is
//! exactly where the `h2` task's `push_h2_event` put the same events, so the
//! event-loop phase order does not move.

use super::*;

use std::collections::HashMap;

use perry_ffi::{get_handle, get_handle_mut, register_handle};

use crate::server::http2_session_settings::Http2SettingsState;

/// A session handle for a connection turnloop just accepted.
pub(crate) fn register_turnloop_server_session(
    server_handle: i64,
    conn_id: i64,
    peer_port: u16,
    encrypted: bool,
    alpn: &str,
    local_settings: Http2SettingsState,
) -> i64 {
    let session_handle = register_handle(Http2SessionHandle {
        server_handle,
        connection_port: peer_port,
        session_event_emitted: false,
        connect_event_emitted: false,
        session_type: 0,
        connected: true,
        encrypted,
        alpn_protocol: alpn.to_string(),
        connecting: false,
        closed: false,
        destroyed: false,
        pending_settings_ack: true,
        authority: String::new(),
        local_settings,
        remote_settings: Http2SettingsState::default(),
        local_window_size: 65_535,
        listeners: HashMap::new(),
        close_callbacks: Vec::new(),
        pending_callbacks: Vec::new(),
        timeout_callback: 0,
        turnloop_conn: conn_id,
    });
    let has_session_listener = get_handle::<Http2SecureServer>(server_handle)
        .map(|server| crate::server::server::server_has_event_listener(&server.base, "session"))
        .unwrap_or(false);
    if has_session_listener {
        push_h2_event(Http2PendingEvent::Session {
            server_handle,
            session_handle,
        });
    }
    session_handle
}

/// Record which turnloop connection carries a session.
pub(crate) fn bind_turnloop_session(session_handle: i64, conn_id: i64) {
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
        session.turnloop_conn = conn_id;
    }
}

/// A client session's local TCP port, which is how `local_server_session_event_ready`
/// pairs it with the server session of an in-process loopback connection.
pub(crate) fn bind_turnloop_client_port(session_handle: i64, port: u16) {
    if port == 0 {
        return;
    }
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
        session.connection_port = port;
    }
}

/// The turnloop connection a session handle rides on, or `None` on the legacy
/// transport. Every control surface routes on this.
pub(crate) fn turnloop_conn_of_session(session_handle: i64) -> Option<i64> {
    get_handle::<Http2SessionHandle>(session_handle)
        .map(|s| s.turnloop_conn)
        .filter(|id| *id != 0)
}

/// The turnloop connection and real stream id behind an `Http2Stream` handle.
pub(crate) fn turnloop_target_of_stream(stream_handle: i64) -> Option<(i64, u32)> {
    let stream = get_handle::<Http2StreamHandle>(stream_handle)?;
    if stream.turnloop_conn == 0 || stream.id <= 0 {
        return None;
    }
    Some((stream.turnloop_conn, stream.id as u32))
}

pub(crate) fn mark_turnloop_client_connected(session_handle: i64, protocol: &str) {
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
        session.connected = true;
        session.connecting = false;
        session.pending_settings_ack = true;
        session.encrypted = protocol == "h2";
        session.alpn_protocol = protocol.to_string();
    }
    push_h2_event(Http2PendingEvent::ClientConnect { session_handle });
}

pub(crate) fn mark_turnloop_session_closed(session_handle: i64) {
    if session_handle == 0 {
        return;
    }
    let notify = match get_handle_mut::<Http2SessionHandle>(session_handle) {
        Some(session) => {
            let first = !session.closed;
            session.closed = true;
            session.destroyed = true;
            session.connecting = false;
            session.turnloop_conn = 0;
            first
        }
        None => false,
    };
    if notify {
        push_h2_event(Http2PendingEvent::ClientClose {
            session_handle,
            callback: 0,
        });
    }
}

pub(crate) fn queue_turnloop_session_error(session_handle: i64, code: &str) {
    if session_handle == 0 {
        return;
    }
    push_h2_event(Http2PendingEvent::ClientError {
        handle: session_handle,
        message: code.to_string(),
    });
}

/// The peer's SETTINGS arrived; Node emits `'remoteSettings'`.
pub(crate) fn queue_turnloop_remote_settings(session_handle: i64, settings: Http2SettingsState) {
    if session_handle == 0 {
        return;
    }
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
        session.remote_settings = settings.clone();
    }
    push_h2_event(Http2PendingEvent::SessionSettingsEvent {
        session_handle,
        event: "remoteSettings",
        settings,
    });
}

pub(crate) fn server_has_stream_listener(server_handle: i64) -> bool {
    get_handle::<Http2SecureServer>(server_handle)
        .map(|server| crate::server::server::server_has_event_listener(&server.base, "stream"))
        .unwrap_or(false)
}

/// A server-side `Http2Stream` object for the `'stream'` event, carrying the
/// **real** RFC 9113 stream id.
pub(crate) fn register_turnloop_stream_handle(
    session_handle: i64,
    conn_id: i64,
    h2_id: i64,
    headers: Vec<(String, String)>,
) -> i64 {
    let mut request_headers = HashMap::new();
    for (name, value) in &headers {
        request_headers.insert(name.clone(), value.clone());
    }
    register_handle(Http2StreamHandle {
        session_handle,
        id: h2_id,
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
        response_status: 200,
        response_headers: Vec::new(),
        turnloop_conn: conn_id,
        turnloop_responded: false,
    })
}

/// A client stream's real id, once `Connection::open` has assigned it.
pub(crate) fn bind_turnloop_stream_id(stream_handle: i64, conn_id: i64, h2_id: i64) {
    if let Some(stream) = get_handle_mut::<Http2StreamHandle>(stream_handle) {
        stream.id = h2_id;
        stream.pending = false;
        stream.turnloop_conn = conn_id;
    }
}

pub(crate) fn queue_turnloop_client_response(stream_handle: i64, headers: HashMap<String, String>) {
    if stream_handle == 0 {
        return;
    }
    if let Some(stream) = get_handle_mut::<Http2StreamHandle>(stream_handle) {
        stream.response_status = headers
            .get(":status")
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(200);
    }
    push_h2_event(Http2PendingEvent::ClientResponse {
        stream_handle,
        headers,
    });
}

pub(crate) fn queue_turnloop_client_body(
    stream_handle: i64,
    body: Vec<u8>,
    _trailers: Vec<(String, String)>,
) {
    if stream_handle == 0 {
        return;
    }
    if !body.is_empty() {
        push_h2_event(Http2PendingEvent::ClientData {
            stream_handle,
            body,
        });
    }
    push_h2_event(Http2PendingEvent::ClientEnd { stream_handle });
}

/// A peer RST_STREAM on one stream. Its siblings are untouched: this pushes an
/// event for exactly one handle and nothing else.
pub(crate) fn queue_turnloop_stream_reset(stream_handle: i64, code: u32) {
    if stream_handle == 0 {
        return;
    }
    if let Some(stream) = get_handle_mut::<Http2StreamHandle>(stream_handle) {
        stream.rst_code = code as i32;
        stream.aborted = code != 0;
        stream.closed = true;
    }
    push_h2_event(Http2PendingEvent::ClientEnd { stream_handle });
}

pub(crate) fn queue_turnloop_stream_error(stream_handle: i64, message: &str) {
    if stream_handle == 0 {
        return;
    }
    push_h2_event(Http2PendingEvent::ClientError {
        handle: stream_handle,
        message: message.to_string(),
    });
}

pub(crate) fn mark_turnloop_stream_closed(stream_handle: i64) {
    if let Some(stream) = get_handle_mut::<Http2StreamHandle>(stream_handle) {
        stream.closed = true;
        stream.destroyed = true;
        stream.turnloop_conn = 0;
    }
}

pub(crate) fn queue_turnloop_goaway(
    session_handle: i64,
    code: u32,
    last_stream: u32,
    opaque: Vec<u8>,
) {
    if session_handle == 0 {
        return;
    }
    push_h2_event(Http2PendingEvent::SessionGoaway {
        session_handle,
        code: code as f64,
        last_stream_id: last_stream as f64,
        opaque_data: opaque,
    });
}

/// A PING acknowledgement: fire the `session.ping(cb)` callback that is waiting.
pub(crate) fn complete_turnloop_ping(session_handle: i64, data: [u8; 8]) {
    let callback = get_handle_mut::<Http2SessionHandle>(session_handle)
        .and_then(|session| {
            if session.pending_callbacks.is_empty() {
                None
            } else {
                Some(session.pending_callbacks.remove(0))
            }
        })
        .unwrap_or(0);
    if callback == 0 {
        return;
    }
    push_h2_event(Http2PendingEvent::SessionPingCallback {
        session_handle,
        callback,
        payload: data.to_vec(),
    });
}

/// The peer acknowledged the SETTINGS the core sent at construction.
///
/// This is not `complete_turnloop_settings`: there is no user callback behind
/// the handshake's own SETTINGS and no `'localSettings'` to emit for it, but
/// `session.pendingSettingsAck` must go false — Node's does, and a session that
/// reported `true` for the life of the connection would be the one observable
/// thing the handshake changes.
pub(crate) fn mark_turnloop_settings_acked(session_handle: i64) {
    if session_handle == 0 {
        return;
    }
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
        session.pending_settings_ack = false;
    }
}

/// The SETTINGS acknowledgement arrived: Node fires `session.settings(obj, cb)`'s
/// callback and emits `'localSettings'`.
pub(crate) fn complete_turnloop_settings(session_handle: i64) {
    if session_handle == 0 {
        return;
    }
    let (callback, settings) = match get_handle_mut::<Http2SessionHandle>(session_handle) {
        Some(session) => {
            session.pending_settings_ack = false;
            let callback = if session.pending_callbacks.is_empty() {
                0
            } else {
                session.pending_callbacks.remove(0)
            };
            (callback, session.local_settings.clone())
        }
        None => return,
    };
    if callback != 0 {
        push_h2_event(Http2PendingEvent::SessionSettingsCallback {
            session_handle,
            callback,
            settings: settings.clone(),
        });
    }
    push_h2_event(Http2PendingEvent::SessionSettingsEvent {
        session_handle,
        event: "localSettings",
        settings,
    });
}
