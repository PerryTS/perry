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

use perry_runtime::{
    js_array_alloc, js_array_is_array, js_array_push, js_closure_call0, js_closure_call1,
    js_get_string_pointer_unified, js_nanbox_pointer, js_object_alloc, js_object_get_field_by_name,
    js_object_set_field_by_name, js_string_from_bytes, ClosureHeader, JSValue, ObjectHeader,
    StringHeader,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::{rustls, server::TlsStream as ServerTlsStream, TlsAcceptor};

const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
const TLS_DISPATCH_MISSING_BITS: u64 = TAG_UNDEFINED_BITS;

mod dispatch;
mod module_api;
mod secure_context;

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

static NEXT_TLS_HANDLE_ID: OnceLock<Mutex<i64>> = OnceLock::new();
static TLS_SERVERS: OnceLock<Mutex<HashMap<i64, TlsServerState>>> = OnceLock::new();
static TLS_SOCKETS: OnceLock<Mutex<HashMap<i64, TlsSocketState>>> = OnceLock::new();
static TLS_LISTENERS: OnceLock<Mutex<HashMap<i64, HashMap<String, Vec<i64>>>>> = OnceLock::new();
static TLS_ONCE_FLAGS: OnceLock<Mutex<HashMap<i64, HashMap<String, HashSet<i64>>>>> =
    OnceLock::new();
static TLS_PENDING_EVENTS: OnceLock<Mutex<Vec<PendingTlsEvent>>> = OnceLock::new();
static TLS_GC_REGISTERED: Once = Once::new();
static RUSTLS_PROVIDER_INSTALLED: Once = Once::new();

struct TlsServerState {
    shutdown_tx: Option<oneshot::Sender<()>>,
    bound_port: u16,
    bound_host: String,
    listening: bool,
    config: Option<Arc<rustls::ServerConfig>>,
    ticket_keys: Vec<u8>,
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
    SocketData(i64, Vec<u8>),
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

unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
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

unsafe fn static_string_array(items: &[&str]) -> *mut perry_runtime::ArrayHeader {
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
    TLS_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named("stdlib:tls", scan_tls_roots_mut);
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
}

fn push_tls_event(event: PendingTlsEvent) {
    pending_events().lock().unwrap().push(event);
    perry_runtime::event_pump::js_notify_main_thread();
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
    let Some(per_handle) = all.get(&handle) else {
        return "[]".to_string();
    };
    let mut parts = Vec::new();
    for (name, callbacks) in per_handle {
        if !callbacks.is_empty() {
            parts.push(format!("\"{}\"", json_escape(name)));
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
    let ptr = string_header_from_string(json);
    f64::from_bits(perry_runtime::json::js_json_parse_or_null(ptr).bits())
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

unsafe fn jsvalue_to_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JSValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    if v.is_any_string() {
        return value_to_string(value).map(|s| s.into_bytes());
    }
    if v.is_pointer() {
        let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
        if perry_runtime::buffer::js_buffer_is_buffer(raw) != 0 {
            let buf = raw as *const perry_runtime::buffer::BufferHeader;
            if !buf.is_null() {
                let len = (*buf).length as usize;
                let data = (buf as *const u8)
                    .add(std::mem::size_of::<perry_runtime::buffer::BufferHeader>());
                return Some(std::slice::from_raw_parts(data, len).to_vec());
            }
        }
    }
    value_to_string(value).map(|s| s.into_bytes())
}

unsafe fn pem_bytes_from_option(options: f64, field: &str) -> Vec<u8> {
    if js_is_undefined_or_null(options) {
        return Vec::new();
    }
    let value = object_field(options, field);
    jsvalue_to_bytes(value).unwrap_or_default()
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
) -> Result<Arc<rustls::ServerConfig>, String> {
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

    #[derive(Debug)]
    struct FixedCert(Arc<rustls::sign::CertifiedKey>);
    impl rustls::server::ResolvesServerCert for FixedCert {
        fn resolve(
            &self,
            _client_hello: rustls::server::ClientHello<'_>,
        ) -> Option<Arc<rustls::sign::CertifiedKey>> {
            Some(self.0.clone())
        }
    }

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(FixedCert(certified_key)));
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

unsafe fn build_error_object(message: &str) -> f64 {
    let obj = js_object_alloc(perry_runtime::error::CLASS_ID_ERROR, 0);
    set_str_field(obj, "name", "Error");
    set_str_field(obj, "message", message);
    js_nanbox_pointer(obj as i64)
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
                        push_tls_event(PendingTlsEvent::SocketClose(socket_id));
                        break;
                    }
                    Ok(n) => {
                        push_tls_event(PendingTlsEvent::SocketData(socket_id, buf[..n].to_vec()));
                    }
                    Err(e) => {
                        push_tls_event(PendingTlsEvent::SocketError(socket_id, e.to_string()));
                        push_tls_event(PendingTlsEvent::SocketClose(socket_id));
                        break;
                    }
                }
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(TlsSocketCommand::Write(bytes)) => {
                        if let Err(e) = transport.write_all(&bytes).await {
                            push_tls_event(PendingTlsEvent::SocketError(socket_id, e.to_string()));
                            push_tls_event(PendingTlsEvent::SocketClose(socket_id));
                            break;
                        }
                    }
                    Some(TlsSocketCommand::End) => {
                        let _ = transport.shutdown().await;
                        push_tls_event(PendingTlsEvent::SocketClose(socket_id));
                        break;
                    }
                    Some(TlsSocketCommand::Destroy) | None => {
                        push_tls_event(PendingTlsEvent::SocketClose(socket_id));
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
    let options = f64_from_raw_bits(options_bits);
    let config = if js_is_undefined_or_null(options) {
        None
    } else {
        match build_server_config_from_options(options) {
            Ok(config) => Some(config),
            Err(_) => None,
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
            config,
            ticket_keys: vec![0; 48],
        },
    );
    listeners().lock().unwrap().insert(id, HashMap::new());
    let listener = pointer_addr(f64_from_raw_bits(listener_bits)).unwrap_or(0) as i64;
    if listener != 0 {
        register_listener(id, "secureConnection".to_string(), listener, false);
    }
    id
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_tlssocket_constructor(
    _socket_bits: i64,
    _options_bits: i64,
) -> i64 {
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
        },
    );
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_listen(handle: i64, port: f64, callback_bits: i64) -> i64 {
    crate::common::async_bridge::ensure_pump_registered();
    ensure_tls_gc_scanner_registered();
    let port = port as u16;
    let host = "0.0.0.0".to_string();
    let config = {
        let mut all = servers().lock().unwrap();
        let Some(server) = all.get_mut(&handle) else {
            return handle;
        };
        let Some(config) = server.config.clone() else {
            push_tls_event(PendingTlsEvent::ServerError(
                handle,
                "tls server requires key and cert".to_string(),
            ));
            return handle;
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        server.shutdown_tx = Some(shutdown_tx);
        server.bound_port = port;
        server.bound_host = host.clone();
        server.listening = true;
        let cb = pointer_addr(f64_from_raw_bits(callback_bits)).unwrap_or(0) as i64;
        if cb != 0 {
            register_listener(handle, "listening".to_string(), cb, true);
        }
        (config, shutdown_rx)
    };
    let (config, mut shutdown_rx) = config;
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
                            let local_addr = stream.local_addr().ok();
                            let peer_addr = Some(peer);
                            let acceptor = acceptor.clone();
                            tokio::spawn(async move {
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        let socket_id = next_tls_handle_id();
                                        let (tx, rx) = mpsc::unbounded_channel::<TlsSocketCommand>();
                                        sockets().lock().unwrap().insert(
                                            socket_id,
                                            TlsSocketState {
                                                cmd_tx: Some(tx),
                                                local_addr,
                                                peer_addr,
                                                authorized: false,
                                                server_side: true,
                                                max_send_fragment: 16 * 1024,
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
                                        push_tls_event(PendingTlsEvent::ServerError(
                                            server_id,
                                            format!("tls handshake: {e}"),
                                        ));
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
        push_tls_event(PendingTlsEvent::ServerClose(server_id));
        if let Some(server) = servers().lock().unwrap().get_mut(&server_id) {
            server.listening = false;
        }
    });
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_server_close(handle: i64, callback_bits: i64) -> i64 {
    let cb = pointer_addr(f64_from_raw_bits(callback_bits)).unwrap_or(0) as i64;
    if cb != 0 {
        register_listener(handle, "close".to_string(), cb, true);
    }
    if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
        server.shutdown_tx.take();
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
pub unsafe extern "C" fn js_tls_server_set_secure_context(handle: i64, options_bits: i64) -> i64 {
    let options = f64_from_raw_bits(options_bits);
    if let Ok(config) = build_server_config_from_options(options) {
        if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
            server.config = Some(config);
        }
    }
    handle
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
pub unsafe extern "C" fn js_tls_server_set_ticket_keys(handle: i64, value_bits: i64) -> i64 {
    let value = f64_from_raw_bits(value_bits);
    if let Some(bytes) = jsvalue_to_bytes(value) {
        if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
            server.ticket_keys = bytes;
        }
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_protocol(handle: i64) -> f64 {
    if is_tls_socket_handle(handle) {
        nanbox_str("TLSv1.3")
    } else {
        undefined()
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_cipher(handle: i64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    json_value_from_str(
        "{\"name\":\"TLS_AES_256_GCM_SHA384\",\"standardName\":\"TLS_AES_256_GCM_SHA384\",\"version\":\"TLSv1.3\"}",
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_peer_certificate(handle: i64, _detailed: f64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    json_value_from_str("{\"subject\":{},\"issuer\":{},\"valid_from\":\"\",\"valid_to\":\"\"}")
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_certificate(handle: i64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    json_value_from_str("{\"subject\":{},\"issuer\":{}}")
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_get_session(handle: i64) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    buffer_from_bytes(&[])
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_is_session_reused(handle: i64) -> f64 {
    if is_tls_socket_handle(handle) {
        f64::from_bits(JSValue::bool(false).bits())
    } else {
        undefined()
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_export_keying_material(
    handle: i64,
    length: f64,
    _label_ptr: i64,
) -> f64 {
    if !is_tls_socket_handle(handle) {
        return undefined();
    }
    let len = length.max(0.0).min(16.0 * 1024.0) as usize;
    buffer_from_bytes(&vec![0; len])
}

#[no_mangle]
pub unsafe extern "C" fn js_tls_socket_set_max_send_fragment(handle: i64, size: f64) -> f64 {
    if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
        socket.max_send_fragment = size.max(512.0).min(16_384.0) as usize;
    }
    f64::from_bits(JSValue::bool(is_tls_socket_handle(handle)).bits())
}

pub fn record_tls_client_handle(handle: i64) {
    if handle <= 0 {
        return;
    }
    crate::common::async_bridge::ensure_pump_registered();
    ensure_tls_gc_scanner_registered();
    sockets()
        .lock()
        .unwrap()
        .entry(handle)
        .or_insert(TlsSocketState {
            cmd_tx: None,
            local_addr: None,
            peer_addr: None,
            authorized: true,
            server_side: false,
            max_send_fragment: 16 * 1024,
        });
}

pub fn is_tls_server_handle(handle: i64) -> bool {
    servers().lock().unwrap().contains_key(&handle)
}

pub fn is_tls_socket_handle(handle: i64) -> bool {
    sockets().lock().unwrap().contains_key(&handle)
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
                let socket = raw_handle_value(socket_id);
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
            PendingTlsEvent::SocketData(socket_id, bytes) => {
                let data = buffer_from_bytes(&bytes);
                for cb in listeners_for(socket_id, "data") {
                    if cb != 0 {
                        js_closure_call1(cb as *const ClosureHeader, data);
                    }
                }
                drain_once_listeners(socket_id, "data");
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
    if servers().lock().unwrap().values().any(|s| s.listening) {
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
