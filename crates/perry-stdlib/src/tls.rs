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

use base64::{engine::general_purpose, Engine as _};
use perry_runtime::array::js_array_get_f64;
use perry_runtime::{
    js_array_alloc, js_array_is_array, js_array_length, js_array_push, js_closure_call0,
    js_closure_call1, js_get_string_pointer_unified, js_nanbox_pointer, js_object_alloc,
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

const DEFAULT_CIPHERS: &str = concat!(
    "TLS_AES_256_GCM_SHA384:",
    "TLS_CHACHA20_POLY1305_SHA256:",
    "TLS_AES_128_GCM_SHA256:",
    "ECDHE-RSA-AES128-GCM-SHA256:",
    "ECDHE-ECDSA-AES128-GCM-SHA256:",
    "ECDHE-RSA-AES256-GCM-SHA384:",
    "ECDHE-ECDSA-AES256-GCM-SHA384:",
    "DHE-RSA-AES128-GCM-SHA256:",
    "ECDHE-RSA-AES128-SHA256:",
    "DHE-RSA-AES128-SHA256:",
    "ECDHE-RSA-AES256-SHA384:",
    "DHE-RSA-AES256-SHA384:",
    "ECDHE-RSA-AES256-SHA256:",
    "DHE-RSA-AES256-SHA256:",
    "HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"
);

const NODE_TLS_CIPHERS: &[&str] = &[
    "aes128-gcm-sha256",
    "aes128-sha",
    "aes128-sha256",
    "aes256-gcm-sha384",
    "aes256-sha",
    "aes256-sha256",
    "dhe-psk-aes128-cbc-sha",
    "dhe-psk-aes128-cbc-sha256",
    "dhe-psk-aes128-gcm-sha256",
    "dhe-psk-aes256-cbc-sha",
    "dhe-psk-aes256-cbc-sha384",
    "dhe-psk-aes256-gcm-sha384",
    "dhe-psk-chacha20-poly1305",
    "dhe-rsa-aes128-gcm-sha256",
    "dhe-rsa-aes128-sha",
    "dhe-rsa-aes128-sha256",
    "dhe-rsa-aes256-gcm-sha384",
    "dhe-rsa-aes256-sha",
    "dhe-rsa-aes256-sha256",
    "dhe-rsa-chacha20-poly1305",
    "ecdhe-ecdsa-aes128-gcm-sha256",
    "ecdhe-ecdsa-aes128-sha",
    "ecdhe-ecdsa-aes128-sha256",
    "ecdhe-ecdsa-aes256-gcm-sha384",
    "ecdhe-ecdsa-aes256-sha",
    "ecdhe-ecdsa-aes256-sha384",
    "ecdhe-ecdsa-chacha20-poly1305",
    "ecdhe-psk-aes128-cbc-sha",
    "ecdhe-psk-aes128-cbc-sha256",
    "ecdhe-psk-aes256-cbc-sha",
    "ecdhe-psk-aes256-cbc-sha384",
    "ecdhe-psk-chacha20-poly1305",
    "ecdhe-rsa-aes128-gcm-sha256",
    "ecdhe-rsa-aes128-sha",
    "ecdhe-rsa-aes128-sha256",
    "ecdhe-rsa-aes256-gcm-sha384",
    "ecdhe-rsa-aes256-sha",
    "ecdhe-rsa-aes256-sha384",
    "ecdhe-rsa-chacha20-poly1305",
    "psk-aes128-cbc-sha",
    "psk-aes128-cbc-sha256",
    "psk-aes128-gcm-sha256",
    "psk-aes256-cbc-sha",
    "psk-aes256-cbc-sha384",
    "psk-aes256-gcm-sha384",
    "psk-chacha20-poly1305",
    "rsa-psk-aes128-cbc-sha",
    "rsa-psk-aes128-cbc-sha256",
    "rsa-psk-aes128-gcm-sha256",
    "rsa-psk-aes256-cbc-sha",
    "rsa-psk-aes256-cbc-sha384",
    "rsa-psk-aes256-gcm-sha384",
    "rsa-psk-chacha20-poly1305",
    "srp-aes-128-cbc-sha",
    "srp-aes-256-cbc-sha",
    "srp-rsa-aes-128-cbc-sha",
    "srp-rsa-aes-256-cbc-sha",
    "tls_aes_128_ccm_8_sha256",
    "tls_aes_128_ccm_sha256",
    "tls_aes_128_gcm_sha256",
    "tls_aes_256_gcm_sha384",
    "tls_chacha20_poly1305_sha256",
];

static ROOT_CERTIFICATES: OnceLock<Vec<String>> = OnceLock::new();
static DEFAULT_CA_CERTIFICATES: OnceLock<Mutex<Option<Vec<String>>>> = OnceLock::new();
static NEXT_SECURE_CONTEXT_ID: OnceLock<Mutex<i64>> = OnceLock::new();
static NEXT_TLS_HANDLE_ID: OnceLock<Mutex<i64>> = OnceLock::new();
static TLS_SERVERS: OnceLock<Mutex<HashMap<i64, TlsServerState>>> = OnceLock::new();
static TLS_SOCKETS: OnceLock<Mutex<HashMap<i64, TlsSocketState>>> = OnceLock::new();
static TLS_LISTENERS: OnceLock<Mutex<HashMap<i64, HashMap<String, Vec<i64>>>>> = OnceLock::new();
static TLS_ONCE_FLAGS: OnceLock<Mutex<HashMap<i64, HashMap<String, HashSet<i64>>>>> =
    OnceLock::new();
static TLS_PENDING_EVENTS: OnceLock<Mutex<Vec<PendingTlsEvent>>> = OnceLock::new();
static TLS_GC_REGISTERED: Once = Once::new();
static RUSTLS_PROVIDER_INSTALLED: Once = Once::new();

#[derive(Default)]
struct PemScan {
    valid: usize,
    had_pem_boundary: bool,
    had_parse_error: bool,
}

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
    local_addr: Option<SocketAddr>,
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
    unsafe {
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
    unsafe {
        let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
        perry_runtime::node_submodules::register_error_code_pub(msg, code);
        let err = perry_runtime::error::js_error_new_with_message(msg);
        perry_runtime::exception::js_throw(js_nanbox_pointer(err as i64))
    }
}

fn der_to_pem(der: &[u8]) -> String {
    let encoded = general_purpose::STANDARD.encode(der);
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

fn load_native_certificates() -> Vec<String> {
    let native = rustls_native_certs::load_native_certs();
    let mut out = Vec::with_capacity(native.certs.len());
    for cert in native.certs {
        out.push(der_to_pem(cert.as_ref()));
    }
    out
}

fn root_certificates() -> &'static Vec<String> {
    ROOT_CERTIFICATES.get_or_init(load_native_certificates)
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

fn ca_store() -> &'static Mutex<Option<Vec<String>>> {
    DEFAULT_CA_CERTIFICATES.get_or_init(|| Mutex::new(None))
}

unsafe fn cert_list_from_array_value(value: f64) -> Result<Vec<String>, ()> {
    if !is_array_value(value) {
        return Err(());
    }
    let Some(addr) = pointer_addr(value) else {
        return Err(());
    };
    let arr = addr as *const perry_runtime::ArrayHeader;
    let len = js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let item = js_array_get_f64(arr, i);
        let Some(s) = value_to_string(item) else {
            return Err(());
        };
        out.push(s);
    }
    Ok(out)
}

fn scan_pem_certificates(pems: &[String]) -> PemScan {
    let mut scan = PemScan::default();
    for pem in pems {
        if pem.contains("-----BEGIN CERTIFICATE-----") {
            scan.had_pem_boundary = true;
        }
        let mut cursor = Cursor::new(pem.as_bytes());
        for cert in rustls_pemfile::certs(&mut cursor) {
            match cert {
                Ok(_) => scan.valid += 1,
                Err(_) => scan.had_parse_error = true,
            }
        }
    }
    scan
}

fn validate_ca_list_for_set(pems: &[String]) {
    if pems.is_empty() {
        return;
    }
    let scan = scan_pem_certificates(pems);
    if scan.valid > 0 {
        return;
    }
    if scan.had_pem_boundary || scan.had_parse_error {
        throw_error(
            "error:0488000D:PEM routines::ASN1 lib",
            "ERR_OSSL_PEM_ASN1_LIB",
        );
    }
    throw_error(
        "No valid certificates found in the provided array",
        "ERR_CRYPTO_OPERATION_FAILED",
    );
}

fn validate_ca_list_for_context(pems: &[String]) {
    if pems.is_empty() {
        return;
    }
    let scan = scan_pem_certificates(pems);
    if scan.valid > 0 {
        return;
    }
    if scan.had_pem_boundary || scan.had_parse_error {
        throw_error(
            "error:0488000D:PEM routines::ASN1 lib",
            "ERR_OSSL_PEM_ASN1_LIB",
        );
    }
    throw_error(
        "No valid certificates found in the provided array",
        "ERR_CRYPTO_OPERATION_FAILED",
    );
}

fn validate_tls_version(value: f64, label: &str) {
    if js_is_undefined_or_null(value) {
        return;
    }
    let text = unsafe { value_to_string(value).unwrap_or_default() };
    match text.as_str() {
        "TLSv1.2" | "TLSv1.3" => {}
        _ => {
            let adjective = if label == "minVersion" {
                "minimum"
            } else {
                "maximum"
            };
            throw_type_error(
                &format!("{text:?} is not a valid {adjective} TLS protocol version"),
                "ERR_TLS_INVALID_PROTOCOL_VERSION",
            );
        }
    }
}

unsafe fn ca_list_from_value(value: f64) -> Result<Vec<String>, ()> {
    if js_is_undefined_or_null(value) {
        return Ok(Vec::new());
    }
    if is_array_value(value) {
        return cert_list_from_array_value(value);
    }
    let Some(s) = value_to_string(value) else {
        return Err(());
    };
    Ok(vec![s])
}

fn next_secure_context_id() -> i64 {
    let lock = NEXT_SECURE_CONTEXT_ID.get_or_init(|| Mutex::new(1));
    let mut guard = lock.lock().unwrap();
    let id = *guard;
    *guard += 1;
    id
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
    // `perry-ext-http-server::tls::build_server_config`.)
    let signing_key = rustls::crypto::ring::default_provider()
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

unsafe fn constructor_value(name: &str) -> f64 {
    let module = b"tls";
    let prop = name.as_bytes();
    perry_runtime::object::js_native_module_property_by_name(
        module.as_ptr(),
        module.len(),
        prop.as_ptr(),
        prop.len(),
    )
}

unsafe fn make_secure_context(options: f64) -> f64 {
    let min_version = if js_is_undefined_or_null(options) {
        undefined()
    } else {
        object_field(options, "minVersion")
    };
    let max_version = if js_is_undefined_or_null(options) {
        undefined()
    } else {
        object_field(options, "maxVersion")
    };
    validate_tls_version(min_version, "minVersion");
    validate_tls_version(max_version, "maxVersion");

    let ca_value = if js_is_undefined_or_null(options) {
        undefined()
    } else {
        object_field(options, "ca")
    };
    let ca = ca_list_from_value(ca_value).unwrap_or_else(|_| {
        throw_type_error(
            "The \"ca\" option must be a string, Buffer, or an array of those values",
            "ERR_INVALID_ARG_TYPE",
        )
    });
    validate_ca_list_for_context(&ca);

    let obj = js_object_alloc(0, 0);
    set_field(obj, "context", next_secure_context_id() as f64);
    set_field(obj, "_secureContext", next_secure_context_id() as f64);
    set_str_field(obj, "minVersion", "TLSv1.2");
    set_str_field(obj, "maxVersion", "TLSv1.3");
    set_field(obj, "constructor", constructor_value("SecureContext"));
    if !ca.is_empty() {
        let ca_arr = string_array(&ca);
        set_field(obj, "ca", js_nanbox_pointer(ca_arr as i64));
    }
    js_nanbox_pointer(obj as i64)
}

fn split_subject_alt_names(san: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in san.split(',') {
        let item = part.trim();
        if let Some(rest) = item.strip_prefix("DNS:") {
            out.push(("DNS".to_string(), rest.trim().to_string()));
        } else if let Some(rest) = item.strip_prefix("IP Address:") {
            out.push(("IP".to_string(), rest.trim().to_string()));
        } else if let Some(rest) = item.strip_prefix("IP:") {
            out.push(("IP".to_string(), rest.trim().to_string()));
        }
    }
    out
}

fn hostname_is_ip(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
}

fn dns_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if pattern == host {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        let Some(rest) = host.strip_suffix(suffix) else {
            return false;
        };
        return rest.ends_with('.') && rest[..rest.len().saturating_sub(1)].find('.').is_none();
    }
    false
}

unsafe fn cn_values(subject_value: f64) -> Vec<String> {
    let cn = object_field(subject_value, "CN");
    if js_is_undefined_or_null(cn) {
        return Vec::new();
    }
    if is_array_value(cn) {
        if let Some(addr) = pointer_addr(cn) {
            let arr = addr as *const perry_runtime::ArrayHeader;
            let len = js_array_length(arr);
            let mut out = Vec::new();
            for i in 0..len {
                if let Some(s) = value_to_string(js_array_get_f64(arr, i)) {
                    out.push(s);
                }
            }
            return out;
        }
    }
    value_to_string(cn).into_iter().collect()
}

unsafe fn make_altname_error(reason: String, host: &str, cert: f64) -> f64 {
    let message = format!("Hostname/IP does not match certificate's altnames: {reason}");
    let obj = js_object_alloc(perry_runtime::error::CLASS_ID_ERROR, 0);
    set_str_field(obj, "name", "Error");
    set_str_field(obj, "message", &message);
    set_str_field(obj, "code", "ERR_TLS_CERT_ALTNAME_INVALID");
    set_str_field(obj, "reason", &reason);
    set_str_field(obj, "host", host);
    set_field(obj, "cert", cert);
    js_nanbox_pointer(obj as i64)
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

fn tls_server_method_name(method: &str) -> bool {
    matches!(
        method,
        "listen"
            | "close"
            | "address"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "listenerCount"
            | "eventNames"
            | "setSecureContext"
            | "getTicketKeys"
            | "setTicketKeys"
            | "ref"
            | "unref"
    )
}

fn tls_socket_introspection_method_name(method: &str) -> bool {
    matches!(
        method,
        "getProtocol"
            | "getCipher"
            | "getPeerCertificate"
            | "getCertificate"
            | "getSession"
            | "isSessionReused"
            | "exportKeyingMaterial"
            | "setMaxSendFragment"
            | "ref"
            | "unref"
    )
}

fn tls_socket_server_method_name(method: &str) -> bool {
    tls_socket_introspection_method_name(method)
        || matches!(method, |"write"| "end"
            | "destroy"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "listenerCount"
            | "eventNames")
}

pub fn should_dispatch_tls_handle(handle: i64, method: &str) -> bool {
    if is_tls_server_handle(handle) {
        return tls_server_method_name(method);
    }
    sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|socket| {
            if socket.server_side {
                tls_socket_server_method_name(method)
            } else {
                tls_socket_introspection_method_name(method)
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
                let cb = callback_bits(args, 1);
                js_tls_server_listen(handle, port, cb);
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
            "listen" | "close" | "address" | "on" | "addListener" | "once" | "off"
            | "removeListener" | "removeAllListeners" | "listenerCount" | "eventNames"
            | "setSecureContext" | "getTicketKeys" | "setTicketKeys" | "ref" | "unref" => {
                return Some(perry_runtime::object::js_class_method_bind(
                    raw_handle_value(handle),
                    property.as_ptr(),
                    property.len(),
                ));
            }
            _ => {}
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
            _ if sockets()
                .lock()
                .unwrap()
                .get(&handle)
                .map(|socket| {
                    if socket.server_side {
                        tls_socket_server_method_name(property)
                    } else {
                        tls_socket_introspection_method_name(property)
                    }
                })
                .unwrap_or(false) =>
            {
                return Some(perry_runtime::object::js_class_method_bind(
                    raw_handle_value(handle),
                    property.as_ptr(),
                    property.len(),
                ));
            }
            _ => {}
        }
    }
    None
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

pub unsafe extern "C" fn js_tls_get_ciphers() -> *mut perry_runtime::ArrayHeader {
    static_string_array(NODE_TLS_CIPHERS)
}

pub unsafe extern "C" fn js_tls_root_certificates() -> *mut perry_runtime::ArrayHeader {
    string_array(root_certificates())
}

pub unsafe extern "C" fn js_tls_get_ca_certificates(
    type_bits: i64,
) -> *mut perry_runtime::ArrayHeader {
    let value = f64_from_raw_bits(type_bits);
    let kind = if js_is_undefined_or_null(value) {
        "default".to_string()
    } else {
        let jsv = JSValue::from_bits(value.to_bits());
        if !jsv.is_any_string() {
            throw_type_error(
                &format!(
                    "The \"type\" argument must be of type string. Received type {}",
                    type_name(value)
                ),
                "ERR_INVALID_ARG_TYPE",
            );
        }
        value_to_string(value).unwrap_or_default()
    };

    match kind.as_str() {
        "default" => {
            if let Some(certs) = ca_store().lock().unwrap().clone() {
                string_array(&certs)
            } else {
                string_array(root_certificates())
            }
        }
        "system" | "bundled" => string_array(root_certificates()),
        "extra" => string_array(&[]),
        _ => throw_type_error(
            &format!("The argument 'type' is invalid. Received {kind:?}"),
            "ERR_INVALID_ARG_VALUE",
        ),
    }
}

pub unsafe extern "C" fn js_tls_set_default_ca_certificates(certs_bits: i64) -> f64 {
    let value = f64_from_raw_bits(certs_bits);
    if !is_array_value(value) {
        throw_type_error(
            &format!(
                "The \"certs\" argument must be an instance of Array. Received type {}",
                type_name(value)
            ),
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let certs = cert_list_from_array_value(value).unwrap_or_else(|_| {
        throw_type_error(
            "The \"certs\" argument must contain strings or Buffer-like values",
            "ERR_INVALID_ARG_TYPE",
        )
    });
    validate_ca_list_for_set(&certs);
    *ca_store().lock().unwrap() = Some(certs);
    undefined()
}

pub unsafe extern "C" fn js_tls_check_server_identity(hostname_bits: i64, cert_bits: i64) -> f64 {
    let hostname_value = f64_from_raw_bits(hostname_bits);
    let cert = f64_from_raw_bits(cert_bits);
    let host = value_to_string(hostname_value).unwrap_or_default();
    let host_is_ip = hostname_is_ip(&host);
    let san = object_field_string(cert, "subjectaltname").unwrap_or_default();
    let san_entries = split_subject_alt_names(&san);
    let ip_names: Vec<String> = san_entries
        .iter()
        .filter(|(kind, _)| kind == "IP")
        .map(|(_, value)| value.clone())
        .collect();
    let dns_names: Vec<String> = san_entries
        .iter()
        .filter(|(kind, _)| kind == "DNS")
        .map(|(_, value)| value.clone())
        .collect();

    if host_is_ip {
        if ip_names.iter().any(|ip| ip == &host) {
            return undefined();
        }
        let reason = format!(
            "IP: {host} is not in the cert's list: {}",
            ip_names.join(", ")
        );
        return make_altname_error(reason, &host, cert);
    }

    if !dns_names.is_empty() {
        if dns_names.iter().any(|pattern| dns_matches(pattern, &host)) {
            return undefined();
        }
        let reason = format!("Host: {host}. is not in the cert's altnames: {san}");
        return make_altname_error(reason, &host, cert);
    }

    let subject = object_field(cert, "subject");
    let cns = cn_values(subject);
    if cns.iter().any(|cn| dns_matches(cn, &host)) {
        return undefined();
    }
    let reason = if cns.is_empty() {
        format!("Host: {host}. is not cert's CN: ")
    } else {
        format!("Host: {host}. is not cert's CN: {}", cns.join(","))
    };
    make_altname_error(reason, &host, cert)
}

pub unsafe extern "C" fn js_tls_create_secure_context(options_bits: i64) -> f64 {
    make_secure_context(f64_from_raw_bits(options_bits))
}

pub unsafe extern "C" fn js_tls_secure_context_constructor(options_bits: i64) -> f64 {
    make_secure_context(f64_from_raw_bits(options_bits))
}

pub unsafe extern "C" fn js_tls_native_dispatch(
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let method =
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("");
    let arg = |idx: usize| -> f64 {
        if idx < args_len && !args_ptr.is_null() {
            *args_ptr.add(idx)
        } else {
            undefined()
        }
    };
    match method {
        "getCiphers" => js_nanbox_pointer(js_tls_get_ciphers() as i64),
        "rootCertificates" => js_nanbox_pointer(js_tls_root_certificates() as i64),
        "getCACertificates" => {
            js_nanbox_pointer(js_tls_get_ca_certificates(arg(0).to_bits() as i64) as i64)
        }
        "setDefaultCACertificates" => js_tls_set_default_ca_certificates(arg(0).to_bits() as i64),
        "checkServerIdentity" => {
            js_tls_check_server_identity(arg(0).to_bits() as i64, arg(1).to_bits() as i64)
        }
        "connect" => {
            // Pass the args through raw — js_tls_connect resolves Node's
            // `connect(options[, cb])` / `connect(port[, host][, options][,
            // cb])` overloads plus the legacy positional form itself (#4971).
            let handle = crate::net::js_tls_connect(arg(0), arg(1), arg(2), arg(3));
            if handle == 0 {
                // Unresolvable args (e.g. no port) — undefined beats a
                // NaN-boxed null pointer that every later method call
                // trips over (#4971).
                return undefined();
            }
            record_tls_client_handle(handle);
            nanbox_handle(handle)
        }
        "createServer" | "Server" => nanbox_handle(js_tls_create_server(
            arg(0).to_bits() as i64,
            arg(1).to_bits() as i64,
        )),
        "TLSSocket" => nanbox_handle(js_tls_tlssocket_constructor(
            arg(0).to_bits() as i64,
            arg(1).to_bits() as i64,
        )),
        "createSecureContext" | "SecureContext" => {
            js_tls_create_secure_context(arg(0).to_bits() as i64)
        }
        "$DEFAULT_CIPHERS" => nanbox_str(DEFAULT_CIPHERS),
        _ => f64::from_bits(TLS_DISPATCH_MISSING_BITS),
    }
}
