//! The hyper-path WebSocket upgrade.
//!
//! Split out of `server.rs` to keep that file under the 2000-line gate, and
//! because it is now a self-contained unit: everything WebSocket-shaped about
//! it lives in `perry-ext-ws`, and what is left here is the hyper protocol
//! switch plus the queue hop to the main thread.
//!
//! Note which path this is. A server that got a turnloop loop answers an
//! attached `WebSocketServer` in `turnloop_serve::conn::on_websocket`, over the
//! connection it already owns. This file is the declining path — a thread with
//! no loop of its own, or a cluster worker.
//!
//! # Who drives the stream
//!
//! This file does, now. `perry-ext-ws` used to take the upgraded stream whole
//! (`register_upgraded_stream<S: AsyncRead + AsyncWrite>`) and spawn its own
//! task over it — which is what kept an async runtime in a crate whose
//! protocol is sans-I/O. The protocol needs bytes in and bytes out, not a
//! stream, so [`adopt_upgraded_stream`] keeps the stream here, where hyper and
//! tokio already live, and hands `perry-ext-ws` a
//! `turnloop_link::Transport` of three function pointers instead. That is the
//! same seam `turnloop_serve` uses for a connection it owns; the only
//! difference is that this one's writer is a channel to a task rather than a
//! turnloop submission.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::server::request::{alloc_incoming_message, IncomingMessage};
use crate::server::ResponseBody;

use super::HttpPendingUpgrade;

/// Phase 4 — WebSocket upgrade dispatch.
///
/// Synchronously builds the 101 response (so hyper drives the
/// protocol switch) and spawns a tokio task that awaits the
/// upgraded stream and hands it to perry-ext-ws, which installs the
/// protocol over it. The
/// resulting connection is registered through perry-ext-ws and an
/// `HttpPendingUpgrade` is pushed to the main-thread upgrade
/// channel; the event-loop fires the user's `'upgrade'` listeners
/// with `(req, wsId, head)`.
pub(super) async fn handle_websocket_upgrade(
    server_handle: i64,
    peer: SocketAddr,
    mut req: Request<Incoming>,
    method: String,
    url: String,
    headers_lower: HashMap<String, String>,
    raw_headers: Vec<(String, String)>,
    upgrade_tx: Arc<mpsc::Sender<HttpPendingUpgrade>>,
) -> Result<Response<ResponseBody>, hyper::Error> {
    // Validate the upgrade and compute its response headers.
    //
    // This used to be a bare `derive_accept_key` plus a literal header block,
    // which validated nothing: neither `Sec-WebSocket-Version` nor
    // `Upgrade: websocket` was checked, and a request with no
    // `Sec-WebSocket-Key` got an empty `Sec-WebSocket-Accept` and a 101 anyway.
    // `perry_ext_ws::accept_headers` is the same `turnloop_websocket::accept`
    // the turnloop path runs — one handshake implementation, not two.
    let request_headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    let response_headers = match perry_ext_ws::accept_headers("GET", "/", &request_headers, &[]) {
        Ok(headers) => headers,
        Err(_) => {
            return Ok(Response::builder()
                .status(400)
                .header("connection", "close")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap())
        }
    };

    // Build the upgraded-protocol IncomingMessage now (no body — WS
    // upgrades carry no request body).
    let mut im = IncomingMessage::new(
        method,
        url,
        headers_lower,
        raw_headers,
        Vec::new(),
        peer.ip().to_string(),
        peer.port(),
    );
    im.complete = true;
    let im_handle = alloc_incoming_message(im);

    // Spawn a task that waits for hyper to perform the protocol
    // switch + completes the tungstenite handshake + hands the
    // resulting stream to perry-ext-ws.
    tokio::spawn(async move {
        let upgraded = match hyper::upgrade::on(&mut req).await {
            Ok(u) => u,
            Err(_) => return,
        };
        // The raw upgraded stream goes straight to perry-ext-ws, which installs
        // the protocol. Constructing a `WebSocketStream` here is what used to
        // put `tokio-tungstenite` in this crate's dependency graph.
        let ws_id = adopt_upgraded_stream(TokioIo::new(upgraded));
        let pending = HttpPendingUpgrade {
            server_handle,
            request_handle: im_handle,
            ws_id,
            raw_socket_id: 0,
            head: Vec::new(),
        };
        let _ = upgrade_tx.send(pending).await;
        perry_ffi::notify_main_thread();
    });

    let mut response = Response::builder().status(101);
    for (name, value) in response_headers {
        response = response.header(name, value);
    }
    Ok(response.body(Full::new(Bytes::new()).boxed()).unwrap())
}

// ── The tokio side of a hyper-upgraded WebSocket ────────────────────────────

/// One read's worth of wire bytes. Matches tungstenite's own default read
/// buffer, so a large message costs the same number of syscalls it used to.
const READ_CHUNK: usize = 128 * 1024;

/// What the protocol layer asks of the stream. The three variants are exactly
/// `turnloop_link::Transport`'s three function pointers.
enum Op {
    Write(Vec<u8>),
    /// Everything queued goes out, then FIN. **Not** `Destroy`: a closing
    /// handshake ends with a close frame written and then a shutdown, and
    /// dropping the stream instead makes the peer report 1006 rather than the
    /// code it was just sent.
    Finish,
    /// `ws.terminate()` and the error paths.
    Destroy,
}

fn senders() -> &'static Mutex<HashMap<i64, mpsc::UnboundedSender<Op>>> {
    static SENDERS: OnceLock<Mutex<HashMap<i64, mpsc::UnboundedSender<Op>>>> = OnceLock::new();
    SENDERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn submit(conn_id: i64, op: Op) {
    let sender = senders()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conn_id)
        .cloned();
    if let Some(sender) = sender {
        let _ = sender.send(op);
    }
}

fn transport_write(conn_id: i64, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    submit(conn_id, Op::Write(bytes.to_vec()));
}

fn transport_finish(conn_id: i64) {
    submit(conn_id, Op::Finish);
}

fn transport_destroy(conn_id: i64) {
    submit(conn_id, Op::Destroy);
}

/// Ids for the connections this path owns. A private domain, because
/// `perry-ext-ws` keys its links by this id and `turnloop_serve` keys its own
/// connections by ids from a different one — two ids that collided would route
/// one connection's frames onto the other's socket.
fn registry_domain() -> perry_ffi::NativeRegistryDomain {
    static DOMAIN: OnceLock<perry_ffi::NativeRegistryDomain> = OnceLock::new();
    *DOMAIN.get_or_init(|| {
        perry_ffi::NativeRegistryDomain::new().expect("http native registry domains exhausted")
    })
}

/// Install `perry-ext-ws`'s protocol on a stream hyper has upgraded, and drive
/// it. Returns the `ws_id` the JS side names the connection by.
pub(super) fn adopt_upgraded_stream<S>(stream: S) -> i64
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let conn_id = perry_ffi::reserve_handle_id_in_domain(registry_domain());
    if conn_id == perry_ffi::INVALID_HANDLE {
        return 0;
    }
    let (sender, receiver) = mpsc::unbounded_channel::<Op>();
    senders()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(conn_id, sender);
    // The link exists before the pump starts, so a frame that arrives in the
    // task's first read has somewhere to decode into.
    let ws_id = perry_ext_ws::adopt_host_connection(
        conn_id,
        perry_ext_ws::turnloop_link::Transport {
            write: transport_write,
            finish: transport_finish,
            destroy: transport_destroy,
        },
        &[],
    );
    tokio::spawn(pump(conn_id, stream, receiver));
    ws_id
}

async fn pump<S>(conn_id: i64, stream: S, mut receiver: mpsc::UnboundedReceiver<Op>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut buffer = vec![0u8; READ_CHUNK];
    loop {
        tokio::select! {
            read = reader.read(&mut buffer) => match read {
                Ok(0) => {
                    perry_ext_ws::turnloop_link::on_eof(conn_id);
                    break;
                }
                Ok(n) => perry_ext_ws::turnloop_link::on_data(conn_id, &buffer[..n]),
                Err(e) => {
                    perry_ext_ws::turnloop_link::on_error(conn_id, &e.to_string());
                    break;
                }
            },
            op = receiver.recv() => match op {
                Some(Op::Write(bytes)) => {
                    if writer.write_all(&bytes).await.is_err() {
                        perry_ext_ws::turnloop_link::on_error(conn_id, "write EPIPE");
                        break;
                    }
                }
                Some(Op::Finish) => {
                    let _ = writer.shutdown().await;
                    break;
                }
                Some(Op::Destroy) => break,
                // Every sender dropped: nothing can ask this stream for
                // anything again.
                None => break,
            },
        }
    }
    senders()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&conn_id);
    // Idempotent on a link the close handshake already retired, and the only
    // report for one it did not.
    perry_ext_ws::turnloop_link::on_closed(conn_id);
    perry_ffi::free_handle_id(conn_id);
}
