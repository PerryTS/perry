//! `node:dgram` networking: registry port allocation, bind (deterministic +
//! real OS socket), datagram send, message/buffer extraction, multicast parsing
//! and `ref`/`unref`.
//!
//! Split out of `dgram.rs` (pure code move). See the trunk module for the data
//! model and shared helpers.

use super::*;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;

use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::value::JSValue;

pub(crate) fn allocate_port(registry: &mut DgramRegistry, address: &str) -> u16 {
    for _ in 0..16384 {
        let port = registry.next_port;
        registry.next_port = if registry.next_port >= 65535 {
            49152
        } else {
            registry.next_port + 1
        };
        if !registry.bound.contains_key(&SocketKey {
            address: address.to_string(),
            port,
        }) {
            return port;
        }
    }
    49152
}

pub(crate) fn remove_bound_socket(socket: f64) {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        return;
    }
    let Some(address) = hidden_string(socket, KEY_ADDRESS) else {
        return;
    };
    let port = hidden_port(socket, KEY_PORT);
    let key = SocketKey { address, port };
    if let Ok(mut registry) = DGRAM_REGISTRY.lock() {
        if registry
            .bound
            .get(&key)
            .is_some_and(|value| value.to_bits() == socket.to_bits())
        {
            registry.bound.remove(&key);
        }
    }
}

pub(crate) fn bind_socket(socket: f64, port: u16, address: String) -> u16 {
    let address = normalize_address(&address, socket);
    let family = family_for_address(&address, socket);
    remove_bound_socket(socket);
    let actual_port = if let Ok(mut registry) = DGRAM_REGISTRY.lock() {
        let actual_port = if port == 0 {
            allocate_port(&mut registry, &address)
        } else {
            port
        };
        registry.bound.insert(
            SocketKey {
                address: address.clone(),
                port: actual_port,
            },
            socket,
        );
        actual_port
    } else {
        port
    };
    set_hidden_value(socket, KEY_ADDRESS, str_value(&address));
    set_hidden_value(socket, KEY_FAMILY, str_value(family));
    set_hidden_value(socket, KEY_PORT, actual_port as f64);
    set_hidden_value(socket, KEY_BOUND, bool_value(true));
    actual_port
}

pub(crate) fn ensure_bound(socket: f64) {
    if is_truthy_hidden(socket, KEY_BOUND) {
        return;
    }
    if deterministic() {
        bind_socket(socket, 0, default_loopback_address(socket));
    } else {
        let _ = real_bind(socket, 0, &default_bind_address(socket));
    }
}

pub(crate) fn lookup_bound_socket(address: &str, port: u16, socket: f64) -> Option<f64> {
    let address = normalize_address(address, socket);
    let fallbacks: &[&str] = if address.contains(':') {
        &[address.as_str(), "::"]
    } else {
        &[address.as_str(), "0.0.0.0"]
    };
    let registry = DGRAM_REGISTRY.lock().ok()?;
    for candidate in fallbacks {
        let key = SocketKey {
            address: (*candidate).to_string(),
            port,
        };
        if let Some(value) = registry.bound.get(&key) {
            return Some(*value);
        }
    }
    None
}

pub(crate) fn build_address_info(address: &str, family: &str, port: u16) -> f64 {
    let obj = js_object_alloc(0, 3);
    js_object_set_field_by_name(obj, key("address"), str_value(address));
    js_object_set_field_by_name(obj, key("family"), str_value(family));
    js_object_set_field_by_name(obj, key("port"), port as f64);
    boxed_pointer(obj as *const u8)
}

pub(crate) fn build_rinfo(address: &str, family: &str, port: u16, size: usize) -> f64 {
    let obj = js_object_alloc(0, 4);
    js_object_set_field_by_name(obj, key("address"), str_value(address));
    js_object_set_field_by_name(obj, key("family"), str_value(family));
    js_object_set_field_by_name(obj, key("port"), port as f64);
    js_object_set_field_by_name(obj, key("size"), size as f64);
    boxed_pointer(obj as *const u8)
}

pub(crate) fn message_value(value: f64) -> Option<(f64, usize)> {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
        if ptr.is_null() {
            return None;
        }
        let buf = crate::buffer::js_buffer_from_string(ptr, 0);
        let len = unsafe { (*buf).length as usize };
        return Some((boxed_pointer(buf as *const u8), len));
    }
    let raw = raw_ptr_from_value(value);
    if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
        let buf = raw as *const crate::buffer::BufferHeader;
        return Some((value, unsafe { (*buf).length as usize }));
    }
    if raw >= 0x10000 && crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        let len = unsafe {
            crate::typedarray::typed_array_bytes(raw as *const crate::typedarray::TypedArrayHeader)
                .map(|bytes| bytes.len())
                .unwrap_or(0)
        };
        return Some((value, len));
    }
    None
}

/// Whether `PERRY_DETERMINISTIC_NET=1` — use the in-process loopback registry
/// instead of real OS sockets (#4911).
pub(crate) fn deterministic() -> bool {
    crate::stub_diag::deterministic_net_enabled()
}

/// The reactor id stashed on a real-mode socket, if it is bound.
pub(crate) fn reactor_id(socket: f64) -> Option<u64> {
    get_hidden_value(socket, KEY_REACTOR_ID)
        .and_then(number_value)
        .map(|n| n as u64)
}

pub(crate) fn live_udp(socket: f64) -> Option<Arc<UdpSocket>> {
    crate::dgram_reactor::udp_for(reactor_id(socket)?)
}

/// Build a `Buffer` JS value from raw datagram bytes.
pub(crate) fn make_buffer(data: &[u8]) -> f64 {
    let buf = crate::buffer::js_buffer_alloc(data.len() as i32, 0);
    unsafe {
        if !buf.is_null() {
            if !data.is_empty() {
                let dst = (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
                // GC_STORE_AUDIT(POINTER_FREE): raw datagram bytes copied into a
                // freshly-allocated Buffer payload — u8 data, never heap pointers.
                std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
            }
            (*buf).length = data.len() as u32;
        }
    }
    boxed_pointer(buf as *const u8)
}

/// Deliver one received datagram to its socket as a `'message'` event. Called
/// on the main thread from [`crate::dgram_reactor::pump`]. The `Buffer` is
/// GC-rooted across the `rinfo` allocation so a collection between the two
/// can't reclaim it.
pub(crate) fn dgram_emit_message(
    socket_bits: u64,
    data: &[u8],
    src_ip: &str,
    src_port: u16,
    src_family: &str,
) {
    let socket = f64::from_bits(socket_bits);
    let scope = crate::gc::RuntimeHandleScope::new();
    let buffer = scope.root_nanbox_f64(make_buffer(data));
    let rinfo = scope.root_nanbox_f64(build_rinfo(src_ip, src_family, src_port, data.len()));
    emit_event_value(
        socket,
        str_value("message"),
        &[buffer.get_nanbox_f64(), rinfo.get_nanbox_f64()],
    );
}

/// Extract the raw bytes to transmit from a `send()` message argument
/// (string → UTF-8, Buffer, or TypedArray/DataView).
pub(crate) fn message_bytes(value: f64) -> Option<Vec<u8>> {
    if let Some(text) = string_to_rust(value) {
        return Some(text.into_bytes());
    }
    let raw = raw_ptr_from_value(value);
    if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
        let buf = raw as *const crate::buffer::BufferHeader;
        unsafe {
            let len = (*buf).length as usize;
            let data = (raw as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
            return Some(std::slice::from_raw_parts(data, len).to_vec());
        }
    }
    if raw >= 0x10000 && crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        return unsafe {
            crate::typedarray::typed_array_bytes(raw as *const crate::typedarray::TypedArrayHeader)
                .map(<[u8]>::to_vec)
        };
    }
    None
}

/// Map a `std::io::ErrorKind` from a socket syscall onto the Node error code.
pub(crate) fn io_error_code(err: &std::io::Error) -> &'static str {
    match err.kind() {
        std::io::ErrorKind::AddrInUse => "EADDRINUSE",
        std::io::ErrorKind::AddrNotAvailable => "EADDRNOTAVAIL",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::ConnectionRefused => "ECONNREFUSED",
        _ => "EINVAL",
    }
}

/// Build (not throw) a Node-style socket error value with `code`/`syscall`.
pub(crate) fn socket_error_value(message: &str, code: &'static str, syscall: &'static str) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    crate::node_submodules::register_error_syscall(msg, syscall);
    let err = crate::error::js_error_new_with_message(msg);
    boxed_pointer(err as *const u8)
}

pub(crate) fn dns_not_found_value(host: &str) -> f64 {
    socket_error_value(
        &format!("getaddrinfo ENOTFOUND {host}"),
        "ENOTFOUND",
        "getaddrinfo",
    )
}

/// Resolve a `send()` destination to a concrete `SocketAddr`. IP literals are
/// used verbatim; hostnames go through `getaddrinfo`.
pub(crate) fn resolve_send_addr(address: &str, port: u16) -> Result<SocketAddr, f64> {
    if let Ok(ip) = address.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    match (address, port).to_socket_addrs() {
        Ok(mut iter) => iter.next().ok_or_else(|| dns_not_found_value(address)),
        Err(_) => Err(dns_not_found_value(address)),
    }
}

/// Real bind: open + bind an OS `UdpSocket`, register it with the reactor (which
/// starts the recv thread), and record the actual local address. On failure
/// returns the error value for the caller to emit as `'error'`.
pub(crate) fn real_bind(socket: f64, port: u16, address: &str) -> Result<(), f64> {
    let address = normalize_address(address, socket);
    let udp = match UdpSocket::bind((address.as_str(), port)) {
        Ok(udp) => udp,
        Err(err) => {
            return Err(socket_error_value(
                &format!("bind {} {address}:{port}", io_error_code(&err)),
                io_error_code(&err),
                "bind",
            ));
        }
    };
    let (actual_address, actual_port, family) = match udp.local_addr() {
        Ok(sa) => (
            sa.ip().to_string(),
            sa.port(),
            if sa.is_ipv4() { "IPv4" } else { "IPv6" },
        ),
        Err(_) => (address.clone(), port, family_for_address(&address, socket)),
    };
    let id = crate::dgram_reactor::register(socket.to_bits(), Arc::new(udp));
    set_hidden_value(socket, KEY_REACTOR_ID, id as f64);
    set_hidden_value(socket, KEY_ADDRESS, str_value(&actual_address));
    set_hidden_value(socket, KEY_FAMILY, str_value(family));
    set_hidden_value(socket, KEY_PORT, actual_port as f64);
    set_hidden_value(socket, KEY_BOUND, bool_value(true));
    Ok(())
}

/// Real `send()`: transmit over the OS socket. Errors go to the callback when
/// one is supplied, otherwise to an `'error'` event (Node semantics).
pub(crate) fn real_send(socket: f64, args: &[f64]) -> f64 {
    let msg = args.first().copied().unwrap_or_else(undefined_value);
    let Some(bytes) = message_bytes(msg) else {
        throw_invalid_message(msg);
    };
    let (port, address) = send_destination(socket, args);
    if let Some(err) = ensure_bound_real(socket) {
        return finish_send(socket, args, Err(err));
    }
    let outcome = match (live_udp(socket), resolve_send_addr(&address, port)) {
        (Some(udp), Ok(dest)) => match udp.send_to(&bytes, dest) {
            Ok(_) => Ok(bytes.len()),
            Err(err) => Err(socket_error_value(
                &format!("send {}", io_error_code(&err)),
                io_error_code(&err),
                "send",
            )),
        },
        (_, Err(err)) => Err(err),
        (None, _) => Err(socket_error_value("send EBADF", "EBADF", "send")),
    };
    finish_send(socket, args, outcome)
}

pub(crate) fn finish_send(socket: f64, args: &[f64], outcome: Result<usize, f64>) -> f64 {
    match (outcome, callback_from_args(args)) {
        (Ok(size), Some(callback)) => {
            call_function(callback, socket, &[null_value(), size as f64]);
        }
        (Ok(_), None) => {}
        (Err(error), Some(callback)) => {
            call_function(callback, socket, &[error]);
        }
        (Err(error), None) => {
            emit_event(socket, "error", &[error]);
        }
    }
    undefined_value()
}

/// Implicit bind on first `send`/`connect` (real mode). Returns an error value
/// if the bind failed.
pub(crate) fn ensure_bound_real(socket: f64) -> Option<f64> {
    if is_truthy_hidden(socket, KEY_BOUND) {
        return None;
    }
    real_bind(socket, 0, &default_bind_address(socket)).err()
}

/// Borrow the live `UdpSocket` and run `f`; no-op when the socket is not bound
/// to a real OS socket (e.g. closed).
pub(crate) fn with_udp<F: FnOnce(&UdpSocket)>(socket: f64, f: F) {
    if let Some(udp) = live_udp(socket) {
        f(&udp);
    }
}

pub(crate) fn parse_multicast_v4(addr: &str) -> Option<Ipv4Addr> {
    addr.parse::<Ipv4Addr>().ok()
}

pub(crate) fn parse_multicast_v6(addr: &str) -> Option<Ipv6Addr> {
    addr.parse::<Ipv6Addr>().ok()
}

/// `socket.ref()` / `socket.unref()` — toggle whether the bound socket keeps
/// the event loop alive. No-op in deterministic mode (no real socket).
pub(crate) fn ref_impl(socket: f64, refed: bool) -> f64 {
    if !deterministic() {
        if let Some(id) = reactor_id(socket) {
            crate::dgram_reactor::set_refed(id, refed);
        }
    }
    socket
}
