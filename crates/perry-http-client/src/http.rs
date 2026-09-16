//! A blocking HTTP/1.1 request, driven to completion on one connection.
//!
//! The protocol is `turnloop_http`'s: [`turnloop_http::client::Request`] for
//! URL/method validation and redirect policy, [`turnloop_http::client::Route`]
//! for direct-versus-proxy addressing and the CONNECT tunnel,
//! [`turnloop_http::client::Http1Connection`] for the wire codec, and
//! `turnloop_http::compression` for `Content-Encoding`. What this module adds
//! is only the driving: send the head, send the body, read until `End`.
//!
//! One request per connection. There is no pool — a CLI invocation makes a
//! handful of requests spread over minutes of build time, so a kept-alive
//! connection would be idle far longer than any server's timeout, and
//! `turnloop_http::client::Pool` exists for hosts that need it.

use std::time::Duration;

use turnloop_http::client::{
    Http1Connection, ProxyEnvironment, RedirectMode, Request as ProtocolRequest, Route,
    TransportRequest,
};
use turnloop_http::http1::{BodyLength, Event, Head, Header, Limits};
use url::Url;

use crate::transport::{self, Connection};
use crate::{Error, Response, Result};

/// Perry's CLI talks to a small number of known services; a 32 MiB ceiling on
/// a decompressed body is far above any of their responses and far below
/// anything that would exhaust a build machine.
const BODY_LIMIT: usize = 32 * 1024 * 1024;

/// Everything a caller can vary per client.
#[derive(Clone, Debug)]
pub struct Options {
    /// Whole-request budget, measured from the first connect attempt. A
    /// redirect chain shares one budget rather than resetting it per hop.
    pub timeout: Duration,
    /// What to do with a 3xx carrying a `Location`.
    pub redirect: RedirectMode,
    /// `User-Agent` sent when the request does not set one.
    pub user_agent: String,
    /// Honour `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`. On by default, because
    /// that is what `reqwest::Client::new()` did before this crate replaced it
    /// and a corporate proxy is the one environment where the difference is
    /// invisible until it is fatal.
    pub use_proxy_environment: bool,
    /// Ceiling on a buffered response body, decompressed.
    ///
    /// `reqwest` had no such ceiling, so this is a new refusal rather than a
    /// preserved one — which is why it is a field: the CLI's JSON endpoints
    /// keep the small default, and the two commands that download a build
    /// artifact into memory raise it deliberately at their call site rather
    /// than the default being raised for everything.
    pub max_body: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            redirect: RedirectMode::Follow,
            user_agent: concat!("perry/", env!("CARGO_PKG_VERSION")).to_string(),
            use_proxy_environment: true,
            max_body: BODY_LIMIT,
        }
    }
}

/// One outbound request, before it is validated against the protocol rules.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

impl Request {
    /// `url` takes anything string-shaped because almost every call site
    /// builds it with `format!`, and `reqwest`'s `IntoUrl` accepted both.
    pub fn new(method: &str, url: impl AsRef<str>) -> Self {
        Self {
            method: method.to_string(),
            url: url.as_ref().to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn get(url: impl AsRef<str>) -> Self {
        Self::new("GET", url)
    }

    pub fn post(url: impl AsRef<str>) -> Self {
        Self::new("POST", url)
    }

    pub fn put(url: impl AsRef<str>) -> Self {
        Self::new("PUT", url)
    }

    pub fn patch(url: impl AsRef<str>) -> Self {
        Self::new("PATCH", url)
    }

    pub fn delete(url: impl AsRef<str>) -> Self {
        Self::new("DELETE", url)
    }

    pub fn head(url: impl AsRef<str>) -> Self {
        Self::new("HEAD", url)
    }

    pub fn options(url: impl AsRef<str>) -> Self {
        Self::new("OPTIONS", url)
    }

    /// Append percent-encoded query parameters, `reqwest`'s `.query(&[..])`.
    ///
    /// Appends rather than replaces, and preserves a query the URL already
    /// carries — which is what `reqwest` does and what two App Store Connect
    /// call sites rely on.
    #[must_use]
    pub fn query(mut self, pairs: &[(&str, &str)]) -> Self {
        if pairs.is_empty() {
            return self;
        }
        let mut encoded = String::new();
        for (key, value) in pairs {
            if !encoded.is_empty() {
                encoded.push('&');
            }
            encoded.push_str(&percent_encode_query(key));
            encoded.push('=');
            encoded.push_str(&percent_encode_query(value));
        }
        // A `#fragment` is never sent on the wire, but splitting on it keeps
        // the URL well-formed for anything that reads `Response::url` back.
        let (before, fragment) = match self.url.split_once('#') {
            Some((b, f)) => (b.to_string(), Some(f.to_string())),
            None => (self.url.clone(), None),
        };
        let separator = if before.contains('?') { '&' } else { '?' };
        self.url = match fragment {
            Some(f) => format!("{before}{separator}{encoded}#{f}"),
            None => format!("{before}{separator}{encoded}"),
        };
        self
    }

    #[must_use]
    pub fn header(mut self, name: &str, value: impl AsRef<[u8]>) -> Self {
        self.headers
            .push((name.to_ascii_lowercase(), value.as_ref().to_vec()));
        self
    }

    /// Set a `Bearer` `Authorization` header.
    #[must_use]
    pub fn bearer(self, token: &str) -> Self {
        self.header("authorization", format!("Bearer {token}"))
    }

    #[must_use]
    pub fn body(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.body = bytes.into();
        self
    }

    /// Serialize `value` as JSON and set `Content-Type: application/json`.
    #[must_use]
    pub fn json_body(self, json: String) -> Self {
        self.header("content-type", "application/json").body(json)
    }

    fn has_header(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    }
}

/// Percent-encode one query component per RFC 3986's `unreserved` set, with
/// space as `+` — the `application/x-www-form-urlencoded` serialisation
/// `reqwest`'s `.query()` produces through `serde_urlencoded`.
fn percent_encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push_str(&format!("{other:02X}"));
            }
        }
    }
    out
}

/// Read the proxy environment once, the way `turnloop_http` expects a host to.
fn proxy_environment() -> ProxyEnvironment {
    fn var(upper: &str, lower: &str) -> Option<String> {
        // Lower case wins, matching curl and reqwest: `http_proxy` is the
        // historical spelling and `HTTP_PROXY` collides with a CGI header.
        std::env::var(lower)
            .ok()
            .or_else(|| std::env::var(upper).ok())
            .filter(|v| !v.is_empty())
    }
    ProxyEnvironment {
        http_proxy: var("HTTP_PROXY", "http_proxy"),
        https_proxy: var("HTTPS_PROXY", "https_proxy"),
        no_proxy: var("NO_PROXY", "no_proxy").unwrap_or_default(),
    }
}

/// Where a streamed response body goes.
///
/// One caller: the self-updater, which writes a release artifact of tens of
/// megabytes to disk while drawing a progress bar. Everything else in the CLI
/// reads JSON or a short error page and takes [`execute`]'s buffered body, so
/// this is deliberately a trait with two methods rather than a second client.
pub trait BodySink {
    /// Called once, when the final response's head arrives — after redirects.
    /// `content_length` is the declared length, absent for a chunked body.
    fn on_head(&mut self, status: u16, content_length: Option<u64>) -> Result<()>;
    /// Called for each body chunk, in order. Returning an error aborts the
    /// transfer, which is how a caller cancels.
    fn on_chunk(&mut self, bytes: &[u8]) -> Result<()>;
}

/// Run one request, following redirects, and return the final response.
pub fn execute(options: &Options, request: Request) -> Result<Response> {
    run(options, request, None)
}

/// Run one request and hand the final response's body to `sink` as it arrives.
///
/// The returned [`Response`] carries the status, headers and final URL with an
/// **empty** body — the bytes went to the sink. `Accept-Encoding` is not sent
/// on this path, so a `Content-Encoding` in the answer is a server that ignored
/// the request and is refused rather than silently written to disk compressed:
/// the one caller writes an archive whose bytes are then hashed against a
/// signed manifest, and a body that is not what the manifest covers must fail
/// loudly.
pub fn execute_streaming(
    options: &Options,
    request: Request,
    sink: &mut dyn BodySink,
) -> Result<Response> {
    run(options, request, Some(sink))
}

fn run(
    options: &Options,
    request: Request,
    mut sink: Option<&mut dyn BodySink>,
) -> Result<Response> {
    let mut protocol = ProtocolRequest::new(&request.url, &request.method)
        .map_err(|e| Error::protocol("invalid request", &e))?;
    for (name, value) in &request.headers {
        protocol.headers.push(Header::new(name, value));
    }
    if !request.has_header("user-agent") && !options.user_agent.is_empty() {
        protocol
            .headers
            .push(Header::new("user-agent", &options.user_agent));
    }
    if !request.has_header("accept") {
        protocol.headers.push(Header::new("accept", "*/*"));
    }
    if sink.is_none() && !request.has_header("accept-encoding") {
        protocol
            .headers
            .push(Header::new("accept-encoding", "gzip, deflate"));
    }
    protocol.body = request.body;

    let proxies = if options.use_proxy_environment {
        proxy_environment()
    } else {
        ProxyEnvironment::default()
    };

    // One budget for the whole chain. `std::time::Instant` is the only clock
    // available before a connection (and therefore a `turnloop::Instant`)
    // exists, so the chain's budget is kept here and each hop converts it.
    let started = std::time::Instant::now();
    let max_redirects = turnloop_http::client::DEFAULT_MAX_REDIRECTS;

    loop {
        let remaining = options
            .timeout
            .checked_sub(started.elapsed())
            .ok_or_else(|| Error::timeout("request"))?;
        // A redirect hop's body is discarded, so the sink must not see it —
        // it is offered only once the status says this response is final.
        // `Request::redirect` is what decides that, and it needs the status
        // first, so the hop runs with the sink withheld and the body buffered;
        // a 3xx body is a few hundred bytes of HTML at most.
        let probe = one_hop(&protocol, &proxies, remaining, options.max_body, None)?;
        let location = probe
            .header("location")
            .map(|v| String::from_utf8_lossy(v).into_owned());
        let resend = protocol
            .redirect(
                probe.status,
                location.as_deref(),
                options.redirect,
                max_redirects,
            )
            .map_err(|e| Error::protocol("redirect", &e))?;
        if resend {
            continue;
        }
        match sink.take() {
            // Streaming and final: re-issue the hop with the sink attached.
            // A GET/HEAD is the only shape this path serves and both are
            // idempotent, so re-issuing is safe; anything else is refused
            // rather than sent twice.
            Some(sink) => {
                if !matches!(protocol.method.as_str(), "GET" | "HEAD") {
                    return Err(Error::other(
                        "a streamed response body is only supported for GET and HEAD",
                    ));
                }
                let remaining = options
                    .timeout
                    .checked_sub(started.elapsed())
                    .ok_or_else(|| Error::timeout("request"))?;
                return one_hop(&protocol, &proxies, remaining, options.max_body, Some(sink));
            }
            None => return Ok(probe),
        }
    }
}

/// One connection's worth of work: connect, optionally tunnel and upgrade,
/// write the request, read the response.
fn one_hop(
    request: &ProtocolRequest,
    proxies: &ProxyEnvironment,
    budget: Duration,
    max_body: usize,
    sink: Option<&mut dyn BodySink>,
) -> Result<Response> {
    let target: Url = request.url.clone();
    let proxy = proxies
        .proxy_for(&target)
        .map_err(|e| Error::protocol("proxy", &e))?;
    let mut route = Route::new(target.clone(), proxy);

    let TransportRequest::Resolve { hostname, port } = route.resolve() else {
        return Err(Error::other("route did not ask to resolve"));
    };
    let addrs = transport::resolve(&hostname, port).map_err(|e| Error::io("resolve", e))?;

    // The connect budget is the whole remaining budget: a server that is slow
    // to accept is not different in kind from one slow to answer.
    let mut conn = Connection::connect(&addrs, budget).map_err(|e| Error::io("connect", e))?;
    // Re-anchored after the connect: the response budget is what is left of
    // the caller's window, measured on the loop's own clock.
    let deadline = conn.deadline_in(budget);

    // An HTTPS request through an HTTP proxy needs a CONNECT tunnel first.
    if let Some(head) = route.connect_head(None) {
        let status = exchange_connect(&mut conn, &head, deadline)?;
        let next = route
            .tunnel_response(status)
            .map_err(|e| Error::protocol("proxy CONNECT", &e))?;
        if let TransportRequest::UpgradeTls { server_name } = next {
            upgrade_tls(&mut conn, &server_name, deadline)?;
        }
    } else if let Some(TransportRequest::UpgradeTls { server_name }) = route.connected() {
        upgrade_tls(&mut conn, &server_name, deadline)?;
    }

    let head = route.request_head(request, None);
    exchange(
        &mut conn,
        &head,
        &request.body,
        &target,
        deadline,
        max_body,
        sink,
    )
}

fn upgrade_tls(
    conn: &mut Connection,
    server_name: &str,
    deadline: turnloop::Instant,
) -> Result<()> {
    let config = crate::tls::client_config()?;
    conn.start_tls(config, server_name, deadline)
        .map_err(|e| Error::io("TLS handshake", e))?;
    Ok(())
}

/// Send a CONNECT and read its status line. The tunnel is established in place
/// on the same connection, so nothing is returned but the status.
fn exchange_connect(
    conn: &mut Connection,
    head: &Head,
    deadline: turnloop::Instant,
) -> Result<u16> {
    let mut http = Http1Connection::new(Limits::default());
    http.start(head, BodyLength::Empty, Some(deadline), None)
        .map_err(|e| Error::protocol("CONNECT", &e))?;
    http.finish_body(&[])
        .map_err(|e| Error::protocol("CONNECT", &e))?;
    drain_output(conn, &mut http, deadline)?;

    let mut pending = Vec::new();
    let mut scratch = Vec::new();
    loop {
        scratch.clear();
        let n = conn
            .read(&mut scratch, deadline)
            .map_err(|e| Error::io("CONNECT response", e))?;
        if n == 0 {
            return Err(Error::other("proxy closed before answering CONNECT"));
        }
        pending.extend_from_slice(&scratch);
        let mut offset = 0usize;
        loop {
            let step = http
                .receive(&pending[offset..])
                .map_err(|e| Error::protocol("CONNECT", &e))?;
            let consumed = step.consumed;
            let produced = step.event.is_some();
            offset += consumed;
            if let Some(Event::Head(head)) = step.event {
                return Ok(head.status);
            }
            // Same rule as `exchange`: step again unless the decoder both
            // consumed nothing and produced nothing.
            if !produced && consumed == 0 {
                break;
            }
        }
        pending.drain(..offset);
    }
}

/// Write the head and body, then read the response to `End`.
#[allow(clippy::too_many_arguments)]
fn exchange(
    conn: &mut Connection,
    head: &Head,
    body: &[u8],
    url: &Url,
    deadline: turnloop::Instant,
    max_body: usize,
    mut sink: Option<&mut dyn BodySink>,
) -> Result<Response> {
    let mut http = Http1Connection::new(Limits::default());
    let length = if body.is_empty() && !matches!(head.method.as_str(), "POST" | "PUT" | "PATCH") {
        BodyLength::Empty
    } else {
        // A bodyless POST still gets `content-length: 0`; Node sends one and
        // P6 recorded its absence as a divergence worth fixing.
        BodyLength::Known(body.len() as u64)
    };
    http.start(head, length, Some(deadline), None)
        .map_err(|e| Error::protocol("request", &e))?;
    if !body.is_empty() {
        http.send_body(body)
            .map_err(|e| Error::protocol("request body", &e))?;
    }
    http.finish_body(&[])
        .map_err(|e| Error::protocol("request body", &e))?;
    drain_output(conn, &mut http, deadline)?;

    let mut status = 0u16;
    let mut headers: Vec<(String, Vec<u8>)> = Vec::new();
    let mut raw_body: Vec<u8> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut scratch: Vec<u8> = Vec::new();
    let mut finished = false;

    while !finished {
        scratch.clear();
        let n = conn
            .read(&mut scratch, deadline)
            .map_err(|e| Error::io("response", e))?;
        if n == 0 {
            if status == 0 {
                return Err(Error::other("server closed before sending a response"));
            }
            // A `Connection: close` body ends at EOF; tell the decoder so it
            // can settle a length-less body rather than waiting forever.
            http.eof().map_err(|e| Error::protocol("response", &e))?;
            break;
        }
        pending.extend_from_slice(&scratch);

        let mut offset = 0usize;
        while !finished {
            let step = http
                .receive(&pending[offset..])
                .map_err(|e| Error::protocol("response", &e))?;
            let consumed = step.consumed;
            let produced = step.event.is_some();
            match step.event {
                Some(Event::Head(h)) => {
                    status = h.status;
                    headers = h
                        .headers
                        .into_iter()
                        .map(|header| (header.name, header.value))
                        .collect();
                    if let Some(sink) = sink.as_deref_mut() {
                        if let Some(encoding) = content_encoding(&headers) {
                            return Err(Error::other(format!(
                                "server applied content-encoding {encoding:?} to a streamed \
                                 response, which was not asked for"
                            )));
                        }
                        let length = headers
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| String::from_utf8_lossy(v).trim().parse().ok());
                        sink.on_head(status, length)?;
                    }
                }
                Some(Event::Informational(_)) => {}
                Some(Event::Body(bytes)) => match sink.as_deref_mut() {
                    Some(sink) => sink.on_chunk(bytes)?,
                    None => {
                        if raw_body.len() + bytes.len() > max_body {
                            return Err(Error::other(format!(
                                "response body exceeds {max_body} bytes"
                            )));
                        }
                        raw_body.extend_from_slice(bytes);
                    }
                },
                Some(Event::Trailers(_)) => {}
                Some(Event::End) => finished = true,
                Some(Event::Upgrade) => {
                    return Err(Error::other("unexpected protocol upgrade"));
                }
                None => {}
            }
            offset += consumed;
            // A step that neither consumed input nor produced an event is the
            // decoder asking for more bytes. Everything else must be stepped
            // again even with `offset == pending.len()`: `Event::End` comes
            // from a transition that consumes NOTHING, so a loop bounded by
            // "while there is input left" never sees a keep-alive response
            // finish. That is P6's five-second bug; here it was a 30-second
            // one, because there is no pool to reuse and no keep-alive timeout
            // to rescue it — see the regression test below.
            if !produced && consumed == 0 {
                break;
            }
        }
        pending.drain(..offset);
        // `drain_output` again: a 100-continue or a keep-alive probe can make
        // the codec produce bytes while the response is being read.
        drain_output(conn, &mut http, deadline)?;
    }

    let body = if sink.is_some() {
        Vec::new()
    } else {
        decode_body(&headers, raw_body, max_body)?
    };
    conn.shutdown();
    Ok(Response {
        status,
        headers,
        body,
        url: url.to_string(),
    })
}

/// Move whatever the codec has produced onto the socket.
fn drain_output(
    conn: &mut Connection,
    http: &mut Http1Connection,
    deadline: turnloop::Instant,
) -> Result<()> {
    loop {
        let out = http.output().to_vec();
        if out.is_empty() {
            return Ok(());
        }
        conn.write_all(&out, deadline)
            .map_err(|e| Error::io("write", e))?;
        http.consume_output(out.len())
            .map_err(|e| Error::protocol("write", &e))?;
    }
}

/// Apply `Content-Encoding`. `reqwest` did this transparently, so a caller
/// that used to read JSON out of a gzipped response must keep doing so.
/// The response's effective `Content-Encoding`, or `None` when it is absent,
/// empty or `identity`.
fn content_encoding(headers: &[(String, Vec<u8>)]) -> Option<String> {
    let (_, value) = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))?;
    let encoding = String::from_utf8_lossy(value).trim().to_ascii_lowercase();
    if encoding.is_empty() || encoding == "identity" {
        return None;
    }
    Some(encoding)
}

fn decode_body(headers: &[(String, Vec<u8>)], raw: Vec<u8>, limit: usize) -> Result<Vec<u8>> {
    let Some(encoding) = content_encoding(headers) else {
        return Ok(raw);
    };
    let mut out = Vec::new();
    // `decode` walks a comma-separated list right-to-left itself.
    turnloop_http::compression::decode(&encoding, &raw, &mut out, limit)
        .map_err(|e| Error::protocol("content-encoding", &e))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keep-alive response: no `Connection: close`, so nothing but the
    /// decoder's own `End` can finish it.
    const KEEP_ALIVE_RESPONSE: &[u8] =
        b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 5\r\n\r\nhello";

    /// Feed `input` to a fresh connection under one of the two loop rules and
    /// report whether the response completed.
    ///
    /// `bounded_by_input` is the rule this module shipped with and which this
    /// test exists to forbid: step only while there are bytes left. The other
    /// is the rule the module uses now: step until a step both consumes
    /// nothing and produces nothing.
    fn drives_to_end(input: &[u8], bounded_by_input: bool) -> bool {
        let mut http = Http1Connection::new(Limits::default());
        let head = Head {
            method: "GET".into(),
            target: "/".into(),
            status: 0,
            version: 1,
            headers: vec![Header::new("host", "example.invalid")],
            keep_alive: true,
        };
        http.start(&head, BodyLength::Empty, None, None)
            .expect("start");
        http.finish_body(&[]).expect("finish_body");
        let _ = http.output().to_vec();
        let len = http.output().len();
        http.consume_output(len).expect("consume_output");

        let mut offset = 0usize;
        let mut finished = false;
        loop {
            if bounded_by_input && offset >= input.len() {
                break;
            }
            if finished {
                break;
            }
            let step = http.receive(&input[offset..]).expect("receive");
            let consumed = step.consumed;
            let produced = step.event.is_some();
            if matches!(step.event, Some(Event::End)) {
                finished = true;
            }
            offset += consumed;
            if !bounded_by_input && !produced && consumed == 0 {
                break;
            }
        }
        finished
    }

    /// The bug this module shipped with, pinned from both sides.
    ///
    /// `turnloop_http`'s `http1::Decoder` emits `Event::End` from a transition
    /// that consumes **zero** bytes, so a feed loop written as "while there is
    /// input left" hands over every byte of a keep-alive response and never
    /// sees it complete. The request then sits until the whole-request deadline
    /// expires — which is what every real server did here, while a local
    /// `Connection: close` fixture passed, because the peer's EOF finished it
    /// instead. Same defect P6 recorded as its five-second bug.
    ///
    /// The first assertion is what keeps this test honest: if the decoder ever
    /// starts consuming a byte for `End`, the old rule would pass too and this
    /// test would stop discriminating — so it fails rather than going quiet.
    #[test]
    fn the_end_event_arrives_from_a_step_that_consumes_nothing() {
        assert!(
            !drives_to_end(KEEP_ALIVE_RESPONSE, true),
            "the input-bounded rule must NOT see End — if it does, this test no \
             longer discriminates and the comment above is stale"
        );
        assert!(
            drives_to_end(KEEP_ALIVE_RESPONSE, false),
            "the shipped rule must drive a keep-alive response to End"
        );
    }

    /// The `Connection: close` shape that hid the bug, for contrast: it too
    /// must complete on data alone, without relying on the peer's EOF.
    #[test]
    fn a_connection_close_response_also_completes_on_data_alone() {
        let input = b"HTTP/1.1 200 OK\r\nconnection: close\r\ncontent-length: 5\r\n\r\nhello";
        assert!(drives_to_end(input, false));
    }

    #[test]
    fn a_chunked_response_completes_on_its_terminator() {
        let input = b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        assert!(drives_to_end(input, false));
    }

    #[test]
    fn query_parameters_are_appended_and_encoded() {
        let request = Request::get("https://api.example.invalid/v1/certificates")
            .query(&[("filter[certificateType]", "IOS_DEVELOPMENT,DEVELOPMENT")]);
        assert_eq!(
            request.url,
            "https://api.example.invalid/v1/certificates\
             ?filter%5BcertificateType%5D=IOS_DEVELOPMENT%2CDEVELOPMENT"
        );
    }

    /// Two App Store Connect call sites put a parameter in the literal URL and
    /// then add more with `.query()`; `reqwest` appended, and so must this.
    #[test]
    fn query_parameters_append_to_an_existing_query() {
        let request = Request::get("https://x.invalid/v1/devices?limit=200")
            .query(&[("filter[platform]", "IOS")]);
        assert_eq!(
            request.url,
            "https://x.invalid/v1/devices?limit=200&filter%5Bplatform%5D=IOS"
        );
    }

    #[test]
    fn query_parameters_stay_ahead_of_a_fragment() {
        let request = Request::get("https://x.invalid/p#frag").query(&[("a", "b c")]);
        assert_eq!(request.url, "https://x.invalid/p?a=b+c#frag");
    }

    #[test]
    fn an_empty_query_changes_nothing() {
        let request = Request::get("https://x.invalid/p").query(&[]);
        assert_eq!(request.url, "https://x.invalid/p");
    }

    #[test]
    fn an_identity_encoding_is_left_alone() {
        let headers = vec![("content-encoding".to_string(), b"identity".to_vec())];
        assert_eq!(
            decode_body(&headers, b"raw".to_vec(), BODY_LIMIT).unwrap(),
            b"raw"
        );
    }

    #[test]
    fn a_missing_content_encoding_is_left_alone() {
        assert_eq!(
            decode_body(&[], b"raw".to_vec(), BODY_LIMIT).unwrap(),
            b"raw"
        );
    }
}
