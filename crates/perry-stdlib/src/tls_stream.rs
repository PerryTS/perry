//! A TLS stream over a tokio transport, driven by `perry-tls-session`'s
//! sans-I/O [`TlsSession`] instead of `tokio_rustls` (turnloop P8 group H).
//!
//! The three bundled surfaces that negotiate TLS here — the `node:tls` server
//! (`tls.rs`), the bundled `net` client's `tls.connect` / `upgradeToTLS`
//! (`net/mod.rs`) and the `wss://` connector (`ws.rs`) — still run on tokio
//! sockets: those are `async_bridge::RUNTIME`'s, which is group L and stays for
//! now. What moves is the TLS engine. It is the same `rustls::unbuffered` core
//! `turnloop-tls` wraps and perry-ext-net's turnloop path already drives, so
//! perry-stdlib no longer depends on `tokio-rustls` at all.
//!
//! The observable contract is `tokio_rustls`'s, kept on purpose so no caller
//! changes behaviour:
//!
//! * a handshake that fails flushes rustls's fatal alert before returning, and
//!   the `io::Error` (`InvalidData`) displays rustls's own text;
//! * TCP EOF mid-handshake is `UnexpectedEof` / `"tls handshake eof"`;
//! * a read after the peer's `close_notify` is a clean EOF, while TCP EOF
//!   without one is rustls's `UnexpectedEof` message;
//! * `poll_shutdown` sends `close_notify` before shutting the transport's
//!   write half, and a transport that is already disconnected is not an error.
//!
//! `poll_read` keeps all its state in the struct, so it is cancel-safe inside a
//! `tokio::select!` exactly as `tokio_rustls`'s was.

use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};

use perry_tls_session::TlsSession;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// `rustls`'s message for TCP EOF without `close_notify` (what
/// `tokio_rustls`'s reader returned). Unbuffered connections have no reader, so
/// the adapter produces it itself.
const UNEXPECTED_EOF_MESSAGE: &str = "peer closed connection without sending TLS close_notify: \
https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof";

/// Ciphertext read per transport read.
const READ_CHUNK: usize = 16 * 1024 + 256;

pub(crate) struct TlsStream<IO> {
    io: IO,
    session: TlsSession,
    /// Ciphertext produced by the session and not yet written.
    out: Vec<u8>,
    out_pos: usize,
    /// Decrypted plaintext not yet handed to the reader.
    plain: Vec<u8>,
    plain_pos: usize,
    read_buf: Box<[u8]>,
    /// The transport reported EOF.
    eof: bool,
    /// `poll_shutdown` has queued `close_notify`.
    write_closed: bool,
}

impl<IO: AsyncRead + AsyncWrite + Unpin> TlsStream<IO> {
    fn new(io: IO, session: TlsSession) -> Self {
        Self {
            io,
            session,
            out: Vec::new(),
            out_pos: 0,
            plain: Vec::new(),
            plain_pos: 0,
            read_buf: vec![0u8; READ_CHUNK].into_boxed_slice(),
            eof: false,
            write_closed: false,
        }
    }

    /// Client handshake over `io` (the `TlsConnector::connect` replacement).
    /// Used by bundled `net` (`tls`) and `wss://` (`bundled-ws`).
    #[cfg_attr(not(any(feature = "tls", feature = "bundled-ws")), allow(dead_code))]
    pub(crate) async fn connect(
        io: IO,
        config: Arc<rustls::ClientConfig>,
        server_name: ServerName<'static>,
    ) -> io::Result<Self> {
        let session = TlsSession::client(config, server_name).map_err(io::Error::other)?;
        let mut stream = Self::new(io, session);
        poll_fn(|cx| stream.poll_handshake(cx)).await?;
        Ok(stream)
    }

    /// Server handshake over an accepted `io` (the `TlsAcceptor::accept`
    /// replacement). Used by the `node:tls` server (`tls-runtime`).
    #[cfg_attr(not(feature = "tls-runtime"), allow(dead_code))]
    pub(crate) async fn accept(io: IO, config: Arc<rustls::ServerConfig>) -> io::Result<Self> {
        let session = TlsSession::server(config).map_err(io::Error::other)?;
        let mut stream = Self::new(io, session);
        poll_fn(|cx| stream.poll_handshake(cx)).await?;
        Ok(stream)
    }

    /// The negotiated session, for protocol / ALPN / SNI / peer-chain queries.
    #[cfg_attr(not(any(feature = "tls-runtime", feature = "tls")), allow(dead_code))]
    pub(crate) fn session(&self) -> &TlsSession {
        &self.session
    }

    /// Run the session and collect what it produced.
    fn pump(&mut self) {
        self.session.pump();
        if self.session.has_output() {
            let produced = self.session.take_output();
            if self.out_pos == self.out.len() {
                self.out = produced;
                self.out_pos = 0;
            } else {
                self.out.extend_from_slice(&produced);
            }
        }
        let plain = self.session.take_plaintext();
        if !plain.is_empty() {
            if self.plain_pos == self.plain.len() {
                self.plain = plain;
                self.plain_pos = 0;
            } else {
                self.plain.extend_from_slice(&plain);
            }
        }
    }

    fn failure_error(&self) -> Option<io::Error> {
        self.session
            .failure()
            .map(|failure| io::Error::new(io::ErrorKind::InvalidData, failure.message.clone()))
    }

    /// Write every pending ciphertext byte to the transport.
    fn poll_write_out(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.out_pos < self.out.len() {
            let n = ready!(Pin::new(&mut self.io).poll_write(cx, &self.out[self.out_pos..]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.out_pos += n;
        }
        self.out.clear();
        self.out_pos = 0;
        Poll::Ready(Ok(()))
    }

    /// Best effort, as `tokio_rustls` does when it reports a TLS error: push
    /// the queued alert out without letting a transport problem replace the
    /// TLS error the caller is about to see.
    fn try_write_alert(&mut self, cx: &mut Context<'_>) {
        let _ = self.poll_write_out(cx);
    }

    /// Read one chunk of ciphertext into the session. `Ok(0)` is transport EOF.
    fn poll_read_in(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let mut buf = ReadBuf::new(&mut self.read_buf);
        ready!(Pin::new(&mut self.io).poll_read(cx, &mut buf))?;
        let n = buf.filled().len();
        if n == 0 {
            self.eof = true;
        } else {
            let (session, bytes) = (&mut self.session, &self.read_buf[..n]);
            session.receive(bytes);
            self.pump();
        }
        Poll::Ready(Ok(n))
    }

    fn poll_handshake(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            self.pump();
            if let Some(error) = self.failure_error() {
                self.try_write_alert(cx);
                return Poll::Ready(Err(error));
            }
            ready!(self.poll_write_out(cx))?;
            if !self.session.is_handshaking() {
                return Poll::Ready(Ok(()));
            }
            if self.eof {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "tls handshake eof",
                )));
            }
            ready!(self.poll_read_in(cx))?;
        }
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> AsyncRead for TlsStream<IO> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if this.plain_pos < this.plain.len() {
                let available = &this.plain[this.plain_pos..];
                let n = available.len().min(buf.remaining());
                buf.put_slice(&available[..n]);
                this.plain_pos += n;
                if this.plain_pos == this.plain.len() {
                    this.plain.clear();
                    this.plain_pos = 0;
                }
                return Poll::Ready(Ok(()));
            }
            if let Some(error) = this.failure_error() {
                this.try_write_alert(cx);
                return Poll::Ready(Err(error));
            }
            if this.session.peer_closed() {
                return Poll::Ready(Ok(()));
            }
            if this.eof {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    UNEXPECTED_EOF_MESSAGE,
                )));
            }
            // Anything the session answered while reading (a TLS 1.3
            // KeyUpdate, say) goes out opportunistically. Neither a full
            // transport nor a write error stops the read: like
            // `tokio_rustls`, write failures surface from the write side.
            if this.out_pos < this.out.len() {
                let _ = this.poll_write_out(cx);
            }
            ready!(this.poll_read_in(cx))?;
        }
    }
}

impl<IO: AsyncRead + AsyncWrite + Unpin> AsyncWrite for TlsStream<IO> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        // Earlier records first: never let plaintext overtake ciphertext that
        // is already queued, and exert backpressure while the transport is
        // full.
        ready!(this.poll_write_out(cx))?;
        if let Some(error) = this.failure_error() {
            return Poll::Ready(Err(error));
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        this.session.write(buf);
        this.pump();
        if let Some(error) = this.failure_error() {
            this.try_write_alert(cx);
            return Poll::Ready(Err(error));
        }
        // The bytes are accepted once encrypted; writing them to the transport
        // may complete on a later poll (flush / the next write), as with
        // `tokio_rustls`, which also reports a write as done once rustls holds
        // it.
        if let Poll::Ready(Err(error)) = this.poll_write_out(cx) {
            return Poll::Ready(Err(error));
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.pump();
        ready!(this.poll_write_out(cx))?;
        Pin::new(&mut this.io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !this.write_closed {
            this.write_closed = true;
            this.session.close_notify();
            this.pump();
        }
        ready!(this.poll_write_out(cx))?;
        match ready!(Pin::new(&mut this.io).poll_shutdown(cx)) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(error) if error.kind() == io::ErrorKind::NotConnected => Poll::Ready(Ok(())),
            Err(error) => Poll::Ready(Err(error)),
        }
    }
}

#[cfg(test)]
mod tests;
