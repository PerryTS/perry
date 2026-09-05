//! WebSocket server construction and HTTP-server attachment.
use super::*;

extern "C" {
    fn js_object_get_field_by_name(
        object: *const perry_ffi::ObjectHeader,
        key: *const StringHeader,
    ) -> JsValue;
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
            shutdown_tx: None,
        });
    }
    let port = port.unwrap();

    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel::<()>();
    let server_handle = register_handle(WsServerHandle {
        listeners: HashMap::new(),
        port,
        host: host.clone(),
        attached_server: None,
        no_server: false,
        is_listening: false,
        client_ids: Vec::new(),
        clients_bits,
        shutdown_tx: Some(shutdown_tx),
    });
    WS_ACTIVE_SERVERS.fetch_add(1, Ordering::Relaxed);
    let handle_id = server_handle;
    // Issue #606 — `spawn_blocking_with_reactor` already runs the closure
    // inside a tokio worker task, so `Handle::current().block_on(fut)` panics
    // with "Cannot start a runtime from within a runtime". Schedule the
    // accept loop as a sibling task on the existing runtime instead.
    // (Same root cause as the v0.5.691 sweep that fixed perry-ext-http's
    // server.rs / https_server.rs / http2_server.rs and perry-ext-ws's
    // `drive_server_client_io` — this site was missed in that sweep.)
    spawn_blocking(move || {
        tokio::spawn(async move {
            let addr = (host.as_str(), port);
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    push_ws_event(PendingWsEvent::ServerError(
                        handle_id,
                        format!("WebSocketServer bind error: {}", e),
                    ));
                    WS_ACTIVE_SERVERS.fetch_sub(1, Ordering::Relaxed);
                    return;
                }
            };
            if let Some(s) = get_handle_mut::<WsServerHandle>(handle_id) {
                s.is_listening = true;
                if let Ok(address) = listener.local_addr() {
                    s.port = address.port();
                    s.host = address.ip().to_string();
                }
            }
            push_ws_event(PendingWsEvent::Listening(handle_id));
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((tcp_stream, _addr)) => {
                                match tokio_tungstenite::accept_async(tcp_stream).await {
                                    Ok(ws_stream) => {
                                        let ws_id = register_handle(WsClientHandle) as usize;
                                        let (tx, rx) = mpsc::unbounded_channel::<WsCommand>();
                                        WS_CONNECTIONS.lock().unwrap().insert(ws_id, WsConnection {
                                            sender: tx,
                                            messages: Vec::new(),
                                            is_open: true,
                                            is_closing: false,
                                            is_closed: false,
                                        });
                                        WS_CLIENT_LISTENERS.lock().unwrap().insert(ws_id, WsClientListeners {
                                            listeners: HashMap::new(),
                                        });
                                        if let Some(s) = get_handle_mut::<WsServerHandle>(handle_id) {
                                            s.client_ids.push(ws_id);
                                        }
                                        WS_CLIENT_PARENT_SERVER.lock().unwrap().insert(ws_id, handle_id);
                                        push_ws_event(PendingWsEvent::Connection(handle_id, ws_id));
                                        drive_server_client_io(ws_id, ws_stream, rx);
                                    }
                                    Err(e) => {
                                        push_ws_event(PendingWsEvent::ServerError(
                                            handle_id,
                                            format!("WebSocket handshake error: {}", e),
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                push_ws_event(PendingWsEvent::ServerError(
                                    handle_id,
                                    format!("accept error: {}", e),
                                ));
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
            if let Some(s) = get_handle_mut::<WsServerHandle>(handle_id) {
                s.is_listening = false;
            }
            WS_ACTIVE_SERVERS.fetch_sub(1, Ordering::Relaxed);
        });
    });
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
