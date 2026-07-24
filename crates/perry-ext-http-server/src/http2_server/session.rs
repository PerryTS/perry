//! Session/server registration, connect-ordering machinery, and the
//! client `connect()` + outbound-request path.

use super::*;

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Request, Version};
use perry_ffi::{
    get_handle, get_handle_mut, iter_handle_ids_of, iter_handles_of, iter_handles_of_mut,
    register_handle, JsValue,
};

use crate::ensure_gc_scanner_registered;
use crate::http2_session_settings::Http2SettingsState;
use crate::types::jsvalue_to_owned_string;

pub(crate) fn register_server_session(server_handle: i64, peer_addr: SocketAddr) -> i64 {
    let session_handle = register_handle(Http2SessionHandle {
        server_handle,
        connection_port: peer_addr.port(),
        session_event_emitted: false,
        connect_event_emitted: false,
        session_type: 0,
        connected: true,
        encrypted: false,
        alpn_protocol: "h2c".to_string(),
        connecting: false,
        closed: false,
        destroyed: false,
        pending_settings_ack: true,
        authority: String::new(),
        local_settings: Http2SettingsState::default(),
        remote_settings: Http2SettingsState::default(),
        local_window_size: 65_535,
        sender: Arc::new(Mutex::new(None)),
        listeners: HashMap::new(),
        close_callbacks: Vec::new(),
        pending_callbacks: Vec::new(),
        timeout_callback: 0,
    });
    let has_session_listener = get_handle::<Http2SecureServer>(server_handle)
        .and_then(|s| s.base.listeners.get("session"))
        .map(|listeners| !listeners.is_empty())
        .unwrap_or(false);
    if has_session_listener {
        push_h2_event(Http2PendingEvent::Session {
            server_handle,
            session_handle,
        });
    }
    session_handle
}

pub(crate) fn mark_session_closed(session_handle: i64) {
    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
        session.closed = true;
        session.destroyed = true;
        if let Ok(mut slot) = session.sender.lock() {
            *slot = None;
        }
    }
}

pub(crate) fn mark_server_sessions_closed(server_handle: i64) {
    iter_handles_of_mut::<Http2SessionHandle, _>(|session| {
        if session.server_handle == server_handle {
            session.closed = true;
            session.destroyed = true;
            if let Ok(mut slot) = session.sender.lock() {
                *slot = None;
            }
        }
    });
}

pub(crate) fn h2_listening_server_for_authority(authority: &str) -> Option<i64> {
    let (_, port, _) = parse_authority(authority);
    let mut matched = None;
    iter_handle_ids_of::<Http2SecureServer, _>(|server_id| {
        if matched.is_some() {
            return;
        }
        if get_handle::<Http2SecureServer>(server_id)
            .map(|server| server.base.listening && server.base.bound_port == port)
            .unwrap_or(false)
        {
            matched = Some(server_id);
        }
    });
    matched
}

pub(crate) fn local_server_handle_for_client(session_handle: i64) -> Option<i64> {
    let session = get_handle::<Http2SessionHandle>(session_handle)?;
    if session.session_type != 1 {
        return None;
    }
    if session.server_handle != 0 {
        return Some(session.server_handle);
    }
    h2_listening_server_for_authority(&session.authority)
}

fn has_active_server_session(server_handle: i64) -> bool {
    let mut active = false;
    iter_handles_of::<Http2SessionHandle, _>(|session| {
        if session.server_handle == server_handle && !session.closed && !session.destroyed {
            active = true;
        }
    });
    active
}

#[allow(dead_code)] // retained: server-session listener probe
fn server_has_session_listener(server_handle: i64) -> bool {
    get_handle::<Http2SecureServer>(server_handle)
        .and_then(|server| server.base.listeners.get("session"))
        .map(|listeners| !listeners.is_empty())
        .unwrap_or(false)
}

#[allow(dead_code)] // retained: server-session emit bookkeeping
fn has_emitted_server_session(server_handle: i64) -> bool {
    let mut emitted = false;
    iter_handles_of::<Http2SessionHandle, _>(|session| {
        if session.server_handle == server_handle
            && session.session_event_emitted
            && !session.closed
            && !session.destroyed
        {
            emitted = true;
        }
    });
    emitted
}

pub(crate) fn local_client_connect_ready(session_handle: i64) -> bool {
    let Some(server_handle) = local_server_handle_for_client(session_handle) else {
        return true;
    };
    // The client `connect` only needs the server session to be ACTIVE (the
    // handshake established), not for the server's `session` EVENT to have
    // fired — Node emits that event after the client connect. Gating on the
    // emitted event forced a `session`-before-`connect` order that Node never
    // produces.
    has_active_server_session(server_handle)
}

pub(crate) fn local_server_session_event_ready(server_session_handle: i64) -> bool {
    let Some(server_session) = get_handle::<Http2SessionHandle>(server_session_handle) else {
        return true;
    };
    if server_session.session_type != 0
        || server_session.server_handle == 0
        || server_session.connection_port == 0
    {
        return true;
    }
    let server_handle = server_session.server_handle;
    let connection_port = server_session.connection_port;
    let mut ready = true;
    iter_handles_of::<Http2SessionHandle, _>(|session| {
        if session.session_type == 1
            && session.server_handle == server_handle
            && session.connection_port == connection_port
            && !session.closed
            && !session.destroyed
            && !session.connect_event_emitted
        {
            ready = false;
        }
    });
    ready
}

async fn connect_h2_stream(
    host: &str,
    port: u16,
    session_handle: i64,
    reserve_pairing_port: bool,
) -> io::Result<tokio::net::TcpStream> {
    if !reserve_pairing_port {
        return tokio::net::TcpStream::connect(format!("{host}:{port}")).await;
    }

    // A same-process server accepts on another Tokio runtime. Reserve and
    // publish the client's ephemeral port *before* connect(), so whichever
    // runtime wakes first can pair the server session with this exact client.
    // The peer port observed by accept() is the same reserved port.
    let addresses: Vec<_> = tokio::net::lookup_host((host, port)).await?.collect();
    let mut last_error = None;
    for address in addresses {
        let socket = if address.is_ipv4() {
            tokio::net::TcpSocket::new_v4()
        } else {
            tokio::net::TcpSocket::new_v6()
        };
        let socket = match socket {
            Ok(socket) => socket,
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };
        let bind_addr = if address.is_ipv4() {
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
        } else {
            SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
        };
        if let Err(err) = socket.bind(bind_addr) {
            last_error = Some(err);
            continue;
        }
        let local_port = match socket.local_addr() {
            Ok(local) => local.port(),
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };
        if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
            session.connection_port = local_port;
        }
        match socket.connect(address).await {
            Ok(stream) => return Ok(stream),
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("no address resolved for {host}:{port}"),
        )
    }))
}

#[no_mangle]
pub unsafe extern "C" fn js_node_http2_connect(
    authority_f64: f64,
    options_f64: f64,
    listener: i64,
) -> i64 {
    ensure_gc_scanner_registered();
    let authority =
        jsvalue_to_owned_string(authority_f64).unwrap_or_else(|| "http://localhost:80".to_string());
    let callback = if listener != 0 {
        listener
    } else {
        closure_arg(Some(options_f64))
    };
    let (host, port, host_port) = parse_authority(&authority);
    let local_server_handle = h2_listening_server_for_authority(&host_port).unwrap_or(0);
    let sender_slot = Arc::new(Mutex::new(None));
    let mut listeners = HashMap::new();
    if callback != 0 {
        listeners
            .entry("connect".to_string())
            .or_insert_with(Vec::new)
            .push(callback);
    }
    let session_handle = register_handle(Http2SessionHandle {
        server_handle: local_server_handle,
        connection_port: 0,
        session_event_emitted: false,
        connect_event_emitted: false,
        session_type: 1,
        connected: false,
        encrypted: false,
        alpn_protocol: "h2c".to_string(),
        connecting: true,
        closed: false,
        destroyed: false,
        pending_settings_ack: false,
        authority: host_port,
        local_settings: Http2SettingsState::default(),
        remote_settings: Http2SettingsState::default(),
        local_window_size: 65_535,
        sender: sender_slot.clone(),
        listeners,
        close_callbacks: Vec::new(),
        pending_callbacks: Vec::new(),
        timeout_callback: 0,
    });

    perry_ffi::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create http2 client runtime");
        runtime.block_on(async move {
            let stream = match connect_h2_stream(
                &host,
                port,
                session_handle,
                local_server_handle != 0,
            )
            .await
            {
                Ok(stream) => {
                    // Node default: TCP_NODELAY on for a freshly-connected socket.
                    let _ = stream.set_nodelay(true);
                    stream
                }
                Err(err) => {
                    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                        session.connecting = false;
                        session.closed = true;
                        session.destroyed = true;
                    }
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: session_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };
            let (sender, connection) = match h2::client::handshake(stream).await {
                Ok(parts) => parts,
                Err(err) => {
                    if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                        session.connecting = false;
                        session.closed = true;
                        session.destroyed = true;
                    }
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: session_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };
            if let Ok(mut slot) = sender_slot.lock() {
                *slot = Some(sender);
            }
            if let Some(session) = get_handle_mut::<Http2SessionHandle>(session_handle) {
                session.connected = true;
                session.connecting = false;
                session.pending_settings_ack = true;
            }
            push_h2_event(Http2PendingEvent::ClientConnect { session_handle });
            let _ = connection.await;
            mark_session_closed(session_handle);
        });
    });

    session_handle
}

pub(crate) fn parse_authority(authority: &str) -> (String, u16, String) {
    let without_scheme = authority
        .strip_prefix("http://")
        .or_else(|| authority.strip_prefix("https://"))
        .unwrap_or(authority);
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
    if let Some(rest) = host_port.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let host = rest[..end].to_string();
            let port = rest[end + 1..]
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(80);
            return (host, port, host_port.to_string());
        }
    }
    let mut parts = host_port.rsplitn(2, ':');
    let maybe_port = parts.next().unwrap_or("");
    let maybe_host = parts.next();
    if let (Some(host), Ok(port)) = (maybe_host, maybe_port.parse::<u16>()) {
        (host.to_string(), port, host_port.to_string())
    } else {
        (host_port.to_string(), 80, host_port.to_string())
    }
}

pub(crate) fn parse_headers_object(value: f64) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let v = JsValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return out;
    }
    let Some(json) = perry_ffi::json_stringify(v) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) else {
        return out;
    };
    let Some(obj) = parsed.as_object() else {
        return out;
    };
    for (key, value) in obj {
        let value = value
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| value.to_string().trim_matches('"').to_string());
        out.insert(key.to_ascii_lowercase(), value);
    }
    out
}

pub(crate) fn start_client_request(stream_handle: i64, body: Vec<u8>) {
    let (session_handle, headers, sender_slot, authority) =
        match get_handle::<Http2StreamHandle>(stream_handle) {
            Some(stream) => {
                let session_handle = stream.session_handle;
                let Some(session) = get_handle::<Http2SessionHandle>(session_handle) else {
                    return;
                };
                (
                    session_handle,
                    stream.request_headers.clone(),
                    session.sender.clone(),
                    session.authority.clone(),
                )
            }
            None => return,
        };

    perry_ffi::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create http2 request runtime");
        runtime.block_on(async move {
            let sender = match sender_slot.lock().ok().and_then(|mut slot| slot.take()) {
                Some(sender) => sender,
                None => {
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: stream_handle,
                        message: "HTTP/2 session is not connected".to_string(),
                    });
                    return;
                }
            };
            let mut sender = match sender.ready().await {
                Ok(sender) => sender,
                Err(err) => {
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: stream_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };

            let method = headers
                .get(":method")
                .cloned()
                .unwrap_or_else(|| "GET".to_string());
            let path = headers
                .get(":path")
                .cloned()
                .unwrap_or_else(|| "/".to_string());
            let uri = format!("http://{}{}", authority, path);
            let mut builder = Request::builder().method(method.as_str()).uri(uri.as_str());
            for (name, value) in &headers {
                if name.starts_with(':') {
                    continue;
                }
                if let (Ok(header_name), Ok(header_value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    builder = builder.header(header_name, header_value);
                }
            }
            let mut request = match builder.body(()) {
                Ok(request) => request,
                Err(err) => {
                    if let Ok(mut slot) = sender_slot.lock() {
                        *slot = Some(sender);
                    }
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: stream_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };
            *request.version_mut() = Version::HTTP_2;
            let end_of_stream = body.is_empty();
            let (response_future, mut send_stream) =
                match sender.send_request(request, end_of_stream) {
                    Ok(parts) => parts,
                    Err(err) => {
                        if let Ok(mut slot) = sender_slot.lock() {
                            *slot = Some(sender);
                        }
                        push_h2_event(Http2PendingEvent::ClientError {
                            handle: stream_handle,
                            message: err.to_string(),
                        });
                        return;
                    }
                };
            if !body.is_empty() {
                let _ = send_stream.send_data(Bytes::from(body), true);
            }
            if let Ok(mut slot) = sender_slot.lock() {
                *slot = Some(sender);
            }
            let response = match response_future.await {
                Ok(response) => response,
                Err(err) => {
                    push_h2_event(Http2PendingEvent::ClientError {
                        handle: stream_handle,
                        message: err.to_string(),
                    });
                    return;
                }
            };
            let mut response_headers = HashMap::new();
            response_headers.insert(
                ":status".to_string(),
                response.status().as_u16().to_string(),
            );
            for (name, value) in response.headers() {
                if let Ok(value) = value.to_str() {
                    response_headers.insert(name.as_str().to_ascii_lowercase(), value.to_string());
                }
            }
            push_h2_event(Http2PendingEvent::ClientResponse {
                stream_handle,
                headers: response_headers,
            });
            let mut body = response.into_body();
            while let Some(chunk) = body.data().await {
                match chunk {
                    Ok(bytes) => {
                        push_h2_event(Http2PendingEvent::ClientData {
                            stream_handle,
                            body: bytes.to_vec(),
                        });
                    }
                    Err(err) => {
                        push_h2_event(Http2PendingEvent::ClientError {
                            handle: stream_handle,
                            message: err.to_string(),
                        });
                        return;
                    }
                }
            }
            let _ = session_handle;
            push_h2_event(Http2PendingEvent::ClientEnd { stream_handle });
        });
    });
}
