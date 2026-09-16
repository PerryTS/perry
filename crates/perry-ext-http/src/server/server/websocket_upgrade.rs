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
//! no loop of its own, or a cluster worker — and `perry-ext-fastify` has its own
//! copy of the same shape.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
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
        let ws_id = perry_ext_ws::register_upgraded_stream(TokioIo::new(upgraded));
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
