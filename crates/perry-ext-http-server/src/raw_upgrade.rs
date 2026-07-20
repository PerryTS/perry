//! #4973 — Node-semantics raw-socket `'upgrade'` dispatch.
//!
//! Node's `http.Server` emits `'upgrade'` with `(req, socket, head)` where
//! `socket` is the **raw net.Socket** and *nothing* has been written to the
//! connection — the listener composes its own `101` response bytes. The
//! pre-existing Phase-4 path (upgrade.rs) instead let hyper write a 101 and
//! wrapped the stream in a tungstenite WebSocket — fine for the
//! `WebSocketServer({ server })` integration, but wrong for the classic
//! handshake-by-hand servers (`test-http-upgrade-server`): hyper's 101 plus
//! the listener's handwritten 101 would both reach the client, and the
//! unconsumed body bytes (`head`) were lost.
//!
//! This module adds the Node-exact path for *keyless* Upgrade requests
//! (no `Sec-WebSocket-Key` — i.e. not a real WebSocket client handshake):
//!
//! 1. When the server has `'upgrade'` listeners, the accept task peeks the
//!    request head off the TCP stream *before* handing anything to hyper.
//! 2. If the head carries `Connection: …upgrade…` + an `Upgrade:` header and
//!    no `Sec-WebSocket-Key`, the stream is handed to perry-ext-net
//!    (`adopt_upgraded_tcp_stream`) so JS sees a standard `net.Socket`
//!    surface, and the `'upgrade'` listeners fire with the unconsumed bytes
//!    after the head as `head`.
//! 3. Anything else (no Upgrade header, real WS handshakes, oversized or
//!    truncated heads) is replayed to hyper byte-for-byte through
//!    `PrefixedStream`, preserving today's behavior.
//!
//! Real WebSocket handshakes (key present) deliberately keep the
//! tungstenite path so `new WebSocketServer({ server })` keeps working.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::request::alloc_incoming_message;
use crate::request::IncomingMessage;
use crate::server::HttpPendingUpgrade;

/// Replays an already-read prefix before the live stream. Write side passes
/// straight through.
pub(crate) struct PrefixedStream<S> {
    prefix: Vec<u8>,
    pos: usize,
    inner: S,
}

impl<S> PrefixedStream<S> {
    pub(crate) fn new(prefix: Vec<u8>, inner: S) -> Self {
        Self {
            prefix,
            pos: 0,
            inner,
        }
    }

    pub(crate) fn empty(inner: S) -> Self {
        Self::new(Vec::new(), inner)
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for PrefixedStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.pos < this.prefix.len() {
            let remaining = &this.prefix[this.pos..];
            let n = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..n]);
            this.pos += n;
            if this.pos >= this.prefix.len() {
                this.prefix = Vec::new();
                this.pos = 0;
            }
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for PrefixedStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

pub(crate) enum PeekResult {
    /// Raw upgrade dispatched; the stream now lives in perry-ext-net.
    Handled,
    /// Not a raw upgrade — continue to hyper, replaying the peeked bytes.
    Passthrough(PrefixedStream<TcpStream>),
}

/// Cap on the peeked head. Node's llhttp default max header size is 16 KiB;
/// we allow 64 KiB before giving up and replaying to hyper (which enforces
/// its own limits).
const MAX_HEAD: usize = 64 * 1024;

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// Peek the request head and dispatch a raw `'upgrade'` if it qualifies.
/// Only called when the server has `'upgrade'` listeners.
pub(crate) async fn peek_and_maybe_dispatch_raw_upgrade(
    server_handle: i64,
    peer: SocketAddr,
    mut stream: TcpStream,
    upgrade_tx: &mpsc::Sender<HttpPendingUpgrade>,
) -> PeekResult {
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut tmp = [0u8; 8 * 1024];
    let head_end = loop {
        if let Some(pos) = find_head_end(&buf) {
            break Some(pos);
        }
        if buf.len() >= MAX_HEAD {
            break None;
        }
        match stream.read(&mut tmp).await {
            Ok(0) => break None,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break None,
        }
    };
    let Some(head_end) = head_end else {
        return PeekResult::Passthrough(PrefixedStream::new(buf, stream));
    };

    let Some((method, url, headers_lower, raw_headers)) = parse_head(&buf[..head_end]) else {
        return PeekResult::Passthrough(PrefixedStream::new(buf, stream));
    };

    let connection_upgrade = headers_lower
        .get("connection")
        .map(|v| v.to_ascii_lowercase().contains("upgrade"))
        .unwrap_or(false);
    let has_upgrade = headers_lower.contains_key("upgrade");
    let has_ws_key = headers_lower.contains_key("sec-websocket-key");
    if !connection_upgrade || !has_upgrade || has_ws_key {
        return PeekResult::Passthrough(PrefixedStream::new(buf, stream));
    }

    // Raw upgrade: unconsumed bytes after the head become `head` (Node's
    // `upgradeHead`), the stream becomes a net.Socket.
    let head_rest = buf[head_end..].to_vec();
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
    let socket_id = perry_ext_net::adopt_upgraded_tcp_stream(stream);
    // #6441: `adopt_upgraded_tcp_stream` returns `INVALID_HANDLE` and drops the
    // stream when the shared net handle-id band is exhausted. Abort the upgrade
    // rather than forward a phantom id-0 socket downstream, and reclaim the
    // just-allocated incoming-message handle so it doesn't orphan.
    if socket_id == perry_ffi::INVALID_HANDLE {
        perry_ffi::drop_handle(im_handle);
        return PeekResult::Handled;
    }

    let pending = HttpPendingUpgrade {
        server_handle,
        request_handle: im_handle,
        ws_id: 0,
        raw_socket_id: socket_id,
        head: head_rest,
    };
    let _ = upgrade_tx.send(pending).await;
    perry_ffi::notify_main_thread();
    PeekResult::Handled
}

/// Minimal HTTP/1.x head parse: request line + headers. Returns
/// `(method, url, lowercased-name header map, raw (name, value) pairs)`.
/// `None` on anything that doesn't look like an HTTP request — the caller
/// replays the bytes to hyper, which produces the proper error response.
#[allow(clippy::type_complexity)]
fn parse_head(
    head: &[u8],
) -> Option<(
    String,
    String,
    HashMap<String, String>,
    Vec<(String, String)>,
)> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split(' ');
    let method = parts.next()?.to_string();
    let url = parts.next()?.to_string();
    let version = parts.next()?;
    if method.is_empty() || !version.starts_with("HTTP/") {
        return None;
    }
    let mut headers_lower = HashMap::new();
    let mut raw_headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        let value = value.trim_start();
        headers_lower.insert(name.to_ascii_lowercase(), value.to_string());
        raw_headers.push((name.to_string(), value.to_string()));
    }
    Some((method, url, headers_lower, raw_headers))
}
