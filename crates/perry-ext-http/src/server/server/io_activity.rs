use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::task::Poll;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// AsyncRead/AsyncWrite passthrough that flips `saw_bytes` on every read that
/// produces data. It also owns the HTTP/1.0 response-head compatibility
/// rewrites, buffering across arbitrary AsyncWrite boundaries.
pub(crate) struct ReadActivity<S> {
    inner: S,
    saw_bytes: Arc<AtomicBool>,
    response_version_rewrite_offset: Option<usize>,
    rewrite_chunked_header: Arc<AtomicBool>,
    response_head: Vec<u8>,
    pending_write: Vec<u8>,
}

impl<S> ReadActivity<S> {
    pub(crate) fn new(
        inner: S,
        saw_bytes: Arc<AtomicBool>,
        rewrite_chunked_header: Arc<AtomicBool>,
    ) -> Self {
        Self {
            inner,
            saw_bytes,
            response_version_rewrite_offset: None,
            rewrite_chunked_header,
            response_head: Vec::new(),
            pending_write: Vec::new(),
        }
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

impl<S: AsyncRead + Unpin> AsyncRead for ReadActivity<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        if matches!(poll, Poll::Ready(Ok(()))) && buf.filled().len() > before {
            this.saw_bytes.store(true, Ordering::SeqCst);
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for ReadActivity<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if !this.pending_write.is_empty() {
            return match Pin::new(&mut this.inner).poll_write(cx, &this.pending_write) {
                Poll::Ready(Ok(written)) if written == this.pending_write.len() => {
                    this.pending_write.clear();
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Poll::Ready(Ok(written)) if written > 0 => {
                    this.pending_write.drain(..written);
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Poll::Ready(Ok(_)) => {
                    Poll::Ready(Err(std::io::Error::from(std::io::ErrorKind::WriteZero)))
                }
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Pending => Poll::Pending,
            };
        }
        if this.response_version_rewrite_offset.is_none() && buf.starts_with(b"HTTP/1.0 ") {
            this.response_version_rewrite_offset = Some(0);
        }
        let rewrite_chunked = this.rewrite_chunked_header.load(Ordering::Acquire);
        if rewrite_chunked {
            // Hyper may split the response head across any number of writes.
            this.response_head.extend_from_slice(buf);
            let Some(head_end) = find_bytes(&this.response_head, b"\r\n\r\n") else {
                return Poll::Ready(Ok(buf.len()));
            };
            let head_end = head_end + 4;
            let mut rewritten = std::mem::take(&mut this.response_head);
            const CONTENT_LENGTH: &[u8] = b"Content-Length: ";
            if let Some(start) = find_bytes(&rewritten[..head_end], CONTENT_LENGTH) {
                let value_start = start + CONTENT_LENGTH.len();
                if let Some(relative_end) = find_bytes(&rewritten[value_start..head_end], b"\r\n") {
                    let end = value_start + relative_end;
                    rewritten.splice(start..end, b"Transfer-Encoding: chunked".iter().copied());
                }
            }
            this.rewrite_chunked_header.store(false, Ordering::Release);
            if rewritten.starts_with(b"HTTP/1.0 ") {
                rewritten[7] = b'1';
                this.response_version_rewrite_offset = None;
            }
            return match Pin::new(&mut this.inner).poll_write(cx, &rewritten) {
                Poll::Ready(Ok(written)) if written == rewritten.len() => {
                    Poll::Ready(Ok(buf.len()))
                }
                Poll::Ready(Ok(written)) => {
                    this.pending_write.extend_from_slice(&rewritten[written..]);
                    Poll::Ready(Ok(buf.len()))
                }
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Pending => {
                    this.pending_write = rewritten;
                    Poll::Ready(Ok(buf.len()))
                }
            };
        }
        if this.response_version_rewrite_offset.is_none() {
            return Pin::new(&mut this.inner).poll_write(cx, buf);
        }
        let mut rewritten = buf.to_vec();
        if let Some(offset) = this.response_version_rewrite_offset {
            if offset <= 7 && 7 - offset < rewritten.len() {
                rewritten[7 - offset] = b'1';
            }
        }
        match Pin::new(&mut this.inner).poll_write(cx, &rewritten) {
            Poll::Ready(Ok(written)) if written == rewritten.len() => {
                if let Some(offset) = this.response_version_rewrite_offset {
                    let next = offset.saturating_add(buf.len());
                    this.response_version_rewrite_offset = (next < 9).then_some(next);
                }
                Poll::Ready(Ok(buf.len()))
            }
            Poll::Ready(Ok(written)) if written > 0 => {
                this.pending_write.extend_from_slice(&rewritten[written..]);
                this.response_version_rewrite_offset = None;
                Poll::Ready(Ok(buf.len()))
            }
            Poll::Ready(Ok(_)) => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(Err(error)) => {
                this.response_version_rewrite_offset = None;
                Poll::Ready(Err(error))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if !this.response_head.is_empty() {
            let mut buffered_head = std::mem::take(&mut this.response_head);
            if buffered_head.starts_with(b"HTTP/1.0 ") {
                buffered_head[7] = b'1';
            }
            this.pending_write.extend_from_slice(&buffered_head);
            this.response_version_rewrite_offset = None;
            this.rewrite_chunked_header.store(false, Ordering::Release);
        }
        while !this.pending_write.is_empty() {
            match Pin::new(&mut this.inner).poll_write(cx, &this.pending_write) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(std::io::Error::from(std::io::ErrorKind::WriteZero)))
                }
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(written)) => {
                    this.pending_write.drain(..written);
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            }
        }
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => Pin::new(&mut self.get_mut().inner).poll_shutdown(cx),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn chunked_rewrite_survives_a_split_response_head() {
        let (stream, mut peer) = tokio::io::duplex(4096);
        let rewrite = Arc::new(AtomicBool::new(true));
        let mut writer =
            ReadActivity::new(stream, Arc::new(AtomicBool::new(false)), rewrite.clone());
        writer
            .write_all(b"HTTP/1.0 200 OK\r\nContent-Len")
            .await
            .unwrap();
        writer
            .write_all(b"gth: 3\r\nConnection: close\r\n\r\nabc")
            .await
            .unwrap();
        writer.shutdown().await.unwrap();

        let mut bytes = Vec::new();
        peer.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(
            bytes,
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nabc"
        );
        assert!(!rewrite.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn shutdown_flushes_an_incomplete_buffered_response_head() {
        let (stream, mut peer) = tokio::io::duplex(4096);
        let rewrite = Arc::new(AtomicBool::new(true));
        let mut writer =
            ReadActivity::new(stream, Arc::new(AtomicBool::new(false)), rewrite.clone());
        writer
            .write_all(b"HTTP/1.0 200 OK\r\nContent-Len")
            .await
            .unwrap();
        writer.shutdown().await.unwrap();

        let mut bytes = Vec::new();
        peer.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"HTTP/1.1 200 OK\r\nContent-Len");
        assert!(!rewrite.load(Ordering::Acquire));
    }

    struct ZeroWriter;

    impl AsyncWrite for ZeroWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(0))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn flush_reports_write_zero_instead_of_hanging() {
        let rewrite = Arc::new(AtomicBool::new(true));
        let mut writer = ReadActivity::new(ZeroWriter, Arc::new(AtomicBool::new(false)), rewrite);
        writer
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Len")
            .await
            .unwrap();
        let error = writer.flush().await.expect_err("zero write must fail");
        assert_eq!(error.kind(), std::io::ErrorKind::WriteZero);
    }
}
