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
    /// Execute a callback-free operation against the proven incarnation.
    /// # Safety
    /// f must not allocate JS, invoke JS, close, reopen or retain its arguments.
    pub unsafe fn with_current<R>(
        &self,
        f: impl FnOnce(&mut TransportCore, f64) -> R,
    ) -> Option<R> {
        let (own, transport) = self.project_current()?;
        Some(f(&mut (*transport.payload).core, own.state))
    }
    /// Root and prove an incoming completion once, then decode plaintext using
    /// that same socket proof for TLS selection, accounting and the deadline.
    /// `None` inside the result asks the caller to consume ciphertext after
    /// this native borrow ends. The returned receiver stays rooted for effects.
    /// # Safety
    /// f must not allocate JS, invoke JS, close, reopen or retain state.
    pub unsafe fn receive<R>(
        value: f64,
        plaintext: bool,
        len: usize,
        park_idle: bool,
        f: impl FnOnce(&RootedSocket, f64) -> R,
    ) -> Option<(Self, Option<R>)> {
        let scope = TransientRootScope::enter();
        let root = scope.root_nanbox(value);
        let (own, transport) = Self::project_windows(root.get())?;
        let incarnation = tl::link_snapshot_handle(&mut (*transport.payload).core, transport.link);
        let socket = Self {
            root,
            incarnation,
            _scope: scope,
        };
        let result = if !plaintext && (*transport.payload).ext.tls.is_some() {
            None
        } else {
            let payload = &mut *own.payload;
            payload.ext.bytes_read = payload.ext.bytes_read.saturating_add(len as u64);
            socket::refresh_timeout_proven(payload, own.link);
            if park_idle {
                let _ = tl::link_deadline_park(&mut (*transport.payload).core, transport.link);
            }
            Some(f(&socket, own.state))
        };
        Some((socket, result))
    }
    unsafe fn project_current(
        &self,
    ) -> Option<(
        perry_ffi::native_payload::PayloadWindow<p::SocketPayload>,
        perry_ffi::native_payload::PayloadWindow<p::SocketPayload>,
    )> {
        let (own, transport) = Self::project_windows(self.value())?;
        if self.incarnation.as_ref().is_some_and(|snapshot| {
            !tl::link_handle_matches(&mut (*transport.payload).core, transport.link, snapshot)
        }) {
            return None;
        }
        Some((own, transport))
    }
    unsafe fn project_windows(
        value: f64,
    ) -> Option<(
        perry_ffi::native_payload::PayloadWindow<p::SocketPayload>,
        perry_ffi::native_payload::PayloadWindow<p::SocketPayload>,
    )> {
        let own = perry_ffi::native_payload::project::<p::SocketPayload>(value, &p::SOCKET).ok()?;
        let transport = if (*own.payload).ext.tls_parent {
            let parent = p::record_get(own.state, "tlsParent");
            perry_ffi::native_payload::project::<p::SocketPayload>(parent, &p::SOCKET).ok()?
        } else {
            perry_ffi::native_payload::PayloadWindow {
                payload: own.payload,
                link: own.link,
                state: own.state,
            }
        };
        Some((own, transport))
    }
    /// Execute consecutive native effects with one incarnation proof.
    /// # Safety
    /// f must not allocate JS, invoke JS, release/reopen or retain the view.
    pub unsafe fn with_native_io<R>(&self, f: impl FnOnce(&mut NativeIo<'_>) -> R) -> Option<R> {
        let (own, transport) = self.project_current()?;
        let mut io = NativeIo {
            payload: &mut *transport.payload,
            link: transport.link,
            state: own.state,
        };
        let result = f(&mut io);
        p::account_socket_proven(io.payload, io.link);
        Some(result)
    }
    /// # Safety
    /// End the projected borrow before JS, close or reopen.
    pub unsafe fn core(&self) -> Result<*mut TransportCore, PayloadMiss> {
        self.with_current(|core, _| core as *mut _)
            .ok_or(PayloadMiss::Closed)
    }
}

/// A callback-free view of the physical transport and its logical JS state.
/// It is borrowed for one operation batch, never stored or used across JS.
pub struct NativeIo<'a> {
    payload: &'a mut p::SocketPayload,
    link: OwnerLink,
    state: f64,
}
impl NativeIo<'_> {
    pub fn can_write_without_js(&self) -> bool {
        self.payload.ext.tls.is_none()
    }
    pub fn write(&mut self, bytes: &[u8], user: u64) -> Result<usize, tl::NetError> {
        assert!(self.can_write_without_js());
        // This is the same submission routine used by Socket.write and TLS.
        unsafe {
            crate::payload_tls::write_proven(
                p::undefined(),
                perry_ffi::native_payload::PayloadWindow {
                    payload: self.payload,
                    link: self.link,
                    state: self.state,
                },
                bytes,
                user,
            )
        }
    }
    pub fn deadline(&mut self, ms: Option<u64>) {
        if let Some(ms) = ms {
            let _ = tl::link_deadline_arm(&mut self.payload.core, self.link, ms);
        } else {
            let _ = tl::link_deadline_cancel(&mut self.payload.core, self.link);
        }
    }
    pub fn park_deadline(&mut self) {
        let _ = tl::link_deadline_park(&mut self.payload.core, self.link);
    }
    /// The binding initializes refed as an ordinary data edge at birth.
    /// Refuse an incomplete record instead of allocating in this window.
    pub fn set_ref(&mut self, referenced: bool) -> bool {
        let value = JsValue::from_bool(referenced);
        if unsafe {
            perry_ffi::object_record_set(JsValue::from_bits(self.state.to_bits()), "refed", value)
        } {
            tl::link_set_ref(&mut self.payload.core, self.link, referenced);
            true
        } else {
            false
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
    let window = transport_window(owner)?;
    Ok(std::ptr::addr_of_mut!((*window.payload).core))
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
/// Read an opaque parser/transport state record without invoking JS.
pub fn record_get(object: f64, key: &str) -> f64 {
    p::record_get(object, key)
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
    // A pump probe is a callback-free state read, including after explicit
    // release while a terminal completion is still owed. Do not allocate a
    // key string, create state or open field scopes on every poll turn.
    let state = perry_ffi::native_payload::js_state(owner, &p::SOCKET, false);
    JsValue::from_bits(p::record_get(state, "refed").to_bits()).to_bool()
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
/// Callback-free physical transport facts; wrappers keep their own JS state.
unsafe fn transport_window(
    owner: f64,
) -> Result<perry_ffi::native_payload::PayloadWindow<p::SocketPayload>, PayloadMiss> {
    let own = perry_ffi::native_payload::project::<p::SocketPayload>(owner, &p::SOCKET)?;
    if (*own.payload).ext.tls_parent {
        perry_ffi::native_payload::project(p::record_get(own.state, "tlsParent"), &p::SOCKET)
    } else {
        Ok(own)
    }
}

pub fn snapshot(owner: f64) -> Option<tl::HandleSnapshot> {
    unsafe {
        let window = transport_window(owner).ok()?;
        tl::link_snapshot_handle(&mut (*window.payload).core, window.link)
    }
}
pub fn matches(owner: f64, snapshot: &tl::HandleSnapshot) -> bool {
    unsafe {
        transport_window(owner).ok().is_some_and(|window| {
            tl::link_handle_matches(&mut (*window.payload).core, window.link, snapshot)
        })
    }
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
