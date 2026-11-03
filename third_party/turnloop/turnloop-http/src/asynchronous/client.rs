//! Pooled async HTTP clients. Body callbacks consume borrowed chunks immediately.
use super::{Http1, Http2};
use crate::{
    client::{
        self, Acquire, ConnectionId, Pool, PoolKey, Protocol, RedirectMode, Request, Route,
        TransportRequest,
    },
    compression::StreamingDecoder,
    http1::{BodyLength, Event, Head, Header, Mode},
    http2,
};
use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use turnloop_io::{AsyncIo, AsyncRead, AsyncWrite, Backend, ExecutorHandle, Instant, turnloop};
use turnloop_tls::{ClientConfig, TlsStream};

/// Client policy; the deadline covers DNS, connect, TLS, upload and response.
pub struct Options {
    pub timeout: Duration,
    /// Maximum wait for 100 Continue before sending a request body.
    pub continue_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_per_origin: usize,
    pub redirects: RedirectMode,
    pub max_redirects: usize,
    pub proxy: client::ProxyEnvironment,
    pub decompress: bool,
    pub decoded_limit: usize,
    pub http2_prior_knowledge: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            continue_timeout: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(30),
            max_per_origin: 4,
            redirects: RedirectMode::Follow,
            max_redirects: 20,
            proxy: Default::default(),
            decompress: true,
            decoded_limit: 64 * 1024 * 1024,
            http2_prior_knowledge: false,
        }
    }
}
enum Transport<B: Backend> {
    Plain(AsyncIo<B>),
    Tls(Box<TlsStream<AsyncIo<B>>>),
}
impl<B: Backend> AsyncRead for Transport<B> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, bytes),
            Self::Tls(s) => Pin::new(&mut **s).poll_read(cx, bytes),
        }
    }
}
impl<B: Backend> AsyncWrite for Transport<B> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, bytes),
            Self::Tls(s) => Pin::new(&mut **s).poll_write(cx, bytes),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(s) => Pin::new(&mut **s).poll_flush(cx),
        }
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_close(cx),
            Self::Tls(s) => Pin::new(&mut **s).poll_close(cx),
        }
    }
}
enum Connection<B: Backend> {
    H1(Box<Http1<Transport<B>>>),
    H2(Box<Http2<Transport<B>>>),
}
/// A client used by a local task. Repeated requests reuse per-origin connections
/// and decoder state. Use multiple clients for independent concurrent dispatchers;
/// the low-level Http2 driver exposes multiplexed streams directly.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub struct Client<B: Backend> {
    executor: ExecutorHandle<B>,
    tls: ClientConfig,
    pub options: Options,
    pool: Pool,
    connections: Vec<(ConnectionId, Connection<B>)>,
    decoders: Decoders,
    unix_seconds: u64,
}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<B: Backend> Client<B> {
    pub fn new(
        executor: ExecutorHandle<B>,
        tls: ClientConfig,
        unix_seconds: u64,
        options: Options,
    ) -> Self {
        Self {
            pool: Pool::new(options.max_per_origin, options.idle_timeout),
            executor,
            tls,
            options,
            connections: Vec::new(),
            decoders: Decoders::default(),
            unix_seconds,
        }
    }
    /// Expire idle pool entries when the host reaches the advertised deadline.
    /// This requires no background task and performs no periodic polling.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pool.next_timeout()
    }
    pub fn expire(&mut self) {
        while let Some(id) = self.pool.handle_timeout(self.executor.now()) {
            if let Some(i) = self.connections.iter().position(|(key, _)| *key == id) {
                self.connections.swap_remove(i);
            }
        }
    }
    /// Fetch with replayable request bytes, redirects and incremental decompression.
    /// Intermediate redirect bodies are drained without being delivered.
    pub async fn request(
        &mut self,
        request: &mut Request,
        mut body: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<Head> {
        let at = self.executor.now() + self.options.timeout;
        loop {
            let mut upload = Slice {
                bytes: &request.body,
            };
            let head = self
                .exchange(
                    request,
                    BodyLength::Known(request.body.len() as u64),
                    &mut upload,
                    at,
                    &mut body,
                    true,
                )
                .await?;
            if !request
                .redirect(
                    head.status,
                    head.get("location")
                        .map(std::str::from_utf8)
                        .transpose()
                        .map_err(io::Error::other)?,
                    self.options.redirects,
                    self.options.max_redirects,
                )
                .map_err(io::Error::other)?
            {
                return Ok(head);
            }
        }
    }
    /// Stream an upload of a known size or chunked HTTP/1 body. Redirects are
    /// returned to the caller because this source cannot necessarily be replayed.
    pub async fn stream<R: AsyncRead + Unpin>(
        &mut self,
        request: &Request,
        length: BodyLength,
        upload: &mut R,
        mut body: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<Head> {
        self.exchange(
            request,
            length,
            upload,
            self.executor.now() + self.options.timeout,
            &mut body,
            false,
        )
        .await
    }
    async fn exchange<R: AsyncRead + Unpin>(
        &mut self,
        request: &Request,
        length: BodyLength,
        upload: &mut R,
        at: Instant,
        body: &mut impl FnMut(&[u8]) -> io::Result<()>,
        follow: bool,
    ) -> io::Result<Head> {
        self.expire();
        let proxy = self
            .options
            .proxy
            .proxy_for(&request.url)
            .map_err(io::Error::other)?;
        let key = PoolKey::new(&request.url, proxy.as_ref());
        let mut route = Route::new(request.url.clone(), proxy);
        let acquisition = self.pool.acquire(&key, self.executor.now());
        let (id, connection) = match acquisition {
            Acquire::Reuse(id) => {
                let i = self
                    .connections
                    .iter()
                    .position(|(key, _)| *key == id)
                    .ok_or_else(|| io::Error::other("pool entry missing"))?;
                (id, Some(self.connections.swap_remove(i).1))
            }
            Acquire::Connect(id) => (id, None),
            Acquire::Wait => return Err(io::ErrorKind::WouldBlock.into()),
        };
        let mut lease = Lease {
            pool: &mut self.pool,
            connections: &mut self.connections,
            id,
            connection,
            reusable: false,
            now: self.executor.now(),
        };
        let executor = self.executor.clone();
        let tls = &self.tls;
        let options = &self.options;
        let time = self.unix_seconds;
        let decoders = &mut self.decoders;
        turnloop_io::deadline(&executor, at, async {
            if lease.connection.is_none() {
                let stream = connect(&executor, &route).await?;
                let stream = if let Some(head) = route.connect_head(None) {
                    let mut tunnel = Http1::new(stream, Mode::Response);
                    tunnel.response_to("CONNECT");
                    tunnel.send_head(&head, BodyLength::Empty).await?;
                    tunnel.finish_body(&[]).await?;
                    let head = tunnel.head().await?;
                    route
                        .tunnel_response(head.status)
                        .map_err(io::Error::other)?;
                    tunnel
                        .event(|e| {
                            if matches!(e, Event::Upgrade) {
                                Ok(())
                            } else {
                                Err(io::Error::other("CONNECT boundary"))
                            }
                        })
                        .await?;
                    let (stream, extra) = tunnel.into_upgrade()?;
                    if !extra.is_empty() {
                        return Err(io::Error::other("unexpected bytes before TLS handshake"));
                    }
                    stream
                } else {
                    stream
                };
                let (transport, protocol) = if request.url.scheme() == "https" {
                    let name = turnloop_tls::rustls::pki_types::ServerName::try_from(
                        request
                            .url
                            .host_str()
                            .ok_or_else(|| io::Error::other("missing host"))?
                            .to_owned(),
                    )
                    .map_err(io::Error::other)?;
                    let stream = TlsStream::connect(stream, tls, name, &executor, at, time).await?;
                    let protocol =
                        client::from_alpn(stream.alpn_protocol()).map_err(io::Error::other)?;
                    (Transport::Tls(Box::new(stream)), protocol)
                } else {
                    (
                        Transport::Plain(stream),
                        if options.http2_prior_knowledge {
                            Protocol::Http2
                        } else {
                            Protocol::Http1
                        },
                    )
                };
                lease
                    .pool
                    .connected(id, protocol, 1)
                    .map_err(io::Error::other)?;
                lease.connection = Some(match protocol {
                    Protocol::Http1 => {
                        Connection::H1(Box::new(Http1::new(transport, Mode::Response)))
                    }
                    Protocol::Http2 => {
                        Connection::H2(Box::new(Http2::new(transport, http2::Role::Client)?))
                    }
                });
            }
            let head = route.request_head(request, None);
            let result = match lease
                .connection
                .as_mut()
                .ok_or_else(|| io::Error::other("connection missing"))?
            {
                Connection::H1(conn) => {
                    if conn.reusable() {
                        conn.reset()?;
                    }
                    conn.response_to(&head.method);
                    conn.send_head(&head, length).await?;
                    let early_response = if head.token("expect", "100-continue")
                        && !matches!(length, BodyLength::Empty | BodyLength::Known(0))
                    {
                        conn.continue_or_head(&executor, executor.now() + options.continue_timeout)
                            .await?
                    } else {
                        None
                    };
                    let uploaded = early_response.is_none();
                    let response = if let Some(response) = early_response {
                        response
                    } else {
                        let mut bytes = [0; 16384];
                        loop {
                            let n = turnloop_io::read(upload, &mut bytes).await?;
                            if n == 0 {
                                break;
                            }
                            conn.send_body(&bytes[..n]).await?;
                        }
                        conn.finish_body(&[]).await?;
                        conn.head().await?
                    };
                    let deliver = !(follow
                        && redirect(&response)
                        && matches!(options.redirects, RedirectMode::Follow));
                    decoders.start(&response, options)?;
                    loop {
                        let mut end = false;
                        let received = conn
                            .event(|event| {
                                match event {
                                    Event::Body(bytes) if deliver => {
                                        decoders.feed(bytes, false, body)?
                                    }
                                    Event::End => end = true,
                                    _ => {}
                                }
                                Ok(())
                            })
                            .await?;
                        if !received {
                            return Err(io::ErrorKind::UnexpectedEof.into());
                        }
                        if end {
                            break;
                        }
                    }
                    if deliver {
                        decoders.feed(&[], true, body)?;
                    }
                    lease.reusable = uploaded && conn.reusable();
                    response
                }
                Connection::H2(conn) => {
                    let mut headers = vec![
                        Header::new(":method", &head.method),
                        Header::new(":scheme", request.url.scheme()),
                        Header::new(":authority", request.url.authority()),
                        Header::new(":path", &head.target),
                    ];
                    headers.extend(
                        head.headers
                            .iter()
                            .filter(|h| {
                                !matches!(
                                    h.name.to_ascii_lowercase().as_str(),
                                    "host" | "connection" | "transfer-encoding" | "upgrade"
                                )
                            })
                            .cloned(),
                    );
                    let id = conn.core.open(&headers, false).map_err(io::Error::other)?;
                    conn.flush().await?;
                    let mut response = None;
                    let mut ended = false;
                    let mut deliver = true;
                    let mut peer_draining = false;
                    let mut receive = |core: &mut http2::Connection,
                                       event: http2::Event<'_>|
                     -> io::Result<()> {
                        match event {
                            http2::Event::Headers {
                                stream,
                                headers,
                                end_stream,
                                ..
                            } if stream == id => {
                                if response.is_none() {
                                    let status = headers
                                        .iter()
                                        .find(|h| h.name == ":status")
                                        .ok_or_else(|| io::Error::other("missing status"))?;
                                    let status = std::str::from_utf8(&status.value)
                                        .map_err(io::Error::other)?
                                        .parse()
                                        .map_err(io::Error::other)?;
                                    if status >= 200 {
                                        let head = Head {
                                            method: String::new(),
                                            target: String::new(),
                                            status,
                                            version: 2,
                                            headers,
                                            keep_alive: true,
                                        };
                                        deliver = !(follow
                                            && redirect(&head)
                                            && matches!(options.redirects, RedirectMode::Follow));
                                        decoders.start(&head, options)?;
                                        response = Some(head);
                                    }
                                }
                                ended |= end_stream;
                            }
                            http2::Event::Data {
                                stream,
                                bytes,
                                end_stream,
                            } if stream == id => {
                                if deliver {
                                    decoders.feed(bytes, false, body)?;
                                }
                                if !end_stream || !peer_draining {
                                    core.release_capacity(stream, bytes.len() as u32)
                                        .map_err(io::Error::other)?;
                                }
                                ended |= end_stream;
                            }
                            http2::Event::Reset { stream, .. } if stream == id => {
                                return Err(io::Error::other("HTTP/2 stream reset"));
                            }
                            http2::Event::Goaway {
                                last_stream, code, ..
                            } => {
                                peer_draining = true;
                                if code != 0 || last_stream < id {
                                    return Err(io::Error::other("HTTP/2 GOAWAY rejected request"));
                                }
                            }
                            _ => {}
                        }
                        Ok(())
                    };
                    let mut bytes = [0; 16384];
                    loop {
                        let n = turnloop_io::read(upload, &mut bytes).await?;
                        if n == 0 {
                            break;
                        }
                        let mut offset = 0;
                        while offset < n {
                            let sent = conn
                                .core
                                .send_data(id, &bytes[offset..n], false)
                                .map_err(io::Error::other)?;
                            offset += sent;
                            conn.flush().await?;
                            if sent == 0 && !conn.event(&mut receive).await? {
                                return Err(io::ErrorKind::UnexpectedEof.into());
                            }
                        }
                    }
                    conn.core
                        .send_data(id, &[], true)
                        .map_err(io::Error::other)?;
                    conn.flush().await?;
                    // The callback owns these borrows only for each event. Check
                    // the peer's END_STREAM through a separate completion flag.
                    drop(receive);
                    while !ended {
                        if !conn
                            .event(|core, event| {
                                match event {
                                    http2::Event::Headers {
                                        stream,
                                        headers,
                                        end_stream,
                                        ..
                                    } if stream == id => {
                                        if response.is_none() {
                                            let status = headers
                                                .iter()
                                                .find(|h| h.name == ":status")
                                                .ok_or_else(|| {
                                                    io::Error::other("missing status")
                                                })?;
                                            let status = std::str::from_utf8(&status.value)
                                                .map_err(io::Error::other)?
                                                .parse()
                                                .map_err(io::Error::other)?;
                                            if status >= 200 {
                                                let head = Head {
                                                    method: String::new(),
                                                    target: String::new(),
                                                    status,
                                                    version: 2,
                                                    headers,
                                                    keep_alive: true,
                                                };
                                                deliver = !(follow
                                                    && redirect(&head)
                                                    && matches!(
                                                        options.redirects,
                                                        RedirectMode::Follow
                                                    ));
                                                decoders.start(&head, options)?;
                                                response = Some(head);
                                            }
                                        }
                                        ended |= end_stream;
                                    }
                                    http2::Event::Data {
                                        stream,
                                        bytes,
                                        end_stream,
                                    } if stream == id => {
                                        if deliver {
                                            decoders.feed(bytes, false, body)?;
                                        }
                                        if !end_stream || !peer_draining {
                                            core.release_capacity(stream, bytes.len() as u32)
                                                .map_err(io::Error::other)?;
                                        }
                                        ended |= end_stream;
                                    }
                                    http2::Event::Reset { stream, .. } if stream == id => {
                                        return Err(io::Error::other("HTTP/2 stream reset"));
                                    }
                                    http2::Event::Goaway {
                                        last_stream, code, ..
                                    } => {
                                        peer_draining = true;
                                        if code != 0 || last_stream < id {
                                            return Err(io::Error::other(
                                                "HTTP/2 GOAWAY rejected request",
                                            ));
                                        }
                                    }
                                    _ => {}
                                }
                                Ok(())
                            })
                            .await?
                        {
                            return Err(io::ErrorKind::UnexpectedEof.into());
                        }
                    }
                    if deliver {
                        decoders.feed(&[], true, body)?;
                    }
                    lease.reusable = !peer_draining;
                    response.ok_or_else(|| io::Error::other("missing response"))?
                }
            };
            lease.now = executor.now();
            Ok(result)
        })
        .await
    }
}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
struct Lease<'a, B: Backend> {
    pool: &'a mut Pool,
    connections: &'a mut Vec<(ConnectionId, Connection<B>)>,
    id: ConnectionId,
    connection: Option<Connection<B>>,
    reusable: bool,
    now: Instant,
}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<B: Backend> Drop for Lease<'_, B> {
    fn drop(&mut self) {
        if self.reusable {
            if let Some(connection) = self.connection.take() {
                self.connections.push((self.id, connection));
            }
            let _ = self.pool.release(self.id, true, self.now);
        } else {
            let _ = self.pool.closed(self.id);
        }
    }
}
async fn connect<B: Backend>(
    executor: &ExecutorHandle<B>,
    route: &Route,
) -> io::Result<AsyncIo<B>> {
    let TransportRequest::Resolve { hostname, port } = route.resolve() else {
        return Err(io::Error::other("route resolution"));
    };
    if let Ok(ip) = hostname.parse() {
        return executor
            .connect(SocketAddr::new(ip, port), Default::default())
            .await
            .map_err(turnloop_io::error);
    }
    let addresses = executor
        .resolve(turnloop::DnsRequest {
            host: hostname,
            port,
        })
        .await
        .map_err(turnloop_io::error)?;
    let mut error = io::Error::other("DNS returned no addresses");
    for address in addresses {
        match executor.connect(address, Default::default()).await {
            Ok(s) => return Ok(s),
            Err(e) => error = turnloop_io::error(e),
        }
    }
    Err(error)
}
fn redirect(head: &Head) -> bool {
    matches!(head.status, 301 | 302 | 303 | 307 | 308) && head.get("location").is_some()
}
struct Slice<'a> {
    bytes: &'a [u8],
}
impl AsyncRead for Slice<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let n = out.len().min(self.bytes.len());
        out[..n].copy_from_slice(&self.bytes[..n]);
        self.bytes = &self.bytes[n..];
        Poll::Ready(Ok(n))
    }
}
#[derive(Default)]
struct Decoders {
    cache: Vec<(String, StreamingDecoder)>,
    selected: Option<usize>,
    input: Vec<u8>,
}
impl Decoders {
    fn start(&mut self, head: &Head, options: &Options) -> io::Result<()> {
        self.input.clear();
        self.selected = None;
        if !options.decompress {
            return Ok(());
        }
        // Repeated Content-Encoding lines are one list (RFC 9110 section 5.3).
        // The common single-line case borrows; only a repeated field pays for
        // joining the lines into one cache key.
        let joined;
        let encoding = if head.values("content-encoding").nth(1).is_some() {
            let mut key = Vec::new();
            for value in head.values("content-encoding") {
                if !key.is_empty() {
                    key.extend_from_slice(b", ");
                }
                key.extend_from_slice(value);
            }
            joined = String::from_utf8(key).map_err(io::Error::other)?;
            joined.as_str()
        } else {
            std::str::from_utf8(head.get("content-encoding").unwrap_or(b"identity"))
                .map_err(io::Error::other)?
        };
        if encoding == "identity" {
            return Ok(());
        }
        let index = match self.cache.iter().position(|(name, _)| name == encoding) {
            Some(i) => {
                self.cache[i]
                    .1
                    .reset(options.decoded_limit)
                    .map_err(io::Error::other)?;
                i
            }
            None => {
                self.cache.push((
                    encoding.into(),
                    StreamingDecoder::new(encoding, options.decoded_limit)
                        .map_err(io::Error::other)?,
                ));
                self.cache.len() - 1
            }
        };
        self.selected = Some(index);
        self.input.reserve(65536);
        Ok(())
    }
    fn feed(
        &mut self,
        bytes: &[u8],
        end: bool,
        body: &mut impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        let Some(index) = self.selected else {
            return if bytes.is_empty() {
                Ok(())
            } else {
                body(bytes)
            };
        };
        self.input.extend_from_slice(bytes);
        let mut offset = 0;
        let mut out = [0; 8192];
        loop {
            let step = self.cache[index]
                .1
                .process(&self.input[offset..], &mut out, end)
                .map_err(io::Error::other)?;
            offset += step.consumed;
            if step.written > 0 {
                body(&out[..step.written])?;
            }
            if step.finished || step.consumed == 0 && step.written == 0 {
                if end && !step.finished {
                    return Err(io::Error::other("truncated compressed body"));
                }
                break;
            }
        }
        self.input.drain(..offset);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).expect("gzip into a Vec");
        encoder.finish().expect("gzip into a Vec")
    }
    fn head(encodings: &[&str]) -> Head {
        Head {
            method: String::new(),
            target: String::new(),
            status: 200,
            version: 1,
            headers: encodings
                .iter()
                .map(|value| Header::new("Content-Encoding", value))
                .collect(),
            keep_alive: true,
        }
    }
    fn decode(decoders: &mut Decoders, head: &Head, wire: &[u8]) -> io::Result<Vec<u8>> {
        decoders.start(head, &Options::default())?;
        let mut out = Vec::new();
        decoders.feed(wire, true, &mut |bytes| {
            out.extend_from_slice(bytes);
            Ok(())
        })?;
        Ok(out)
    }

    /// PerryTS/turnloop#79: repeated Content-Encoding lines are one list, so
    /// two `gzip` lines decode twice rather than once.
    #[test]
    fn repeated_content_encoding_lines_decode_as_one_list() {
        let body = b"a body compressed twice, one coding per header line".repeat(50);
        let wire = gzip(&gzip(&body));
        let mut decoders = Decoders::default();
        assert_eq!(
            decode(&mut decoders, &head(&["gzip", "gzip"]), &wire).unwrap(),
            body
        );
        // The same chain on one line shares the joined cache entry.
        assert_eq!(
            decode(&mut decoders, &head(&["gzip, gzip"]), &wire).unwrap(),
            body
        );
        assert_eq!(decoders.cache.len(), 1);
        // A single line still decodes once.
        let once = gzip(&body);
        assert_eq!(
            decode(&mut decoders, &head(&["gzip"]), &once).unwrap(),
            body
        );
    }
}
