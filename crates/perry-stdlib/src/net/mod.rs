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

use perry_runtime::{ClosureHeader, JSValue, StringHeader, js_closure_call0, js_closure_call1};
use perry_runtime::buffer::{js_buffer_alloc, BufferHeader};
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
        perry_runtime::gc::gc_register_root_scanner(scan_net_roots);
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
fn scan_net_roots(mark: &mut dyn FnMut(f64)) {
    if let Ok(listeners) = NET_LISTENERS.lock() {
        for per_socket in listeners.values() {
            for cb_vec in per_socket.values() {
                for &cb in cb_vec.iter() {
                    if cb != 0 {
                        let boxed = f64::from_bits(
                            0x7FFD_0000_0000_0000
                                | (cb as u64 & 0x0000_FFFF_FFFF_FFFF),
                        );
                        mark(boxed);
                    }
                }
            }
        }
    }
}

struct SocketState {
    cmd_tx: mpsc::UnboundedSender<SocketCommand>,
    is_open: bool,
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

// ─── FFI: net.createConnection(port, host) ───────────────────────────────────

/// `net.createConnection(port, host)` — returns a handle immediately;
/// connection happens in the background and emits `'connect'` or `'error'`.
///
/// Argument order matches Node.js: port (number) first, host (string) second.
/// Signature matches NATIVE_MODULE_TABLE entry
/// `{ module: "net", method: "createConnection", args: &[NA_F64, NA_STR], ret: NR_PTR }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_connect(port: f64, host_ptr: i64) -> i64 {
    let host = match string_from_header_i64(host_ptr) {
        Some(h) => h,
        None => return 0,
    };
    let port = port as u16;
    spawn_socket_task(host, port, /* direct_tls: */ None)
}

// ─── FFI: tls.connect(host, port, servername) ────────────────────────────────

/// `tls.connect(host, port, servername)` — opens a plain TCP socket and
/// immediately runs the TLS handshake before firing `'connect'`. Use this
/// for protocols that start TLS from byte 0 (HTTPS, SMTP with SMTPS, etc.).
///
/// For protocols that negotiate TLS mid-stream (Postgres' `SSLRequest`,
/// SMTP STARTTLS), use `net.createConnection` then `socket.upgradeToTLS`
/// instead.
///
/// Signature matches `{ module: "tls", method: "connect",
/// args: &[NA_STR, NA_F64, NA_STR], ret: NR_PTR }`.
#[cfg(feature = "tls")]
#[no_mangle]
pub unsafe extern "C" fn js_tls_connect(
    host_ptr: i64,
    port: f64,
    servername_ptr: i64,
    verify: f64,
) -> i64 {
    let host = match string_from_header_i64(host_ptr) {
        Some(h) => h,
        None => return 0,
    };
    let servername = match string_from_header_i64(servername_ptr) {
        Some(s) => s,
        None => host.clone(),
    };
    let port = port as u16;
    let verify = verify != 0.0;
    spawn_socket_task(host, port, Some((servername, verify)))
}

/// Internal: allocate the handle, spawn the tokio task.
/// `direct_tls` = Some((servername, verify)) runs a TLS handshake before
/// firing 'connect'; None keeps the socket in plain TCP mode.
fn spawn_socket_task(host: String, port: u16, direct_tls: Option<(String, bool)>) -> i64 {
    ensure_gc_scanner_registered();
    let id = next_id();
    let (tx, mut rx) = mpsc::unbounded_channel::<SocketCommand>();

    NET_SOCKETS.lock().unwrap().insert(id, SocketState {
        cmd_tx: tx,
        is_open: false,
    });
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
                push_event(PendingNetEvent::Error(id, "tls feature not compiled in".to_string()));
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

/// `socket.write(buffer)` — enqueues bytes for the writer task.
/// Signature matches `{ has_receiver: true, method: "write", args: &[NA_PTR], ret: NR_VOID }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_write(handle: i64, buf_ptr: i64) {
    if buf_ptr == 0 || (buf_ptr as usize) < 0x1000 {
        return;
    }
    let buf = buf_ptr as *const BufferHeader;
    let len = (*buf).length as usize;
    let data_ptr = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len).to_vec();

    let sockets = NET_SOCKETS.lock().unwrap();
    if let Some(s) = sockets.get(&handle) {
        let _ = s.cmd_tx.send(SocketCommand::Write(bytes));
    }
}

// ─── FFI: socket.end() ───────────────────────────────────────────────────────

/// `socket.end()` — graceful shutdown: stops further writes, lets reads drain.
/// Signature matches `{ has_receiver: true, method: "end", args: &[], ret: NR_VOID }`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_end(handle: i64) {
    let sockets = NET_SOCKETS.lock().unwrap();
    if let Some(s) = sockets.get(&handle) {
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
    let entry = listeners.entry(handle).or_insert_with(HashMap::new);
    entry.entry(event).or_insert_with(Vec::new).push(cb);
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
    if cmd_tx.send(SocketCommand::UpgradeTls { servername, verify, reply: reply_tx }).is_err() {
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
#[no_mangle]
pub unsafe extern "C" fn js_net_process_pending() -> i32 {
    let events: Vec<PendingNetEvent> = {
        let mut g = NET_PENDING_EVENTS.lock().unwrap();
        g.drain(..).collect()
    };
    let count = events.len() as i32;

    for ev in events {
        match ev {
            PendingNetEvent::Connect(id) => {
                for cb in listeners_for(id, "connect") {
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
                let bytes = msg.as_bytes();
                let s = perry_runtime::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                let s_f64 = f64::from_bits(
                    0x7FFF_0000_0000_0000u64 | (s as u64 & 0x0000_FFFF_FFFF_FFFF)
                );
                for cb in cbs {
                    if cb != 0 {
                        js_closure_call1(cb as *const ClosureHeader, s_f64);
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

    count
}

fn listeners_for(id: i64, event: &str) -> Vec<i64> {
    NET_LISTENERS.lock().unwrap()
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
