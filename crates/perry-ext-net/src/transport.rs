//! The `Transport` enum — a `net.Socket`'s underlying stream, either plain
//! TCP or a rustls-wrapped TLS stream, swappable at runtime by
//! `socket.upgradeToTLS`. Split out of `lib.rs` to keep that file under the
//! 2000-line CI gate; the logic is unchanged.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

pub(crate) enum Transport {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl Transport {
    /// Set `TCP_NODELAY` on the underlying TCP socket. For a TLS transport the
    /// option lives on the wrapped TCP stream (`get_ref().0`), so reach through
    /// the rustls wrapper to the kernel socket. Matches Node's `socket.setNoDelay`,
    /// which toggles Nagle's algorithm on the raw connection regardless of TLS.
    pub(crate) fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        match self {
            Transport::Plain(s) => s.set_nodelay(nodelay),
            Transport::Tls(s) => s.get_ref().0.set_nodelay(nodelay),
        }
    }

    /// Read the current `TCP_NODELAY` state off the underlying socket.
    /// Test-only observability seam for the nodelay command-path test.
    #[cfg(test)]
    pub(crate) fn nodelay(&self) -> io::Result<bool> {
        match self {
            Transport::Plain(s) => s.nodelay(),
            Transport::Tls(s) => s.get_ref().0.nodelay(),
        }
    }
}

impl AsyncRead for Transport {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_read(cx, buf),
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
            Transport::Tls(s) => Pin::new(&mut **s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_flush(cx),
            Transport::Tls(s) => Pin::new(&mut **s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Transport::Tls(s) => Pin::new(&mut **s).poll_shutdown(cx),
        }
    }
}
