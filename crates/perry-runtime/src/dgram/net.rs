//! `node:dgram` networking: registry port allocation, bind (deterministic +
//! real OS socket), datagram send, message/buffer extraction, multicast parsing
//! and `ref`/`unref`.
//!
//! Split out of `dgram.rs` (pure code move). See the trunk module for the data
//! model and shared helpers.

use super::*;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;

#[cfg(not(target_os = "wasi"))]
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

// The socket error a completed or failed operation reports: turnloop's on a
// native target, the WASI reactor's (the same type, `wasi_net/mod.rs`) there.
#[cfg(target_os = "wasi")]
pub(crate) use crate::turnloop_net::NodeError;
#[cfg(not(target_os = "wasi"))]
pub(crate) use crate::turnloop_proc::NodeError;

use crate::object::{js_object_alloc, js_object_set_field_by_name};

pub(crate) fn allocate_port(registry: &mut DgramRegistry, address: &str) -> u16 {
    for _ in 0..16384 {
        let port = registry.next_port;
        registry.next_port = if registry.next_port == 65535 {
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

/// A `'message'` event's `rinfo`, `{ address, family, port, size }` in node's
/// order: born in one allocation on its declared final shape (`DGRAM_RINFO`),
/// each value stored by slot. A fresh, ordinary, mutable object per message,
/// as node's object literal is. `address` becomes one fresh string (node makes
/// a new one per message); the family is a static atom.
pub(crate) fn build_rinfo(address: &[u8], ipv6: bool, port: u16, size: usize) -> f64 {
    use crate::object::runtime_layouts::{dgram_family_atom, dgram_rinfo_slot as slot};
    let address = crate::string::js_string_from_bytes(address.as_ptr(), address.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let address = scope.root_nanbox_u64(JSValue::string_ptr(address).bits());
    unsafe {
        let object = crate::object::layout_desc::perry_alloc_layout(
            crate::object::runtime_layouts::DGRAM_RINFO::desc(),
        );
        let fields =
            crate::meta_slots_of!(crate::meta_obj_runtime_born!(object as *mut _)).cast::<u64>();
        let family = JSValue::string_ptr(dgram_family_atom(ipv6) as *mut _).bits();
        for (index, bits) in [
            (slot::ADDRESS, address.get_nanbox_u64()),
            (slot::FAMILY, family),
            (slot::PORT, (port as f64).to_bits()),
            (slot::SIZE, (size as f64).to_bits()),
        ] {
            crate::gc::runtime_store_object_jsvalue_slot(
                object as usize,
                fields.add(index) as usize,
                index,
                bits,
            );
        }
        boxed_pointer(object as *const u8)
    }
}

/// The text of `ip` as node prints it, in a stack buffer: IPv4 by hand, IPv6
/// through its `Display` into the same buffer. No heap allocation.
pub(crate) struct IpText {
    buf: [u8; 64],
    len: usize,
}

impl IpText {
    pub(crate) fn new(ip: &IpAddr) -> Self {
        let mut text = IpText {
            buf: [0; 64],
            len: 0,
        };
        match ip {
            IpAddr::V4(v4) => {
                for (i, octet) in v4.octets().into_iter().enumerate() {
                    if i > 0 {
                        text.push(b'.');
                    }
                    if octet >= 100 {
                        text.push(b'0' + octet / 100);
                    }
                    if octet >= 10 {
                        text.push(b'0' + octet / 10 % 10);
                    }
                    text.push(b'0' + octet % 10);
                }
            }
            IpAddr::V6(v6) => {
                use std::fmt::Write as _;
                let _ = write!(text, "{v6}");
            }
        }
        text
    }

    fn push(&mut self, byte: u8) {
        self.buf[self.len] = byte;
        self.len += 1;
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl std::fmt::Write for IpText {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let end = self.len + s.len();
        if end > self.buf.len() {
            return Err(std::fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
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
    crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::Buffer, data)
}

/// Deliver one received datagram to its socket as a `'message'` event. Called
/// on the main thread from [`crate::dgram_reactor::pump`]. The `Buffer` is
/// GC-rooted across the `rinfo` allocation so a collection between the two
/// can't reclaim it.
pub(crate) fn dgram_emit_message(socket_bits: u64, data: &[u8], src: SocketAddr) {
    let socket = f64::from_bits(socket_bits);
    let scope = crate::gc::RuntimeHandleScope::new();
    let buffer = scope.root_nanbox_f64(make_buffer(data));
    let address = IpText::new(&src.ip());
    let rinfo = scope.root_nanbox_f64(build_rinfo(
        address.as_bytes(),
        src.is_ipv6(),
        src.port(),
        data.len(),
    ));
    emit_event(
        socket,
        "message",
        &[buffer.get_nanbox_f64(), rinfo.get_nanbox_f64()],
    );
}

/// Extract the raw bytes to transmit from a `send()` message argument
/// (string → UTF-8, Buffer, or TypedArray/DataView).
pub(crate) fn message_bytes(value: f64) -> Option<Vec<u8>> {
    message_bytes_inner(value, true)
}

fn message_bytes_inner(value: f64, allow_list: bool) -> Option<Vec<u8>> {
    if let Some(text) = string_to_rust(value) {
        return Some(text.into_bytes());
    }
    let raw = raw_ptr_from_value(value);
    if raw >= 0x10000
        && (crate::buffer::is_registered_buffer(raw)
            || crate::typedarray_hook_slots::lookup_typed_array_kind(raw).is_some())
    {
        return crate::buffer::bytes::no_gc(|scope| {
            crate::buffer::bytes::bytes(crate::value::js_nanbox_pointer(raw as i64), scope)
                .ok()
                .map(<[u8]>::to_vec)
        });
    }
    if crate::array::js_array_is_array(value).to_bits() == crate::value::TAG_TRUE {
        // Node accepts one top-level buffer list, but each list element must
        // itself be a string or ArrayBuffer view. Besides matching that
        // contract, rejecting nested lists prevents cycles from recursing
        // until the native stack overflows.
        if !allow_list {
            return None;
        }
        let array = raw as *const crate::array::ArrayHeader;
        let mut bytes = Vec::new();
        for index in 0..crate::array::js_array_length(array) {
            bytes.extend(message_bytes_inner(
                crate::array::js_array_get_f64(array, index),
                false,
            )?);
        }
        return Some(bytes);
    }
    None
}

/// The Node error code of a socket syscall failure: libuv's name for the OS
/// errno (the one table `util.getSystemErrorMap()` serves).
pub(crate) fn io_error_code(err: &std::io::Error) -> &'static str {
    crate::io_error::io_error_code(err)
}

/// Build (not throw) a Node-style socket error value with `code`/`syscall`.
pub(crate) fn socket_error_value(message: &str, code: &'static str, syscall: &'static str) -> f64 {
    socket_core_error(message, code, syscall).value()
}

pub(crate) fn socket_core_error(
    message: &str,
    code: &'static str,
    syscall: &'static str,
) -> crate::error::CoreError {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    crate::node_submodules::register_error_syscall(msg, syscall);
    let err = crate::error::CoreError::error(msg);
    err
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
    let is_v6 = string_eq(
        get_hidden_value(socket, KEY_TYPE).unwrap_or_else(|| str_value("udp4")),
        b"udp6",
    );
    let socket_addr = match (address.as_str(), port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.find(|candidate| candidate.is_ipv6() == is_v6))
    {
        Some(address) => address,
        None => {
            return Err(bind_error_value(
                &format!("bind EADDRNOTAVAIL {address}:{port}"),
                "EADDRNOTAVAIL",
                &address,
                port,
            ));
        }
    };
    let udp = match open_udp_socket(
        socket_addr,
        is_truthy_hidden(socket, KEY_REUSE_ADDR),
        is_v6 && is_truthy_hidden(socket, KEY_IPV6_ONLY),
    ) {
        Ok(udp) => udp,
        Err(err) => {
            return Err(bind_error_value(
                &format!("bind {} {address}:{port}", io_error_code(&err)),
                io_error_code(&err),
                &address,
                port,
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

fn bind_error_value(message: &str, code: &'static str, address: &str, port: u16) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(socket_error_value(message, code, "bind"));
    let address = scope.root_nanbox_f64(str_value(address));
    crate::node_submodules::set_error_user_prop(
        crate::value::js_nanbox_get_pointer(error.get_nanbox_f64()) as usize,
        "address",
        address.get_nanbox_f64(),
    );
    crate::node_submodules::set_error_user_prop(
        crate::value::js_nanbox_get_pointer(error.get_nanbox_f64()) as usize,
        "port",
        port as f64,
    );
    error.get_nanbox_f64()
}

#[cfg(not(target_os = "wasi"))]
fn open_udp_socket(
    address: SocketAddr,
    reuse_addr: bool,
    ipv6_only: bool,
) -> std::io::Result<UdpSocket> {
    let domain = if address.is_ipv6() {
        Domain::IPV6
    } else {
        Domain::IPV4
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    if reuse_addr {
        configure_reuse_addr(&socket)?;
    }
    if address.is_ipv6() && ipv6_only {
        socket.set_only_v6(true)?;
    }
    socket.bind(&SockAddr::from(address))?;
    Ok(socket.into())
}

/// WASI: a wasi:sockets UDP socket through std (wasi-libc), bound and switched
/// to non-blocking, because the guest has no threads and its reactor polls
/// (`wasi_net/dgram.rs`). wasi:sockets binds IPv6 sockets with its own
/// `ipv6-only` default and std cannot set it before `bind`, so an explicit
/// `ipv6Only: true` is refused rather than silently ignored.
#[cfg(target_os = "wasi")]
fn open_udp_socket(
    address: SocketAddr,
    reuse_addr: bool,
    ipv6_only: bool,
) -> std::io::Result<UdpSocket> {
    if address.is_ipv6() && ipv6_only {
        return Err(std::io::ErrorKind::Unsupported.into());
    }
    if reuse_addr {
        configure_reuse_addr()?;
    }
    let udp = UdpSocket::bind(address)?;
    udp.set_nonblocking(true)?;
    Ok(udp)
}

/// WASI `reuseAddr`: wasi:sockets has no SO_REUSEADDR/SO_REUSEPORT, and a
/// guest cannot share a port with another socket. The option is accepted and
/// has no effect (docs/WASI_SUPPORT.md, `node:dgram`); this is the one
/// place that decision lives.
#[cfg(target_os = "wasi")]
fn configure_reuse_addr() -> std::io::Result<()> {
    Ok(())
}

// libuv's UV_UDP_REUSEADDR uses SO_REUSEPORT on Apple/BSD because those
// kernels give it the multicast port-sharing semantics Node expects. Linux
// uses SO_REUSEADDR; SO_REUSEPORT there load-balances instead.
#[cfg(not(target_os = "wasi"))]
fn configure_reuse_addr(socket: &Socket) -> std::io::Result<()> {
    #[cfg(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        socket.set_reuse_port(true)
    }
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    {
        socket.set_reuse_address(true)
    }
}

/// Real `send()`: transmit over the OS socket. Errors go to the callback when
/// one is supplied, otherwise to an `'error'` event (Node semantics).
pub(crate) fn real_send_bytes(
    socket: f64,
    args: &[f64],
    bytes: Vec<u8>,
    port: u16,
    address: String,
) -> f64 {
    if let Some(err) = ensure_bound_real(socket) {
        return finish_send(socket, args, Err(err));
    }
    let dest = match resolve_send_addr(&address, port) {
        Ok(dest) => dest,
        Err(err) => return finish_send(socket, args, Err(err)),
    };
    // P2: on the turnloop path the datagram is queued on the loop and the
    // callback fires from its completion. That is not an optimisation — the
    // descriptor the driver adopted is non-blocking, and the retained
    // duplicate shares that open file description, so a `send_to` here would
    // fail with `EWOULDBLOCK` the moment the socket buffer filled instead of
    // blocking as it used to (`dgram_reactor`'s module note).
    let mut bytes = bytes;
    if let Some(id) = reactor_id(socket) {
        let callback_bits = callback_from_args(args).map(f64::to_bits).unwrap_or(0);
        match crate::dgram_reactor::send_on_loop(id, bytes, dest, callback_bits) {
            Ok(()) => return undefined_value(),
            // Not on the loop (a `worker_threads` agent before P3/P4, or a
            // host where loop creation failed): fall through to the
            // synchronous send with the bytes handed back.
            Err(crate::dgram_reactor::SendRefusal::NotOnLoop(returned)) => bytes = returned,
            // The driver refused the submission and the bytes went with it.
            Err(crate::dgram_reactor::SendRefusal::Refused) => {
                return finish_send(
                    socket,
                    args,
                    Err(socket_error_value("send EBADF", "EBADF", "send")),
                )
            }
        }
    }
    let outcome = match live_udp(socket) {
        Some(udp) => match udp.send_to(&bytes, dest) {
            Ok(_) => Ok(bytes.len()),
            Err(err) => Err(socket_error_value(
                &format!("send {}", io_error_code(&err)),
                io_error_code(&err),
                "send",
            )),
        },
        None => Err(socket_error_value("send EBADF", "EBADF", "send")),
    };
    finish_send(socket, args, outcome)
}

/// Report the outcome of a datagram the driver carried.
///
/// The two reporting shapes are Node's and are unchanged from the synchronous
/// path: a supplied callback receives `(err, bytes)` and *suppresses* the
/// error event; without one, a failure becomes an `'error'` event on the
/// socket. Only the moment the outcome is known moved.
pub(crate) fn complete_send(
    socket: f64,
    callback_bits: u64,
    outcome: Result<usize, NodeError>,
    mut context: Option<crate::async_context::AsyncContextSnapshot>,
) {
    let result = try_complete_send(socket, callback_bits, outcome, &mut context);
    // A JS throw skips Rust destructors: release the native snapshot before rethrowing.
    drop(context);
    if let Err(error) = result {
        crate::exception::rethrow_caught(error);
    }
}

fn try_complete_send(
    socket: f64,
    callback_bits: u64,
    outcome: Result<usize, NodeError>,
    context: &mut Option<crate::async_context::AsyncContextSnapshot>,
) -> Result<(), crate::exception::Caught> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let callback = scope.root_nanbox_u64(callback_bits);
    // take_send removed the registry's roots; keep every store live through allocations/hooks.
    let context_roots = context
        .as_ref()
        .map(|snapshot| crate::async_context::root_snapshot(&scope, snapshot));
    let failed = outcome.is_err();
    let (error, bytes) = match outcome {
        Ok(size) => (null_value(), size as f64),
        Err(error) => (
            crate::exception::catch_thrown(|| node_error_value(error))?,
            undefined_value(),
        ),
    };
    let error = scope.root_nanbox_f64(error);
    if callback_bits == 0 {
        if failed {
            crate::exception::catch_thrown(|| {
                emit_event(socket.get_nanbox_f64(), "error", &[error.get_nanbox_f64()]);
                undefined_value()
            })?;
        }
        return Ok(());
    }
    if let Some(snapshot) = context.as_mut() {
        crate::async_context::refresh_snapshot_from_roots(
            snapshot,
            context_roots.as_ref().unwrap(),
        );
        let previous = crate::async_context::enter_context(snapshot);
        crate::async_context::push_context_guard(
            crate::async_context::ContextGuardAction::RestoreSnapshot(previous),
        );
    }
    let result = crate::exception::catch_thrown(|| {
        defer_send_callback(
            callback.get_nanbox_f64(),
            socket.get_nanbox_f64(),
            error.get_nanbox_f64(),
            bytes,
        );
        undefined_value()
    });
    if context.is_some() {
        if let Some(action) = crate::async_context::pop_context_guard() {
            crate::async_context::apply_context_guard(action);
        }
    }
    result.map(|_| ())
}

/// Surface a receive-side driver failure as Node does: an `'error'` event on
/// the socket.
pub(crate) fn emit_socket_error(socket: f64, error: NodeError) {
    emit_event(socket, "error", &[node_error_value(error)]);
}

fn node_error_value(error: NodeError) -> f64 {
    let syscall = if error.syscall.is_empty() {
        "send"
    } else {
        error.syscall
    };
    socket_error_value(&format!("{syscall} {}", error.code), error.code, syscall)
}

pub(crate) fn finish_send(socket: f64, args: &[f64], outcome: Result<usize, f64>) -> f64 {
    match (outcome, callback_from_args(args)) {
        (Ok(size), Some(callback)) => {
            defer_send_callback(callback, socket, null_value(), size as f64);
        }
        (Ok(_), None) => {}
        (Err(error), Some(callback)) => {
            defer_send_callback(callback, socket, error, undefined_value());
        }
        (Err(error), None) => {
            emit_event(socket, "error", &[error]);
        }
    }
    undefined_value()
}

extern "C" fn deferred_send_callback(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let callback = crate::closure::js_closure_get_capture_f64(closure, 0);
    let error = crate::closure::js_closure_get_capture_f64(closure, 2);
    let bytes = crate::closure::js_closure_get_capture_f64(closure, 3);
    call_function(callback, undefined_value(), &[error, bytes]);
    undefined_value()
}

fn defer_send_callback(callback: f64, socket: f64, error: f64, bytes: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(callback);
    let socket = scope.root_nanbox_f64(socket);
    let error = scope.root_nanbox_f64(error);
    let closure = crate::closure::js_closure_alloc(
        crate::fn_info!(deferred_send_callback, 0; with_declared(0)),
        4,
    );
    crate::closure::js_closure_set_capture_f64(closure, 0, callback.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(closure, 1, socket.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(closure, 2, error.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(closure, 3, bytes);
    crate::builtins::js_queue_microtask(closure as i64);
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

#[cfg(all(test, unix))]
mod errno_code_tests {
    use super::io_error_code;

    #[test]
    fn socket_errors_are_named_by_the_libuv_errno_table() {
        for (errno, name) in [
            (libc::EADDRINUSE, "EADDRINUSE"),
            (libc::EMSGSIZE, "EMSGSIZE"),
            (libc::ENETUNREACH, "ENETUNREACH"),
            (libc::EAFNOSUPPORT, "EAFNOSUPPORT"),
            (libc::EHOSTUNREACH, "EHOSTUNREACH"),
        ] {
            assert_eq!(
                io_error_code(&std::io::Error::from_raw_os_error(errno)),
                name
            );
        }
    }
}

/// rinfo is node's `{ address, family, port, size }` literal: keys in that
/// order, a fresh object per message, ordinary and writable afterwards.
#[cfg(test)]
mod rinfo_tests {
    use super::*;
    use crate::object::{
        js_object_delete_field, js_object_get_field_by_name_f64, js_object_get_prototype_of,
        js_object_keys, ObjectHeader,
    };

    fn obj(v: f64) -> *mut ObjectHeader {
        raw_ptr_from_value(v) as *mut ObjectHeader
    }

    fn keys(v: f64) -> Vec<String> {
        let arr = js_object_keys(obj(v));
        (0..crate::array::js_array_length(arr))
            .filter_map(|i| string_to_rust(crate::array::js_array_get_f64(arr, i)))
            .collect()
    }

    fn get(v: f64, k: &str) -> f64 {
        js_object_get_field_by_name_f64(obj(v), key(k))
    }

    #[test]
    fn rinfo_is_a_fresh_ordinary_object_in_node_key_order() {
        let _stable = crate::gc::GcSuppressScope::new();
        let a = build_rinfo(b"127.0.0.1", false, 4242, 32);
        let b = build_rinfo(b"127.0.0.1", false, 4242, 32);
        assert_ne!(a.to_bits(), b.to_bits(), "each message gets a new object");
        assert_eq!(keys(a), ["address", "family", "port", "size"]);
        assert_eq!(string_to_rust(get(a, "address")).unwrap(), "127.0.0.1");
        assert_eq!(string_to_rust(get(a, "family")).unwrap(), "IPv4");
        assert_eq!(get(a, "port"), 4242.0);
        assert_eq!(get(a, "size"), 32.0);
        let v6 = build_rinfo(b"::1", true, 7, 0);
        assert_eq!(keys(v6), ["address", "family", "port", "size"]);
        assert_eq!(string_to_rust(get(v6, "family")).unwrap(), "IPv6");
        assert_eq!(string_to_rust(get(v6, "address")).unwrap(), "::1");
        // [[Prototype]] is Object.prototype, as a literal's.
        let plain = boxed_pointer(js_object_alloc(0, 0) as *const u8);
        assert_eq!(
            js_object_get_prototype_of(a).to_bits(),
            js_object_get_prototype_of(plain).to_bits()
        );
        // Writable, extensible, deletable; the other message is untouched.
        js_object_set_field_by_name(obj(a), key("port"), 1.0);
        js_object_set_field_by_name(obj(a), key("extra"), 2.0);
        assert_eq!(get(a, "port"), 1.0);
        assert_eq!(get(b, "port"), 4242.0);
        assert_eq!(keys(a), ["address", "family", "port", "size", "extra"]);
        assert_eq!(js_object_delete_field(obj(a), key("address")), 1);
        assert_eq!(keys(a), ["family", "port", "size", "extra"]);
        assert_eq!(keys(b), ["address", "family", "port", "size"]);
    }

    #[test]
    fn ip_text_prints_as_node() {
        for (ip, text) in [
            ("0.0.0.0", "0.0.0.0"),
            ("255.10.1.100", "255.10.1.100"),
            ("10.0.99.9", "10.0.99.9"),
            ("::1", "::1"),
            ("::ffff:1.2.3.4", "::ffff:1.2.3.4"),
            ("fe80::1:2", "fe80::1:2"),
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert_eq!(IpText::new(&ip).as_bytes(), text.as_bytes());
            assert_eq!(IpText::new(&ip).as_bytes(), ip.to_string().as_bytes());
        }
    }
}
