//! The ordinary Socket transport shared with HTTP and attached protocols.
//!
//! Callers retain sockets, parsers and callbacks as traced JS edges. These
//! functions never reserve an id or translate one to a cell. Core projections
//! are short native borrows; no projection may survive JS, close or reopen.

use crate::{payload_io as io, payload_socket as socket, payload_transport as p};
use perry_ffi::native_payload::{OwnerLink, PayloadMiss};
use perry_ffi::turnloop_net::{self as tl, TransportCore};
use perry_ffi::{JsValue, TransientRootScope, TransientRootedNanbox};

pub const ROUTE: u8 = io::ROUTE;

/// A temporary rooted receiver for a protocol operation. Never put this in
/// a payload or a pending queue. Read `value()` again after allocation or JS.
pub struct RootedSocket {
    root: TransientRootedNanbox,
    incarnation: Option<tl::HandleSnapshot>,
    _scope: TransientRootScope,
}
impl RootedSocket {
    pub fn new(value: f64) -> Self {
        let scope = TransientRootScope::enter();
        let root = scope.root_nanbox(value);
        let incarnation = snapshot(root.get());
        Self {
            root,
            incarnation,
            _scope: scope,
        }
    }
    pub fn value(&self) -> f64 {
        self.root.get()
    }
    pub fn link(&self) -> Result<OwnerLink, PayloadMiss> {
        socket_link(self.value())
    }
    pub fn is_current(&self) -> bool {
        self.incarnation
            .as_ref()
            .is_none_or(|snapshot| matches(self.value(), snapshot))
    }
    /// # Safety
    /// End the projected borrow before JS, close or reopen.
    pub unsafe fn core(&self) -> Result<*mut TransportCore, PayloadMiss> {
        if self.is_current() {
            core(self.value())
        } else {
            Err(PayloadMiss::Closed)
        }
    }
}

pub fn new_socket(route: u8, options: f64) -> f64 {
    socket::new_socket(route, options)
}
pub fn new_server(options: f64, callback: f64) -> f64 {
    crate::payload_server::new_server(ROUTE, options, callback)
}
pub fn enabled() -> bool {
    io::enabled()
}
pub fn listen_tcp(
    owner: f64,
    host: &str,
    port: u16,
    backlog: u32,
    reuse_port: bool,
    no_delay: bool,
    tls_config: Option<std::sync::Arc<rustls::ServerConfig>>,
) -> Result<tl::Endpoint, tl::NetError> {
    crate::payload_server::listen_tcp(owner, host, port, backlog, reuse_port, no_delay, tls_config)
}
pub fn socket_link(owner: f64) -> Result<OwnerLink, PayloadMiss> {
    socket::transport_link(owner)
}
pub fn server_link(owner: f64) -> Result<OwnerLink, PayloadMiss> {
    p::server_link(owner)
}

/// # Safety
/// Root the owner before calling. End the projection before any operation
/// that can run JS, explicitly close the owner or attach a new incarnation.
pub unsafe fn core(owner: f64) -> Result<*mut TransportCore, PayloadMiss> {
    p::socket_core(socket::transport_link(owner)?)
}
/// # Safety
/// Same projection contract as `core`, for a listener.
pub unsafe fn listener_core(owner: f64) -> Result<*mut TransportCore, PayloadMiss> {
    p::server_core(p::server_link(owner)?)
}

/// The result is a JS value; root it before allocating or running JS.
pub fn state(owner: f64) -> f64 {
    socket::state(owner)
}
pub fn server_state(owner: f64) -> f64 {
    crate::payload_server::state(owner)
}

/// Snapshot children from every live listener incarnation's ordinary edges.
/// Closed listeners retain their children until the final child's close.
pub fn for_each_server_child(owner: f64, mut visit: impl FnMut(&RootedSocket)) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(server_state(owner.get()));
    let active = scope.root_nanbox(own_get(state.get(), "activeGroup"));
    let closing = scope.root_nanbox(own_get(state.get(), "closeCallbacks"));
    let mut groups = vec![active];
    let values = |array: f64| -> Vec<f64> {
        let value = perry_ffi::JsValue::from_bits(array.to_bits());
        if !value.is_pointer() {
            return Vec::new();
        }
        let array = value.as_pointer::<perry_ffi::ArrayHeader>();
        (0..unsafe { perry_ffi::js_array_length(array) })
            .map(|index| f64::from_bits(unsafe { perry_ffi::js_array_get(array, index).bits() }))
            .collect()
    };
    #[cfg(test)]
    let include_closing =
        std::env::var("PERRY_NET_A_CHILDREN_SABOTAGE").as_deref() != Ok("old_group");
    #[cfg(not(test))]
    let include_closing = true;
    if include_closing {
        groups.extend(
            values(closing.get())
                .into_iter()
                .map(|group| scope.root_nanbox(group)),
        );
    }
    let mut children = Vec::new();
    for group in groups {
        if !perry_ffi::JsValue::from_bits(group.get().to_bits()).is_pointer() {
            continue;
        }
        let array = scope.root_nanbox(own_get(group.get(), "children"));
        children.extend(
            values(array.get())
                .into_iter()
                .map(|child| scope.root_nanbox(child)),
        );
    }
    for child in children {
        visit(&RootedSocket::new(child.get()));
    }
}
pub fn own_get(object: f64, key: &str) -> f64 {
    p::own_get(object, key)
}
pub fn own_set(object: f64, key: &str, value: f64) {
    p::own_set(object, key, value)
}

/// Install the parser's ordinary edge and its memory-only explicit release.
///
/// # Safety
/// `release` must be static code which neither runs JS nor reaches the
/// driver. The parser's Rust payload must contain no JS values or GC pointers.
pub unsafe fn set_codec(owner: f64, key: &str, codec: f64, release: unsafe fn(f64)) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let codec = scope.root_nanbox(codec);
    let link = socket::link(owner.get());
    let state = scope.root_nanbox(socket::state(owner.get()));
    p::own_set(state.get(), key, codec.get());
    let physical =
        scope.root_nanbox(unsafe { perry_ffi::native_payload::link_event_owner(link) }.unwrap());
    let physical_state = scope.root_nanbox(socket::state(physical.get()));
    p::own_set(physical_state.get(), "codecOwner", owner.get());
    (*p::socket_ptr(link).unwrap_or_else(crate::payload_prototype::throw_miss))
        .ext
        .close_codec = Some(release);
}

pub(crate) fn release_codec(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let release = unsafe {
        socket::transport_link(owner.get()).ok().and_then(|link| {
            p::socket_ptr(link)
                .ok()
                .and_then(|payload| (*payload).ext.close_codec.take())
        })
    };
    if let Some(release) = release {
        let link = socket::link(owner.get());
        let physical = scope
            .root_nanbox(unsafe { perry_ffi::native_payload::link_event_owner(link) }.unwrap());
        let physical_state = scope.root_nanbox(socket::state(physical.get()));
        let codec_owner = scope.root_nanbox(p::own_get(physical_state.get(), "codecOwner"));
        p::own_set(physical_state.get(), "codecOwner", p::undefined());
        unsafe { release(codec_owner.get()) };
    }
}

pub fn set_route(owner: f64, route: u8) -> Result<(), tl::NetError> {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    unsafe { p::set_route(socket::link(owner.get()), route) }
}
pub fn emit(owner: f64, event: &str, args: &[f64]) -> f64 {
    crate::payload_events::emit(owner, event, args)
}
pub fn set_ref(owner: f64, referenced: bool) {
    socket::set_ref(owner, referenced);
}
pub fn has_ref(owner: f64) -> bool {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    if socket_link(owner.get()).is_err() {
        return false;
    }
    let state = scope.root_nanbox(state(owner.get()));
    JsValue::from_bits(own_get(state.get(), "refed").to_bits()).to_bool()
}
pub fn destroy(owner: f64) {
    socket::destroy(owner, p::undefined());
}
pub fn destroy_error(owner: f64, code: &str, message: &str) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let error = scope.root_nanbox(socket::error(code, message));
    socket::destroy(owner.get(), error.get());
}
pub fn eof(owner: f64) {
    io::eof(owner);
}
pub fn flow(owner: f64) {
    socket::flow(owner);
}
pub fn adopted(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    unsafe {
        (*p::socket_ptr(link).unwrap_or_else(crate::payload_prototype::throw_miss))
            .ext
            .opened = true;
    }
    socket::update_addresses(link);
}
pub fn received(owner: f64, len: usize) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    unsafe {
        if let Ok(payload) = p::socket_ptr(link) {
            (*payload).ext.bytes_read = (*payload).ext.bytes_read.saturating_add(len as u64);
        }
    }
    socket::refresh_timeout(link);
}
pub fn endpoint(owner: f64, peer: bool) -> Option<tl::Endpoint> {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    unsafe {
        let core = p::socket_core(link).ok()?;
        if peer {
            tl::link_peer_address(&mut *core, link)
        } else {
            tl::link_local_address(&mut *core, link)
        }
    }
}
pub fn deadline_arm(owner: f64, ms: u64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    unsafe {
        if let Ok(core) = p::socket_core(link) {
            let _ = tl::link_deadline_arm(&mut *core, link, ms);
        }
    }
}
pub fn deadline_park(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    unsafe {
        if let Ok(core) = p::socket_core(link) {
            let _ = tl::link_deadline_park(&mut *core, link);
        }
    }
}
pub fn deadline_cancel(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    unsafe {
        if let Ok(core) = p::socket_core(link) {
            let _ = tl::link_deadline_cancel(&mut *core, link);
        }
    }
}
pub fn queued_bytes(owner: f64) -> usize {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let link = socket::link(owner.get());
    unsafe {
        p::socket_core(link)
            .ok()
            .map_or(0, |core| tl::link_queued_bytes(&mut *core, link))
    }
}
pub fn snapshot(owner: f64) -> Option<tl::HandleSnapshot> {
    socket::transport_link(owner).ok().and_then(io::snapshot)
}
pub fn matches(owner: f64, snapshot: &tl::HandleSnapshot) -> bool {
    socket::transport_link(owner)
        .ok()
        .is_some_and(|link| io::matches(link, snapshot))
}
pub fn call(callback: f64, this: perry_ffi::JsThis, args: &[f64]) -> f64 {
    crate::payload_events::call(callback, this, args)
}
pub fn close_server(owner: f64) {
    crate::payload_server::close(owner, p::undefined());
}
pub fn connect(owner: f64, a: f64, b: f64, c: f64) -> f64 {
    socket::connect(owner, a, b, c)
}
pub fn get(owner: f64, key: &str) -> f64 {
    socket::get(owner, key)
}

/// Protocol bytes use the Socket's TLS session and native write accounting.
/// They do not own JS callbacks; the protocol payload retains its own bytes.
pub fn write(owner: f64, bytes: &[u8], user: u64) -> Result<usize, tl::NetError> {
    crate::payload_tls::write(owner, bytes, user)
}
pub fn shutdown(owner: f64, user: u64) -> Result<(), tl::NetError> {
    crate::payload_tls::shutdown(owner, user)
}
pub fn install_server_tls(
    owner: f64,
    config: std::sync::Arc<rustls::ServerConfig>,
) -> Result<(), String> {
    crate::payload_tls::install_server(owner, config)
}

/// Owned certificate facts for a prebuilt client configuration. Callback
/// identity and promises belong to the Socket's ordinary JS state.
#[derive(Clone)]
pub struct ClientTlsMetadata {
    pub servername: String,
    pub verify: bool,
    pub ca: Option<Vec<Vec<u8>>>,
    pub certificate_pem: Vec<u8>,
}

pub fn install_client_tls(
    owner: f64,
    config: std::sync::Arc<rustls::ClientConfig>,
    server_name: rustls::pki_types::ServerName<'static>,
    metadata: ClientTlsMetadata,
) -> Result<(), String> {
    crate::payload_tls::install_client_config(
        owner,
        config,
        server_name,
        metadata.servername,
        metadata.verify,
        crate::TlsClientConfigData::for_cached_config(metadata.ca, metadata.certificate_pem),
    )
}
pub fn tls_installed(owner: f64) -> bool {
    socket::transport_link(owner).is_ok_and(crate::payload_tls::installed)
}
pub fn receive_tls(owner: f64, bytes: &[u8]) {
    crate::payload_tls::receive(owner, bytes);
}

/// Common transport completions still belong to the Socket when its parser
/// owns the route. Data/EOF/deadlines remain for the protocol sink to decode.
/// The completion's owed ref roots its cell; this function roots the owner.
///
/// # Safety
/// `event` must be the live completion supplied by the runtime to this sink.
pub unsafe fn dispatch_common(event: &tl::NetCompletion) -> bool {
    let Some(link) = event.link() else {
        return false;
    };
    let Some(owner) = perry_ffi::native_payload::link_event_owner(link) else {
        return true;
    };
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    if socket::transport_link(owner.get()).is_err() {
        return true;
    }
    match event.kind {
        tl::NET_CONNECT => io::connected(owner.get()),
        tl::NET_WROTE => {
            if crate::payload_tls::installed(link) {
                crate::payload_tls::wrote(owner.get(), event.len);
            } else {
                io::wrote(owner.get(), event.user, event.len, event.queued);
            }
        }
        tl::NET_SHUTDOWN => io::shutdown(owner.get(), event.user),
        tl::NET_CLOSED => io::socket_closed(owner.get(), event),
        _ => return false,
    }
    true
}

/// Install an ordinary emitter listener; callbacks remain solely in JS fields.
pub fn once(owner: f64, event: &str, callback: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let event = scope.root_nanbox(crate::payload_events::string(event));
    extern "C" {
        fn js_node_stream_method_once(owner: i64, event: f64, callback: f64) -> f64;
    }
    unsafe {
        js_node_stream_method_once(
            crate::payload_transport::raw_owner(owner.get()),
            event.get(),
            callback.get(),
        );
    }
}

/// Remove an ordinary emitter listener while keeping all operands rooted.
pub fn remove_listener(owner: f64, event: &str, callback: f64) {
    if !socket::is_callback(callback) {
        return;
    }
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let event = scope.root_nanbox(crate::payload_events::string(event));
    extern "C" {
        fn js_node_stream_method_remove_listener(owner: i64, event: f64, callback: f64) -> f64;
    }
    unsafe {
        js_node_stream_method_remove_listener(
            crate::payload_transport::raw_owner(owner.get()),
            event.get(),
            callback.get(),
        );
    }
}
