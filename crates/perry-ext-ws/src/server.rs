//! WebSocket server construction and HTTP-server attachment.
//!
//! # The standalone server
//!
//! `new WebSocketServer({ port })` binds through [`perry_http_server`], the
//! shared HTTP/1.1 server core, and takes the upgrade through its
//! [`perry_http_server::Host`] hook. That is not incidental reuse: a `ws`
//! server *is* an HTTP server that answers exactly one kind of request, and
//! the thing it has to get right before the `101` — decoding a request head,
//! answering a malformed one with a `400`, not parsing a pipelined frame as a
//! second request — is that core's, already tested. What used to be here was a
//! `tokio::net::TcpListener` accept loop plus a hand-rolled `read`-until-head
//! loop (`accept_on_stream`), and it is gone.
//!
//! `perry-http-server`'s upgrade hook went in *for this caller*: its module
//! header recorded the hook as withheld until a WebSocket implementation
//! existed that did not need an owned `AsyncRead + AsyncWrite`, and
//! [`crate::turnloop_link`] is that implementation.
use super::*;

use perry_http_server::{Request as HttpRequest, Response as HttpResponse, Upgraded};

/// The listener slot this crate's standalone server binds on.
///
/// Distinct from [`crate::turnloop_io::SUBSYSTEM`]: a slot holds one sink
/// function, and these are two — `perry-http-server`'s, which decodes HTTP on
/// the accepted connections, and this crate's own, which drives outbound
/// clients.
pub(crate) const SERVER_SUBSYSTEM: u8 = 8;

/// `ws` does not idle-close a WebSocket, and an upgraded connection is exempt
/// from the keep-alive deadline anyway. A plain HTTP request to a `ws` port is
/// answered with a `400` and closed, so nothing on this listener is ever an
/// idle keep-alive connection.
const IDLE_CLOSE_MS: u64 = 0;

/// The `perry_http_server::Host` behind one standalone `WebSocketServer`.
///
/// Every method runs inside the completion sink — on the loop thread, after a
/// turn — so nothing here runs JS. What an upgrade produces is a
/// `PendingWsEvent::Connection`, which `js_ws_process_pending` dispatches on
/// its own tick, exactly where the accept task's channel send used to deliver
/// it.
struct WsHost {
    server_handle: Handle,
}

impl perry_http_server::Host for WsHost {
    /// A request that is not an upgrade. `ws` answers `400 Bad Request` and
    /// closes, rather than leaving a browser hanging on a plain `GET /`.
    fn on_request(&self, request: HttpRequest) {
        perry_http_server::respond(
            request.conn_id,
            request.seq,
            HttpResponse {
                status: 400,
                headers: vec![
                    ("Connection".to_string(), "close".to_string()),
                    ("Content-Length".to_string(), "0".to_string()),
                ],
                ..Default::default()
            },
        );
    }

    fn takes_upgrades(&self) -> bool {
        true
    }

    fn on_upgrade(&self, request: HttpRequest, leftover: Vec<u8>) {
        let conn_id = request.conn_id;
        let server_handle = self.server_handle;
        let accepted = crate::accept_http_upgrade(
            &request,
            &leftover,
            crate::HTTP_SERVER_TRANSPORT,
            &[],
            |ws_id| {
                // The parent link is what routes a frame pipelined behind the
                // handshake to the server's own `'message'` listener, and
                // queueing `'connection'` here is what keeps that frame's
                // event from reaching the pump ahead of it. Both must happen
                // before the leftover is decoded, which is what this callback
                // is for.
                WS_CLIENT_PARENT_SERVER
                    .lock()
                    .unwrap()
                    .insert(ws_id, server_handle);
                push_ws_event(PendingWsEvent::Connection(server_handle, ws_id));
            },
        );
        if let Err(refusal) = accepted {
            perry_http_server::write_raw(conn_id, &refusal.response);
            push_ws_event(PendingWsEvent::ServerError(
                server_handle,
                format!("WebSocket handshake error: {}", refusal.message),
            ));
            perry_http_server::finish(conn_id);
        }
    }

    fn on_upgraded(&self, conn_id: i64, event: Upgraded<'_>) {
        if crate::drive_http_upgraded(conn_id, event) {
            // A half-close the protocol layer is finished with: end our side
            // gracefully rather than cancelling what it just queued.
            perry_http_server::finish(conn_id);
        }
    }
}

extern "C" {
    fn js_object_get_field_by_name(
        object: *const perry_ffi::ObjectHeader,
        key: *const StringHeader,
    ) -> JsValue;
}

/// Read one named field off a JS object value.
///
/// # Safety
/// `key` must be a Perry-runtime `StringHeader`.
pub(super) unsafe fn object_field_by_name(object: JsValue, key: *const StringHeader) -> JsValue {
    if !object.is_pointer() {
        return JsValue::from_bits(0x7FFC_0000_0000_0001);
    }
    js_object_get_field_by_name(object.as_pointer::<perry_ffi::ObjectHeader>(), key)
}

pub(super) fn value_string(value: JsValue) -> Option<String> {
    if value.is_short_string() {
        let mut bytes = [0; 5];
        let len = value.short_string_to_buf(&mut bytes)?;
        Some(String::from_utf8_lossy(&bytes[..len]).into_owned())
    } else {
        unsafe { read_str(value.as_string_ptr()) }
    }
}

/// `new WebSocketServer({ port })` — sync ctor; spawns the accept loop.
///
/// #1113: `new WebSocketServer({ noServer: true })` must NOT bind a
/// TCP port or spawn the accept loop — it's a passive registry whose
/// connections arrive exclusively via `wss.handleUpgrade(...)` driven
/// by a host server's `'upgrade'` event (fastify's `app.server` or
/// `node:http`). For that shape we register a listener-only handle and
/// return early; `WS_ACTIVE_SERVERS` is left untouched so a noServer
/// wss doesn't keep the event loop alive on its own (the host server's
/// has-active gate — `js_fastify_has_active` — does that).
#[no_mangle]
pub extern "C" fn js_ws_server_new(opts_f64: f64) -> Handle {
    ensure_runtime_hooks_registered();
    let scope = perry_ffi::TransientRootScope::enter();
    let opts = scope.root_nanbox(opts_f64);
    // Allocate each property key before reloading the rooted options receiver.
    let field = |key| {
        let key = alloc_string(key);
        let value = JsValue::from_bits(opts.get().to_bits());
        if !value.is_pointer() {
            return JsValue::UNDEFINED;
        }
        unsafe { js_object_get_field_by_name(value.as_pointer(), key.as_raw()) }
    };
    let port_value = field("port");
    let port = if port_value.is_number() {
        Some(port_value.to_number() as u16)
    } else {
        None
    };
    let no_server = field("noServer").to_bool();
    let attached = field("server");
    let attached_server = if attached.is_pointer() {
        Some((attached.bits() & POINTER_MASK) as i64)
    } else {
        None
    };
    let host_value = field("host");
    let host = value_string(host_value).unwrap_or_else(|| "0.0.0.0".into());
    let clients_bits = alloc_set(4).bits();

    if no_server || attached_server.is_some() || port.is_none() {
        return register_handle(WsServerHandle {
            listeners: HashMap::new(),
            port: 0,
            host,
            attached_server,
            no_server,
            is_listening: false,
            client_ids: Vec::new(),
            clients_bits,
            listener_id: None,
        });
    }
    let port = port.unwrap();

    let server_handle = register_handle(WsServerHandle {
        listeners: HashMap::new(),
        port,
        host: host.clone(),
        attached_server: None,
        no_server: false,
        is_listening: false,
        client_ids: Vec::new(),
        clients_bits,
        listener_id: None,
    });

    if !perry_http_server::available(SERVER_SUBSYSTEM) {
        // This agent owns no `turnloop::Loop` — a `worker_threads` agent —
        // and this crate has no second transport since the tokio accept
        // loop was deleted. Say so on `'error'` rather than returning a
        // handle that silently never listens.
        push_ws_event(PendingWsEvent::ServerError(
            server_handle,
            "WebSocketServer bind error: no event loop on this thread".to_string(),
        ));
        return server_handle;
    }

    let bound = perry_http_server::listen(
        SERVER_SUBSYSTEM,
        std::sync::Arc::new(WsHost { server_handle }),
        &host,
        port,
        511,
        false,
        // `ws` sets TCP_NODELAY on accepted sockets; without it a small frame
        // can sit in Nagle's queue behind the handshake.
        true,
        IDLE_CLOSE_MS,
    );
    match bound {
        Ok(bound) => {
            // The bind is synchronous, so `wss.address()` is already correct
            // inside a `listen(0)` program's first tick.
            if let Some(server) = get_handle_mut::<WsServerHandle>(server_handle) {
                server.is_listening = true;
                server.port = bound.port;
                server.host = bound.address;
                server.listener_id = Some(bound.listener_id);
            }
            WS_ACTIVE_SERVERS.fetch_add(1, Ordering::Relaxed);
            push_ws_event(PendingWsEvent::Listening(server_handle));
        }
        Err(e) => {
            push_ws_event(PendingWsEvent::ServerError(
                server_handle,
                format!("WebSocketServer bind error: {}", e.message()),
            ));
        }
    }
    server_handle
}

/// Return the persistent `Set` exposed as `WebSocketServer.clients`.
///
/// The Set is allocated with the server, updated before connection/close
/// callbacks run, and rooted through the server handle for its full lifetime.
#[no_mangle]
pub extern "C" fn js_ws_server_clients(handle: i64) -> f64 {
    get_handle_mut::<WsServerHandle>(handle)
        .map(|server| f64::from_bits(server.clients_bits))
        .unwrap_or_else(|| f64::from_bits(JsValue::UNDEFINED.bits()))
}

// The HTTP wrapper depends on ws for stream handoff. A host-supplied address
// reader keeps that dependency one-way and stores only a code pointer.
type HostAddress = fn(Handle) -> Option<(String, u16)>;
static HOST_ADDRESS: std::sync::OnceLock<HostAddress> = std::sync::OnceLock::new();

pub fn register_http_address_reader(reader: HostAddress) {
    let _ = HOST_ADDRESS.set(reader);
}

fn attached_servers(host: Handle) -> Vec<Handle> {
    let mut result = Vec::new();
    perry_ffi::iter_handle_ids_of::<WsServerHandle, _>(|id| result.push(id));
    result.retain(|id| {
        get_handle_mut::<WsServerHandle>(*id).is_some_and(|s| s.attached_server == Some(host))
    });
    result
}

pub fn has_attached_server(host: Handle) -> bool {
    !attached_servers(host).is_empty()
}

/// Called on the JS thread after HTTP has adopted the upgraded stream.
pub fn accept_attached_connection(host: Handle, request: f64, client: i64) {
    for server in attached_servers(host) {
        WS_CLIENT_PARENT_SERVER
            .lock()
            .unwrap()
            .insert(client as usize, server);
        track_server_client(server, client as usize);
        emit_server_event(
            server,
            "connection",
            f64::from_bits(client_js_value(client as usize).bits()),
            request,
            2,
        );
    }
}

pub fn attached_server_listening(host: Handle) {
    for server in attached_servers(host) {
        emit_server_event(server, "listening", undefined(), undefined(), 0);
    }
}

pub(super) fn undefined() -> f64 {
    f64::from_bits(JsValue::UNDEFINED.bits())
}

pub(super) fn decode_client_id(value: f64) -> usize {
    if value.to_bits() & TAG_MASK == POINTER_TAG {
        (value.to_bits() & POINTER_MASK) as usize
    } else {
        value as usize
    }
}

/// Snapshot and root listeners and arguments before invoking user code.
fn emit_server_event(handle: Handle, event: &str, first: f64, second: f64, argc: usize) -> i32 {
    let scope = perry_ffi::TransientRootScope::enter();
    let listeners = scope.root_addrs(&listeners_on_server(handle, event));
    let first = scope.root_nanbox(first);
    let second = scope.root_nanbox(second);
    let had_listeners = !listeners.is_empty();
    for cb in listeners {
        if cb.get() == 0 {
            continue;
        }
        unsafe {
            let closure = JsClosure::from_raw(cb.get() as *const RawClosureHeader);
            match argc {
                0 => {
                    closure.call0();
                }
                1 => {
                    closure.call1(first.get());
                }
                _ => {
                    closure.call2(first.get(), second.get());
                }
            }
        }
    }
    i32::from(had_listeners)
}

/// # Safety
/// `event` must point to a live runtime string.
#[no_mangle]
pub unsafe extern "C" fn js_ws_server_emit(
    handle: i64,
    event: *const StringHeader,
    first: f64,
    second: f64,
) -> i32 {
    let Some(event) = read_str(event) else {
        return 0;
    };
    emit_server_event(handle, &event, first, second, 2)
}

#[no_mangle]
pub extern "C" fn js_ws_server_address(handle: i64) -> f64 {
    let Some((attached, no_server, listening, host, port)) =
        get_handle_mut::<WsServerHandle>(handle).map(|s| {
            (
                s.attached_server,
                s.no_server,
                s.is_listening,
                s.host.clone(),
                s.port,
            )
        })
    else {
        return f64::from_bits(JsValue::NULL.bits());
    };
    if no_server {
        perry_ffi::throw_with_code(
            "The server is operating in \"noServer\" mode",
            "ERR_WEBSOCKET_NO_SERVER",
            perry_ffi::ErrorKind::Error,
        );
    }
    let address = if let Some(host) = attached {
        HOST_ADDRESS.get().and_then(|reader| reader(host))
    } else if listening {
        Some((host, port))
    } else {
        None
    };
    let Some((host, port)) = address else {
        return f64::from_bits(JsValue::NULL.bits());
    };
    let scope = perry_ffi::TransientRootScope::enter();
    let family = if host.contains(':') { "IPv6" } else { "IPv4" };
    let address = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string(&host).as_raw()).bits(),
    ));
    let family = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string(family).as_raw()).bits(),
    ));
    let (keys, shape) = perry_ffi::build_object_shape(&["address", "family", "port"]);
    unsafe {
        let object =
            perry_ffi::js_object_alloc_with_shape(shape, 3, keys.as_ptr(), keys.len() as u32);
        perry_ffi::js_object_set_field(object, 0, JsValue::from_bits(address.get().to_bits()));
        perry_ffi::js_object_set_field(object, 1, JsValue::from_bits(family.get().to_bits()));
        perry_ffi::js_object_set_field(object, 2, JsValue::from_number(port as f64));
        f64::from_bits(JsValue::from_object_ptr(object).bits())
    }
}
