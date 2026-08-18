//! Handle-based method and property dispatch for the `node:tls` server and
//! socket surfaces.
//!
//! Split out of `tls.rs` to keep that file under the 2000-line lint cap
//! (`scripts/check_file_size.sh`). Items are moved verbatim; the three `pub`
//! entry points are re-exported from the parent module so
//! `crate::tls::should_dispatch_tls_handle` / `dispatch_tls_handle` /
//! `dispatch_tls_property` keep resolving unchanged.

use perry_runtime::{JSValue, StringHeader};

use super::{
    event_names_json, f64_from_raw_bits, is_tls_server_handle, is_tls_socket_handle,
    js_tls_server_address, js_tls_server_close, js_tls_server_event_names,
    js_tls_server_get_ticket_keys, js_tls_server_listen, js_tls_server_listener_count,
    js_tls_server_on, js_tls_server_once, js_tls_server_remove_all_listeners,
    js_tls_server_remove_listener, js_tls_server_set_secure_context, js_tls_server_set_ticket_keys,
    js_tls_socket_export_keying_material, js_tls_socket_get_certificate, js_tls_socket_get_cipher,
    js_tls_socket_get_peer_certificate, js_tls_socket_get_protocol, js_tls_socket_get_session,
    js_tls_socket_is_session_reused, js_tls_socket_set_max_send_fragment, json_value_from_str,
    jsvalue_to_bytes, listener_count, nanbox_handle, nanbox_str, pointer_addr, raw_handle_value,
    register_listener, remove_all_listeners, remove_listener, servers, sockets, string_from_header,
    undefined, TlsSocketCommand, TAG_UNDEFINED_BITS,
};

fn tls_server_method_name_static(method: &str) -> Option<&'static [u8]> {
    match method {
        "listen" => Some(b"listen"),
        "close" => Some(b"close"),
        "address" => Some(b"address"),
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "once" => Some(b"once"),
        "off" => Some(b"off"),
        "removeListener" => Some(b"removeListener"),
        "removeAllListeners" => Some(b"removeAllListeners"),
        "listenerCount" => Some(b"listenerCount"),
        "eventNames" => Some(b"eventNames"),
        "setSecureContext" => Some(b"setSecureContext"),
        "getTicketKeys" => Some(b"getTicketKeys"),
        "setTicketKeys" => Some(b"setTicketKeys"),
        "ref" => Some(b"ref"),
        "unref" => Some(b"unref"),
        _ => None,
    }
}

fn tls_socket_introspection_method_name_static(method: &str) -> Option<&'static [u8]> {
    match method {
        "getProtocol" => Some(b"getProtocol"),
        "getCipher" => Some(b"getCipher"),
        "getPeerCertificate" => Some(b"getPeerCertificate"),
        "getCertificate" => Some(b"getCertificate"),
        "getSession" => Some(b"getSession"),
        "isSessionReused" => Some(b"isSessionReused"),
        "exportKeyingMaterial" => Some(b"exportKeyingMaterial"),
        "setMaxSendFragment" => Some(b"setMaxSendFragment"),
        "ref" => Some(b"ref"),
        "unref" => Some(b"unref"),
        _ => None,
    }
}

fn tls_socket_server_method_name_static(method: &str) -> Option<&'static [u8]> {
    tls_socket_introspection_method_name_static(method).or_else(|| match method {
        "write" => Some(b"write"),
        "end" => Some(b"end"),
        "destroy" => Some(b"destroy"),
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "once" => Some(b"once"),
        "off" => Some(b"off"),
        "removeListener" => Some(b"removeListener"),
        "removeAllListeners" => Some(b"removeAllListeners"),
        "listenerCount" => Some(b"listenerCount"),
        "eventNames" => Some(b"eventNames"),
        _ => None,
    })
}

/// `js_class_method_bind` retains the name pointer in the closure, so make a
/// non-static forwarded property slice impossible to pass accidentally.
unsafe fn bind_static_handle_method(handle: i64, method: &'static [u8]) -> f64 {
    perry_runtime::object::js_class_method_bind(
        raw_handle_value(handle),
        method.as_ptr(),
        method.len(),
    )
}

pub fn should_dispatch_tls_handle(handle: i64, method: &str) -> bool {
    if is_tls_server_handle(handle) {
        return tls_server_method_name_static(method).is_some();
    }
    sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|socket| {
            if socket.server_side {
                tls_socket_server_method_name_static(method).is_some()
            } else {
                tls_socket_introspection_method_name_static(method).is_some()
            }
        })
        .unwrap_or(false)
}

unsafe fn event_arg(args: &[f64], idx: usize) -> i64 {
    args.get(idx)
        .copied()
        .map(|value| (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64)
        .unwrap_or(0)
}

unsafe fn callback_bits(args: &[f64], idx: usize) -> i64 {
    args.get(idx)
        .copied()
        .map(|v| v.to_bits() as i64)
        .unwrap_or(TAG_UNDEFINED_BITS as i64)
}

pub unsafe fn dispatch_tls_handle(handle: i64, method: &str, args: &[f64]) -> f64 {
    if is_tls_server_handle(handle) {
        match method {
            "listen" => {
                let port = args.first().copied().unwrap_or(0.0);
                let host_or_cb = callback_bits(args, 1);
                let cb = callback_bits(args, 2);
                js_tls_server_listen(handle, port, host_or_cb, cb);
                return nanbox_handle(handle);
            }
            "close" => {
                js_tls_server_close(handle, callback_bits(args, 0));
                return nanbox_handle(handle);
            }
            "address" => {
                let ptr = js_tls_server_address(handle);
                return f64::from_bits(perry_runtime::json::js_json_parse_or_null(ptr).bits());
            }
            "on" | "addListener" => {
                js_tls_server_on(handle, event_arg(args, 0), callback_bits(args, 1));
                return nanbox_handle(handle);
            }
            "once" => {
                js_tls_server_once(handle, event_arg(args, 0), callback_bits(args, 1));
                return nanbox_handle(handle);
            }
            "off" | "removeListener" => {
                js_tls_server_remove_listener(handle, event_arg(args, 0), callback_bits(args, 1));
                return nanbox_handle(handle);
            }
            "removeAllListeners" => {
                js_tls_server_remove_all_listeners(handle, event_arg(args, 0));
                return nanbox_handle(handle);
            }
            "listenerCount" => return js_tls_server_listener_count(handle, event_arg(args, 0)),
            "eventNames" => {
                let ptr = js_tls_server_event_names(handle);
                return f64::from_bits(perry_runtime::json::js_json_parse_or_null(ptr).bits());
            }
            "setSecureContext" => {
                js_tls_server_set_secure_context(handle, callback_bits(args, 0));
                return nanbox_handle(handle);
            }
            "getTicketKeys" => return js_tls_server_get_ticket_keys(handle),
            "setTicketKeys" => {
                js_tls_server_set_ticket_keys(handle, callback_bits(args, 0));
                return nanbox_handle(handle);
            }
            "ref" | "unref" => return nanbox_handle(handle),
            _ => {}
        }
    }

    if is_tls_socket_handle(handle) {
        match method {
            "write" if !args.is_empty() => {
                if let Some(socket) = sockets().lock().unwrap().get(&handle) {
                    if let Some(tx) = &socket.cmd_tx {
                        if let Some(bytes) = jsvalue_to_bytes(args[0]) {
                            let _ = tx.send(TlsSocketCommand::Write(bytes));
                        }
                    }
                }
                return f64::from_bits(TAG_UNDEFINED_BITS);
            }
            "end" => {
                if let Some(socket) = sockets().lock().unwrap().get(&handle) {
                    if let Some(tx) = &socket.cmd_tx {
                        if let Some(value) = args.first().copied() {
                            if let Some(bytes) = jsvalue_to_bytes(value) {
                                if !bytes.is_empty() {
                                    let _ = tx.send(TlsSocketCommand::Write(bytes));
                                }
                            }
                        }
                        let _ = tx.send(TlsSocketCommand::End);
                    }
                }
                return f64::from_bits(TAG_UNDEFINED_BITS);
            }
            "destroy" => {
                if let Some(socket) = sockets().lock().unwrap().get(&handle) {
                    if let Some(tx) = &socket.cmd_tx {
                        let _ = tx.send(TlsSocketCommand::Destroy);
                    }
                }
                return f64::from_bits(TAG_UNDEFINED_BITS);
            }
            "on" | "addListener" => {
                let event = string_from_header(event_arg(args, 0) as *const StringHeader);
                if let Some(event) = event {
                    let cb =
                        pointer_addr(f64_from_raw_bits(callback_bits(args, 1))).unwrap_or(0) as i64;
                    register_listener(handle, event, cb, false);
                }
                return nanbox_handle(handle);
            }
            "once" => {
                let event = string_from_header(event_arg(args, 0) as *const StringHeader);
                if let Some(event) = event {
                    let cb =
                        pointer_addr(f64_from_raw_bits(callback_bits(args, 1))).unwrap_or(0) as i64;
                    register_listener(handle, event, cb, true);
                }
                return nanbox_handle(handle);
            }
            "off" | "removeListener" => {
                if let Some(event) = string_from_header(event_arg(args, 0) as *const StringHeader) {
                    let cb =
                        pointer_addr(f64_from_raw_bits(callback_bits(args, 1))).unwrap_or(0) as i64;
                    remove_listener(handle, &event, cb);
                }
                return nanbox_handle(handle);
            }
            "removeAllListeners" => {
                let event = string_from_header(event_arg(args, 0) as *const StringHeader);
                remove_all_listeners(handle, event.as_deref());
                return nanbox_handle(handle);
            }
            "listenerCount" => {
                return string_from_header(event_arg(args, 0) as *const StringHeader)
                    .map(|event| listener_count(handle, &event))
                    .unwrap_or(0.0);
            }
            "eventNames" => return json_value_from_str(&event_names_json(handle)),
            "getProtocol" => return js_tls_socket_get_protocol(handle),
            "getCipher" => return js_tls_socket_get_cipher(handle),
            "getPeerCertificate" => {
                return js_tls_socket_get_peer_certificate(
                    handle,
                    args.first().copied().unwrap_or(undefined()),
                )
            }
            "getCertificate" => return js_tls_socket_get_certificate(handle),
            "getSession" => return js_tls_socket_get_session(handle),
            "isSessionReused" => return js_tls_socket_is_session_reused(handle),
            "exportKeyingMaterial" => {
                return js_tls_socket_export_keying_material(
                    handle,
                    args.first().copied().unwrap_or(0.0),
                    event_arg(args, 1),
                )
            }
            "setMaxSendFragment" => {
                return js_tls_socket_set_max_send_fragment(
                    handle,
                    args.first().copied().unwrap_or(0.0),
                )
            }
            "ref" | "unref" => return nanbox_handle(handle),
            _ => {}
        }
    }
    undefined()
}

pub unsafe fn dispatch_tls_property(handle: i64, property: &str) -> Option<f64> {
    if is_tls_server_handle(handle) {
        match property {
            "listening" => {
                let value = servers()
                    .lock()
                    .unwrap()
                    .get(&handle)
                    .map(|s| s.listening)
                    .unwrap_or(false);
                return Some(f64::from_bits(JSValue::bool(value).bits()));
            }
            _ => {}
        }
        if let Some(method) = tls_server_method_name_static(property) {
            return Some(bind_static_handle_method(handle, method));
        }
    }
    if is_tls_socket_handle(handle) {
        match property {
            "encrypted" => return Some(f64::from_bits(JSValue::bool(true).bits())),
            "authorized" => {
                let authorized = sockets()
                    .lock()
                    .unwrap()
                    .get(&handle)
                    .map(|s| s.authorized)
                    .unwrap_or(false);
                return Some(f64::from_bits(JSValue::bool(authorized).bits()));
            }
            "authorizationError" => {
                return Some(f64::from_bits(perry_runtime::JSValue::null().bits()))
            }
            "servername" => return Some(nanbox_str("localhost")),
            "alpnProtocol" => return Some(f64::from_bits(perry_runtime::JSValue::null().bits())),
            _ => {}
        }
        let method = sockets().lock().unwrap().get(&handle).and_then(|socket| {
            if socket.server_side {
                tls_socket_server_method_name_static(property)
            } else {
                tls_socket_introspection_method_name_static(property)
            }
        });
        if let Some(method) = method {
            return Some(bind_static_handle_method(handle, method));
        }
    }
    None
}

#[cfg(test)]
mod static_method_name_tests {
    use super::*;

    fn assert_static_lookup(lookup: fn(&str) -> Option<&'static [u8]>, names: &[&str]) {
        for name in names {
            let owned = (*name).to_owned();
            let found = lookup(&owned).expect("known method must resolve");
            assert_eq!(found, name.as_bytes());
            assert_ne!(
                found.as_ptr(),
                owned.as_ptr(),
                "lookup borrowed the forwarded property name for {name}"
            );
            assert_eq!(found.as_ptr(), lookup(name).unwrap().as_ptr());
        }
        assert!(lookup("notATlsMethod").is_none());
    }

    #[test]
    fn tls_method_name_lookups_return_static_literals() {
        assert_static_lookup(
            tls_server_method_name_static,
            &[
                "listen",
                "close",
                "address",
                "on",
                "addListener",
                "once",
                "off",
                "removeListener",
                "removeAllListeners",
                "listenerCount",
                "eventNames",
                "setSecureContext",
                "getTicketKeys",
                "setTicketKeys",
                "ref",
                "unref",
            ],
        );
        assert_static_lookup(
            tls_socket_introspection_method_name_static,
            &[
                "getProtocol",
                "getCipher",
                "getPeerCertificate",
                "getCertificate",
                "getSession",
                "isSessionReused",
                "exportKeyingMaterial",
                "setMaxSendFragment",
                "ref",
                "unref",
            ],
        );
        assert_static_lookup(
            tls_socket_server_method_name_static,
            &[
                "getProtocol",
                "getCipher",
                "getPeerCertificate",
                "getCertificate",
                "getSession",
                "isSessionReused",
                "exportKeyingMaterial",
                "setMaxSendFragment",
                "ref",
                "unref",
                "write",
                "end",
                "destroy",
                "on",
                "addListener",
                "once",
                "off",
                "removeListener",
                "removeAllListeners",
                "listenerCount",
                "eventNames",
            ],
        );
    }
}
