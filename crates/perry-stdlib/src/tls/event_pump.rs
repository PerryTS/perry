//! Main-thread TLS event delivery and active-handle accounting.

use super::*;

pub fn record_tls_client_handle(handle: i64) {
    if handle <= 0 {
        return;
    }
    crate::common::async_bridge::ensure_pump_registered();
    ensure_tls_gc_scanner_registered();
    if !perry_runtime::tls::is_tls_client_handle(handle) {
        unsafe {
            perry_runtime::tls::js_tls_client_record_start(
                handle,
                undefined(),
                std::ptr::null(),
                0,
            );
        }
    }
}

pub fn is_tls_server_handle(handle: i64) -> bool {
    servers().lock().unwrap().contains_key(&handle)
}

pub fn is_tls_socket_handle(handle: i64) -> bool {
    sockets().lock().unwrap().contains_key(&handle)
        || perry_runtime::tls::is_tls_client_handle(handle)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_process_pending() -> i32 {
    let mut events = {
        let mut pending = pending_events().lock().unwrap();
        std::mem::take(&mut *pending)
    };
    let count = events.len() as i32;
    for event in events.drain(..) {
        match event {
            PendingTlsEvent::ServerListening(server_id) => {
                let callbacks = {
                    let mut all = listeners().lock().unwrap();
                    all.get_mut(&server_id)
                        .and_then(|per| per.remove("listening"))
                        .unwrap_or_default()
                };
                for cb in callbacks {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                drain_once_listeners(server_id, "listening");
            }
            PendingTlsEvent::ServerSecureConnection(server_id, socket_id) => {
                let socket = nanbox_handle(socket_id);
                for event_name in ["secureConnection", "connection"] {
                    for cb in listeners_for(server_id, event_name) {
                        if cb != 0 {
                            js_closure_call1(cb as *const ClosureHeader, socket);
                        }
                    }
                    drain_once_listeners(server_id, event_name);
                }
            }
            PendingTlsEvent::ServerClose(server_id) => {
                let callbacks = {
                    let mut all = listeners().lock().unwrap();
                    all.get_mut(&server_id)
                        .and_then(|per| per.remove("close"))
                        .unwrap_or_default()
                };
                for cb in callbacks {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                servers().lock().unwrap().remove(&server_id);
                listeners().lock().unwrap().remove(&server_id);
                once_flags().lock().unwrap().remove(&server_id);
            }
            PendingTlsEvent::ServerError(server_id, msg) => {
                let err = build_error_object(&msg);
                for cb in listeners_for(server_id, "error") {
                    if cb != 0 {
                        js_closure_call1(cb as *const ClosureHeader, err);
                    }
                }
                drain_once_listeners(server_id, "error");
            }
            PendingTlsEvent::ServerTlsClientError(server_id, socket_id, msg, code) => {
                let err = build_error_object_with_code(&msg, code.as_deref());
                let socket = nanbox_handle(socket_id);
                for cb in listeners_for(server_id, "tlsClientError") {
                    if cb != 0 {
                        js_closure_call2(cb as *const ClosureHeader, err, socket);
                    }
                }
                drain_once_listeners(server_id, "tlsClientError");
            }
            PendingTlsEvent::SocketData(socket_id, bytes) => {
                let data = buffer_from_bytes(&bytes);
                for cb in listeners_for(socket_id, "data") {
                    if cb != 0 {
                        js_closure_call1(cb as *const ClosureHeader, data);
                    }
                }
                drain_once_listeners(socket_id, "data");
            }
            PendingTlsEvent::SocketEnd(socket_id) => {
                for cb in listeners_for(socket_id, "end") {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                drain_once_listeners(socket_id, "end");
            }
            PendingTlsEvent::SocketClose(socket_id) => {
                for cb in listeners_for(socket_id, "close") {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                sockets().lock().unwrap().remove(&socket_id);
                listeners().lock().unwrap().remove(&socket_id);
                once_flags().lock().unwrap().remove(&socket_id);
            }
            PendingTlsEvent::SocketError(socket_id, msg) => {
                let err = build_error_object(&msg);
                for cb in listeners_for(socket_id, "error") {
                    if cb != 0 {
                        js_closure_call1(cb as *const ClosureHeader, err);
                    }
                }
                drain_once_listeners(socket_id, "error");
            }
        }
    }
    count
}

pub fn js_tls_has_active_handles() -> i32 {
    if !pending_events().lock().unwrap().is_empty() {
        return 1;
    }
    if servers()
        .lock()
        .unwrap()
        .values()
        .any(|server| server.listening || (server.closing && server.active_connections > 0))
    {
        return 1;
    }
    if sockets()
        .lock()
        .unwrap()
        .values()
        .any(|s| s.server_side && s.cmd_tx.is_some())
    {
        return 1;
    }
    0
}
