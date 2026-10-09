//! Socket and listener ownership for link-routed transports.
//!
//! Rust owns bytes, codec state and driver capabilities. Ordinary JS state
//! owns callbacks, the server edge, parser/codec objects and terminal caches.
//! No object address or JS value is retained in these Rust fields.

use perry_ffi::native_payload::{self as np, OwnerLink, PayloadFamily, PayloadMiss};
use perry_ffi::turnloop_net::{self as tl, TransportPayload};
use perry_ffi::{JsValue, ObjectHeader, TransientRootScope};
use std::collections::VecDeque;
use std::net::SocketAddr;

pub(crate) const SOCKET_CLASS_ID: u32 = perry_ffi::native_class_ids::NET_SOCKET;
pub(crate) const SERVER_CLASS_ID: u32 = perry_ffi::native_class_ids::NET_SERVER;

static SOCKET_VTABLE: perry_ffi::native_stream::PayloadVTable =
    perry_ffi::native_stream::payload_vtable::<SocketPayload>(None);
pub(crate) static SOCKET: PayloadFamily =
    PayloadFamily::new::<SocketPayload>(SOCKET_CLASS_ID, "Socket", true, &SOCKET_VTABLE)
        .with_constructor_length(1)
        .with_installer(super::payload_prototype::install_socket);
static SERVER_VTABLE: perry_ffi::native_stream::PayloadVTable =
    perry_ffi::native_stream::payload_vtable::<ServerPayload>(None);
pub(crate) static SERVER: PayloadFamily =
    PayloadFamily::new::<ServerPayload>(SERVER_CLASS_ID, "Server", true, &SERVER_VTABLE)
        .with_constructor_length(2)
        .with_installer(super::payload_prototype::install_server);

pub(crate) type SocketPayload = TransportPayload<SocketFields>;
pub(crate) type ServerPayload = TransportPayload<ServerFields>;

#[derive(Default)]
pub(crate) struct SocketFields {
    /// A TLSSocket wrapper owns its JS state; its parent Socket owns the
    /// physical transport. The parent itself is an ordinary JS state edge.
    pub tls_parent: bool,
    pub local: Option<SocketAddr>,
    pub peer: Option<SocketAddr>,
    pub host: String,
    pub port: u16,
    pub encoding: Option<String>,
    pub timeout_ms: u64,
    pub tcp_async_id: u64,
    pub connect_async_id: u64,
    pub shutdown_async_id: u64,
    pub connecting: bool,
    pub opened: bool,
    pub read_ended: bool,
    pub read_end_emitted: bool,
    pub write_ended: bool,
    pub shutdown_done: bool,
    pub allow_half_open: bool,
    pub read_started: bool,
    pub paused: bool,
    pub flowing: Option<bool>,
    pub need_drain: bool,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub queued: usize,
    pub callback_seq: u64,
    pub cork_depth: u32,
    pub cork_bytes: Vec<u8>,
    pub cork_users: Vec<u64>,
    pub coalesced_users: VecDeque<(u64, Vec<u64>)>,
    pub extra_end_users: Vec<u64>,
    pub read_buffer: VecDeque<u8>,
    pub raw_consumer: bool,
    pub raw_error: Option<String>,
    pub tls: Option<Box<TlsLayer>>,
    pub direct_tls: Option<(String, bool, crate::TlsClientConfigData)>,
    pub held_tls_writes: Vec<(Vec<u8>, u64)>,
    pub held_tls_end: Option<u64>,
    pub type_of_service: u8,
    pub bun: bool,
    /// Explicit-close cleanup for a separately owned protocol payload.
    /// This is a static code pointer, never JS or a heap address. Its target
    /// may release the parser/codec payload but may not run JS or the driver.
    /// Drop of SocketFields does not invoke it: GC traces the ordinary edge.
    pub close_codec: Option<unsafe fn(f64)>,
}

/// The sans-I/O session and its byte accounting. The upgrade promise and
/// checkServerIdentity callback are fields of the owner's JS state.
pub(crate) struct TlsLayer {
    pub session: crate::turnloop_tls::TlsSession,
    pub servername: String,
    pub verify: bool,
    pub config: crate::TlsClientConfigData,
    pub cipher_written: u64,
    pub cipher_acked: u64,
    pub pending: VecDeque<TlsWrite>,
    pub pending_shutdown: Option<u64>,
    pub secure_emitted: bool,
    pub closing: bool,
    pub server: bool,
}
pub(crate) struct TlsWrite {
    pub user: u64,
    pub plain_len: usize,
    pub mark: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ServerFields {
    pub local: Option<SocketAddr>,
    pub path: Option<String>,
    pub async_id: u64,
    pub allow_half_open: bool,
    pub pause_on_connect: bool,
    pub listening: bool,
    /// The HTTPS delegate owns a sans-I/O configuration, never a JS value.
    pub tls_config: Option<std::sync::Arc<rustls::ServerConfig>>,
}

impl SocketFields {
    fn retained_bytes(&self) -> usize {
        use std::mem::size_of;
        self.host.capacity()
            + self.encoding.as_ref().map_or(0, String::capacity)
            + self.cork_bytes.capacity()
            + self.cork_users.capacity() * size_of::<u64>()
            + self.coalesced_users.capacity() * size_of::<(u64, Vec<u64>)>()
            + self
                .coalesced_users
                .iter()
                .map(|(_, users)| users.capacity() * size_of::<u64>())
                .sum::<usize>()
            + self.extra_end_users.capacity() * size_of::<u64>()
            + self.read_buffer.capacity()
            + self.raw_error.as_ref().map_or(0, String::capacity)
            + self.direct_tls.as_ref().map_or(0, |(host, _, config)| {
                host.capacity() + config.retained_bytes()
            })
            + self.held_tls_writes.capacity() * size_of::<(Vec<u8>, u64)>()
            + self
                .held_tls_writes
                .iter()
                .map(|(bytes, _)| bytes.capacity())
                .sum::<usize>()
            + self.tls.as_ref().map_or(0, |tls| {
                size_of::<TlsLayer>()
                    + tls.session.retained_bytes()
                    + tls.servername.capacity()
                    + tls.config.retained_bytes()
                    + tls.pending.capacity() * size_of::<TlsWrite>()
            })
    }
}

/// Temporary binding cleanup, never stored in a payload. Its caller roots
/// the owner until this guard drops. Native event callbacks catch JS throws
/// before returning here; release/reopen is checked again at cleanup.
pub(crate) struct AccountSocket(pub OwnerLink);
pub(crate) struct AccountServer(pub OwnerLink);
impl Drop for AccountServer {
    fn drop(&mut self) {
        unsafe {
            account_server(self.0);
        }
    }
}
impl Drop for AccountSocket {
    fn drop(&mut self) {
        unsafe {
            account_socket(self.0);
        }
    }
}

pub(crate) unsafe fn account_socket(link: OwnerLink) {
    let Ok(payload) = socket_ptr(link) else {
        return;
    };
    let fields = (*payload).ext.retained_bytes();
    let core = std::ptr::addr_of_mut!((*payload).core);
    let bytes =
        std::mem::size_of::<SocketPayload>() + fields + tl::link_retained_bytes(&mut *core, link);
    np::link_set_external_bytes(link, &SOCKET, bytes);
}
pub(crate) unsafe fn account_server(link: OwnerLink) {
    let Ok(payload) = server_ptr(link) else {
        return;
    };
    let fields = (*payload).ext.path.as_ref().map_or(0, String::capacity);
    let core = std::ptr::addr_of_mut!((*payload).core);
    let bytes =
        std::mem::size_of::<ServerPayload>() + fields + tl::link_retained_bytes(&mut *core, link);
    np::link_set_external_bytes(link, &SERVER, bytes);
}

extern "C" {
    fn js_event_emitter_subclass_init(owner: f64, options: f64) -> f64;
    fn js_object_set_field_by_name(
        object: *mut ObjectHeader,
        key: *const perry_ffi::StringHeader,
        value: f64,
    );
}

pub(crate) fn undefined() -> f64 {
    f64::from_bits(JsValue::UNDEFINED.bits())
}
pub(crate) fn boxed_addr(addr: i64) -> f64 {
    f64::from_bits(JsValue::from_object_ptr(addr as *mut u8).bits())
}
pub(crate) fn raw_owner(owner: f64) -> i64 {
    JsValue::from_bits(owner.to_bits()).as_pointer::<u8>() as i64
}

/// Allocate before any driver submission, so every token names this cell
/// from the first connect, resolve, accept-install or read onward.
pub(crate) fn alloc_socket(route: u8, fields: SocketFields) -> f64 {
    super::payload_io::register();
    let scope = TransientRootScope::enter();
    let bytes = std::mem::size_of::<SocketPayload>() + fields.retained_bytes();
    let owner = scope.root_nanbox(unsafe {
        np::alloc_in(
            &SOCKET,
            "net",
            SocketPayload::new(route, fields),
            bytes,
            &[],
        )
    });
    unsafe {
        js_event_emitter_subclass_init(owner.get(), undefined());
    }
    owner.get()
}
pub(crate) fn alloc_server(route: u8, fields: ServerFields) -> f64 {
    super::payload_io::register();
    let scope = TransientRootScope::enter();
    let bytes =
        std::mem::size_of::<ServerPayload>() + fields.path.as_ref().map_or(0, String::capacity);
    let owner = scope.root_nanbox(unsafe {
        np::alloc_in(
            &SERVER,
            "net",
            ServerPayload::new(route, fields),
            bytes,
            &[],
        )
    });
    unsafe {
        js_event_emitter_subclass_init(owner.get(), undefined());
    }
    owner.get()
}

/// Back a source subclass's actual receiver. No temporary native owner is
/// allocated or aliased, and attach preserves its class and prototype.
pub(crate) fn attach_socket(owner: f64, route: u8, fields: SocketFields) -> bool {
    super::payload_io::register();
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let bytes = std::mem::size_of::<SocketPayload>() + fields.retained_bytes();
    if !unsafe {
        np::attach_to_object(
            owner.get(),
            &SOCKET,
            SocketPayload::new(route, fields),
            bytes,
        )
    } {
        return false;
    }
    unsafe {
        js_event_emitter_subclass_init(owner.get(), undefined());
    }
    true
}
pub(crate) fn attach_server(owner: f64, route: u8, fields: ServerFields) -> bool {
    super::payload_io::register();
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let bytes =
        std::mem::size_of::<ServerPayload>() + fields.path.as_ref().map_or(0, String::capacity);
    if !unsafe {
        np::attach_to_object(
            owner.get(),
            &SERVER,
            ServerPayload::new(route, fields),
            bytes,
        )
    } {
        return false;
    }
    unsafe {
        js_event_emitter_subclass_init(owner.get(), undefined());
    }
    true
}

pub(crate) fn socket_link(owner: f64) -> Result<OwnerLink, PayloadMiss> {
    np::receiver_link(owner, &SOCKET)
}
pub(crate) fn server_link(owner: f64) -> Result<OwnerLink, PayloadMiss> {
    np::receiver_link(owner, &SERVER)
}

/// # Safety
/// A rooted owner or an outstanding driver ref keeps this checked cell alive.
/// End each projected borrow before JS, release or reopen.
pub(crate) unsafe fn socket_ptr(link: OwnerLink) -> Result<*mut SocketPayload, PayloadMiss> {
    np::link_payload_ptr(link, &SOCKET)
}
pub(crate) unsafe fn server_ptr(link: OwnerLink) -> Result<*mut ServerPayload, PayloadMiss> {
    np::link_payload_ptr(link, &SERVER)
}

pub(crate) fn own_get(owner: f64, key: &str) -> f64 {
    f64::from_bits(perry_ffi::object_field_by_name(JsValue::from_bits(owner.to_bits()), key).bits())
}
pub(crate) fn own_set(owner: f64, key: &str, value: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let value = scope.root_nanbox(value);
    let key = scope.root_addr(perry_ffi::alloc_string(key).as_raw() as i64);
    unsafe {
        js_object_set_field_by_name(
            JsValue::from_bits(owner.get().to_bits()).as_pointer(),
            key.get() as *const perry_ffi::StringHeader,
            value.get(),
        );
    }
}

/// JS state follows the owner through moving collections and survives release.
/// Callers root the result before any later allocation.
pub(crate) unsafe fn socket_state(link: OwnerLink, create: bool) -> f64 {
    np::link_js_state(link, &SOCKET, create)
}
pub(crate) unsafe fn server_state(link: OwnerLink, create: bool) -> f64 {
    np::link_js_state(link, &SERVER, create)
}

/// Disjoint projection of the opaque core. No borrow of SocketFields remains
/// alive while the driver runs or a completion reaches a binding sink.
pub(crate) unsafe fn socket_core(link: OwnerLink) -> Result<*mut tl::TransportCore, PayloadMiss> {
    Ok(std::ptr::addr_of_mut!((*socket_ptr(link)?).core))
}
pub(crate) unsafe fn server_core(link: OwnerLink) -> Result<*mut tl::TransportCore, PayloadMiss> {
    Ok(std::ptr::addr_of_mut!((*server_ptr(link)?).core))
}

pub(crate) unsafe fn reopen_socket(link: OwnerLink, route: u8) -> Result<(), np::AttachMiss> {
    np::attach_link(
        link,
        &SOCKET,
        SocketPayload::new(route, SocketFields::default()),
        std::mem::size_of::<SocketPayload>(),
    )
}
pub(crate) unsafe fn reopen_server(
    link: OwnerLink,
    route: u8,
    fields: ServerFields,
) -> Result<(), np::AttachMiss> {
    let bytes =
        std::mem::size_of::<ServerPayload>() + fields.path.as_ref().map_or(0, String::capacity);
    np::attach_link(link, &SERVER, ServerPayload::new(route, fields), bytes)
}

/// Route changes keep the same handle, multishot read and cell. The caller
/// owns the ordinary parser/codec edge and releases it after this store.
pub(crate) unsafe fn set_route(link: OwnerLink, route: u8) -> Result<(), tl::NetError> {
    let core = socket_core(link).map_err(|_| tl::NetError {
        code: "EBADF".into(),
        syscall: "route".into(),
        errno: -9,
        no_loop: false,
    })?;
    tl::link_set_route(&mut *core, link, route)
}

// Dropping any field above is memory-only. Driver close belongs to explicit
// methods; TransportCore::drop never dereferences a driver or a thread-local.
const _: () = assert!(std::mem::offset_of!(SocketPayload, core) == 0);
const _: () = assert!(std::mem::offset_of!(ServerPayload, core) == 0);
