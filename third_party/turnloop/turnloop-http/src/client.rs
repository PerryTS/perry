//! Host-driven fetch transport policy. DNS, environment, sockets and time are inputs.
//! JS body conversions, promise dispatch and abort reason objects remain in Perry.
use crate::{
    Error, Result,
    http1::{Head, Header},
};
use std::{
    cmp::{Ordering, Reverse},
    collections::{BinaryHeap, HashMap},
    hash::Hash,
    net::IpAddr,
    time::{Duration, Instant},
};
use url::Url;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Http1,
    Http2,
}
pub fn from_alpn(alpn: Option<&[u8]>) -> Result<Protocol> {
    match alpn {
        Some(b"h2") => Ok(Protocol::Http2),
        None | Some(b"http/1.1") => Ok(Protocol::Http1),
        _ => Err(Error::new("UND_ERR_NOT_SUPPORTED", "unsupported ALPN")),
    }
}
/// Resolver implementations enqueue host operations; they must not block here.
pub trait Resolver {
    type Token: Copy;
    fn resolve(&mut self, hostname: &str, port: u16) -> Result<Self::Token>;
    fn poll(&mut self, token: Self::Token, addresses: &mut Vec<IpAddr>) -> Option<Result<()>>;
    fn cancel(&mut self, token: Self::Token);
}
pub const DEFAULT_MAX_REDIRECTS: usize = 20;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectMode {
    Follow,
    Manual,
    Error,
}
#[derive(Debug, Clone)]
pub struct Request {
    pub url: Url,
    pub method: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    pub replayable: bool,
    pub redirects: usize,
}
impl Request {
    pub fn new(url: &str, method: &str) -> Result<Self> {
        let url = Url::parse(url).map_err(|_| Error::new("ERR_INVALID_URL", "invalid URL"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(Error::new("ERR_INVALID_URL", "unsupported URL scheme"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Error::new(
                "UND_ERR_INVALID_ARG",
                "URL contains credentials",
            ));
        }
        http::Method::from_bytes(method.as_bytes())
            .map_err(|_| Error::new("UND_ERR_INVALID_ARG", "invalid method"))?;
        if ["CONNECT", "TRACE", "TRACK"]
            .iter()
            .any(|m| method.eq_ignore_ascii_case(m))
        {
            return Err(Error::new("UND_ERR_INVALID_ARG", "forbidden fetch method"));
        }
        let method = if ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"]
            .iter()
            .any(|m| method.eq_ignore_ascii_case(m))
        {
            method.to_ascii_uppercase()
        } else {
            method.into()
        };
        Ok(Self {
            url,
            method,
            headers: Vec::new(),
            body: Vec::new(),
            replayable: true,
            redirects: 0,
        })
    }
    pub fn head(&self, absolute_form: bool) -> Head {
        let mut url = self.url.clone();
        url.set_fragment(None);
        let target = if absolute_form {
            url.as_str().to_owned()
        } else {
            let mut target = url.path().to_string();
            if let Some(q) = url.query() {
                target.push('?');
                target.push_str(q);
            }
            target
        };
        let mut headers = self.headers.clone();
        headers.retain(|h| !h.name.eq_ignore_ascii_case("host"));
        headers.push(Header::new("host", authority(&url)));
        Head {
            method: self.method.clone(),
            target,
            status: 0,
            version: 1,
            headers,
            keep_alive: true,
        }
    }
    /// `true` means resend the modified request; `false` exposes the response as-is.
    pub fn redirect(
        &mut self,
        status: u16,
        location: Option<&str>,
        mode: RedirectMode,
        max: usize,
    ) -> Result<bool> {
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Ok(false);
        }
        if mode == RedirectMode::Manual {
            return Ok(false);
        }
        if mode == RedirectMode::Error {
            return Err(Error::new("UND_ERR_REQ_RETRY", "redirect mode is error"));
        }
        let Some(location) = location else {
            return Ok(false);
        };
        if self.redirects >= max {
            return Err(Error::new("UND_ERR_REDIRECT", "redirect count exceeded"));
        }
        let next = self
            .url
            .join(location)
            .map_err(|_| Error::new("ERR_INVALID_URL", "invalid redirect URL"))?;
        if !matches!(next.scheme(), "http" | "https")
            || !next.username().is_empty()
            || next.password().is_some()
        {
            return Err(Error::new("UND_ERR_INVALID_ARG", "invalid redirect target"));
        }
        let rewrite = (matches!(status, 301 | 302) && self.method == "POST")
            || (status == 303 && self.method != "GET" && self.method != "HEAD");
        if !rewrite && !self.replayable {
            return Err(Error::new(
                "UND_ERR_REQ_RETRY",
                "streaming body cannot be replayed",
            ));
        }
        if self.url.origin() != next.origin() {
            self.headers.retain(|h| {
                ![
                    "authorization",
                    "proxy-authorization",
                    "cookie",
                    "cookie2",
                    "host",
                ]
                .iter()
                .any(|name| h.name.eq_ignore_ascii_case(name))
            });
        }
        if rewrite {
            self.method = "GET".into();
            self.body.clear();
            self.replayable = true;
            self.headers.retain(|h| {
                ![
                    "content-encoding",
                    "content-language",
                    "content-location",
                    "content-type",
                    "content-length",
                    "transfer-encoding",
                ]
                .iter()
                .any(|name| h.name.eq_ignore_ascii_case(name))
            });
        }
        self.url = next;
        self.redirects += 1;
        Ok(true)
    }
}
fn authority(url: &Url) -> String {
    let mut result = url.host_str().unwrap_or("").to_string();
    if let Some(port) = url.port() {
        result.push(':');
        result.push_str(&port.to_string());
    }
    result
}
#[derive(Debug, Default, Clone)]
pub struct ProxyEnvironment {
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub no_proxy: String,
}
impl ProxyEnvironment {
    /// Values are supplied by the host from HTTP_PROXY/HTTPS_PROXY/NO_PROXY (or lowercase overrides).
    pub fn proxy_for(&self, url: &Url) -> Result<Option<Url>> {
        if bypass(url, &self.no_proxy) {
            return Ok(None);
        }
        let proxy = if url.scheme() == "https" {
            self.https_proxy.as_deref()
        } else {
            self.http_proxy.as_deref()
        };
        proxy
            .filter(|p| !p.is_empty())
            .map(|p| {
                let parsed =
                    Url::parse(p).map_err(|_| Error::new("ERR_INVALID_URL", "invalid proxy"))?;
                if parsed.scheme() != "http" || parsed.host_str().is_none() {
                    return Err(Error::new(
                        "UND_ERR_NOT_SUPPORTED",
                        "only HTTP proxies are supported",
                    ));
                }
                Ok(parsed)
            })
            .transpose()
    }
}
fn bypass(url: &Url, list: &str) -> bool {
    let host = url.host_str().unwrap_or("");
    list.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            let (domain, port) = if let Some((h, p)) = entry.rsplit_once(':') {
                if let Ok(p) = p.parse::<u16>() {
                    (h, Some(p))
                } else {
                    (entry, None)
                }
            } else {
                (entry, None)
            };
            if port.is_some() && port != url.port_or_known_default() {
                return false;
            }
            let domain = domain.trim_start_matches("*.").trim_start_matches('.');
            host.eq_ignore_ascii_case(domain)
                || (host.len() > domain.len()
                    && host.as_bytes()[host.len() - domain.len() - 1] == b'.'
                    && host[host.len() - domain.len()..].eq_ignore_ascii_case(domain))
        })
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolKey {
    pub origin: String,
    pub proxy: Option<String>,
}
impl PoolKey {
    pub fn new(url: &Url, proxy: Option<&Url>) -> Self {
        Self {
            origin: url.origin().ascii_serialization(),
            proxy: proxy.map(ToString::to_string),
        }
    }
}
/// Issued by [`Pool::acquire`]. Ids are never reused, so a stale id can never
/// name a newer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub u64);
/// A host-chosen key for one in-flight request's deadline in [`Pool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

/// A deadline heap keyed by `K`, holding at most one deadline per key.
///
/// [`next_timeout`](Self::next_timeout) is O(1) and
/// [`set`](Self::set)/[`pop_expired`](Self::pop_expired) are O(log n), so a host
/// with many connections or requests never rescans them to arm its timer.
/// Replaced deadlines stay in the heap until they surface and are discarded;
/// the heap is rebuilt whenever they outnumber the live ones, so its size stays
/// within a constant factor of [`len`](Self::len).
pub struct Deadlines<K> {
    heap: BinaryHeap<Reverse<Entry<K>>>,
    live: HashMap<K, (Instant, u64)>,
    sequence: u64,
}
struct Entry<K> {
    at: Instant,
    sequence: u64,
    key: K,
}
impl<K> PartialEq for Entry<K> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl<K> Eq for Entry<K> {}
impl<K> PartialOrd for Entry<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<K> Ord for Entry<K> {
    // Sequence numbers are unique, so equal deadlines fire in the order set.
    fn cmp(&self, other: &Self) -> Ordering {
        (self.at, self.sequence).cmp(&(other.at, other.sequence))
    }
}
impl<K: Copy + Eq + Hash> Default for Deadlines<K> {
    fn default() -> Self {
        Self::new()
    }
}
impl<K: Copy + Eq + Hash> Deadlines<K> {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            live: HashMap::new(),
            sequence: 0,
        }
    }
    /// Replace `key`'s deadline; `None` removes it.
    pub fn set(&mut self, key: K, deadline: Option<Instant>) {
        match deadline {
            Some(at) if self.live.get(&key).is_some_and(|&(d, _)| d == at) => return,
            Some(at) => {
                self.sequence += 1;
                self.live.insert(key, (at, self.sequence));
                self.heap.push(Reverse(Entry {
                    at,
                    sequence: self.sequence,
                    key,
                }));
            }
            None => {
                if self.live.remove(&key).is_none() {
                    return;
                }
            }
        }
        self.settle();
    }
    pub fn get(&self, key: K) -> Option<Instant> {
        self.live.get(&key).map(|&(at, _)| at)
    }
    /// The earliest live deadline.
    pub fn next_timeout(&self) -> Option<Instant> {
        self.heap.peek().map(|Reverse(entry)| entry.at)
    }
    /// Remove and return one key whose deadline is at or before `now`, earliest
    /// first. Call repeatedly until `None`.
    pub fn pop_expired(&mut self, now: Instant) -> Option<K> {
        if self.next_timeout()? > now {
            return None;
        }
        let Reverse(entry) = self.heap.pop()?;
        self.live.remove(&entry.key);
        self.settle();
        Some(entry.key)
    }
    pub fn len(&self) -> usize {
        self.live.len()
    }
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
    /// Keep the top of the heap live, so `next_timeout` can simply peek.
    fn settle(&mut self) {
        if self.heap.len() > 2 * self.live.len() + 16 {
            self.heap = self
                .live
                .iter()
                .map(|(&key, &(at, sequence))| Reverse(Entry { at, sequence, key }))
                .collect();
            return;
        }
        while let Some(Reverse(top)) = self.heap.peek() {
            if self.live.get(&top.key) == Some(&(top.at, top.sequence)) {
                break;
            }
            self.heap.pop();
        }
    }
    #[cfg(test)]
    fn heap_len(&self) -> usize {
        self.heap.len()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acquire {
    Connect(ConnectionId),
    Reuse(ConnectionId),
    Wait,
}
struct Slot {
    id: ConnectionId,
    key: PoolKey,
    active: usize,
    capacity: usize,
    connecting: bool,
    deadline: Option<Instant>,
    closed: bool,
}
/// Connection reuse plus every client-wide deadline: idle connections and, when
/// the host registers them, in-flight requests. One [`next_timeout`] covers all
/// of them without visiting each connection or request.
///
/// [`next_timeout`]: Pool::next_timeout
pub struct Pool {
    slots: Vec<Slot>,
    max_per_host: usize,
    idle: Duration,
    next: u64,
    idle_deadlines: Deadlines<ConnectionId>,
    request_deadlines: Deadlines<RequestId>,
}
impl Pool {
    pub fn new(max_per_host: usize, idle: Duration) -> Self {
        Self {
            slots: Vec::new(),
            max_per_host,
            idle,
            next: 1,
            idle_deadlines: Deadlines::new(),
            request_deadlines: Deadlines::new(),
        }
    }
    /// A Connect reservation counts immediately, so concurrent callers cannot overbook.
    pub fn acquire(&mut self, key: &PoolKey, now: Instant) -> Acquire {
        if let Some(s) = self.slots.iter_mut().find(|s| {
            !s.closed
                && s.key == *key
                && !s.connecting
                && s.active < s.capacity
                && s.deadline.is_none_or(|d| d > now)
        }) {
            s.active += 1;
            s.deadline = None;
            let id = s.id;
            self.idle_deadlines.set(id, None);
            return Acquire::Reuse(id);
        }
        if self
            .slots
            .iter()
            .filter(|s| !s.closed && s.key == *key)
            .count()
            >= self.max_per_host
        {
            return Acquire::Wait;
        }
        let id = ConnectionId(self.next);
        self.next += 1;
        let slot = Slot {
            id,
            key: key.clone(),
            active: 1,
            capacity: 1,
            connecting: true,
            deadline: None,
            closed: false,
        };
        if let Some(s) = self.slots.iter_mut().find(|s| s.closed) {
            *s = slot;
        } else {
            self.slots.push(slot);
        }
        Acquire::Connect(id)
    }
    fn slot(&mut self, id: ConnectionId) -> Result<&mut Slot> {
        self.slots
            .iter_mut()
            .find(|s| s.id == id && !s.closed)
            .ok_or(Error::new("UND_ERR_CLOSED", "unknown connection"))
    }
    pub fn connected(
        &mut self,
        id: ConnectionId,
        protocol: Protocol,
        max_streams: usize,
    ) -> Result<()> {
        let s = self.slot(id)?;
        s.connecting = false;
        s.capacity = if protocol == Protocol::Http1 {
            1
        } else {
            max_streams
        };
        Ok(())
    }
    pub fn release(&mut self, id: ConnectionId, reusable: bool, now: Instant) -> Result<()> {
        let idle = self.idle;
        let s = self.slot(id)?;
        if s.active == 0 {
            return Err(Error::new("UND_ERR_INVALID_ARG", "duplicate pool release"));
        }
        s.active -= 1;
        if !reusable {
            s.capacity = 0;
        }
        if s.active == 0 {
            let deadline = if s.capacity == 0 { now } else { now + idle };
            s.deadline = Some(deadline);
            self.idle_deadlines.set(id, Some(deadline));
        }
        Ok(())
    }
    pub fn closed(&mut self, id: ConnectionId) -> Result<()> {
        self.slot(id)?.closed = true;
        self.idle_deadlines.set(id, None);
        Ok(())
    }
    /// Whether `id` still names a live connection: acquired and neither closed,
    /// forgotten nor expired by [`handle_timeout`](Self::handle_timeout).
    pub fn contains(&self, id: ConnectionId) -> bool {
        self.slots.iter().any(|s| s.id == id && !s.closed)
    }
    /// Drop `id` whatever its state, for a host whose own socket table no longer
    /// has it (closed without telling the pool, or never found). Outstanding
    /// acquisitions go with it, so its per-host place is free for the next
    /// `acquire`. Returns whether `id` was live; a stale id is not an error.
    pub fn forget(&mut self, id: ConnectionId) -> bool {
        let live = self.slot(id).map(|s| s.closed = true).is_ok();
        self.idle_deadlines.set(id, None);
        live
    }
    /// The earliest idle-connection or registered request deadline, in O(1).
    pub fn next_timeout(&self) -> Option<Instant> {
        self.idle_deadlines
            .next_timeout()
            .into_iter()
            .chain(self.request_deadlines.next_timeout())
            .min()
    }
    /// Repeatedly call to obtain all Close requests at this time. No clock or timer is owned.
    pub fn handle_timeout(&mut self, now: Instant) -> Option<ConnectionId> {
        while let Some(id) = self.idle_deadlines.pop_expired(now) {
            if let Ok(s) = self.slot(id) {
                s.closed = true;
                return Some(id);
            }
        }
        None
    }
    /// Register (or with `None` clear) a request's next deadline, typically the
    /// `next_timeout()` of its [`Lifecycle`] or [`Http1Connection`] after each
    /// call that may move it. The pool never inspects the request itself.
    pub fn set_request_deadline(&mut self, id: RequestId, deadline: Option<Instant>) {
        self.request_deadlines.set(id, deadline);
    }
    pub fn request_deadline(&self, id: RequestId) -> Option<Instant> {
        self.request_deadlines.get(id)
    }
    /// Repeatedly call to obtain every request whose registered deadline is at or
    /// before `now`. Its registration is removed: call that request's
    /// `handle_timeout(now)` and register its new `next_timeout()`, if any.
    pub fn handle_request_timeout(&mut self, now: Instant) -> Option<RequestId> {
        self.request_deadlines.pop_expired(now)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Connect,
    Headers,
    Body,
    Done,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    Success,
    Error(Error),
}
pub struct Lifecycle {
    phase: Phase,
    deadline: Option<Instant>,
    terminal: Option<Completion>,
    delivered: bool,
}
impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            phase: Phase::Connect,
            deadline: None,
            terminal: None,
            delivered: false,
        }
    }
}
impl Lifecycle {
    pub fn transition(&mut self, phase: Phase, deadline: Option<Instant>) {
        if self.terminal.is_none() {
            self.phase = phase;
            self.deadline = deadline;
        }
    }
    pub fn next_timeout(&self) -> Option<Instant> {
        self.deadline
    }
    pub fn handle_timeout(&mut self, now: Instant) {
        if self.deadline.is_some_and(|d| d <= now) {
            let (code, message) = match self.phase {
                Phase::Connect => ("UND_ERR_CONNECT_TIMEOUT", "Connect Timeout Error"),
                Phase::Headers => ("UND_ERR_HEADERS_TIMEOUT", "Headers Timeout Error"),
                _ => ("UND_ERR_BODY_TIMEOUT", "Body Timeout Error"),
            };
            self.finish(Completion::Error(Error::new(code, message)));
        }
    }
    pub fn abort(&mut self) {
        self.finish(Completion::Error(Error::new(
            "UND_ERR_ABORTED",
            "Request aborted",
        )));
    }
    pub fn finish(&mut self, result: Completion) {
        if self.terminal.is_none() {
            self.terminal = Some(result);
            self.phase = Phase::Done;
            self.deadline = None;
        }
    }
    pub fn poll(&mut self) -> Option<Completion> {
        if self.delivered {
            return None;
        }
        let event = self.terminal?;
        self.delivered = true;
        Some(event)
    }
}
/// After a CONNECT 2xx upgrade, the host feeds remaining bytes into TLS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportRequest {
    Resolve { hostname: String, port: u16 },
    UpgradeTls { server_name: String },
    Ready(Protocol),
}
pub struct Route {
    target: Url,
    proxy: Option<Url>,
    tunnel: bool,
}
impl Route {
    pub fn new(target: Url, proxy: Option<Url>) -> Self {
        Self {
            target,
            proxy,
            tunnel: false,
        }
    }
    pub fn resolve(&self) -> TransportRequest {
        let peer = self.proxy.as_ref().unwrap_or(&self.target);
        TransportRequest::Resolve {
            hostname: peer.host_str().unwrap_or("").into(),
            port: peer.port_or_known_default().unwrap_or(80),
        }
    }
    pub fn connect_head(&self, authorization: Option<&[u8]>) -> Option<Head> {
        if self.proxy.is_none() || self.target.scheme() != "https" {
            return None;
        }
        let target = format!(
            "{}:{}",
            self.target.host_str().unwrap_or(""),
            self.target.port_or_known_default().unwrap_or(443)
        );
        let mut headers = vec![Header::new("host", &target)];
        if let Some(value) = authorization {
            headers.push(Header::new("proxy-authorization", value));
        } else if let Some(value) = self.proxy.as_ref().and_then(proxy_authorization) {
            headers.push(Header::new("proxy-authorization", value));
        }
        Some(Head {
            method: "CONNECT".into(),
            target,
            status: 0,
            version: 1,
            headers,
            keep_alive: true,
        })
    }
    /// Serialize origin-form for direct/tunnel traffic and absolute-form for HTTP proxies.
    /// Proxy credentials are never forwarded inside an HTTPS tunnel.
    pub fn request_head(&self, request: &Request, authorization: Option<&[u8]>) -> Head {
        let proxy_http = self.proxy.is_some() && self.target.scheme() == "http";
        let mut head = request.head(proxy_http);
        head.headers
            .retain(|h| !h.name.eq_ignore_ascii_case("proxy-authorization"));
        if proxy_http {
            if let Some(auth) = authorization {
                head.headers.push(Header::new("proxy-authorization", auth));
            } else if let Some(value) = self.proxy.as_ref().and_then(proxy_authorization) {
                head.headers.push(Header::new("proxy-authorization", value));
            }
        }
        head
    }
    pub fn tunnel_response(&mut self, status: u16) -> Result<TransportRequest> {
        if !(200..300).contains(&status) {
            return Err(Error::new("UND_ERR_PRX_TLS", "proxy CONNECT rejected"));
        }
        self.tunnel = true;
        Ok(TransportRequest::UpgradeTls {
            server_name: self.target.host_str().unwrap_or("").into(),
        })
    }
    pub fn connected(&self) -> Option<TransportRequest> {
        if self.target.scheme() == "https" {
            if self.proxy.is_some() && !self.tunnel {
                None
            } else {
                Some(TransportRequest::UpgradeTls {
                    server_name: self.target.host_str().unwrap_or("").into(),
                })
            }
        } else {
            Some(TransportRequest::Ready(Protocol::Http1))
        }
    }
}
pub fn transport_error(kind: std::io::ErrorKind) -> Error {
    match kind {
        std::io::ErrorKind::ConnectionRefused => Error::new("ECONNREFUSED", "connect ECONNREFUSED"),
        std::io::ErrorKind::ConnectionReset => Error::new("ECONNRESET", "read ECONNRESET"),
        std::io::ErrorKind::NotFound => Error::new("ENOTFOUND", "getaddrinfo ENOTFOUND"),
        std::io::ErrorKind::TimedOut => Error::new("ETIMEDOUT", "connect ETIMEDOUT"),
        _ => Error::new("UND_ERR_SOCKET", "socket error"),
    }
}

/// A single non-pipelined HTTP/1 client connection. All byte buffers are reused.
/// The host submits output, acknowledges completion, and supplies received bytes.
/// Do not mutate the connection while a completion-layer write borrows `output()`.
pub struct Http1Connection {
    decoder: crate::http1::Decoder,
    encoder: Option<crate::http1::Encoder>,
    output: Vec<u8>,
    output_pos: usize,
    lifecycle: Lifecycle,
    active: bool,
    used: bool,
    upload_finished: bool,
    close_requested: bool,
    expect_deadline: Option<Instant>,
    waiting_continue: bool,
}
impl Http1Connection {
    pub fn new(limits: crate::http1::Limits) -> Self {
        Self {
            decoder: crate::http1::Decoder::new(crate::http1::Mode::Response, limits),
            encoder: None,
            output: Vec::new(),
            output_pos: 0,
            lifecycle: Lifecycle::default(),
            active: false,
            used: false,
            upload_finished: false,
            close_requested: false,
            expect_deadline: None,
            waiting_continue: false,
        }
    }
    pub fn start(
        &mut self,
        head: &Head,
        length: crate::http1::BodyLength,
        headers_deadline: Option<Instant>,
        continue_deadline: Option<Instant>,
    ) -> Result<()> {
        if self.active || !self.output().is_empty() || self.used && !self.reusable() {
            return Err(Error::new(
                "UND_ERR_NOT_SUPPORTED",
                "HTTP/1 pipelining is disabled or connection closed",
            ));
        }
        let encoder = crate::http1::Encoder::start(head, length, &mut self.output)?;
        if self.used {
            self.decoder.reset()?;
        }
        self.decoder.response_to(&head.method);
        self.encoder = Some(encoder);
        self.active = true;
        self.used = true;
        self.upload_finished = false;
        self.close_requested = head.token("connection", "close");
        self.lifecycle = Lifecycle::default();
        self.lifecycle.transition(Phase::Headers, headers_deadline);
        self.waiting_continue = head.token("expect", "100-continue")
            && !matches!(
                length,
                crate::http1::BodyLength::Empty | crate::http1::BodyLength::Known(0)
            );
        self.expect_deadline = if self.waiting_continue {
            continue_deadline
        } else {
            None
        };
        Ok(())
    }
    pub fn can_send_body(&self) -> bool {
        self.active
            && !self.waiting_continue
            && !self.upload_finished
            && self.lifecycle.terminal.is_none()
    }
    pub fn send_body(&mut self, bytes: &[u8]) -> Result<()> {
        if !self.can_send_body() {
            return Err(Error::new(
                "UND_ERR_INVALID_ARG",
                "request body is not writable",
            ));
        }
        self.encoder.as_mut().unwrap().body(bytes, &mut self.output)
    }
    pub fn finish_body(&mut self, trailers: &[Header]) -> Result<()> {
        if !self.can_send_body() {
            return Err(Error::new(
                "UND_ERR_INVALID_ARG",
                "request body is not writable",
            ));
        }
        self.encoder
            .as_mut()
            .unwrap()
            .finish(trailers, &mut self.output)?;
        self.upload_finished = true;
        Ok(())
    }
    pub fn output(&self) -> &[u8] {
        &self.output[self.output_pos..]
    }
    pub fn consume_output(&mut self, n: usize) -> Result<()> {
        if n > self.output().len() {
            return Err(Error::new(
                "UND_ERR_INVALID_ARG",
                "write acknowledgement exceeds output",
            ));
        }
        self.output_pos += n;
        if self.output_pos == self.output.len() {
            self.output.clear();
            self.output_pos = 0;
        }
        Ok(())
    }
    pub fn receive<'a>(&mut self, input: &'a [u8]) -> Result<crate::http1::Step<'a>> {
        if !self.active || self.lifecycle.terminal.is_some() {
            return Err(Error::new("UND_ERR_CLOSED", "no active request"));
        }
        let step = match self.decoder.receive(input) {
            Ok(step) => step,
            Err(e) => {
                self.fail(e);
                return Err(e);
            }
        };
        match &step.event {
            Some(crate::http1::Event::Informational(h)) if h.status == 100 => {
                self.waiting_continue = false;
                self.expect_deadline = None;
            }
            Some(crate::http1::Event::Head(_)) => {
                self.expect_deadline = None;
                self.waiting_continue = false;
                if !self.upload_finished {
                    self.close_requested = true;
                    self.upload_finished = true;
                }
                self.lifecycle.transition(Phase::Body, None);
            }
            Some(crate::http1::Event::End | crate::http1::Event::Upgrade) => {
                self.lifecycle.finish(Completion::Success);
                self.active = false;
            }
            _ => {}
        }
        Ok(step)
    }
    /// Refresh body timeout on progress using host-supplied time.
    pub fn set_body_deadline(&mut self, deadline: Option<Instant>) {
        self.lifecycle.transition(Phase::Body, deadline);
    }
    pub fn eof(&mut self) -> Result<()> {
        match self.decoder.eof() {
            Ok(()) => Ok(()),
            Err(e) => {
                self.fail(e);
                Err(e)
            }
        }
    }
    pub fn next_timeout(&self) -> Option<Instant> {
        self.lifecycle
            .next_timeout()
            .into_iter()
            .chain(self.expect_deadline)
            .min()
    }
    pub fn handle_timeout(&mut self, now: Instant) {
        self.lifecycle.handle_timeout(now);
        if self.lifecycle.terminal.is_some() {
            self.active = false;
            self.close_requested = true;
            self.expect_deadline = None;
        } else if self.expect_deadline.is_some_and(|d| d <= now) {
            self.waiting_continue = false;
            self.expect_deadline = None;
        }
    }
    pub fn abort(&mut self) {
        self.lifecycle.abort();
        self.active = false;
        self.close_requested = true;
        self.expect_deadline = None;
    }
    pub fn fail(&mut self, error: Error) {
        self.lifecycle.finish(Completion::Error(error));
        self.active = false;
        self.close_requested = true;
        self.expect_deadline = None;
    }
    pub fn poll_completion(&mut self) -> Option<Completion> {
        self.lifecycle.poll()
    }
    pub fn reusable(&self) -> bool {
        !self.active && !self.close_requested && self.lifecycle.delivered && self.decoder.reusable()
    }
}

fn proxy_authorization(proxy: &Url) -> Option<String> {
    use base64::Engine;
    if proxy.username().is_empty() && proxy.password().is_none() {
        return None;
    }
    let mut credential =
        percent_encoding::percent_decode_str(proxy.username()).collect::<Vec<u8>>();
    credential.push(b':');
    credential.extend(percent_encoding::percent_decode_str(
        proxy.password().unwrap_or(""),
    ));
    Some(format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(credential)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_deadlines_do_not_accumulate() {
        let now = Instant::now();
        let mut deadlines = Deadlines::new();
        for key in 0..100u32 {
            deadlines.set(key, Some(now + Duration::from_secs(1000)));
        }
        // A request refreshing its body deadline on every chunk, far from the top.
        for step in 0..100_000u64 {
            deadlines.set(7, Some(now + Duration::from_secs(2000 + step)));
            assert!(deadlines.heap_len() <= 2 * deadlines.len() + 17);
        }
        assert_eq!(deadlines.len(), 100);
        assert_eq!(
            deadlines.next_timeout(),
            Some(now + Duration::from_secs(1000))
        );
        let late = now + Duration::from_secs(1_000_000);
        let mut popped = 0;
        while let Some(key) = deadlines.pop_expired(late) {
            assert_eq!(key == 7, popped == 99, "the refreshed key fires last");
            popped += 1;
        }
        assert_eq!(popped, 100);
        assert_eq!(deadlines.heap_len(), 0);
    }
}
