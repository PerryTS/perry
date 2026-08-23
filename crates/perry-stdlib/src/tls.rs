//! `node:tls` helper/catalog surface backed by rustls.
//!
//! Client socket transport still lives in `net`; this module covers the
//! module-level helpers, SecureContext shape, TLS server acceptor, and the
//! TLSSocket introspection surface layered over rustls.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::task::{Context, Poll};

use crate::common::string_from_header;
use perry_runtime::{
    js_array_alloc, js_array_is_array, js_array_push, js_closure_call0, js_closure_call1,
    js_closure_call2, js_get_string_pointer_unified, js_nanbox_pointer, js_object_alloc,
    js_object_get_field_by_name, js_object_set_field_by_name, js_string_from_bytes, ClosureHeader,
    JSValue, ObjectHeader, StringHeader,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::{rustls, server::TlsStream as ServerTlsStream, TlsAcceptor};

const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
const TLS_DISPATCH_MISSING_BITS: u64 = TAG_UNDEFINED_BITS;

mod client_verifier;
mod dispatch;
mod module_api;
mod socket_api;
// Re-export the handle-dispatch and module-level entry points so
// `crate::tls::…` (and the `pub use tls::*` glob in `lib.rs`) keep resolving
// them exactly as before the split.
pub use dispatch::{dispatch_tls_handle, dispatch_tls_property, should_dispatch_tls_handle};
pub use module_api::{
    js_tls_check_server_identity, js_tls_convert_alpn_protocols, js_tls_create_secure_context,
    js_tls_get_ca_certificates, js_tls_get_ciphers, js_tls_native_dispatch,
    js_tls_root_certificates, js_tls_secure_context_constructor,
    js_tls_set_default_ca_certificates,
};
pub use socket_api::{
    js_tls_socket_export_keying_material, js_tls_socket_get_certificate, js_tls_socket_get_cipher,
    js_tls_socket_get_ephemeral_key_info, js_tls_socket_get_finished,
    js_tls_socket_get_peer_certificate, js_tls_socket_get_peer_finished,
    js_tls_socket_get_peer_x509_certificate, js_tls_socket_get_protocol, js_tls_socket_get_session,
    js_tls_socket_get_shared_sigalgs, js_tls_socket_get_x509_certificate,
    js_tls_socket_is_session_reused, js_tls_socket_set_key_cert,
    js_tls_socket_set_max_send_fragment,
};

static NEXT_TLS_HANDLE_ID: OnceLock<Mutex<i64>> = OnceLock::new();
static TLS_SERVERS: OnceLock<Mutex<HashMap<i64, TlsServerState>>> = OnceLock::new();
static TLS_SOCKETS: OnceLock<Mutex<HashMap<i64, TlsSocketState>>> = OnceLock::new();
static TLS_LISTENERS: OnceLock<Mutex<HashMap<i64, HashMap<String, Vec<i64>>>>> = OnceLock::new();
static TLS_ONCE_FLAGS: OnceLock<Mutex<HashMap<i64, HashMap<String, HashSet<i64>>>>> =
    OnceLock::new();
static TLS_PENDING_EVENTS: OnceLock<Mutex<Vec<PendingTlsEvent>>> = OnceLock::new();
static RUSTLS_PROVIDER_INSTALLED: Once = Once::new();

thread_local! {
    // The mutable-root scanner registry is thread-local, so this latch must be too.
    static TLS_GC_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct TlsServerState {
    shutdown_tx: Option<oneshot::Sender<()>>,
    bound_port: u16,
    bound_host: String,
    listening: bool,
    active_connections: usize,
    closing: bool,
    close_event_queued: bool,
    config: Option<Arc<rustls::ServerConfig>>,
    ticket_keys: Vec<u8>,
    allow_half_open: bool,
    pause_on_connect: bool,
    certificate: Vec<u8>,
    cert_resolver: Option<Arc<DynamicCertResolver>>,
    sni_callback: i64,
    alpn_callback: i64,
    alpn_protocols: Vec<Vec<u8>>,
    sni_errors: HashMap<String, String>,
}

#[derive(Debug)]
struct DynamicCertResolver {
    default: Mutex<(Arc<rustls::sign::CertifiedKey>, Vec<u8>)>,
    contexts: Mutex<Vec<(String, Arc<rustls::sign::CertifiedKey>, Vec<u8>)>>,
}

impl DynamicCertResolver {
    fn name_matches(pattern: &str, servername: &str) -> bool {
        let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
        let servername = servername.trim_end_matches('.').to_ascii_lowercase();
        if let Some(suffix) = pattern.strip_prefix("*.") {
            return servername.strip_suffix(suffix).is_some_and(|prefix| {
                prefix.ends_with('.') && !prefix[..prefix.len() - 1].contains('.')
            });
        }
        pattern == servername
    }

    fn selected(&self, servername: Option<&str>) -> (Arc<rustls::sign::CertifiedKey>, Vec<u8>) {
        if let Some(servername) = servername {
            if let Some((_, key, der)) = self
                .contexts
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|(pattern, _, _)| Self::name_matches(pattern, servername))
            {
                return (key.clone(), der.clone());
            }
        }
        self.default.lock().unwrap().clone()
    }
}

impl rustls::server::ResolvesServerCert for DynamicCertResolver {
    fn resolve(
        &self,
        client_hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(self.selected(client_hello.server_name()).0)
    }
}

#[derive(Debug)]
struct EmptyCertResolver;

impl rustls::server::ResolvesServerCert for EmptyCertResolver {
    fn resolve(
        &self,
        _client_hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        None
    }
}

struct TlsSocketState {
    cmd_tx: Option<mpsc::UnboundedSender<TlsSocketCommand>>,
    #[allow(dead_code)] // captured socket local address for future localAddress exposure
    local_addr: Option<SocketAddr>,
    #[allow(dead_code)] // captured socket peer address for future remoteAddress exposure
    peer_addr: Option<SocketAddr>,
    authorized: bool,
    server_side: bool,
    max_send_fragment: usize,
    allow_half_open: bool,
    locally_constructed: bool,
    authorization_error: Option<String>,
    protocol: Option<String>,
    alpn_protocol: Option<String>,
    servername: Option<String>,
    peer_certificate: Vec<u8>,
    own_certificate: Vec<u8>,
    server_handle: Option<i64>,
}

enum TlsSocketCommand {
    Write(Vec<u8>),
    End,
    Destroy,
}

enum PendingTlsEvent {
    ServerListening(i64),
    ServerSecureConnection(i64, i64),
    ServerClose(i64),
    ServerError(i64, String),
    ServerTlsClientError(i64, i64, String, Option<String>),
    SocketData(i64, Vec<u8>),
    SocketEnd(i64),
    SocketClose(i64),
    SocketError(i64, String),
}

struct TlsServerTransport(ServerTlsStream<TcpStream>);

impl AsyncRead for TlsServerTransport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsServerTransport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

fn nanbox_str(s: &str) -> f64 {
    {
        let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    }
}

unsafe fn value_to_string(value: f64) -> Option<String> {
    let ptr = js_get_string_pointer_unified(value);
    if ptr != 0 {
        return string_from_header(ptr as *const StringHeader);
    }
    let coerced = perry_runtime::builtins::js_string_coerce(value);
    string_from_header(coerced as *const StringHeader)
}

fn f64_from_raw_bits(raw_bits: i64) -> f64 {
    f64::from_bits(raw_bits as u64)
}

fn js_is_undefined_or_null(value: f64) -> bool {
    let jsv = JSValue::from_bits(value.to_bits());
    jsv.is_undefined() || jsv.is_null()
}

fn pointer_addr(value: f64) -> Option<usize> {
    let jsv = JSValue::from_bits(value.to_bits());
    if jsv.is_pointer() {
        Some((value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize)
    } else {
        None
    }
}

fn is_array_value(value: f64) -> bool {
    JSValue::from_bits(js_array_is_array(value).to_bits()).as_bool()
}

unsafe fn object_field(value: f64, name: &str) -> f64 {
    let Some(addr) = pointer_addr(value) else {
        return undefined();
    };
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(js_object_get_field_by_name(addr as *const ObjectHeader, key).bits())
}

unsafe fn object_field_string(value: f64, name: &str) -> Option<String> {
    let field = object_field(value, name);
    if js_is_undefined_or_null(field) {
        None
    } else {
        value_to_string(field)
    }
}

unsafe fn set_field(obj: *mut ObjectHeader, key: &str, value: f64) {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(obj, key_ptr, value);
}

unsafe fn set_str_field(obj: *mut ObjectHeader, key: &str, value: &str) {
    set_field(obj, key, nanbox_str(value));
}

fn type_name(value: f64) -> &'static str {
    let jsv = JSValue::from_bits(value.to_bits());
    if jsv.is_undefined() {
        "undefined"
    } else if jsv.is_null() {
        "object"
    } else if jsv.is_bool() {
        "boolean"
    } else if jsv.is_any_string() {
        "string"
    } else if jsv.is_number() || jsv.is_int32() {
        "number"
    } else if jsv.is_pointer() {
        "object"
    } else {
        "object"
    }
}

fn throw_type_error(message: &str, code: &'static str) -> ! {
    perry_runtime::fs::validate::throw_type_error_with_code(message, code)
}

fn throw_plain_type_error(message: &str) -> ! {
    let msg = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg);
    perry_runtime::exception::js_throw(js_nanbox_pointer(err as i64))
}

fn is_closure_value(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_pointer() && perry_runtime::closure::is_closure_ptr(js.as_pointer::<u8>() as usize)
}

fn throw_error(message: &str, code: &'static str) -> ! {
    {
        let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
        perry_runtime::node_submodules::register_error_code_pub(msg, code);
        let err = perry_runtime::error::js_error_new_with_message(msg);
        perry_runtime::exception::js_throw(js_nanbox_pointer(err as i64))
    }
}

unsafe fn string_array(items: &[String]) -> *mut perry_runtime::ArrayHeader {
    let mut arr = js_array_alloc(items.len() as u32);
    for item in items {
        let s = js_string_from_bytes(item.as_ptr(), item.len() as u32);
        arr = js_array_push(arr, JSValue::string_ptr(s));
    }
    arr
}

fn servers() -> &'static Mutex<HashMap<i64, TlsServerState>> {
    TLS_SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sockets() -> &'static Mutex<HashMap<i64, TlsSocketState>> {
    TLS_SOCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn listeners() -> &'static Mutex<HashMap<i64, HashMap<String, Vec<i64>>>> {
    TLS_LISTENERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn once_flags() -> &'static Mutex<HashMap<i64, HashMap<String, HashSet<i64>>>> {
    TLS_ONCE_FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pending_events() -> &'static Mutex<Vec<PendingTlsEvent>> {
    TLS_PENDING_EVENTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn next_tls_handle_id() -> i64 {
    let lock = NEXT_TLS_HANDLE_ID.get_or_init(|| Mutex::new(0x70000));
    let mut guard = lock.lock().unwrap();
    let id = *guard;
    *guard += 1;
    id
}

fn nanbox_handle(handle: i64) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000u64 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF))
}

fn raw_handle_value(handle: i64) -> f64 {
    f64::from_bits(handle as u64)
}

fn ensure_crypto_provider_installed() {
    RUSTLS_PROVIDER_INSTALLED.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

fn ensure_tls_gc_scanner_registered() {
    TLS_GC_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        perry_runtime::gc::gc_register_mutable_root_scanner_named("stdlib:tls", scan_tls_roots_mut);
        registered.set(true);
    });
}

fn scan_tls_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut all) = listeners().lock() {
        for per_handle in all.values_mut() {
            for callbacks in per_handle.values_mut() {
                for cb in callbacks.iter_mut() {
                    visitor.visit_i64_slot(cb);
                }
            }
        }
    }
    if let Ok(mut all) = servers().lock() {
        for server in all.values_mut() {
            if server.sni_callback != 0 {
                visitor.visit_i64_slot(&mut server.sni_callback);
            }
            if server.alpn_callback != 0 {
                visitor.visit_i64_slot(&mut server.alpn_callback);
            }
        }
    }
}

fn push_tls_event(event: PendingTlsEvent) {
    pending_events().lock().unwrap().push(event);
    perry_runtime::event_pump::js_notify_main_thread();
}

fn schedule_tls_socket_close(socket_id: i64) {
    crate::common::async_bridge::spawn(async move {
        // Node emits readable `end` before the socket's terminal `close` turn.
        // The boundary also lets `server.close()` observe the connection count
        // reaching zero and queue its callback between those socket events.
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        push_tls_event(PendingTlsEvent::SocketClose(socket_id));
    });
}

fn tls_server_connection_started(server_id: i64) -> bool {
    let mut all = servers().lock().unwrap();
    let Some(server) = all.get_mut(&server_id) else {
        return false;
    };
    if server.closing || server.close_event_queued {
        return false;
    }
    server.active_connections += 1;
    true
}

fn tls_server_connection_finished(server_id: i64) {
    let emit_close = {
        let mut all = servers().lock().unwrap();
        let Some(server) = all.get_mut(&server_id) else {
            return;
        };
        server.active_connections = server.active_connections.saturating_sub(1);
        let emit = server.closing && server.active_connections == 0 && !server.close_event_queued;
        if emit {
            server.close_event_queued = true;
        }
        emit
    };
    if emit_close {
        push_tls_event(PendingTlsEvent::ServerClose(server_id));
    }
}

fn tls_server_begin_close(server_id: i64) {
    let emit_close = {
        let mut all = servers().lock().unwrap();
        let Some(server) = all.get_mut(&server_id) else {
            return;
        };
        server.listening = false;
        server.closing = true;
        let emit = server.active_connections == 0 && !server.close_event_queued;
        if emit {
            server.close_event_queued = true;
        }
        emit
    };
    if emit_close {
        push_tls_event(PendingTlsEvent::ServerClose(server_id));
    }
}

fn listeners_for(handle: i64, event: &str) -> Vec<i64> {
    listeners()
        .lock()
        .unwrap()
        .get(&handle)
        .and_then(|m| m.get(event).cloned())
        .unwrap_or_default()
}

fn register_listener(handle: i64, event: String, cb: i64, once: bool) {
    if cb == 0 {
        return;
    }
    listeners()
        .lock()
        .unwrap()
        .entry(handle)
        .or_default()
        .entry(event.clone())
        .or_default()
        .push(cb);
    if once {
        once_flags()
            .lock()
            .unwrap()
            .entry(handle)
            .or_default()
            .entry(event)
            .or_default()
            .insert(cb);
    }
}

fn drain_once_listeners(handle: i64, event: &str) {
    let to_drop = {
        let mut flags = once_flags().lock().unwrap();
        let Some(per_handle) = flags.get_mut(&handle) else {
            return;
        };
        let Some(set) = per_handle.remove(event) else {
            return;
        };
        if per_handle.is_empty() {
            flags.remove(&handle);
        }
        set
    };
    if to_drop.is_empty() {
        return;
    }
    if let Some(per_handle) = listeners().lock().unwrap().get_mut(&handle) {
        if let Some(callbacks) = per_handle.get_mut(event) {
            callbacks.retain(|cb| !to_drop.contains(cb));
            if callbacks.is_empty() {
                per_handle.remove(event);
            }
        }
    }
}

fn remove_listener(handle: i64, event: &str, cb: i64) {
    if let Some(per_handle) = listeners().lock().unwrap().get_mut(&handle) {
        if let Some(callbacks) = per_handle.get_mut(event) {
            if let Some(pos) = callbacks.iter().position(|item| *item == cb) {
                callbacks.remove(pos);
            }
            if callbacks.is_empty() {
                per_handle.remove(event);
            }
        }
    }
    if let Some(per_handle) = once_flags().lock().unwrap().get_mut(&handle) {
        if let Some(callbacks) = per_handle.get_mut(event) {
            callbacks.remove(&cb);
            if callbacks.is_empty() {
                per_handle.remove(event);
            }
        }
    }
}

fn remove_all_listeners(handle: i64, event: Option<&str>) {
    if let Some(per_handle) = listeners().lock().unwrap().get_mut(&handle) {
        if let Some(event) = event {
            per_handle.remove(event);
        } else {
            per_handle.clear();
        }
    }
    if let Some(per_handle) = once_flags().lock().unwrap().get_mut(&handle) {
        if let Some(event) = event {
            per_handle.remove(event);
        } else {
            per_handle.clear();
        }
    }
}

fn listener_count(handle: i64, event: &str) -> f64 {
    listeners()
        .lock()
        .unwrap()
        .get(&handle)
        .and_then(|m| m.get(event))
        .map(|callbacks| callbacks.len() as f64)
        .unwrap_or(0.0)
}

fn event_names_json(handle: i64) -> String {
    let all = listeners().lock().unwrap();
    let mut parts = Vec::new();
    // A TLS server is layered on a net.Server and Node exposes that parent's
    // internal raw-connection listener through eventNames().
    if is_tls_server_handle(handle) {
        parts.push("\"connection\"".to_string());
    }
    if let Some(per_handle) = all.get(&handle) {
        for (name, callbacks) in per_handle {
            if !callbacks.is_empty() {
                parts.push(format!("\"{}\"", json_escape(name)));
            }
        }
    }
    format!("[{}]", parts.join(","))
}

fn json_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

unsafe fn string_header_from_string(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

unsafe fn json_value_from_str(json: &str) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let source = scope.root_string_ptr(string_header_from_string(json));
    f64::from_bits(perry_runtime::json::js_json_parse_or_null(source.get_raw_const_ptr()).bits())
}

unsafe fn buffer_from_bytes(bytes: &[u8]) -> f64 {
    let buf = perry_runtime::buffer::js_buffer_alloc(bytes.len() as i32, 0);
    if buf.is_null() {
        return undefined();
    }
    let data = (buf as *mut u8).add(std::mem::size_of::<perry_runtime::buffer::BufferHeader>());
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
    (*buf).length = bytes.len() as u32;
    js_nanbox_pointer(buf as i64)
}

fn certificate_attr_value(atv: &x509_cert::attr::AttributeTypeAndValue) -> String {
    use x509_cert::der::Encode;
    atv.value
        .decode_as::<x509_cert::der::asn1::Utf8StringRef>()
        .map(|value| value.as_str().to_string())
        .or_else(|_| {
            atv.value
                .decode_as::<x509_cert::der::asn1::PrintableStringRef>()
                .map(|value| value.as_str().to_string())
        })
        .or_else(|_| {
            atv.value
                .decode_as::<x509_cert::der::asn1::Ia5StringRef>()
                .map(|value| value.as_str().to_string())
        })
        .unwrap_or_else(|_| {
            let bytes = atv.value.to_der().unwrap_or_default();
            String::from_utf8_lossy(bytes.get(2..).unwrap_or(&bytes)).into_owned()
        })
}

unsafe fn certificate_name_object(name: &x509_cert::name::Name) -> f64 {
    let obj = js_object_alloc(0, 0);
    for rdn in name.iter_rdn() {
        for atv in rdn.iter() {
            let key = match atv.oid.to_string().as_str() {
                "2.5.4.3" => "CN".to_string(),
                "2.5.4.6" => "C".to_string(),
                "2.5.4.10" => "O".to_string(),
                "2.5.4.11" => "OU".to_string(),
                other => other.to_string(),
            };
            set_str_field(obj, &key, &certificate_attr_value(atv));
        }
    }
    js_nanbox_pointer(obj as i64)
}

fn certificate_subject_alt_name(cert: &x509_cert::Certificate) -> Option<String> {
    use x509_cert::der::Decode;
    use x509_cert::ext::pkix::name::GeneralName;
    let extension = cert
        .tbs_certificate()
        .extensions()?
        .iter()
        .find(|extension| extension.extn_id.to_string() == "2.5.29.17")?;
    let san =
        x509_cert::ext::pkix::SubjectAltName::from_der(extension.extn_value.as_bytes()).ok()?;
    let values = san
        .0
        .iter()
        .filter_map(|name| match name {
            GeneralName::DnsName(value) => Some(format!("DNS:{}", value.as_str())),
            GeneralName::IpAddress(value) if value.as_bytes().len() == 4 => {
                let bytes = value.as_bytes();
                Some(format!(
                    "IP Address:{}.{}.{}.{}",
                    bytes[0], bytes[1], bytes[2], bytes[3]
                ))
            }
            GeneralName::IpAddress(value) if value.as_bytes().len() == 16 => {
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(value.as_bytes());
                Some(format!("IP Address:{}", std::net::Ipv6Addr::from(bytes)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join(", "))
}

unsafe fn legacy_certificate_object(der: &[u8], detailed: f64) -> f64 {
    use x509_cert::der::Decode;
    let Ok(cert) = x509_cert::Certificate::from_der(der) else {
        return json_value_from_str("{}");
    };
    let obj = js_object_alloc(0, 0);
    let tbs = cert.tbs_certificate();
    set_field(obj, "subject", certificate_name_object(tbs.subject()));
    set_field(obj, "issuer", certificate_name_object(tbs.issuer()));
    if let Some(san) = certificate_subject_alt_name(&cert) {
        set_str_field(obj, "subjectaltname", &san);
    }
    set_field(obj, "raw", buffer_from_bytes(der));
    set_str_field(obj, "valid_from", "");
    set_str_field(obj, "valid_to", "");
    let value = js_nanbox_pointer(obj as i64);
    if perry_runtime::value::js_is_truthy(detailed) != 0 {
        set_field(obj, "issuerCertificate", value);
    }
    value
}

unsafe fn jsvalue_to_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JSValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    if v.is_any_string() {
        return value_to_string(value).map(|s| s.into_bytes());
    }
    if v.is_pointer() {
        let mut len = 0u32;
        let data = perry_runtime::buffer::js_value_buffer_or_typedarray_data(value, &mut len);
        if !data.is_null() {
            return Some(std::slice::from_raw_parts(data, len as usize).to_vec());
        }
    }
    None
}

unsafe fn alpn_protocols_from_value(value: f64) -> Result<Option<Vec<Vec<u8>>>, &'static str> {
    if js_is_undefined_or_null(value) {
        return Ok(None);
    }
    if is_array_value(value) {
        let arr = pointer_addr(value).unwrap_or(0) as *const perry_runtime::ArrayHeader;
        let len = perry_runtime::js_array_length(arr);
        let mut protocols = Vec::with_capacity(len as usize);
        for index in 0..len {
            let item = perry_runtime::array::js_array_get_f64(arr, index);
            if !JSValue::from_bits(item.to_bits()).is_any_string() {
                return Err("type");
            }
            let bytes = value_to_string(item).unwrap_or_default().into_bytes();
            if bytes.len() > u8::MAX as usize {
                return Err("range");
            }
            protocols.push(bytes);
        }
        return Ok(Some(protocols));
    }
    let Some(encoded) = jsvalue_to_bytes(value) else {
        return Err("plain-type");
    };
    let mut protocols = Vec::new();
    let mut offset = 0usize;
    while offset < encoded.len() {
        let len = encoded[offset] as usize;
        offset += 1;
        if len == 0 || offset + len > encoded.len() {
            return Err("plain-type");
        }
        protocols.push(encoded[offset..offset + len].to_vec());
        offset += len;
    }
    Ok(Some(protocols))
}

fn validate_alpn_protocols(value: f64) -> Option<Vec<Vec<u8>>> {
    match unsafe { alpn_protocols_from_value(value) } {
        Ok(protocols) => protocols,
        Err("range") => perry_runtime::fs::validate::throw_range_error_named(
            "ALPN protocol names must not exceed 255 bytes",
            "ERR_OUT_OF_RANGE",
        ),
        Err("type") => throw_type_error(
            "The \"ALPNProtocols\" option must contain only strings",
            "ERR_INVALID_ARG_TYPE",
        ),
        Err(_) => throw_plain_type_error(
            "The \"ALPNProtocols\" option must be an Array or ArrayBufferView",
        ),
    }
}

unsafe fn validate_server_options(options: f64) -> (f64, bool, bool, Option<Vec<Vec<u8>>>) {
    if js_is_undefined_or_null(options) {
        return (options, false, false, None);
    }
    if is_closure_value(options) {
        return (undefined(), false, false, None);
    }
    if pointer_addr(options).is_none() || is_array_value(options) {
        throw_type_error(
            &format!(
                "The \"options\" argument must be of type object. Received {}",
                type_name(options)
            ),
            "ERR_INVALID_ARG_TYPE",
        );
    }

    let _ = perry_runtime::tls::js_tls_create_secure_context(options);
    for field in ["handshakeTimeout", "sessionTimeout"] {
        let value = object_field(options, field);
        let js = JSValue::from_bits(value.to_bits());
        if !js.is_undefined() && !js.is_number() && !js.is_int32() {
            throw_type_error(
                &format!("The \"options.{field}\" property must be of type number"),
                "ERR_INVALID_ARG_TYPE",
            );
        }
    }
    let sni_callback = object_field(options, "SNICallback");
    if !js_is_undefined_or_null(sni_callback) && !is_closure_value(sni_callback) {
        throw_type_error(
            "The \"options.SNICallback\" property must be of type function",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let alpn_callback = object_field(options, "ALPNCallback");
    let alpn_value = object_field(options, "ALPNProtocols");
    if !js_is_undefined_or_null(alpn_callback) {
        if !is_closure_value(alpn_callback) {
            throw_type_error(
                "The \"options.ALPNCallback\" property must be of type function",
                "ERR_INVALID_ARG_TYPE",
            );
        }
        if !js_is_undefined_or_null(alpn_value) {
            throw_type_error(
                "ALPNCallback and ALPNProtocols are mutually exclusive",
                "ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS",
            );
        }
    }
    let protocols = validate_alpn_protocols(alpn_value);

    let ticket_keys = object_field(options, "ticketKeys");
    if !JSValue::from_bits(ticket_keys.to_bits()).is_undefined() {
        if JSValue::from_bits(ticket_keys.to_bits()).is_any_string() {
            throw_type_error(
                "The \"options.ticketKeys\" property must be an ArrayBufferView",
                "ERR_INVALID_ARG_TYPE",
            );
        }
        let Some(bytes) = jsvalue_to_bytes(ticket_keys) else {
            throw_type_error(
                "The \"options.ticketKeys\" property must be an ArrayBufferView",
                "ERR_INVALID_ARG_TYPE",
            );
        };
        if bytes.len() != 48 {
            throw_type_error(
                "The \"options.ticketKeys\" property must be exactly 48 bytes",
                "ERR_INVALID_ARG_VALUE",
            );
        }
    }
    let allow_half_open =
        perry_runtime::value::js_is_truthy(object_field(options, "allowHalfOpen")) != 0;
    let pause_on_connect =
        perry_runtime::value::js_is_truthy(object_field(options, "pauseOnConnect")) != 0;
    (options, allow_half_open, pause_on_connect, protocols)
}

unsafe fn pem_bytes_from_option(options: f64, field: &str) -> Vec<u8> {
    if js_is_undefined_or_null(options) {
        return Vec::new();
    }
    let value = object_field(options, field);
    jsvalue_to_bytes(value).unwrap_or_default()
}

unsafe fn pem_materials_from_option(options: f64, field: &str) -> Vec<Vec<u8>> {
    if js_is_undefined_or_null(options) {
        return Vec::new();
    }
    let value = object_field(options, field);
    if let Some(array) = pointer_addr(value)
        .filter(|_| is_array_value(value))
        .map(|address| address as *const perry_runtime::ArrayHeader)
    {
        let mut out = Vec::new();
        for index in 0..perry_runtime::js_array_length(array) {
            let item = perry_runtime::array::js_array_get_f64(array, index);
            if let Some(bytes) = jsvalue_to_bytes(item) {
                out.push(bytes);
            }
        }
        return out;
    }
    jsvalue_to_bytes(value)
        .map(|value| vec![value])
        .unwrap_or_default()
}

fn parse_cert_chain(pem: &[u8]) -> Vec<CertificateDer<'static>> {
    let mut cursor = Cursor::new(pem);
    rustls_pemfile::certs(&mut cursor)
        .filter_map(|cert| cert.ok())
        .collect()
}

fn parse_private_key(pem: &[u8]) -> Option<PrivateKeyDer<'static>> {
    let mut cursor = Cursor::new(pem);
    if let Some(Ok(key)) = rustls_pemfile::pkcs8_private_keys(&mut cursor).next() {
        return Some(PrivateKeyDer::Pkcs8(key));
    }
    let mut cursor = Cursor::new(pem);
    if let Some(Ok(key)) = rustls_pemfile::rsa_private_keys(&mut cursor).next() {
        return Some(PrivateKeyDer::Pkcs1(key));
    }
    let mut cursor = Cursor::new(pem);
    if let Some(Ok(key)) = rustls_pemfile::ec_private_keys(&mut cursor).next() {
        return Some(PrivateKeyDer::Sec1(key));
    }
    None
}

unsafe fn build_server_config_from_options(
    options: f64,
) -> Result<(Arc<rustls::ServerConfig>, Arc<DynamicCertResolver>), String> {
    let cert_pem = pem_bytes_from_option(options, "cert");
    let key_pem = pem_bytes_from_option(options, "key");
    let certs = parse_cert_chain(&cert_pem);
    let Some(key) = parse_private_key(&key_pem) else {
        return Err("tls.createServer: no recognized PEM private key".to_string());
    };
    if certs.is_empty() {
        return Err("tls.createServer: empty certificate chain".to_string());
    }
    ensure_crypto_provider_installed();
    // #4906: bypass `with_single_cert`'s webpki leaf parse, which rejects
    // the X.509 v1 certs in Node's test fixtures (`UnsupportedCertVersion`).
    // Node serves whatever cert/key the user supplies; load the signing
    // key directly and install a fixed-cert resolver. (Mirrors
    // `perry-ext-http::tls::build_server_config`.)
    // The minimal auto-optimized `tls` graph uses rustls's default AWS-LC
    // provider (installed by `ensure_crypto_provider_installed`) and does not
    // enable the optional `ring` module. Load the key through that same
    // provider so a TLS-only program can build without unrelated features
    // pulling `ring` in by feature unification.
    let signing_key = rustls::crypto::aws_lc_rs::default_provider()
        .key_provider
        .load_private_key(key)
        .map_err(|e| format!("rustls: build server config: {e}"))?;
    let certified_key = Arc::new(rustls::sign::CertifiedKey::new(certs, signing_key));

    let certificate_der = certified_key
        .cert
        .first()
        .map(|certificate| certificate.as_ref().to_vec())
        .unwrap_or_default();
    let resolver = Arc::new(DynamicCertResolver {
        default: Mutex::new((certified_key, certificate_der)),
        contexts: Mutex::new(Vec::new()),
    });

    let version_mask = perry_runtime::tls::js_tls_effective_version_mask(options);
    let mut versions = Vec::new();
    if version_mask & 0b10 != 0 {
        versions.push(&rustls::version::TLS13);
    }
    if version_mask & 0b01 != 0 {
        versions.push(&rustls::version::TLS12);
    }
    let builder = rustls::ServerConfig::builder_with_provider(
        rustls::crypto::aws_lc_rs::default_provider().into(),
    )
    .with_protocol_versions(&versions)
    .map_err(|error| format!("tls protocol versions: {error}"))?;
    let request_cert =
        perry_runtime::value::js_is_truthy(object_field(options, "requestCert")) != 0;
    let reject_unauthorized =
        perry_runtime::value::js_is_truthy(object_field(options, "rejectUnauthorized")) != 0;
    let builder = if request_cert {
        let mut roots = rustls::RootCertStore::empty();
        let mut configured = Vec::new();
        let mut materials = pem_materials_from_option(options, "ca");
        if materials.is_empty() && perry_runtime::tls::js_tls_default_ca_is_configured() != 0 {
            let configured = perry_runtime::tls::js_tls_get_ca_certificates(undefined());
            if let Some(array) = pointer_addr(configured) {
                let array = array as *const perry_runtime::ArrayHeader;
                for index in 0..perry_runtime::js_array_length(array) {
                    if let Some(bytes) =
                        jsvalue_to_bytes(perry_runtime::array::js_array_get_f64(array, index))
                    {
                        materials.push(bytes);
                    }
                }
            }
        }
        for material in materials {
            let mut cursor = Cursor::new(material);
            for cert in rustls_pemfile::certs(&mut cursor).flatten() {
                configured.push(cert.as_ref().to_vec());
                let _ = roots.add(cert);
            }
        }
        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots));
        let verifier = if reject_unauthorized {
            verifier
        } else {
            verifier.allow_unauthenticated()
        };
        let verifier = verifier
            .build()
            .map_err(|error| format!("tls client verifier: {error}"))?;
        builder.with_client_cert_verifier(Arc::new(client_verifier::NodeConfiguredClientVerifier {
            inner: verifier,
            configured,
        }))
    } else {
        builder.with_no_client_auth()
    };
    let mut config = builder.with_cert_resolver(resolver.clone());
    config.alpn_protocols = Vec::new();
    Ok((Arc::new(config), resolver))
}

fn build_empty_server_config() -> Arc<rustls::ServerConfig> {
    ensure_crypto_provider_installed();
    Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(EmptyCertResolver)),
    )
}

unsafe fn failed_server_socket(server_handle: i64, servername: Option<String>) -> i64 {
    let socket_id = next_tls_handle_id();
    sockets().lock().unwrap().insert(
        socket_id,
        TlsSocketState {
            cmd_tx: None,
            local_addr: None,
            peer_addr: None,
            authorized: false,
            server_side: true,
            max_send_fragment: 16 * 1024,
            allow_half_open: false,
            locally_constructed: false,
            authorization_error: None,
            protocol: None,
            alpn_protocol: None,
            servername,
            peer_certificate: Vec::new(),
            own_certificate: servers()
                .lock()
                .unwrap()
                .get(&server_handle)
                .map(|server| server.certificate.clone())
                .unwrap_or_default(),
            server_handle: Some(server_handle),
        },
    );
    listeners()
        .lock()
        .unwrap()
        .insert(socket_id, HashMap::new());
    socket_id
}

extern "C" fn tls_sni_completion(closure: *const ClosureHeader, error: f64, context: f64) -> f64 {
    unsafe {
        let server_handle = perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as i64;
        let hostname = value_to_string(perry_runtime::closure::js_closure_get_capture_f64(
            closure, 1,
        ))
        .unwrap_or_default();
        if !js_is_undefined_or_null(error) {
            let message = pointer_addr(error)
                .map(|_| object_field(error, "message"))
                .and_then(|value| value_to_string(value))
                .or_else(|| value_to_string(error))
                .unwrap_or_else(|| "TLS SNI callback failed".to_string());
            if let Some(server) = servers().lock().unwrap().get_mut(&server_handle) {
                server.sni_errors.insert(hostname, message);
            }
            return undefined();
        }
        if js_is_undefined_or_null(context) {
            return undefined();
        }
        if let Ok((_config, resolver)) = build_server_config_from_options(context) {
            let selected = resolver.default.lock().unwrap().clone();
            if let Some(server_resolver) = servers()
                .lock()
                .unwrap()
                .get(&server_handle)
                .and_then(|server| server.cert_resolver.clone())
            {
                server_resolver
                    .contexts
                    .lock()
                    .unwrap()
                    .push((hostname, selected.0, selected.1));
            }
        }
        undefined()
    }
}

/// Run main-thread TLS selection callbacks before the async client task starts.
/// Returns 0 on success, 1 for an invalid ALPN callback result, 2 for static
/// ALPN mismatch, and 3 for an SNI callback error.
#[no_mangle]
pub unsafe extern "C" fn js_tls_client_preflight(
    port: f64,
    servername_ptr: *const u8,
    servername_len: usize,
    options: f64,
) -> i32 {
    let servername = if servername_ptr.is_null() {
        String::new()
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(servername_ptr, servername_len))
            .into_owned()
    };
    let server_handle = servers()
        .lock()
        .unwrap()
        .iter()
        .find(|(_, server)| server.listening && (port == 0.0 || server.bound_port == port as u16))
        .map(|(handle, _)| *handle);
    let Some(server_handle) = server_handle else {
        return 0;
    };
    let (sni_callback, alpn_callback, server_protocols) = servers()
        .lock()
        .unwrap()
        .get(&server_handle)
        .map(|server| {
            (
                server.sni_callback,
                server.alpn_callback,
                server.alpn_protocols.clone(),
            )
        })
        .unwrap_or_default();

    if sni_callback != 0 && !servername.is_empty() {
        perry_runtime::closure::js_register_closure_arity(tls_sni_completion as *const u8, 2);
        let completion =
            perry_runtime::closure::js_closure_alloc(tls_sni_completion as *const u8, 2);
        perry_runtime::closure::js_closure_set_capture_ptr(completion, 0, server_handle);
        perry_runtime::closure::js_closure_set_capture_f64(completion, 1, nanbox_str(&servername));
        js_closure_call2(
            sni_callback as *const ClosureHeader,
            nanbox_str(&servername),
            js_nanbox_pointer(completion as i64),
        );
        let error = servers()
            .lock()
            .unwrap()
            .get_mut(&server_handle)
            .and_then(|server| server.sni_errors.remove(&servername));
        if let Some(message) = error {
            let socket_id = failed_server_socket(server_handle, Some(servername));
            push_tls_event(PendingTlsEvent::ServerTlsClientError(
                server_handle,
                socket_id,
                message,
                None,
            ));
            return 3;
        }
    }

    let client_protocols =
        validate_alpn_protocols(object_field(options, "ALPNProtocols")).unwrap_or_default();
    if alpn_callback != 0 {
        let socket_id = failed_server_socket(server_handle, Some(servername.clone()));
        let protocols: Vec<String> = client_protocols
            .iter()
            .map(|protocol| String::from_utf8_lossy(protocol).into_owned())
            .collect();
        let argument = js_object_alloc(0, 0);
        js_object_set_field_by_name(
            argument,
            js_string_from_bytes(b"protocols".as_ptr(), 9),
            js_nanbox_pointer(string_array(&protocols) as i64),
        );
        let callback_socket = nanbox_handle(socket_id);
        // Node invokes an object-literal ALPNCallback with the pending
        // TLSSocket, not the closure's original options-object receiver.
        let rebound_callback = perry_runtime::closure::js_closure_unbox_callee_checked_rebind(
            nanbox_handle(alpn_callback),
            callback_socket,
        );
        let previous_this = perry_runtime::object::js_implicit_this_set(callback_socket);
        let selected = js_closure_call1(
            rebound_callback as *const ClosureHeader,
            js_nanbox_pointer(argument as i64),
        );
        perry_runtime::object::js_implicit_this_set(previous_this);
        let selected = value_to_string(selected).map(String::into_bytes);
        sockets().lock().unwrap().remove(&socket_id);
        listeners().lock().unwrap().remove(&socket_id);
        if selected
            .as_ref()
            .is_none_or(|selected| !client_protocols.contains(selected))
        {
            let socket_id = failed_server_socket(server_handle, Some(servername));
            push_tls_event(PendingTlsEvent::ServerTlsClientError(
                server_handle,
                socket_id,
                "ALPN callback returned a protocol that was not offered".to_string(),
                Some("ERR_TLS_ALPN_CALLBACK_INVALID_RESULT".to_string()),
            ));
            return 1;
        }
    } else if !server_protocols.is_empty()
        && !client_protocols.is_empty()
        && !server_protocols
            .iter()
            .any(|protocol| client_protocols.contains(protocol))
    {
        let socket_id = failed_server_socket(server_handle, Some(servername));
        push_tls_event(PendingTlsEvent::ServerTlsClientError(
            server_handle,
            socket_id,
            "no application protocol".to_string(),
            Some("ERR_SSL_NO_APPLICATION_PROTOCOL".to_string()),
        ));
        return 2;
    }
    socket_api::record_original_servername(server_handle, servername);
    0
}

unsafe fn build_error_object(message: &str) -> f64 {
    let obj = js_object_alloc(perry_runtime::error::CLASS_ID_ERROR, 0);
    set_str_field(obj, "name", "Error");
    set_str_field(obj, "message", message);
    js_nanbox_pointer(obj as i64)
}

unsafe fn build_error_object_with_code(message: &str, code: Option<&str>) -> f64 {
    let error = build_error_object(message);
    if let (Some(code), Some(address)) = (code, pointer_addr(error)) {
        set_str_field(address as *mut ObjectHeader, "code", code);
    }
    error
}

async fn run_tls_socket_task(
    socket_id: i64,
    stream: ServerTlsStream<TcpStream>,
    mut rx: mpsc::UnboundedReceiver<TlsSocketCommand>,
) {
    let mut transport = TlsServerTransport(stream);
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            read_result = transport.read(&mut buf) => {
                match read_result {
                    Ok(0) => {
                        // Reply to the peer's close_notify before dropping TCP.
                        // Otherwise the client intermittently sees UnexpectedEof.
                        let _ = transport.shutdown().await;
                        push_tls_event(PendingTlsEvent::SocketEnd(socket_id));
                        if let Some(server_id) = sockets()
                            .lock()
                            .unwrap()
                            .get(&socket_id)
                            .and_then(|socket| socket.server_handle)
                        {
                            tls_server_connection_finished(server_id);
                        }
                        schedule_tls_socket_close(socket_id);
                        break;
                    }
                    Ok(n) => {
                        push_tls_event(PendingTlsEvent::SocketData(socket_id, buf[..n].to_vec()));
                    }
                    Err(e) => {
                        push_tls_event(PendingTlsEvent::SocketError(socket_id, e.to_string()));
                        if let Some(server_id) = sockets()
                            .lock()
                            .unwrap()
                            .get(&socket_id)
                            .and_then(|socket| socket.server_handle)
                        {
                            tls_server_connection_finished(server_id);
                        }
                        schedule_tls_socket_close(socket_id);
                        break;
                    }
                }
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(TlsSocketCommand::Write(bytes)) => {
                        if let Err(e) = transport.write_all(&bytes).await {
                            push_tls_event(PendingTlsEvent::SocketError(socket_id, e.to_string()));
                            if let Some(server_id) = sockets()
                                .lock()
                                .unwrap()
                                .get(&socket_id)
                                .and_then(|socket| socket.server_handle)
                            {
                                tls_server_connection_finished(server_id);
                            }
                            schedule_tls_socket_close(socket_id);
                            break;
                        }
                    }
                    Some(TlsSocketCommand::End) => {
                        let _ = transport.shutdown().await;
                    }
                    Some(TlsSocketCommand::Destroy) | None => {
                        if let Some(server_id) = sockets()
                            .lock()
                            .unwrap()
                            .get(&socket_id)
                            .and_then(|socket| socket.server_handle)
                        {
                            tls_server_connection_finished(server_id);
                        }
                        schedule_tls_socket_close(socket_id);
                        break;
                    }
                }
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_create_server(options_bits: i64, listener_bits: i64) -> i64 {
    crate::common::async_bridge::ensure_pump_registered();
    ensure_tls_gc_scanner_registered();
    let _ = socket_api::shared_signature_algorithms();
    let original_options = f64_from_raw_bits(options_bits);
    let mut listener_value = f64_from_raw_bits(listener_bits);
    if is_closure_value(original_options) {
        listener_value = original_options;
    }
    let (options, allow_half_open, pause_on_connect, protocols) =
        validate_server_options(original_options);
    let sni_callback = pointer_addr(object_field(options, "SNICallback")).unwrap_or(0) as i64;
    let alpn_callback = pointer_addr(object_field(options, "ALPNCallback")).unwrap_or(0) as i64;
    let configured_protocols = protocols.clone().unwrap_or_default();
    let (config, cert_resolver) = if js_is_undefined_or_null(options) {
        (Some(build_empty_server_config()), None)
    } else {
        match build_server_config_from_options(options) {
            Ok((mut config, resolver)) => {
                if let Some(protocols) = protocols {
                    Arc::make_mut(&mut config).alpn_protocols = protocols;
                } else if alpn_callback != 0 {
                    Arc::make_mut(&mut config).alpn_protocols =
                        vec![b"h2".to_vec(), b"http/1.1".to_vec(), b"acme-tls/1".to_vec()];
                }
                (Some(config), Some(resolver))
            }
            Err(_) => (None, None),
        }
    };
    let id = next_tls_handle_id();
    servers().lock().unwrap().insert(
        id,
        TlsServerState {
            shutdown_tx: None,
            bound_port: 0,
            bound_host: String::new(),
            listening: false,
            active_connections: 0,
            closing: false,
            close_event_queued: false,
            config,
            ticket_keys: vec![0; 48],
            allow_half_open,
            pause_on_connect,
            certificate: parse_cert_chain(&pem_bytes_from_option(options, "cert"))
                .first()
                .map(|certificate| certificate.as_ref().to_vec())
                .unwrap_or_default(),
            cert_resolver,
            sni_callback,
            alpn_callback,
            alpn_protocols: configured_protocols,
            sni_errors: HashMap::new(),
        },
    );
    listeners().lock().unwrap().insert(id, HashMap::new());
    let listener = if is_closure_value(listener_value) {
        pointer_addr(listener_value).unwrap_or(0) as i64
    } else {
        0
    };
    if listener != 0 {
        register_listener(id, "secureConnection".to_string(), listener, false);
    }
    id
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_tlssocket_constructor(socket_bits: i64, options_bits: i64) -> i64 {
    let socket = f64_from_raw_bits(socket_bits);
    let options = f64_from_raw_bits(options_bits);
    if !js_is_undefined_or_null(options) {
        let _ = validate_alpn_protocols(object_field(options, "ALPNProtocols"));
    }
    let standalone = js_is_undefined_or_null(socket);
    let requested_half_open = !js_is_undefined_or_null(options)
        && perry_runtime::value::js_is_truthy(object_field(options, "allowHalfOpen")) != 0;
    let handle = next_tls_handle_id();
    sockets().lock().unwrap().insert(
        handle,
        TlsSocketState {
            cmd_tx: None,
            local_addr: None,
            peer_addr: None,
            authorized: false,
            server_side: false,
            max_send_fragment: 16 * 1024,
            allow_half_open: standalone && requested_half_open,
            locally_constructed: true,
            authorization_error: None,
            protocol: Some("TLSv1.3".to_string()),
            alpn_protocol: None,
            servername: None,
            peer_certificate: Vec::new(),
            own_certificate: Vec::new(),
            server_handle: None,
        },
    );
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_listen(
    handle: i64,
    port: f64,
    host_or_callback_bits: i64,
    callback_bits: i64,
) -> i64 {
    crate::common::async_bridge::ensure_pump_registered();
    ensure_tls_gc_scanner_registered();
    let port = port as u16;
    let host_or_callback = f64_from_raw_bits(host_or_callback_bits);
    let callback = f64_from_raw_bits(callback_bits);
    let host = if JSValue::from_bits(host_or_callback.to_bits()).is_any_string() {
        value_to_string(host_or_callback).unwrap_or_else(|| "0.0.0.0".to_string())
    } else {
        "0.0.0.0".to_string()
    };
    let callback_bits = if pointer_addr(callback).is_some() {
        callback_bits
    } else if pointer_addr(host_or_callback).is_some()
        && !JSValue::from_bits(host_or_callback.to_bits()).is_any_string()
    {
        host_or_callback_bits
    } else {
        TAG_UNDEFINED_BITS as i64
    };
    let config = {
        let mut all = servers().lock().unwrap();
        let Some(server) = all.get_mut(&handle) else {
            return handle;
        };
        let config = server
            .config
            .clone()
            .unwrap_or_else(build_empty_server_config);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        server.shutdown_tx = Some(shutdown_tx);
        server.bound_port = port;
        server.bound_host = host.clone();
        server.listening = true;
        server.active_connections = 0;
        server.closing = false;
        server.close_event_queued = false;
        let cb = pointer_addr(f64_from_raw_bits(callback_bits)).unwrap_or(0) as i64;
        if cb != 0 {
            register_listener(handle, "listening".to_string(), cb, true);
        }
        (
            config,
            shutdown_rx,
            server.cert_resolver.clone(),
            server.allow_half_open,
        )
    };
    let (config, mut shutdown_rx, cert_resolver, allow_half_open) = config;
    let server_id = handle;
    crate::common::async_bridge::spawn(async move {
        let bind = format!("{}:{}", host, port);
        let listener = match TcpListener::bind(&bind).await {
            Ok(listener) => listener,
            Err(e) => {
                push_tls_event(PendingTlsEvent::ServerError(
                    server_id,
                    format!("bind {bind}: {e}"),
                ));
                push_tls_event(PendingTlsEvent::ServerClose(server_id));
                if let Some(server) = servers().lock().unwrap().get_mut(&server_id) {
                    server.listening = false;
                }
                return;
            }
        };
        if let Ok(local) = listener.local_addr() {
            if let Some(server) = servers().lock().unwrap().get_mut(&server_id) {
                server.bound_port = local.port();
                server.bound_host = local.ip().to_string();
            }
        }
        push_tls_event(PendingTlsEvent::ServerListening(server_id));
        let acceptor = TlsAcceptor::from(config);
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, peer)) => {
                            if !tls_server_connection_started(server_id) {
                                drop(stream);
                                continue;
                            }
                            let local_addr = stream.local_addr().ok();
                            let peer_addr = Some(peer);
                            let original_servername =
                                socket_api::take_original_servername(server_id);
                            let acceptor = acceptor.clone();
                            let cert_resolver = cert_resolver.clone();
                            tokio::spawn(async move {
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        let connection = tls_stream.get_ref().1;
                                        let protocol = match connection.protocol_version() {
                                            Some(rustls::ProtocolVersion::TLSv1_2) => Some("TLSv1.2".to_string()),
                                            Some(rustls::ProtocolVersion::TLSv1_3) => Some("TLSv1.3".to_string()),
                                            _ => None,
                                        };
                                        let alpn_protocol = connection.alpn_protocol()
                                            .map(|value| String::from_utf8_lossy(value).into_owned());
                                        let servername = original_servername
                                            .or_else(|| connection.server_name().map(str::to_string));
                                        let own_certificate = cert_resolver
                                            .as_ref()
                                            .map(|resolver| resolver.selected(servername.as_deref()).1)
                                            .unwrap_or_default();
                                        let peer_certificate = connection.peer_certificates()
                                            .and_then(|certificates| certificates.first())
                                            .map(|certificate| certificate.as_ref().to_vec())
                                            .unwrap_or_default();
                                        let authorized = !peer_certificate.is_empty();
                                        let socket_id = next_tls_handle_id();
                                        let (tx, rx) = mpsc::unbounded_channel::<TlsSocketCommand>();
                                        sockets().lock().unwrap().insert(
                                            socket_id,
                                            TlsSocketState {
                                                cmd_tx: Some(tx),
                                                local_addr,
                                                peer_addr,
                                                authorized,
                                                server_side: true,
                                                max_send_fragment: 16 * 1024,
                                                allow_half_open,
                                                locally_constructed: false,
                                                authorization_error: (!authorized)
                                                    .then(|| "UNABLE_TO_GET_ISSUER_CERT".to_string()),
                                                protocol,
                                                alpn_protocol,
                                                servername,
                                                peer_certificate,
                                                own_certificate,
                                                server_handle: Some(server_id),
                                            },
                                        );
                                        listeners().lock().unwrap().insert(socket_id, HashMap::new());
                                        push_tls_event(PendingTlsEvent::ServerSecureConnection(
                                            server_id,
                                            socket_id,
                                        ));
                                        run_tls_socket_task(socket_id, tls_stream, rx).await;
                                    }
                                    Err(e) => {
                                        let socket_id = next_tls_handle_id();
                                        sockets().lock().unwrap().insert(
                                            socket_id,
                                            TlsSocketState {
                                                cmd_tx: None,
                                                local_addr,
                                                peer_addr,
                                                authorized: false,
                                                server_side: true,
                                                max_send_fragment: 16 * 1024,
                                                allow_half_open,
                                                locally_constructed: false,
                                                authorization_error: Some(e.to_string()),
                                                protocol: None,
                                                alpn_protocol: None,
                                                servername: None,
                                                peer_certificate: Vec::new(),
                                                own_certificate: Vec::new(),
                                                server_handle: Some(server_id),
                                            },
                                        );
                                        listeners().lock().unwrap().insert(socket_id, HashMap::new());
                                        push_tls_event(PendingTlsEvent::ServerTlsClientError(
                                            server_id,
                                            socket_id,
                                            format!("tls handshake: {e}"),
                                            None,
                                        ));
                                        tls_server_connection_finished(server_id);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            push_tls_event(PendingTlsEvent::ServerError(
                                server_id,
                                format!("accept: {e}"),
                            ));
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    break;
                }
            }
        }
        tls_server_begin_close(server_id);
    });
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_close(handle: i64, callback_bits: i64) -> i64 {
    let cb = pointer_addr(f64_from_raw_bits(callback_bits)).unwrap_or(0) as i64;
    if cb != 0 {
        register_listener(handle, "close".to_string(), cb, true);
    }
    let shutdown_tx = servers()
        .lock()
        .unwrap()
        .get_mut(&handle)
        .and_then(|server| server.shutdown_tx.take());
    tls_server_begin_close(handle);
    if let Some(shutdown_tx) = shutdown_tx {
        let _ = shutdown_tx.send(());
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_address(handle: i64) -> *mut StringHeader {
    let json = match servers().lock().unwrap().get(&handle) {
        Some(server) if server.listening => {
            let family = if server.bound_host.contains(':') {
                "IPv6"
            } else {
                "IPv4"
            };
            format!(
                "{{\"port\":{},\"address\":\"{}\",\"family\":\"{}\"}}",
                server.bound_port,
                json_escape(&server.bound_host),
                family
            )
        }
        _ => "null".to_string(),
    };
    string_header_from_string(&json)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_on(handle: i64, event_ptr: i64, cb_bits: i64) -> i64 {
    let Some(event) = string_from_header(event_ptr as *const StringHeader) else {
        return handle;
    };
    let cb = pointer_addr(f64_from_raw_bits(cb_bits)).unwrap_or(0) as i64;
    register_listener(handle, event, cb, false);
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_once(handle: i64, event_ptr: i64, cb_bits: i64) -> i64 {
    let Some(event) = string_from_header(event_ptr as *const StringHeader) else {
        return handle;
    };
    let cb = pointer_addr(f64_from_raw_bits(cb_bits)).unwrap_or(0) as i64;
    register_listener(handle, event, cb, true);
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_remove_listener(
    handle: i64,
    event_ptr: i64,
    cb_bits: i64,
) -> i64 {
    if let Some(event) = string_from_header(event_ptr as *const StringHeader) {
        let cb = pointer_addr(f64_from_raw_bits(cb_bits)).unwrap_or(0) as i64;
        remove_listener(handle, &event, cb);
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_remove_all_listeners(handle: i64, event_ptr: i64) -> i64 {
    let event = string_from_header(event_ptr as *const StringHeader);
    remove_all_listeners(handle, event.as_deref());
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_listener_count(handle: i64, event_ptr: i64) -> f64 {
    string_from_header(event_ptr as *const StringHeader)
        .map(|event| listener_count(handle, &event))
        .unwrap_or(0.0)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_event_names(handle: i64) -> *mut StringHeader {
    string_header_from_string(&event_names_json(handle))
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_set_secure_context(handle: i64, options_bits: i64) {
    let options = f64_from_raw_bits(options_bits);
    let js = JSValue::from_bits(options.to_bits());
    if js.is_null()
        || js.is_undefined()
        || pointer_addr(options).is_none()
        || is_array_value(options)
    {
        throw_type_error(
            "The \"options\" argument must be of type object",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let _ = perry_runtime::tls::js_tls_create_secure_context(options);
    if let Ok((config, new_resolver)) = build_server_config_from_options(options) {
        if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
            let selected = new_resolver.default.lock().unwrap().clone();
            if let Some(resolver) = &server.cert_resolver {
                *resolver.default.lock().unwrap() = selected.clone();
            } else {
                server.cert_resolver = Some(new_resolver);
                server.config = Some(config);
            }
            server.certificate = selected.1;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_add_context(
    handle: i64,
    hostname: f64,
    context: f64,
) -> f64 {
    if !JSValue::from_bits(hostname.to_bits()).is_any_string() {
        throw_plain_type_error("The \"hostname\" argument must be of type string");
    }
    let Some(hostname) = value_to_string(hostname) else {
        throw_type_error(
            "The \"hostname\" argument must be of type string",
            "ERR_INVALID_ARG_TYPE",
        );
    };
    if hostname.is_empty() {
        throw_type_error(
            "The \"hostname\" argument must not be empty",
            "ERR_TLS_REQUIRED_SERVER_NAME",
        );
    }
    if !perry_runtime::tls::is_secure_context_instance(context)
        && (pointer_addr(context).is_none() || is_array_value(context))
    {
        throw_type_error(
            "The \"context\" argument must be a SecureContext or options object",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let _ = perry_runtime::tls::js_tls_create_secure_context(context);
    let Ok((_config, resolver)) = build_server_config_from_options(context) else {
        throw_type_error("Invalid TLS context", "ERR_INVALID_ARG_VALUE");
    };
    let selected = resolver.default.lock().unwrap().clone();
    if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
        if let Some(server_resolver) = &server.cert_resolver {
            server_resolver
                .contexts
                .lock()
                .unwrap()
                .push((hostname, selected.0, selected.1));
        }
    }
    undefined()
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_get_ticket_keys(handle: i64) -> f64 {
    let keys = servers()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|server| server.ticket_keys.clone())
        .unwrap_or_else(|| vec![0; 48]);
    buffer_from_bytes(&keys)
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_set_ticket_keys(handle: i64, value_bits: i64) {
    let value = f64_from_raw_bits(value_bits);
    if JSValue::from_bits(value.to_bits()).is_any_string() {
        throw_type_error(
            "The \"keys\" argument must be an ArrayBufferView",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let Some(bytes) = jsvalue_to_bytes(value) else {
        throw_type_error(
            "The \"keys\" argument must be an ArrayBufferView",
            "ERR_INVALID_ARG_TYPE",
        );
    };
    if bytes.len() != 48 {
        throw_error(
            "Ticket keys must be exactly 48 bytes",
            "ERR_INVALID_ARG_VALUE",
        );
    }
    if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
        server.ticket_keys = bytes;
    }
}

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

// TLS methods are called only by symbols emitted into generated object files.
// Keep every public FFI entry point in the auto-optimized stdlib archive so
// whole-program LTO cannot discard them before the generated object is linked.
struct KeepTlsFfi<const N: usize>(
    #[allow(dead_code)] [*const (); N], // link-time keepalive anchor; field never read
);
// SAFETY: the pointers are retained for linking only and are never read or
// dereferenced, so sharing the static anchor between threads is sound.
unsafe impl<const N: usize> Sync for KeepTlsFfi<N> {}
#[used]
static KEEP_TLS_FFI: KeepTlsFfi<23> = KeepTlsFfi([
    js_tls_create_server as *const (),
    js_tls_tlssocket_constructor as *const (),
    js_tls_server_listen as *const (),
    js_tls_server_close as *const (),
    js_tls_server_address as *const (),
    js_tls_server_on as *const (),
    js_tls_server_once as *const (),
    js_tls_server_remove_listener as *const (),
    js_tls_server_remove_all_listeners as *const (),
    js_tls_server_listener_count as *const (),
    js_tls_server_event_names as *const (),
    js_tls_server_set_secure_context as *const (),
    js_tls_server_get_ticket_keys as *const (),
    js_tls_server_set_ticket_keys as *const (),
    js_tls_socket_get_protocol as *const (),
    js_tls_socket_get_cipher as *const (),
    js_tls_socket_get_peer_certificate as *const (),
    js_tls_socket_get_certificate as *const (),
    js_tls_socket_get_session as *const (),
    js_tls_socket_is_session_reused as *const (),
    js_tls_socket_export_keying_material as *const (),
    js_tls_socket_set_max_send_fragment as *const (),
    js_tls_process_pending as *const (),
]);
