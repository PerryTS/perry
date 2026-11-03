//! Async protocol drivers. All waits belong to the host's turnloop executor.
use crate::{http1, http2};
use std::io;
use turnloop_io::{CloseOnDrop as Abort, Output, Stream};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod client;
pub mod server;

/// Retained HTTP/1 input, output and codec state over a TCP, pipe or TLS stream.
pub struct Http1<S> {
    stream: Option<S>,
    decoder: http1::Decoder,
    input: Vec<u8>,
    output: Vec<u8>,
    encoder: Option<http1::Encoder>,
    ended: bool,
    upgraded: bool,
}
impl<S: Stream> Http1<S> {
    pub fn new(stream: S, mode: http1::Mode) -> Self {
        Self {
            stream: Some(stream),
            decoder: http1::Decoder::new(mode, Default::default()),
            input: Vec::with_capacity(65536),
            output: Vec::with_capacity(32768),
            encoder: None,
            ended: false,
            upgraded: false,
        }
    }
    /// A request that asked to upgrade ends the message too, and a server that
    /// declined it may reuse the connection. A response upgrade never can.
    pub fn reusable(&self) -> bool {
        self.stream.is_some() && (self.ended || self.upgraded) && self.decoder.reusable()
    }
    pub fn abort(&mut self) {
        self.stream.take();
    }
    pub fn reset(&mut self) -> io::Result<()> {
        self.decoder.reset().map_err(io::Error::other)?;
        self.ended = false;
        self.upgraded = false;
        Ok(())
    }
    pub fn response_to(&mut self, method: &str) {
        self.decoder.response_to(method);
    }
    pub async fn send_head(
        &mut self,
        head: &http1::Head,
        length: http1::BodyLength,
    ) -> io::Result<()> {
        self.encoder = Some(encode_head(head, length, &mut self.output)?);
        self.flush().await
    }
    pub async fn send_body(&mut self, bytes: &[u8]) -> io::Result<()> {
        // Bound wire scratch even when the application supplies a huge body.
        for chunk in bytes.chunks(16384) {
            self.encoder
                .as_mut()
                .ok_or_else(|| io::Error::other("send head first"))?
                .body(chunk, &mut self.output)
                .map_err(io::Error::other)?;
            self.flush().await?;
        }
        Ok(())
    }
    pub async fn finish_body(&mut self, trailers: &[http1::Header]) -> io::Result<()> {
        self.encoder
            .as_mut()
            .ok_or_else(|| io::Error::other("send head first"))?
            .finish(trailers, &mut self.output)
            .map_err(io::Error::other)?;
        self.flush().await
    }
    async fn flush(&mut self) -> io::Result<()> {
        let mut guard = Abort::new(&mut self.stream);
        turnloop_io::write_all(guard.stream()?, &self.output).await?;
        self.output.clear();
        guard.commit();
        Ok(())
    }
    /// Receive a single event. Body slices are borrowed only during the callback.
    /// Returning false means EOF after a complete message; partial EOF is an error.
    pub async fn event(
        &mut self,
        mut receive: impl FnMut(http1::Event<'_>) -> io::Result<()>,
    ) -> io::Result<bool> {
        if self.ended || self.upgraded {
            return Ok(false);
        }
        let mut guard = Abort::new(&mut self.stream);
        loop {
            let step = self
                .decoder
                .receive(&self.input)
                .map_err(io::Error::other)?;
            let emitted = step.event.is_some();
            let consumed = step.consumed;
            if let Some(event) = step.event {
                self.ended = matches!(event, http1::Event::End);
                self.upgraded = matches!(event, http1::Event::Upgrade);
                receive(event)?;
            }
            self.input.drain(..consumed);
            if emitted {
                guard.commit();
                return Ok(true);
            }
            if consumed == 0 && append(guard.stream()?, &mut self.input).await? == 0 {
                self.decoder.eof().map_err(io::Error::other)?;
            }
        }
    }
    pub async fn head(&mut self) -> io::Result<http1::Head> {
        loop {
            let mut head = None;
            let received = self
                .event(|event| {
                    if let http1::Event::Head(h) = event {
                        head = Some(h);
                    }
                    Ok(())
                })
                .await?;
            if let Some(head) = head {
                return Ok(head);
            }
            if !received {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
    }
    /// Wait for 100 Continue, a final response, or the continue deadline. A
    /// continue timeout preserves partial input and permits uploading the body;
    /// dropping this future still closes the connection.
    pub async fn continue_or_head<B: turnloop_io::Backend>(
        &mut self,
        executor: &turnloop_io::ExecutorHandle<B>,
        at: turnloop_io::Instant,
    ) -> io::Result<Option<http1::Head>> {
        let mut guard = Abort::new(&mut self.stream);
        let result = executor
            .timeout_at(at, async {
                loop {
                    let step = self
                        .decoder
                        .receive(&self.input)
                        .map_err(io::Error::other)?;
                    let consumed = step.consumed;
                    let result = match step.event {
                        Some(http1::Event::Informational(h)) if h.status == 100 => Some(None),
                        Some(http1::Event::Head(h)) => Some(Some(h)),
                        Some(http1::Event::Informational(_)) | None => None,
                        _ => return Err(io::Error::other("expected response head")),
                    };
                    self.input.drain(..consumed);
                    if let Some(result) = result {
                        return Ok(result);
                    }
                    if consumed == 0 && append(guard.stream()?, &mut self.input).await? == 0 {
                        self.decoder.eof().map_err(io::Error::other)?;
                    }
                }
            })
            .await;
        let result = match result {
            Ok(result) => result?,
            Err(e) if e.kind == turnloop_io::turnloop::ErrorKind::TimedOut => None,
            Err(e) => return Err(turnloop_io::error(e)),
        };
        guard.commit();
        Ok(result)
    }
    /// Transfer an upgraded transport and every byte following the HTTP head.
    pub fn into_upgrade(mut self) -> io::Result<(S, Vec<u8>)> {
        if !self.upgraded {
            return Err(io::Error::other("HTTP upgrade boundary not consumed"));
        }
        Ok((self.stream.take().ok_or_else(closed)?, self.input))
    }
    /// Transfer ownership at a caller-validated protocol boundary (for example a
    /// server accepting an upgrade). Includes all unread bytes without copying.
    pub fn into_parts(mut self) -> io::Result<(S, Vec<u8>)> {
        Ok((self.stream.take().ok_or_else(closed)?, self.input))
    }
    pub fn into_inner(mut self) -> io::Result<S> {
        if !self.input.is_empty() {
            return Err(io::Error::other("unconsumed transport bytes"));
        }
        self.stream.take().ok_or_else(closed)
    }
}
fn closed() -> io::Error {
    io::ErrorKind::NotConnected.into()
}
async fn append<S: Stream>(stream: &mut S, input: &mut Vec<u8>) -> io::Result<usize> {
    std::future::poll_fn(|cx| {
        let start = input.len();
        let capacity = input.capacity();
        if start == capacity {
            return std::task::Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP input limit",
            )));
        }
        input.resize(capacity, 0);
        let result = std::pin::Pin::new(&mut *stream).poll_read(cx, &mut input[start..]);
        let n = match &result {
            std::task::Poll::Ready(Ok(n)) => *n,
            _ => 0,
        };
        input.truncate(start + n);
        result
    })
    .await
}
impl Output for http2::Connection {
    fn output(&self) -> &[u8] {
        self.output()
    }
    fn consume_output(&mut self, n: usize) -> io::Result<()> {
        self.consume_output(n).map_err(io::Error::other)
    }
}
/// HTTP/2 multiplexing driver. Callbacks handle borrowed DATA then explicitly
/// release capacity; blocked sends resume only after receiving peer credit.
pub struct Http2<S> {
    stream: Option<S>,
    pub core: http2::Connection,
    input: Vec<u8>,
    shutdown: Option<server::Shutdown>,
    draining: bool,
}
impl<S: Stream> Http2<S> {
    pub fn new(stream: S, role: http2::Role) -> io::Result<Self> {
        Ok(Self {
            stream: Some(stream),
            core: http2::Connection::new(role, Default::default()).map_err(io::Error::other)?,
            input: Vec::with_capacity(65536),
            shutdown: None,
            draining: false,
        })
    }
    pub async fn flush(&mut self) -> io::Result<()> {
        let mut guard = Abort::new(&mut self.stream);
        turnloop_io::drain(guard.stream()?, &mut self.core).await?;
        guard.commit();
        Ok(())
    }
    /// Receive one frame/event, flushing automatic control replies. False is EOF.
    pub async fn event(
        &mut self,
        mut receive: impl FnMut(&mut http2::Connection, http2::Event<'_>) -> io::Result<()>,
    ) -> io::Result<bool> {
        let mut guard = Abort::new(&mut self.stream);
        turnloop_io::drain(guard.stream()?, &mut self.core).await?;
        loop {
            let step = match self.core.receive(&self.input) {
                Ok(step) => step,
                Err(e) => {
                    turnloop_io::drain(guard.stream()?, &mut self.core).await?;
                    return Err(io::Error::other(e));
                }
            };
            let progressed = step.consumed > 0 || step.event.is_some();
            if let Some(event) = step.event {
                receive(&mut self.core, event)?;
            }
            self.input.drain(..step.consumed);
            turnloop_io::drain(guard.stream()?, &mut self.core).await?;
            if progressed {
                guard.commit();
                return Ok(true);
            }
            let n = if let Some(signal) = self.shutdown.as_ref().filter(|_| !self.draining) {
                match signal.until(append(guard.stream()?, &mut self.input)).await {
                    Some(result) => result?,
                    None => {
                        self.draining = true;
                        self.core.shutdown().map_err(io::Error::other)?;
                        turnloop_io::drain(guard.stream()?, &mut self.core).await?;
                        guard.commit();
                        return Ok(true);
                    }
                }
            } else {
                append(guard.stream()?, &mut self.input).await?
            };
            if n == 0 {
                self.core.eof();
                guard.commit();
                return Ok(false);
            }
        }
    }
    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.core.shutdown().map_err(io::Error::other)?;
        self.flush().await
    }
}

fn response_length(head: &http1::Head, length: http1::BodyLength) -> http1::BodyLength {
    if matches!(length, http1::BodyLength::Empty)
        && head.status >= 200
        && !matches!(head.status, 204 | 304)
    {
        http1::BodyLength::Known(0)
    } else {
        length
    }
}

fn encode_head(
    head: &http1::Head,
    length: http1::BodyLength,
    output: &mut Vec<u8>,
) -> io::Result<http1::Encoder> {
    let encoder = http1::Encoder::start(head, response_length(head, length), output)
        .map_err(io::Error::other)?;
    if !head.keep_alive
        && head.status != 101
        && !head.token("connection", "upgrade")
        && !head.token("connection", "close")
    {
        output.truncate(output.len() - 2);
        output.extend_from_slice(b"connection: close\r\n\r\n");
    }
    Ok(encoder)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub mod web;
