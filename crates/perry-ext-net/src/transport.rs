//! The `Transport` enum — a `net.Socket`'s underlying stream, either plain
//! TCP or a rustls-wrapped TLS stream, swappable at runtime by
//! `socket.upgradeToTLS`. Split out of `lib.rs` to keep that file under the
//! 2000-line CI gate; the logic is unchanged.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

#[cfg(unix)]
pub(crate) trait IpcStream: AsyncRead + AsyncWrite + Send + Unpin {
    fn raw_fd(&self) -> RawFd;
}

#[cfg(unix)]
impl<T> IpcStream for T
where
    T: AsyncRead + AsyncWrite + Send + Unpin + AsRawFd,
{
    fn raw_fd(&self) -> RawFd {
        self.as_raw_fd()
    }
}

#[cfg(not(unix))]
pub(crate) trait IpcStream: AsyncRead + AsyncWrite + Send + Unpin {}

#[cfg(not(unix))]
impl<T> IpcStream for T where T: AsyncRead + AsyncWrite + Send + Unpin {}

pub(crate) enum Transport {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
    Ipc(Box<dyn IpcStream>),
}

impl Transport {
    /// Borrow the live kernel descriptor without transferring ownership.
    /// Claude Code reads it through Node's private `socket._handle.fd` shape
    /// before passing it to the read-only `Bun.ant` peer-credential hooks.
    pub(crate) fn raw_fd(&self) -> Option<i32> {
        #[cfg(unix)]
        {
            Some(match self {
                Transport::Plain(stream) => stream.as_raw_fd(),
                Transport::Tls(stream) => stream.get_ref().0.as_raw_fd(),
                Transport::Ipc(stream) => stream.raw_fd(),
            })
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Set `TCP_NODELAY` on the underlying TCP socket. For a TLS transport the
    /// option lives on the wrapped TCP stream (`get_ref().0`), so reach through
    /// the rustls wrapper to the kernel socket. Matches Node's `socket.setNoDelay`,
    /// which toggles Nagle's algorithm on the raw connection regardless of TLS.
    pub(crate) fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        match self {
            Transport::Plain(s) => s.set_nodelay(nodelay),
            Transport::Tls(s) => s.get_ref().0.set_nodelay(nodelay),
            // Pipes do not use Nagle's algorithm. Node accepts setNoDelay on
            // every net.Socket, including pipe-backed sockets, as a no-op.
            Transport::Ipc(_) => Ok(()),
        }
    }

    /// Read the current `TCP_NODELAY` state off the underlying socket.
    /// Test-only observability seam for the nodelay command-path test.
    #[cfg(test)]
    pub(crate) fn nodelay(&self) -> io::Result<bool> {
        match self {
            Transport::Plain(s) => s.nodelay(),
            Transport::Tls(s) => s.get_ref().0.nodelay(),
            Transport::Ipc(_) => Ok(false),
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
            Transport::Ipc(s) => Pin::new(&mut **s).poll_read(cx, buf),
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
            Transport::Ipc(s) => Pin::new(&mut **s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_flush(cx),
            Transport::Tls(s) => Pin::new(&mut **s).poll_flush(cx),
            Transport::Ipc(s) => Pin::new(&mut **s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Transport::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Transport::Tls(s) => Pin::new(&mut **s).poll_shutdown(cx),
            Transport::Ipc(s) => Pin::new(&mut **s).poll_shutdown(cx),
        }
    }
}
