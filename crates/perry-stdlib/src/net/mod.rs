//! Raw TCP socket module — Node-compatible `net.Socket` surface with
//! TLS upgrade support (A2).
//!
//! Event-driven, async over tokio, mirroring the proven pattern in `ws.rs`:
//! one tokio task per socket reads in a `select!` loop and drives an mpsc
//! command channel for writes/end/destroy/upgrade. Read data is queued as
//! raw `Vec<u8>` into `NET_PENDING_EVENTS` and converted to `Buffer` on the
//! main thread inside `js_net_process_pending` — see the arena-safety rule
//! in `common/async_bridge.rs`.
//!
//! The `Transport` enum lets a single socket id keep the same handle across
//! a plain→TLS upgrade: `SocketCommand::UpgradeTls` moves the `TcpStream`
//! into `tokio_rustls::connect()`, then stores the resulting `TlsStream`
//! back under the same id. This is what Postgres' `SSLRequest` flow needs —
//! write 8 bytes in plain, read one byte (`'S'`/`'N'`), then upgrade.
//!
//! FFI signature conventions (match NATIVE_MODULE_TABLE in perry-codegen):
//! - Receiver handles and `NA_PTR` args arrive as `i64` (codegen calls
//!   `unbox_to_i64` on the NaN-boxed value before the FFI invocation).
//! - `NA_STR` args arrive as `i64` StringHeader pointers (pre-unboxed via
//!   `js_get_string_pointer_unified`).
//! - `NA_F64` args arrive as `f64`.
//! - `NR_PTR` return is `i64` and the codegen NaN-boxes with POINTER_TAG;
//!   `NR_VOID` returns nothing and the codegen substitutes `undefined`.

use perry_runtime::buffer::{js_buffer_alloc, BufferHeader};
use perry_runtime::{js_closure_call0, js_closure_call1, ClosureHeader, JSValue, StringHeader};
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};

use crate::common::async_bridge::spawn;

#[cfg(feature = "tls")]
use std::sync::Arc;
#[cfg(feature = "tls")]
use tokio_rustls::{client::TlsStream, rustls, TlsConnector};

// ─── Transport enum (plain or TLS, swappable at runtime) ─────────────────────

enum Transport {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Transport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => Pin::new(&mut **s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Transport {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => Pin::new(&mut **s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => Pin::new(&mut **s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "tls")]
            Transport::Tls(s) => Pin::new(&mut **s).poll_shutdown(cx),
        }
    }
}

// ─── Handle storage ──────────────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref NET_SOCKETS: Mutex<HashMap<i64, SocketState>> = Mutex::new(HashMap::new());
    static ref NET_LISTENERS: Mutex<HashMap<i64, HashMap<String, Vec<i64>>>> = Mutex::new(HashMap::new());
    static ref NET_PENDING_EVENTS: Mutex<Vec<PendingNetEvent>> = Mutex::new(Vec::new());
    static ref NEXT_NET_ID: Mutex<i64> = Mutex::new(1);
}

static NET_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the net GC root scanner exactly once. Safe to call from any
/// `js_net_*` entry point on the main thread. Mirrors the pattern in
/// `cron.rs::ensure_gc_scanner_registered`.
fn ensure_gc_scanner_registered() {
    NET_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named("stdlib:net", scan_net_roots_mut);
    });
}

/// GC root scanner for net.Socket event listener closures.
///
/// Socket event listeners (`sock.on('data', cb)` etc.) are closures that
/// may be garbage-collectible from the user's perspective after the call
/// to `.on()` returns — the closure literal is only referenced by the
/// native-side `NET_LISTENERS` map. Without this scanner, any GC cycle
/// between `.on()` and the next dispatch would sweep the closure; the
/// next `js_closure_call1` would dereference freed memory. This was a
/// latent bug until v0.5.25 made GC fire during synchronous decode
/// loops (issue #35).
#[allow(dead_code)]
fn scan_net_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(mark);
    scan_net_roots_mut(&mut visitor);
}

fn scan_net_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut listeners) = NET_LISTENERS.lock() {
        for per_socket in listeners.values_mut() {
            for cb_vec in per_socket.values_mut() {
                for cb in cb_vec.iter_mut() {
                    visitor.visit_i64_slot(cb);
                }
            }
        }
    }
}

struct SocketState {
    cmd_tx: mpsc::UnboundedSender<SocketCommand>,
    /// `Some` only between `js_net_socket_alloc` and the first
    /// `js_net_socket_method_connect` — held here so the deferred connect
    /// path (issue #422: `new net.Socket()` then `sock.connect(port, host)`)
    /// can move it into the spawned tokio task at connect time. Stays
    /// `None` for the eager factory paths (`createConnection` / `tls.connect`)
    /// where the rx flows straight into the task.
    pending_rx: Option<mpsc::UnboundedReceiver<SocketCommand>>,
    is_open: bool,
    type_of_service: u8,
}

enum SocketCommand {
    Write(Vec<u8>),
    End,
    Destroy,
    #[cfg(feature = "tls")]
    UpgradeTls {
        servername: String,
        verify: bool,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

enum PendingNetEvent {
    Connect(i64),
    Data(i64, Vec<u8>),
    Close(i64),
    Error(i64, String),
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

unsafe fn string_from_header_i64(ptr: i64) -> Option<String> {
    let p = ptr as usize;
    if p < 0x1000 {
        return None;
    }
    let hdr = ptr as *const StringHeader;
    let len = (*hdr).byte_len as usize;
    let data_ptr = (hdr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Issue #770 — true iff `val_f64` carries `POINTER_TAG` (0x7FFD), i.e.
/// it's a real heap-pointer NaN-box (object or closure). Plain `f64`
/// ports like `80.0` never reach this band, and `undefined` / `null`
/// land in `0x7FFC` so they're cleanly rejected — which matters
/// because the dispatch table pads missing user args with
/// `TAG_UNDEFINED`.
fn is_nanboxed_pointer(val_f64: f64) -> bool {
    (val_f64.to_bits() >> 48) == 0x7FFD
}

unsafe fn unbox_pointer(val_f64: f64) -> *mut u8 {
    let bits = val_f64.to_bits();
    (bits & 0x0000_FFFF_FFFF_FFFF) as *mut u8
}

/// Issue #1131 — read a NaN-boxed JS value as the raw bytes for
/// `socket.write(chunk)`. Mirror of perry-ext-net's
/// `jsvalue_to_socket_bytes` (the live path for `node:net` imports is
/// the perry-ext-net copy after the well-known flip; this bundled-net
/// copy stays in sync so the HANDLE_METHOD_DISPATCH fallback through
/// `dispatch_net_socket` is correct too). A JS string is a 20-byte
/// `StringHeader`; a Buffer is an 8-byte `BufferHeader` — reading one
/// through the other's layout (the pre-#1131 unconditional
/// `*BufferHeader` cast) emits garbage. Probe `BUFFER_REGISTRY` first.
unsafe fn jsvalue_to_socket_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JSValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    if v.is_string() {
        let ptr = unbox_pointer(value) as *const StringHeader;
        if ptr.is_null() {
            return None;
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        return Some(std::slice::from_raw_parts(data, len).to_vec());
    }
    if v.is_pointer() {
        let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
        if perry_runtime::buffer::js_buffer_is_buffer(raw) != 0 {
            let buf = raw as *const BufferHeader;
            if !buf.is_null() {
                let len = (*buf).length as usize;
                let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
                return Some(std::slice::from_raw_parts(data, len).to_vec());
            }
        }
        let sptr = raw as *const StringHeader;
        if !sptr.is_null() {
            let len = (*sptr).byte_len as usize;
            if len <= (1 << 30) {
                let data = (sptr as *const u8).add(std::mem::size_of::<StringHeader>());
                return Some(std::slice::from_raw_parts(data, len).to_vec());
            }
        }
        return None;
    }
    if v.is_number() {
        return Some(v.to_number().to_string().into_bytes());
    }
    if v.is_bool() {
        return Some(
            if v.to_bool() { "true" } else { "false" }
                .to_string()
                .into_bytes(),
        );
    }
    None
}

unsafe fn get_object_string_field(obj_f64: f64, field_name: &str) -> Option<String> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const perry_runtime::ObjectHeader;
    if obj_ptr.is_null() {
        return None;
    }
    let key = perry_runtime::js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key);
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_string() {
        return string_from_header_i64(val.as_string_ptr() as i64);
    }
    if val.is_number() {
        return Some(format!("{}", val.as_number() as i64));
    }
    None
}

unsafe fn get_object_number_field(obj_f64: f64, field_name: &str) -> Option<f64> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const perry_runtime::ObjectHeader;
    if obj_ptr.is_null() {
        return None;
    }
    let key = perry_runtime::js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key);
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_number() {
        return Some(val.as_number());
    }
    if val.is_string() {
        if let Some(s) = string_from_header_i64(val.as_string_ptr() as i64) {
            if let Ok(n) = s.parse::<f64>() {
                return Some(n);
            }
        }
    }
    None
}

/// Read a boolean option off a NaN-boxed JS object. Accepts real
/// booleans plus numbers (`rejectUnauthorized: 0` shows up in npm
/// code). `None` when the field is absent/undefined/null. #4971.
unsafe fn get_object_bool_field(obj_f64: f64, field_name: &str) -> Option<bool> {
    if !is_nanboxed_pointer(obj_f64) {
        return None;
    }
    let obj_ptr = unbox_pointer(obj_f64) as *const perry_runtime::ObjectHeader;
    if obj_ptr.is_null() {
        return None;
    }
    let key = perry_runtime::js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let val = perry_runtime::js_object_get_field_by_name(obj_ptr, key);
    if val.is_undefined() || val.is_null() {
        return None;
    }
    if val.is_bool() {
        return Some(val.to_bool());
    }
    if val.is_number() {
        return Some(val.as_number() != 0.0);
    }
    None
}

/// Issue #770 — build an `Error`-shaped object `{ message: msg }` so
/// `socket.on('error', err => err.message)` works. Returns a NaN-boxed
/// f64 pointing at the object, falling back to a bare string on alloc
/// failure. Packed-keys format (NUL-delimited names + hash shape id)
/// mirrors `crates/perry-stdlib/src/sqlite.rs::build_packed_keys`.
unsafe fn build_error_object(msg: &str) -> f64 {
    use perry_runtime::JSValue;
    let name = b"message";
    let packed: Vec<u8> = name.to_vec();
    let mut shape_id: u32 = 0x4E45_0000; // "NE" — net error
    for &b in name {
        shape_id = shape_id.wrapping_mul(31).wrapping_add(b as u32);
    }
    shape_id = shape_id.wrapping_add(1);
    let s_msg = perry_runtime::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let obj = perry_runtime::js_object_alloc_with_shape(
        shape_id,
        1,
        packed.as_ptr(),
        packed.len() as u32,
    );
    if obj.is_null() {
        return f64::from_bits(0x7FFF_0000_0000_0000u64 | (s_msg as u64 & 0x0000_FFFF_FFFF_FFFF));
    }
    perry_runtime::js_object_set_field(obj, 0, JSValue::string_ptr(s_msg));
    let obj_bits = (obj as u64 & 0x0000_FFFF_FFFF_FFFF) | 0x7FFD_0000_0000_0000;
    f64::from_bits(obj_bits)
}

fn next_id() -> i64 {
    let mut g = NEXT_NET_ID.lock().unwrap();
    let id = *g;
    *g += 1;
    id
}

fn push_event(ev: PendingNetEvent) {
    NET_PENDING_EVENTS.lock().unwrap().push(ev);
    // Issue #84: wake the main thread so the event is dispatched on the
    // very next loop iteration instead of after the old 10 ms sleep.
    perry_runtime::event_pump::js_notify_main_thread();
}

fn mark_closed(id: i64) {
    if let Some(s) = NET_SOCKETS.lock().unwrap().get_mut(&id) {
        s.is_open = false;
    }
}

// ─── rustls config (TLS feature only) ────────────────────────────────────────

#[cfg(feature = "tls")]
fn build_tls_connector(verify: bool) -> Result<TlsConnector, String> {
    // rustls panics resolving the process-level CryptoProvider when both
    // `ring` and `aws-lc-rs` end up in the dep graph. Server paths install
    // one before their first handshake; a client-only program (no tls/https
    // server) reached `ClientConfig::builder()` with none installed once
    // #4971 made `tls.connect` actually resolve its host. Idempotent —
    // `install_default` errors (ignored) if a provider is already set.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    if !verify {
        return build_tls_connector_insecure();
    }
    // System trust store. Aligns with Perry's broader rustls-only stance
    // (reqwest / tokio-tungstenite / mongodb all use rustls) — no OpenSSL.
    let mut root_store = rustls::RootCertStore::empty();
    // rustls-native-certs 0.8 returns a CertificateResult with separate
    // `.certs` and `.errors` fields; we accept per-cert failures rather
    // than bail, matching the crate's own documented pattern.
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        let _ = root_store.add(cert);
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Insecure TLS — accept any server cert without verifying chain or hostname.
/// Maps to Postgres `sslmode=require` (encryption without auth) and is the
/// right default for local dev against self-signed certs. Real deployments
/// should pass `verify: true` (the default) so the system trust store and
/// hostname validation apply.
#[cfg(feature = "tls")]
fn build_tls_connector_insecure() -> Result<TlsConnector, String> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, SignatureScheme};

    #[derive(Debug)]
    struct NoVerify;

    impl ServerCertVerifier for NoVerify {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::ED25519,
            ]
        }
    }

    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

// ─── FFI: net.createConnection / net.connect ─────────────────────────────────

/// `net.createConnection(...)` / `net.connect(...)` — returns a handle
/// immediately; connection happens in the background and emits
/// `'connect'` or `'error'`.
///
/// Supports both Node overloads (issue #770):
///   - Positional: `net.connect(port, host, cb?)` — `arg1_f64` is the
///     port, `arg2_f64` is the host (NaN-boxed string), `arg3_f64` is
///     the optional connectListener.
///   - Options object: `net.connect({ host, port }, cb?)` — `arg1_f64`
///     is a NaN-boxed pointer to the options object; `arg2_f64` is the
///     optional connectListener; `arg3_f64` is unused (the dispatch
///     table pads it with `undefined`).
///
/// The `connectListener` is auto-registered as a `'connect'` listener
/// on the new socket handle, matching Node spec.
///
/// Signature matches NATIVE_MODULE_TABLE entry
/// `{ module: "net", method: "connect" | "createConnection", args: &[NA_F64, NA_F64, NA_F64], ret: NR_PTR }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_connect(arg1_f64: f64, arg2_f64: f64, arg3_f64: f64) -> i64 {
    fn register_connect_cb(handle: i64, cb_f64: f64) {
        if handle == 0 || !is_nanboxed_pointer(cb_f64) {
            return;
        }
        let cb_ptr = unsafe { unbox_pointer(cb_f64) } as i64;
        if cb_ptr == 0 {
            return;
        }
        let mut listeners = NET_LISTENERS.lock().unwrap();
        listeners
            .entry(handle)
            .or_default()
            .entry("connect".to_string())
            .or_default()
            .push(cb_ptr);
    }

    if is_nanboxed_pointer(arg1_f64) {
        let host = match get_object_string_field(arg1_f64, "host")
            .or_else(|| get_object_string_field(arg1_f64, "hostname"))
        {
            Some(h) if !h.is_empty() => h,
            _ => "localhost".to_string(),
        };
        let port = match get_object_number_field(arg1_f64, "port") {
            Some(p) => {
                perry_runtime::net_validate::js_net_validate_connect_port(p);
                p as u16
            }
            None => return 0,
        };
        let handle = spawn_socket_task(host, port, /* direct_tls: */ None);
        register_connect_cb(handle, arg2_f64);
        return handle;
    }
    // Positional form: arg2 is a NaN-boxed string, arg3 is the cb.
    perry_runtime::net_validate::js_net_validate_connect_port(arg1_f64);
    let host_ptr = perry_runtime::js_get_string_pointer_unified(arg2_f64);
    let host = match string_from_header_i64(host_ptr) {
        Some(h) => h,
        None => return 0,
    };
    let port = arg1_f64 as u16;
    let handle = spawn_socket_task(host, port, /* direct_tls: */ None);
    register_connect_cb(handle, arg3_f64);
    handle
}

// ─── FFI: new net.Socket() (alloc-only, deferred connect) ────────────────────

/// `new net.Socket()` — allocates an unconnected socket handle. The TCP
/// connection is deferred until `js_net_socket_method_connect` is called
/// (`sock.connect(port, host)`).
///
/// Pre-issue-#422 the only path into the net module was the eager
/// `net.createConnection(port, host)` factory, which both allocates the
/// handle AND kicks off the connect in one shot. Real-world TS code
/// (including pure-TS Postgres / MySQL / MQTT drivers) commonly takes
/// the `new net.Socket()` + later `.connect(...)` shape, where listener
/// registration sits between the two — that pattern needs a separate
/// allocator.
///
/// Signature matches NATIVE_MODULE_TABLE entry
/// `{ module: "net", method: "Socket", args: &[], ret: NR_PTR }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_alloc() -> i64 {
    ensure_gc_scanner_registered();
    let id = next_id();
    let (tx, rx) = mpsc::unbounded_channel::<SocketCommand>();
    NET_SOCKETS.lock().unwrap().insert(
        id,
        SocketState {
            cmd_tx: tx,
            pending_rx: Some(rx),
            is_open: false,
            type_of_service: 0,
        },
    );
    NET_LISTENERS.lock().unwrap().insert(id, HashMap::new());
    id
}

// ─── FFI: socket.connect(port, host) (instance method on existing handle) ─────

/// `socket.connect(port, host)` — initiates a TCP connection on a socket
/// previously allocated by `new net.Socket()`. Spawns the same tokio task
/// shape as `js_net_socket_connect`, but pulls its receiver out of the
/// `SocketState::pending_rx` slot rather than allocating a fresh channel,
/// so any listener already registered (`sock.on('data', cb)` etc.) sees
/// the same handle id once the connect completes.
///
/// If `pending_rx` is already empty (already connected, or unknown handle)
/// this pushes an `'error'` event rather than silently dropping — matches
/// Node's behavior where calling `.connect()` twice on the same socket
/// emits `Error: already connected`.
///
/// Signature matches NATIVE_MODULE_TABLE entry
/// `{ has_receiver: true, method: "connect", class_filter: Some("Socket"),
///    args: &[NA_F64, NA_STR], ret: NR_VOID }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_method_connect(handle: i64, port: f64, host_ptr: i64) {
    perry_runtime::net_validate::js_net_validate_connect_port(port);
    let host = match string_from_header_i64(host_ptr) {
        Some(h) => h,
        None => {
            push_event(PendingNetEvent::Error(
                handle,
                "socket.connect: invalid host string".to_string(),
            ));
            return;
        }
    };
    let port = port as u16;

    // Move the deferred-connect rx out of the SocketState. After this
    // take, subsequent .connect() calls land in the `None` arm below.
    let mut rx = {
        let mut guard = NET_SOCKETS.lock().unwrap();
        match guard.get_mut(&handle).and_then(|s| s.pending_rx.take()) {
            Some(rx) => rx,
            None => {
                push_event(PendingNetEvent::Error(
                    handle,
                    "socket already connected (or unknown handle)".to_string(),
                ));
                return;
            }
        }
    };

    spawn(async move {
        let addr = format!("{}:{}", host, port);
        let tcp = match TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                push_event(PendingNetEvent::Error(handle, format!("{}", e)));
                push_event(PendingNetEvent::Close(handle));
                mark_closed(handle);
                return;
            }
        };

        if let Some(s) = NET_SOCKETS.lock().unwrap().get_mut(&handle) {
            s.is_open = true;
        }
        push_event(PendingNetEvent::Connect(handle));

        run_socket_task(handle, Transport::Plain(tcp), &mut rx).await;
    });
}

// ─── FFI: tls.connect ────────────────────────────────────────────────────────

/// `tls.connect(...)` — opens a plain TCP socket and immediately runs the
/// TLS handshake before firing `'connect'`/`'secureConnect'`. Use this for
/// protocols that start TLS from byte 0 (HTTPS, SMTP with SMTPS, etc.).
///
/// For protocols that negotiate TLS mid-stream (Postgres' `SSLRequest`,
/// SMTP STARTTLS), use `net.createConnection` then `socket.upgradeToTLS`
/// instead.
///
/// Resolves Node's overloads plus Perry's legacy positional form — kept in
/// sync with the perry-ext-net copy, which is the live path after the
/// well-known flip (#4971):
///
/// - `tls.connect(options[, callback])` — `port` required; `host`/`hostname`
///   default `"localhost"`; `servername` defaults to the host;
///   `rejectUnauthorized: false` disables cert verification.
/// - `tls.connect(port[, host][, options][, callback])`
/// - Legacy Perry positional: `tls.connect(host, port, servername?, verify?)`.
///
/// Signature matches `{ module: "tls", method: "connect",
/// args: &[NA_F64, NA_F64, NA_F64, NA_F64], ret: NR_PTR }`.
#[cfg(feature = "tls")]
#[no_mangle]
pub unsafe extern "C" fn js_tls_connect(arg1: f64, arg2: f64, arg3: f64, arg4: f64) -> i64 {
    extern "C" {
        fn js_value_is_closure(value_bits: i64) -> i32;
    }
    let is_closure =
        |v: f64| is_nanboxed_pointer(v) && js_value_is_closure(v.to_bits() as i64) != 0;
    let as_string = |v: f64| -> Option<String> {
        if !JSValue::from_bits(v.to_bits()).is_string() {
            return None;
        }
        string_from_header_i64(perry_runtime::js_get_string_pointer_unified(v))
    };
    // Cert verification only goes off when the caller says so explicitly —
    // a missing/undefined flag keeps it on.
    let explicitly_off = |v: f64| -> bool {
        let j = JSValue::from_bits(v.to_bits());
        (j.is_bool() && !j.to_bool()) || (j.is_number() && j.as_number() == 0.0)
    };

    let (host, port, servername, verify, cb_f64);
    if let Some(h) = as_string(arg1) {
        // Legacy Perry positional: (host, port, servername?, verify?).
        let p = JSValue::from_bits(arg2.to_bits());
        if !p.is_number() {
            return 0;
        }
        port = p.as_number() as u16;
        servername = as_string(arg3).unwrap_or_else(|| h.clone());
        host = h;
        verify = !explicitly_off(arg4);
        cb_f64 = None;
    } else if is_nanboxed_pointer(arg1) && !is_closure(arg1) {
        // Node options form: tls.connect(options[, callback]).
        port = match get_object_number_field(arg1, "port") {
            Some(p) => {
                perry_runtime::net_validate::js_net_validate_connect_port(p);
                p as u16
            }
            None => return 0,
        };
        host = match get_object_string_field(arg1, "host")
            .or_else(|| get_object_string_field(arg1, "hostname"))
        {
            Some(h) if !h.is_empty() => h,
            _ => "localhost".to_string(),
        };
        servername = get_object_string_field(arg1, "servername").unwrap_or_else(|| host.clone());
        verify = get_object_bool_field(arg1, "rejectUnauthorized").unwrap_or(true);
        cb_f64 = if is_closure(arg2) { Some(arg2) } else { None };
    } else if JSValue::from_bits(arg1.to_bits()).is_number() {
        // Node positional form: tls.connect(port[, host][, options][, cb]).
        perry_runtime::net_validate::js_net_validate_connect_port(arg1);
        port = arg1 as u16;
        let mut opt_host: Option<String> = None;
        let mut opts: Option<f64> = None;
        let mut cb: Option<f64> = None;
        for v in [arg2, arg3, arg4] {
            if opt_host.is_none() {
                if let Some(h) = as_string(v) {
                    opt_host = Some(h);
                    continue;
                }
            }
            if is_closure(v) {
                cb = cb.or(Some(v));
            } else if is_nanboxed_pointer(v) {
                opts = opts.or(Some(v));
            }
        }
        host = opt_host
            .or_else(|| {
                opts.and_then(|o| {
                    get_object_string_field(o, "host")
                        .or_else(|| get_object_string_field(o, "hostname"))
                })
            })
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "localhost".to_string());
        servername = opts
            .and_then(|o| get_object_string_field(o, "servername"))
            .unwrap_or_else(|| host.clone());
        verify = opts
            .and_then(|o| get_object_bool_field(o, "rejectUnauthorized"))
            .unwrap_or(true);
        cb_f64 = cb;
    } else {
        return 0;
    }

    let handle = spawn_socket_task(host, port, Some((servername, verify)));
    crate::tls::record_tls_client_handle(handle);
    if let Some(cb) = cb_f64 {
        if handle != 0 {
            let cb_ptr = unbox_pointer(cb) as i64;
            if cb_ptr != 0 {
                NET_LISTENERS
                    .lock()
                    .unwrap()
                    .entry(handle)
                    .or_default()
                    .entry("secureConnect".to_string())
                    .or_default()
                    .push(cb_ptr);
            }
        }
    }
    handle
}

/// Internal: allocate the handle, spawn the tokio task.
/// `direct_tls` = Some((servername, verify)) runs a TLS handshake before
/// firing 'connect'; None keeps the socket in plain TCP mode.
fn spawn_socket_task(host: String, port: u16, direct_tls: Option<(String, bool)>) -> i64 {
    ensure_gc_scanner_registered();
    let id = next_id();
    let (tx, mut rx) = mpsc::unbounded_channel::<SocketCommand>();

    NET_SOCKETS.lock().unwrap().insert(
        id,
        SocketState {
            cmd_tx: tx,
            pending_rx: None,
            is_open: false,
            type_of_service: 0,
        },
    );
    NET_LISTENERS.lock().unwrap().insert(id, HashMap::new());

    spawn(async move {
        let addr = format!("{}:{}", host, port);
        let tcp = match TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                push_event(PendingNetEvent::Error(id, format!("{}", e)));
                push_event(PendingNetEvent::Close(id));
                mark_closed(id);
                return;
            }
        };

        // Direct-TLS path: run the TLS handshake before signalling connect.
        let transport = match direct_tls {
            #[cfg(feature = "tls")]
            Some((servername, verify)) => match do_tls_handshake(tcp, &servername, verify).await {
                Ok(tls) => Transport::Tls(Box::new(tls)),
                Err(e) => {
                    push_event(PendingNetEvent::Error(id, e));
                    push_event(PendingNetEvent::Close(id));
                    mark_closed(id);
                    return;
                }
            },
            #[cfg(not(feature = "tls"))]
            Some(_) => {
                push_event(PendingNetEvent::Error(
                    id,
                    "tls feature not compiled in".to_string(),
                ));
                push_event(PendingNetEvent::Close(id));
                mark_closed(id);
                return;
            }
            None => Transport::Plain(tcp),
        };

        if let Some(s) = NET_SOCKETS.lock().unwrap().get_mut(&id) {
            s.is_open = true;
        }
        push_event(PendingNetEvent::Connect(id));

        run_socket_task(id, transport, &mut rx).await;
    });

    id
}

#[cfg(feature = "tls")]
async fn do_tls_handshake(
    tcp: TcpStream,
    servername: &str,
    verify: bool,
) -> Result<TlsStream<TcpStream>, String> {
    let connector = build_tls_connector(verify)?;
    let server_name = rustls::pki_types::ServerName::try_from(servername.to_string())
        .map_err(|e| format!("invalid servername '{}': {}", servername, e))?;
    connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| format!("tls handshake: {}", e))
}

/// The read/write/command loop. Shared by plain-TCP and direct-TLS paths.
async fn run_socket_task(
    id: i64,
    initial_transport: Transport,
    rx: &mut mpsc::UnboundedReceiver<SocketCommand>,
) {
    let mut transport: Option<Transport> = Some(initial_transport);
    let mut buf = vec![0u8; 16 * 1024];

    loop {
        let t = match transport.as_mut() {
            Some(t) => t,
            None => break, // transport taken and not restored → end task
        };

        tokio::select! {
            read_result = t.read(&mut buf) => {
                match read_result {
                    Ok(0) => {
                        push_event(PendingNetEvent::Close(id));
                        mark_closed(id);
                        break;
                    }
                    Ok(n) => {
                        push_event(PendingNetEvent::Data(id, buf[..n].to_vec()));
                    }
                    Err(e) => {
                        push_event(PendingNetEvent::Error(id, format!("{}", e)));
                        push_event(PendingNetEvent::Close(id));
                        mark_closed(id);
                        break;
                    }
                }
            }
            cmd = rx.recv() => {
                match cmd {
                    Some(SocketCommand::Write(bytes)) => {
                        if let Err(e) = t.write_all(&bytes).await {
                            push_event(PendingNetEvent::Error(id, format!("{}", e)));
                            push_event(PendingNetEvent::Close(id));
                            mark_closed(id);
                            break;
                        }
                    }
                    Some(SocketCommand::End) => {
                        let _ = t.shutdown().await;
                    }
                    Some(SocketCommand::Destroy) | None => {
                        push_event(PendingNetEvent::Close(id));
                        mark_closed(id);
                        break;
                    }
                    #[cfg(feature = "tls")]
                    Some(SocketCommand::UpgradeTls { servername, verify, reply }) => {
                        // Take the plain TcpStream out of the enum, run the
                        // handshake, and put a TlsStream back under the same id.
                        // Done inline (blocks reads until handshake completes),
                        // which is what the Postgres SSLRequest flow expects.
                        let old = transport.take();
                        match old {
                            Some(Transport::Plain(tcp)) => {
                                match do_tls_handshake(tcp, &servername, verify).await {
                                    Ok(tls) => {
                                        transport = Some(Transport::Tls(Box::new(tls)));
                                        crate::tls::record_tls_client_handle(id);
                                        let _ = reply.send(Ok(()));
                                    }
                                    Err(e) => {
                                        let _ = reply.send(Err(e.clone()));
                                        push_event(PendingNetEvent::Error(id, e));
                                        push_event(PendingNetEvent::Close(id));
                                        mark_closed(id);
                                        break;
                                    }
                                }
                            }
                            Some(already_tls @ Transport::Tls(_)) => {
                                transport = Some(already_tls);
                                let _ = reply.send(Err("socket is already TLS".to_string()));
                            }
                            None => {
                                let _ = reply.send(Err("socket closed".to_string()));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

// ─── FFI: socket.write(buf) ──────────────────────────────────────────────────

/// `socket.write(chunk)` — enqueues bytes for the writer task.
/// Issue #1131 — `chunk_bits` is the full NaN-boxed JS value (codegen
/// passes `NA_JSV`; the dispatch shim passes `args[0].to_bits()`), not
/// a pre-stripped `BufferHeader` pointer. `jsvalue_to_socket_bytes`
/// probes Buffer-vs-string-vs-number and reads the correct layout so
/// `socket.write("ping")` sends the UTF-8 bytes instead of garbage.
/// Signature matches `{ has_receiver: true, method: "write", args: &[NA_JSV], ret: NR_VOID }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_write(handle: i64, chunk_bits: i64) {
    let bytes = match jsvalue_to_socket_bytes(f64::from_bits(chunk_bits as u64)) {
        Some(b) => b,
        None => return,
    };

    let sockets = NET_SOCKETS.lock().unwrap();
    if let Some(s) = sockets.get(&handle) {
        let _ = s.cmd_tx.send(SocketCommand::Write(bytes));
    }
}

// ─── FFI: socket.end([data]) ─────────────────────────────────────────────────

/// `socket.end([data])` — optionally write a final chunk, then graceful
/// shutdown: stops further writes, lets reads drain.
///
/// Issue #1852 — Node's `socket.end(data)` writes `data` then sends FIN.
/// `chunk_bits` is the full NaN-boxed JS value (NA_JSV); `undefined`/`null`
/// (the no-arg `socket.end()` form) yields no bytes. Kept in sync with the
/// live perry-ext-net copy so the `js_net_socket_end` symbol has one
/// signature regardless of which archive links.
/// Signature matches `{ has_receiver: true, method: "end", args: &[NA_JSV], ret: NR_VOID }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_end(handle: i64, chunk_bits: i64) {
    let sockets = NET_SOCKETS.lock().unwrap();
    if let Some(s) = sockets.get(&handle) {
        if let Some(bytes) = jsvalue_to_socket_bytes(f64::from_bits(chunk_bits as u64)) {
            if !bytes.is_empty() {
                let _ = s.cmd_tx.send(SocketCommand::Write(bytes));
            }
        }
        let _ = s.cmd_tx.send(SocketCommand::End);
    }
}

// ─── FFI: socket.destroy() ───────────────────────────────────────────────────

/// `socket.destroy()` — hard close, fires `'close'`.
/// Signature matches `{ has_receiver: true, method: "destroy", args: &[], ret: NR_VOID }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_destroy(handle: i64) {
    let sockets = NET_SOCKETS.lock().unwrap();
    if let Some(s) = sockets.get(&handle) {
        let _ = s.cmd_tx.send(SocketCommand::Destroy);
    }
}

#[no_mangle]
pub extern "C" fn js_net_socket_get_type_of_service(handle: i64) -> f64 {
    NET_SOCKETS
        .lock()
        .ok()
        .and_then(|sockets| sockets.get(&handle).map(|s| s.type_of_service as f64))
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_net_socket_set_type_of_service(handle: i64, tos: f64) -> i64 {
    let tos = perry_runtime::net_validate::js_net_validate_tos(tos) as u8;
    if let Some(s) = NET_SOCKETS.lock().unwrap().get_mut(&handle) {
        s.type_of_service = tos;
    }
    handle
}

// ─── FFI: socket.on(event, callback) ─────────────────────────────────────────

/// `socket.on(event, cb)` — registers a listener. Closures are stored as
/// raw `i64` pointers and invoked from `js_net_process_pending` on the
/// main thread.
///
/// Signature matches `{ has_receiver: true, method: "on", args: &[NA_STR, NA_PTR], ret: NR_VOID }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_on(handle: i64, event_ptr: i64, cb: i64) {
    ensure_gc_scanner_registered();
    let event = match string_from_header_i64(event_ptr) {
        Some(e) => e,
        None => return,
    };
    let mut listeners = NET_LISTENERS.lock().unwrap();
    let entry = listeners.entry(handle).or_default();
    entry.entry(event).or_default().push(cb);
}

// ─── FFI: socket.upgradeToTLS(servername) -> Promise ─────────────────────────

/// `socket.upgradeToTLS(servername)` — sends an UpgradeTls command to the
/// socket's task and returns a Promise that resolves when the TLS handshake
/// completes (or rejects on failure).
///
/// This is the Postgres-style primitive: after `SSLRequest` + `'S'` response,
/// the TS-side driver calls this to swap the transport from plain TCP to
/// TLS on the same connection.
///
/// Signature matches `{ has_receiver: true, method: "upgradeToTLS",
/// args: &[NA_STR], ret: NR_PTR }` with an async Promise return.
#[cfg(feature = "tls")]
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_upgrade_tls(
    handle: i64,
    servername_ptr: i64,
    verify: f64,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as *mut u8;

    let servername = match string_from_header_i64(servername_ptr) {
        Some(s) => s,
        None => {
            let err = "invalid servername".to_string();
            crate::common::async_bridge::spawn_for_promise(promise_ptr, async move {
                Err::<u64, String>(err)
            });
            return promise;
        }
    };

    let cmd_tx = {
        let sockets = NET_SOCKETS.lock().unwrap();
        match sockets.get(&handle) {
            Some(s) => s.cmd_tx.clone(),
            None => {
                let err = format!("socket {} not found", handle);
                crate::common::async_bridge::spawn_for_promise(promise_ptr, async move {
                    Err::<u64, String>(err)
                });
                return promise;
            }
        }
    };

    let (reply_tx, reply_rx) = oneshot::channel::<Result<(), String>>();
    let verify = verify != 0.0;
    if cmd_tx
        .send(SocketCommand::UpgradeTls {
            servername,
            verify,
            reply: reply_tx,
        })
        .is_err()
    {
        let err = "socket task is gone".to_string();
        crate::common::async_bridge::spawn_for_promise(promise_ptr, async move {
            Err::<u64, String>(err)
        });
        return promise;
    }

    crate::common::async_bridge::spawn_for_promise(promise_ptr, async move {
        match reply_rx.await {
            Ok(Ok(())) => {
                // Resolve with undefined. Bits for TAG_UNDEFINED:
                Ok(0x7FFC_0000_0000_0001u64)
            }
            Ok(Err(msg)) => Err(msg),
            Err(_) => Err("upgrade reply dropped".to_string()),
        }
    });

    promise
}

// ─── Main-thread event pump ──────────────────────────────────────────────────

/// Dispatches queued socket events to JS listeners on the main thread.
/// Called from `common::async_bridge::js_stdlib_process_pending`.
///
/// Per the arena-safety rule: JSValue construction (Buffer, error string)
/// happens HERE on the main thread, never in the tokio read task.
///
/// #1114 followup (mysql wedge): this pump runs on EVERY iteration of the
/// generated event loop AND every iteration of every inline `await` poll
/// loop. `@perryts/mysql` (pure-TS driver) drives all its bytes through
/// `net.Socket`, so under a `setInterval` + async-query JobLoop this
/// function is the dominant per-tick path. The original `Vec::drain(..)
/// .collect()` allocated a fresh Vec every call (mirroring the fastify
/// wedge that e538caa7 fixed) → GC `madvise` page-churn. Reuse a
/// per-thread scratch buffer (moved out across dispatch so a re-entrant
/// pump from inside a user callback is safe; capacity retained → zero
/// steady-state allocation).
#[no_mangle]
pub unsafe extern "C" fn js_net_process_pending() -> i32 {
    thread_local! {
        static SCRATCH: std::cell::RefCell<Vec<PendingNetEvent>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let mut events = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
    events.clear();
    {
        let mut g = NET_PENDING_EVENTS.lock().unwrap();
        events.append(&mut *g);
    }
    let count = events.len() as i32;

    for ev in events.drain(..) {
        match ev {
            PendingNetEvent::Connect(id) => {
                for cb in listeners_for(id, "connect") {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                for cb in listeners_for(id, "secureConnect") {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
            }
            PendingNetEvent::Data(id, bytes) => {
                let cbs = listeners_for(id, "data");
                if cbs.is_empty() {
                    continue;
                }
                // Construct Buffer on the main thread.
                let buf = js_buffer_alloc(bytes.len() as i32, 0);
                if buf.is_null() {
                    continue;
                }
                let buf_data = (buf as *mut u8).add(std::mem::size_of::<BufferHeader>());
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_data, bytes.len());
                (*buf).length = bytes.len() as u32;

                let buf_f64 = f64::from_bits(JSValue::pointer(buf as *const u8).bits());
                for cb in cbs {
                    if cb != 0 {
                        js_closure_call1(cb as *const ClosureHeader, buf_f64);
                    }
                }
            }
            PendingNetEvent::Error(id, msg) => {
                let cbs = listeners_for(id, "error");
                if cbs.is_empty() {
                    continue;
                }
                // Issue #770 — emit an Error-shaped object `{message: msg}`
                // so user code can read `err.message`. Pre-fix the listener
                // received a raw NaN-boxed string and `err.message` came
                // back as `undefined`.
                let err_f64 = build_error_object(&msg);
                for cb in cbs {
                    if cb != 0 {
                        js_closure_call1(cb as *const ClosureHeader, err_f64);
                    }
                }
            }
            PendingNetEvent::Close(id) => {
                for cb in listeners_for(id, "close") {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                NET_LISTENERS.lock().unwrap().remove(&id);
                NET_SOCKETS.lock().unwrap().remove(&id);
            }
        }
    }

    // Restore the (capacity-retaining) buffer to the thread-local so the
    // next tick reuses it. A re-entrant pump call during dispatch may
    // have left a grown buffer in the slot — keep whichever is larger.
    SCRATCH.with(|s| {
        let mut slot = s.borrow_mut();
        if events.capacity() >= slot.capacity() {
            *slot = events;
        }
    });

    count
}

fn listeners_for(id: i64, event: &str) -> Vec<i64> {
    NET_LISTENERS
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|m| m.get(event).cloned())
        .unwrap_or_default()
}

/// Returns 1 if there are pending events or live sockets keeping the loop alive.
///
/// "Live" here means *registered* — including sockets still establishing
/// their TCP connection. Counting only `is_open` sockets caused the runtime
/// to exit before async `connect` ever completed (the is_open flag flips
/// inside the spawned task, after `await TcpStream::connect`).
pub fn js_net_has_active_handles() -> i32 {
    if !NET_PENDING_EVENTS.lock().unwrap().is_empty() {
        return 1;
    }
    if !NET_SOCKETS.lock().unwrap().is_empty() {
        return 1;
    }
    0
}

/// True iff `handle` is a currently-registered net socket id. Used by
/// the runtime's HANDLE_METHOD_DISPATCH path to route `someSock.method(...)`
/// through to the right FFI when codegen couldn't statically tag the
/// receiver type (e.g. when the socket lives behind a wrapper function
/// or inside a struct field).
pub fn is_net_socket_handle(handle: i64) -> bool {
    NET_SOCKETS.lock().unwrap().contains_key(&handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scanner_emits_socket_listeners() {
        {
            let mut listeners = NET_LISTENERS.lock().unwrap();
            listeners.clear();
            listeners.insert(
                7,
                HashMap::from([
                    ("data".to_string(), vec![0x1234_5678]),
                    ("error".to_string(), vec![0x2345_6780]),
                ]),
            );
        }

        let mut emitted = Vec::new();
        scan_net_roots(&mut |value| emitted.push(value.to_bits()));

        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x1234_5678)));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
        NET_LISTENERS.lock().unwrap().clear();
    }
}
