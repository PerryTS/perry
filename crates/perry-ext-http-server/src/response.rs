//! `ServerResponse` — the Node.js Writable stream returned to a
//! `(req, res) => …` handler. Phase 1 buffers chunks until `.end()`
//! is called, then sends the assembled response back to hyper via
//! the per-request oneshot channel.

use std::collections::HashMap;
use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::{Body, Frame, SizeHint};
use hyper::header::{HeaderName, HeaderValue};
use hyper::{HeaderMap, Response, StatusCode};
use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, register_handle, JsClosure, JsValue,
    RawClosureHeader, StringHeader,
};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::oneshot;

use crate::request::{emit_no_arg_to_listeners, handle_to_pointer_f64};
use crate::types::{
    js_json_stringify, js_node_setheaders_entries_json, js_value_is_closure, jsvalue_to_body_bytes,
    jsvalue_to_owned_string, read_string_header, PTR_MASK, STRING_TAG, TAG_FALSE, TAG_NULL,
    TAG_TRUE, TAG_UNDEFINED,
};

/// Node's default `highWaterMark` for an HTTP `OutgoingMessage` (16 KiB).
/// `res.write()` returns `false` once the buffered body grows past this,
/// signalling backpressure so producer loops (`while (res.write(buf))`)
/// terminate instead of spinning forever (#4909).
const DEFAULT_HIGH_WATER_MARK: usize = 16 * 1024;

pub type ResponseBody = BoxBody<Bytes, Infallible>;

// ------------------------------------------------------------------
// #4907 — Node-compatible header / argument validation.
//
// `res.setHeader` / `res.removeHeader` throw after headers are sent, and
// `setHeader` rejects non-token field names. `res.writeEarlyHints` validates
// its `hints` argument. Each throws an `ERR_*`-coded error that unwinds back
// to the JS handler frame.
// ------------------------------------------------------------------

/// Node HTTP token bytes (`tchar`, mirrored from `lib/_http_common.js`).
fn http_is_token_byte(b: u8) -> bool {
    matches!(b,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
        | b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*'
        | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
}

fn http_is_valid_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(http_is_token_byte)
}

/// Returns whether the response's headers have already been flushed. A throw
/// must fire even when the handle has gone away, so callers check this before
/// touching state.
fn response_headers_sent(handle: i64) -> bool {
    get_handle::<ServerResponse>(handle)
        .map(|sr| sr.headers_sent)
        .unwrap_or(false)
}

/// `lib/internal/http.js` link-header format: a `<uri>` followed by at least
/// one `;`-separated parameter. Faithful enough for the invalid cases the
/// corpus exercises (`'</>; '`, `'rel=preload; </scripts.js>'`,
/// `'invalid string'`).
fn is_valid_link_header(value: &str) -> bool {
    let s = value.trim();
    if !s.starts_with('<') {
        return false;
    }
    let close = match s.find('>') {
        Some(i) => i,
        None => return false,
    };
    let rest = s[close + 1..].trim_start();
    if !rest.starts_with(';') {
        return false;
    }
    let params = rest[1..].trim();
    if params.is_empty() {
        return false;
    }
    params.split(';').all(|p| !p.trim().is_empty())
}

struct TrailerBody {
    body: Option<Bytes>,
    trailers: Option<HeaderMap>,
}

/// One frame of a streaming response body: a data chunk from
/// `res.write(...)`, or the trailer block from `res.addTrailers` delivered
/// after the final chunk.
pub enum StreamFrame {
    Data(Bytes),
    Trailers(HeaderMap),
}

/// Streaming response body — frames flow from the JS thread
/// (`res.write`/`res.end`) through an unbounded channel into hyper. The
/// channel closing (sender dropped at `.end()`) ends the body. Size hint
/// stays unknown so hyper uses chunked transfer-encoding, matching Node's
/// wire behavior for a response whose headers flush before the body is
/// complete. `in_flight` tracks bytes queued but not yet handed to hyper —
/// the JS side reads it for the `res.write()` backpressure return and the
/// `'drain'` edge.
pub struct ChannelBody {
    rx: tokio::sync::mpsc::UnboundedReceiver<StreamFrame>,
    in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Body for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(StreamFrame::Data(b))) => {
                self.in_flight
                    .fetch_sub(b.len(), std::sync::atomic::Ordering::AcqRel);
                Poll::Ready(Some(Ok(Frame::data(b))))
            }
            Poll::Ready(Some(StreamFrame::Trailers(t))) => {
                Poll::Ready(Some(Ok(Frame::trailers(t))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::new()
    }
}

/// The body half of a [`HyperResponseShape`]: fully buffered (the classic
/// single-shot `res.end(body)` path, which keeps Content-Length semantics)
/// or streaming (headers flushed early by `res.flushHeaders()` /
/// `res.write(...)`, body frames following over a channel).
pub enum ShapeBody {
    Full(Vec<u8>),
    Stream {
        rx: tokio::sync::mpsc::UnboundedReceiver<StreamFrame>,
        in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    },
}

impl Body for TrailerBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if let Some(body) = self.body.take() {
            return Poll::Ready(Some(Ok(Frame::data(body))));
        }
        if let Some(trailers) = self.trailers.take() {
            return Poll::Ready(Some(Ok(Frame::trailers(trailers))));
        }
        Poll::Ready(None)
    }

    fn size_hint(&self) -> SizeHint {
        // Keep the upper bound unknown when trailers are present so Hyper
        // does not synthesize Content-Length and suppress trailing headers.
        SizeHint::new()
    }
}

/// Per-request handle backing `ServerResponse` JS-side.
pub struct ServerResponse {
    pub status_code: u16,
    pub status_message: Option<String>,
    /// Lowercase-keyed header map (the lookup table). For array-valued
    /// headers this holds Node's scalar coercion (the array's elements
    /// joined with `, `); the per-element values live in
    /// `header_value_lists` so the wire layer can emit one line each.
    pub headers: HashMap<String, String>,
    /// Lowercase-keyed multi-value header map. Populated when a header is
    /// assigned an array value (e.g. `Set-Cookie`): Node emits one header
    /// line per element rather than a single comma-joined line, so the wire
    /// serializer expands these into repeated `name: value` lines (#4826).
    pub header_value_lists: HashMap<String, Vec<String>>,
    /// Lowercase-keyed trailer map for HTTP trailers emitted after the
    /// response body, per Node's `ServerResponse.addTrailers` contract.
    pub trailers: HashMap<String, String>,
    /// Lowercase → original-case map so `getHeaderNames()` returns
    /// what the user originally set (matches Node behavior).
    pub raw_header_names: HashMap<String, String>,
    pub raw_trailer_names: HashMap<String, String>,
    pub headers_sent: bool,
    /// True once `writeHead()` has committed the status line + headers (Node's
    /// `_header`). Distinct from `headers_sent` (the wire flush, set at
    /// `write`/`end`) so the normal deferred-send path is unaffected, but a
    /// post-`writeHead` `setHeaders()` still throws `ERR_HTTP_HEADERS_SENT`
    /// like Node (#4965).
    pub header_committed: bool,
    pub writable_ended: bool,
    pub writable_finished: bool,
    pub send_date: bool,
    pub strict_content_length: bool,
    pub req_handle: i64,
    /// True for direct `new http.OutgoingMessage()` handles. They share the
    /// outgoing header/writable surface but are not a live ServerResponse.
    pub outgoing_message_only: bool,
    /// Body chunks accumulated by `.write(chunk)` calls. Assembled
    /// + flushed when `.end()` is called.
    pub buffered_body: Vec<u8>,
    /// One-shot back to hyper's service fn — taken on `.end()`, or earlier
    /// by `begin_streaming` when the headers flush before the body is done.
    pub response_tx: Option<oneshot::Sender<HyperResponseShape>>,
    /// Live body channel once the response head has been flushed early
    /// (`res.flushHeaders()` / first `res.write(...)`). `Some` means
    /// streaming mode: subsequent chunks go straight to the wire and
    /// `.end()` closes the channel instead of sending a buffered shape.
    pub stream_tx: Option<tokio::sync::mpsc::UnboundedSender<StreamFrame>>,
    /// Bytes written to the stream channel but not yet handed to hyper.
    /// Backs the `res.write()` backpressure return (`false` past the HWM)
    /// and the `'drain'` edge the pump emits when it sinks below it again.
    pub stream_in_flight: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    /// True after a streaming `res.write()` returned `false`; the pump
    /// fires `'drain'` (once) when the in-flight count drops below the HWM.
    pub needs_drain: bool,
    /// Event-name → list of registered listener closure pointers.
    pub listeners: HashMap<String, Vec<i64>>,
    /// #4904: true for `new http.ServerResponse(req)` instances (and any
    /// response wired through `assignSocket`) — `.end()` flushes through
    /// `standalone_socket` instead of the hyper oneshot.
    pub standalone: bool,
    /// #4904: the JS Writable assigned via `res.assignSocket(socket)`.
    /// `TAG_UNDEFINED` while unassigned.
    pub standalone_socket: f64,
    /// #4904: `req.method` captured from the standalone constructor's
    /// request argument — `HEAD` suppresses the body on flush.
    pub standalone_req_method: Option<String>,
    /// #4904: `res.write(chunk, cb)` callbacks, invoked in order when the
    /// buffered body flushes on `.end()`.
    pub pending_write_callbacks: Vec<i64>,
}

/// Owned shape produced by `.end()` — the per-request oneshot channel
/// drops back to hyper carrying this.
pub struct HyperResponseShape {
    pub status: u16,
    pub status_message: Option<String>,
    pub headers: Vec<(String, String)>,
    pub trailers: Vec<(String, String)>,
    pub body: ShapeBody,
}

impl HyperResponseShape {
    /// Build a hyper `Response<BoxBody<Bytes, Infallible>>` ready to return from the
    /// service fn.
    pub fn into_hyper(self) -> Response<ResponseBody> {
        // Fast path: common status codes resolve to a pre-validated
        // `StatusCode` constant, skipping `from_u16`'s numeric range-check
        // + `unwrap_or` on every response. Uncommon / custom codes keep the
        // parsing path, so the resulting status is identical for every code.
        let status = crate::response_fast::status_code_const(self.status)
            .unwrap_or_else(|| StatusCode::from_u16(self.status).unwrap_or(StatusCode::OK));
        let mut builder = Response::builder().status(status);
        // `res.statusMessage = 'Custom Message'` must reach the HTTP/1
        // status line (test-http-status-message reads it off the raw
        // socket). hyper emits it via the ReasonPhrase extension.
        if let Some(msg) = self.status_message.as_deref() {
            if !msg.is_empty() {
                if let Ok(reason) = hyper::ext::ReasonPhrase::try_from(msg.to_string()) {
                    if let Some(ext) = builder.extensions_mut() {
                        ext.insert(reason);
                    }
                }
            }
        }
        for (k, v) in self.headers {
            builder = builder.header(k, v);
        }
        let full = match self.body {
            ShapeBody::Stream { rx, in_flight } => {
                return builder.body(ChannelBody { rx, in_flight }.boxed()).unwrap();
            }
            ShapeBody::Full(bytes) => bytes,
        };
        let trailers = self.trailers;
        let body = if trailers.is_empty() {
            Full::new(Bytes::from(full)).boxed()
        } else {
            let mut map = HeaderMap::new();
            for (name, value) in trailers {
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(&value),
                ) {
                    map.insert(name, value);
                }
            }
            TrailerBody {
                body: Some(Bytes::from(full)),
                trailers: Some(map),
            }
            .boxed()
        };
        builder.body(body).unwrap()
    }

    /// Inject Node-compatible default `Connection` / `Keep-Alive` headers
    /// (#2132). Node's HTTP/1.x server appends `Connection: keep-alive` plus
    /// `Keep-Alive: timeout=<keepAliveTimeout/1000>` whenever the connection
    /// is kept alive, and `Connection: close` otherwise. Hyper drives the
    /// transport-level keep-alive itself but does not surface these headers in
    /// the response bytes, so byte-for-byte parity tests — and any client
    /// reading `res.headers.connection` / `res.headers['keep-alive']` — see
    /// them missing. Add them before handing the shape to hyper, unless the
    /// handler already set a `Connection` header explicitly. HTTP/2 manages
    /// connection reuse at the protocol level, so it gets neither header.
    pub fn apply_default_connection_headers(
        &mut self,
        version: hyper::Version,
        req_connection: Option<&str>,
        keep_alive_timeout_ms: f64,
    ) {
        if self
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("connection"))
        {
            return;
        }
        if matches!(version, hyper::Version::HTTP_2 | hyper::Version::HTTP_3) {
            return;
        }

        let conn_lower = req_connection.map(str::to_ascii_lowercase);
        let has_token = |tok: &str| {
            conn_lower
                .as_deref()
                .map(|c| c.split(',').any(|t| t.trim() == tok))
                .unwrap_or(false)
        };

        // HTTP/1.0 defaults to close (keep-alive only when explicitly
        // requested); HTTP/1.1 defaults to keep-alive unless asked to close.
        let should_keep_alive = if version == hyper::Version::HTTP_10 {
            has_token("keep-alive")
        } else {
            !has_token("close")
        };

        if should_keep_alive && keep_alive_timeout_ms > 0.0 {
            self.headers
                .push(("Connection".to_string(), "keep-alive".to_string()));
            let secs = (keep_alive_timeout_ms / 1000.0).floor().max(0.0) as u64;
            // Fast path: the `Keep-Alive: timeout=N` value is interned for the
            // timeouts servers commonly run with (Node's 5 s default, etc.), so
            // the per-response `format!` only fires for an unusual timeout. The
            // interned string equals `format!("timeout={}", secs)` exactly.
            let value = crate::response_fast::keep_alive_header_value(secs)
                .map(str::to_string)
                .unwrap_or_else(|| format!("timeout={}", secs));
            self.headers.push(("Keep-Alive".to_string(), value));
        } else {
            self.headers
                .push(("Connection".to_string(), "close".to_string()));
        }
    }
}

impl ServerResponse {
    pub fn new(response_tx: oneshot::Sender<HyperResponseShape>) -> Self {
        Self {
            status_code: 200,
            status_message: None,
            headers: HashMap::new(),
            header_value_lists: HashMap::new(),
            trailers: HashMap::new(),
            raw_header_names: HashMap::new(),
            header_committed: false,
            raw_trailer_names: HashMap::new(),
            headers_sent: false,
            writable_ended: false,
            writable_finished: false,
            send_date: true,
            strict_content_length: false,
            req_handle: 0,
            outgoing_message_only: false,
            buffered_body: Vec::new(),
            response_tx: Some(response_tx),
            stream_tx: None,
            stream_in_flight: None,
            needs_drain: false,
            listeners: HashMap::new(),
            standalone: false,
            standalone_socket: f64::from_bits(TAG_UNDEFINED),
            standalone_req_method: None,
            pending_write_callbacks: Vec::new(),
        }
    }

    pub fn outgoing_message() -> Self {
        let (tx, _rx) = oneshot::channel::<HyperResponseShape>();
        let mut response = Self::new(tx);
        response.send_date = false;
        response.outgoing_message_only = true;
        response
    }

    pub fn with_request_handle(mut self, req_handle: i64) -> Self {
        self.req_handle = req_handle;
        self
    }

    /// Snapshot the current header map as `Vec<(orig_name, value)>`
    /// preserving original case. Array-valued headers (tracked in
    /// `header_value_lists`) expand to one entry per element so the wire
    /// layer emits a separate header line each (#4826).
    pub fn snapshot_headers(&self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.headers.len());
        for (lower_k, v) in &self.headers {
            let orig = self
                .raw_header_names
                .get(lower_k)
                .cloned()
                .unwrap_or_else(|| lower_k.clone());
            if let Some(values) = self.header_value_lists.get(lower_k) {
                for elem in values {
                    out.push((orig.clone(), elem.clone()));
                }
            } else {
                out.push((orig, v.clone()));
            }
        }
        out
    }

    fn snapshot_trailers(&self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.trailers.len());
        for (lower_k, v) in &self.trailers {
            let orig = self
                .raw_trailer_names
                .get(lower_k)
                .cloned()
                .unwrap_or_else(|| lower_k.clone());
            out.push((orig, v.clone()));
        }
        out
    }

    /// Auto-fill `Content-Length` if unset and we know the full body.
    fn ensure_content_length(&mut self) {
        // A response with trailers must not declare a fixed Content-Length:
        // the body length alone doesn't bound the response (trailing headers
        // still follow), and some clients/proxies treat a present
        // Content-Length as "body complete, no trailers expected".
        if !self.trailers.is_empty() {
            return;
        }
        if !self.headers.contains_key("content-length")
            && !self.headers.contains_key("transfer-encoding")
        {
            let len = self.buffered_body.len();
            self.headers
                .insert("content-length".to_string(), len.to_string());
            self.raw_header_names
                .insert("content-length".to_string(), "Content-Length".to_string());
        }
    }
}

// ============================================================================
// FFI surface
// ============================================================================

/// `res.statusCode = N` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_status(handle: i64, code: f64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent && code.is_finite() && code > 0.0 {
            sr.status_code = code as u16;
        }
    }
}

/// `res.statusCode` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_status(handle: i64) -> f64 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| {
            if sr.outgoing_message_only {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                sr.status_code as f64
            }
        })
        .unwrap_or(200.0)
}

/// `res.statusMessage = "..."` setter.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_status_message(
    handle: i64,
    msg_ptr: *const StringHeader,
) {
    let msg = read_string_header(msg_ptr as *mut _);
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            sr.status_message = msg;
        }
    }
}

/// `res.setHeader(name, value)`. `value` arrives as a raw NaN-boxed
/// JSValue (`NA_F64`) so array values (e.g. `Set-Cookie`) can be detected
/// and stored as a per-element list — Node emits one header line per array
/// element rather than a single comma-joined / JSON-stringified line
/// (#4826). Scalar values are coerced to a string as before.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_header(
    handle: i64,
    name_ptr: *const StringHeader,
    value: f64,
) {
    let name = read_string_header(name_ptr as *mut _).unwrap_or_default();
    // #4907 — Node's `OutgoingMessage.setHeader` throws if headers are already
    // sent, then validates the field name, before touching any state.
    if response_headers_sent(handle) {
        perry_ffi::throw_with_code(
            "Cannot set headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    if name.is_empty() {
        return;
    }
    if !http_is_valid_token(&name) {
        perry_ffi::throw_with_code(
            &format!("Header name must be a valid HTTP token [\"{name}\"]"),
            "ERR_INVALID_HTTP_TOKEN",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    let lower = name.to_lowercase();

    // Detect an array value via JSON: an array serializes to `[ … ]` and
    // parses back to `serde_json::Value::Array`. Anything else is coerced
    // to its string form (matching the previous `NA_STR` behavior).
    let jsv = JsValue::from_bits(value.to_bits());
    let array_elems: Option<Vec<String>> = if jsv.is_pointer() {
        let ptr = js_json_stringify(value, 0);
        if ptr.is_null() {
            None
        } else {
            read_string_header(ptr).and_then(|json| {
                match serde_json::from_str::<serde_json::Value>(&json) {
                    Ok(serde_json::Value::Array(items)) => Some(
                        items
                            .into_iter()
                            .map(|item| match item {
                                serde_json::Value::String(s) => s,
                                other => other.to_string(),
                            })
                            .collect(),
                    ),
                    _ => None,
                }
            })
        }
    } else {
        None
    };

    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            if let Some(elems) = array_elems {
                sr.headers.insert(lower.clone(), elems.join(", "));
                sr.header_value_lists.insert(lower.clone(), elems);
            } else {
                sr.headers.insert(
                    lower.clone(),
                    jsvalue_to_owned_string(value).unwrap_or_default(),
                );
                sr.header_value_lists.remove(&lower);
            }
            sr.raw_header_names.insert(lower, name);
        }
    }
}

/// `res.setHeader(name, value)` chainable wrapper for static dispatch.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_header_self(
    handle: i64,
    name_ptr: *const StringHeader,
    value: f64,
) -> i64 {
    js_node_http_res_set_header(handle, name_ptr, value);
    handle
}

/// `res.getHeader(name)` — case-insensitive lookup. Returns `null`
/// when the header isn't set.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_get_header(
    handle: i64,
    name_ptr: *const StringHeader,
) -> f64 {
    let name = match read_string_header(name_ptr as *mut _) {
        Some(s) => s.to_lowercase(),
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    if let Some(sr) = get_handle::<ServerResponse>(handle) {
        if let Some(v) = sr.headers.get(&name) {
            let header = alloc_string(v);
            return f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK));
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `res.removeHeader(name)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_remove_header(
    handle: i64,
    name_ptr: *const StringHeader,
) {
    // #4907 — Node's `OutgoingMessage.removeHeader` throws once headers are
    // sent (distinct "remove" wording from `setHeader`).
    if response_headers_sent(handle) {
        perry_ffi::throw_with_code(
            "Cannot remove headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    let name = match read_string_header(name_ptr as *mut _) {
        Some(s) => s.to_lowercase(),
        None => return,
    };
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            sr.headers.remove(&name);
            sr.header_value_lists.remove(&name);
            sr.raw_header_names.remove(&name);
        }
    }
}

/// `res.hasHeader(name)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_has_header(
    handle: i64,
    name_ptr: *const StringHeader,
) -> i32 {
    let name = match read_string_header(name_ptr as *mut _) {
        Some(s) => s.to_lowercase(),
        None => return 0,
    };
    if let Some(sr) = get_handle::<ServerResponse>(handle) {
        if sr.headers.contains_key(&name) {
            return 1;
        }
    }
    0
}

/// `res.hasHeader(name)` boolean wrapper for static dispatch.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_has_header_value(
    handle: i64,
    name_ptr: *const StringHeader,
) -> f64 {
    f64::from_bits(JsValue::from_bool(js_node_http_res_has_header(handle, name_ptr) != 0).bits())
}

/// `res.appendHeader(name, value)` — append another string value to the
/// in-memory header slot. Multi-value header storage is represented as the
/// same comma-joined string Node exposes through `String(res.getHeader())`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_append_header(
    handle: i64,
    name_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> i64 {
    let name = read_string_header(name_ptr as *mut _).unwrap_or_default();
    let value = read_string_header(value_ptr as *mut _).unwrap_or_default();
    if name.is_empty() {
        return handle;
    }
    let lower = name.to_lowercase();
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            if let Some(list) = sr.header_value_lists.get_mut(&lower) {
                // Already a multi-value header (e.g. Set-Cookie): append a new
                // element so it emits its own wire line (#4826).
                list.push(value.clone());
                let joined = list.join(", ");
                sr.headers.insert(lower.clone(), joined);
            } else {
                sr.headers
                    .entry(lower.clone())
                    .and_modify(|existing| {
                        existing.push(',');
                        existing.push_str(&value);
                    })
                    .or_insert(value);
            }
            sr.raw_header_names.entry(lower).or_insert(name);
        }
    }
    handle
}

/// `res.getHeaders()` — JSON-stringify the lowercase-keyed map.
/// TS-side parses with `JSON.parse`.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_headers_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<ServerResponse>(handle)
        .map(|sr| serde_json::to_string(&sr.headers).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());
    alloc_string(&s).as_raw()
}

/// `res.getHeaderNames()` — JSON-stringify the list of lowercase
/// header names (matches Node — `getHeaderNames` returns lowercase).
#[no_mangle]
pub extern "C" fn js_node_http_res_get_header_names_json(handle: i64) -> *mut StringHeader {
    let s = get_handle::<ServerResponse>(handle)
        .map(|sr| {
            let mut names: Vec<&String> = sr.headers.keys().collect();
            names.sort();
            serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string())
        })
        .unwrap_or_else(|| "[]".to_string());
    alloc_string(&s).as_raw()
}

/// `res.setHeaders(headers)` — Node accepts only a `Headers` or a `Map`
/// (anything else is `ERR_INVALID_ARG_TYPE`), and throws
/// `ERR_HTTP_HEADERS_SENT` if the head was already committed. The runtime's
/// `js_node_setheaders_entries_json` normalizes the argument into a JSON
/// `[name, value]` entries array (or null for an invalid type) WITHOUT ever
/// dereferencing a registry handle — the old path JSON-stringified the
/// `Headers` handle directly, walking its fetch-band id (`0x40000`+) as a heap
/// `GcHeader` and segfaulting nondeterministically (#4965).
#[no_mangle]
pub extern "C" fn js_node_http_res_set_headers(handle: i64, headers_value: f64) -> i64 {
    // Node order: the headers-sent check fires before the argument is
    // validated. `header_committed` covers a prior `writeHead`; `headers_sent`
    // covers an already-flushed body.
    let committed = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.headers_sent || sr.header_committed)
        .unwrap_or(false);
    if committed {
        perry_ffi::throw_with_code(
            "Cannot set headers after they are sent to the client",
            "ERR_HTTP_HEADERS_SENT",
            perry_ffi::ErrorKind::Error,
        );
    }
    let entries_ptr = unsafe { js_node_setheaders_entries_json(headers_value) };
    if entries_ptr.is_null() {
        perry_ffi::throw_with_code(
            "The \"headers\" argument must be an instance of Headers or Map.",
            "ERR_INVALID_ARG_TYPE",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    if let Some(json) = read_string_header(entries_ptr) {
        if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
            if !sr.headers_sent {
                apply_headers_entries(sr, &json);
            }
        }
    }
    handle
}

/// Apply a normalized `setHeaders` entries array: `[[name, value], …]` where
/// `value` is a string or (for `Set-Cookie`/multi-valued headers) an array of
/// strings. The pairwise (vs object) shape preserves a `Set-Cookie` array as a
/// per-element list so the wire layer emits one line each (#4826/#4965).
fn apply_headers_entries(sr: &mut ServerResponse, json: &str) {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(json)
    else {
        return;
    };
    for item in items {
        let serde_json::Value::Array(pair) = item else {
            continue;
        };
        let mut pair = pair.into_iter();
        let (Some(name_v), Some(value_v)) = (pair.next(), pair.next()) else {
            continue;
        };
        let name = match name_v {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        };
        if name.is_empty() {
            continue;
        }
        let lower = name.to_lowercase();
        if let serde_json::Value::Array(elems) = value_v {
            let elems: Vec<String> = elems
                .into_iter()
                .map(|item| match item {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                })
                .collect();
            sr.headers.insert(lower.clone(), elems.join(", "));
            sr.header_value_lists.insert(lower.clone(), elems);
        } else {
            let value = match value_v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            sr.headers.insert(lower.clone(), value);
            sr.header_value_lists.remove(&lower);
        }
        sr.raw_header_names.insert(lower, name);
    }
}

/// `res.statusMessage` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_get_status_message(handle: i64) -> f64 {
    if let Some(sr) = get_handle::<ServerResponse>(handle) {
        if let Some(message) = &sr.status_message {
            let header = alloc_string(message);
            return f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK));
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `res.finished` getter. Node aliases this to the ended state.
#[no_mangle]
pub extern "C" fn js_node_http_res_finished(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.writable_ended { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.sendDate` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_send_date(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.send_date { 1 } else { 0 })
        .unwrap_or(1)
}

/// `res.sendDate = bool` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_send_date(handle: i64, value: f64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.send_date = jsvalue_truthy(value);
    }
}

/// `res.strictContentLength` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_strict_content_length(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.strict_content_length { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.strictContentLength = bool` setter.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_strict_content_length(handle: i64, value: f64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.strict_content_length = jsvalue_truthy(value);
    }
}

/// Paired request handle for `res.req`.
#[no_mangle]
pub extern "C" fn js_node_http_res_req_handle(handle: i64) -> i64 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| sr.req_handle)
        .unwrap_or(0)
}

/// `res.headersSent` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_headers_sent(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.headers_sent { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.writableEnded` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_writable_ended(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.writable_ended { 1 } else { 0 })
        .unwrap_or(0)
}

/// `res.writableFinished` getter.
#[no_mangle]
pub extern "C" fn js_node_http_res_writable_finished(handle: i64) -> i32 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| if sr.writable_finished { 1 } else { 0 })
        .unwrap_or(0)
}

/// Merge a JSON-encoded header object (`{"Content-Type":"text/plain",...}`)
/// into a `ServerResponse`'s header map, preserving the original case for
/// `getHeaderNames()` while keying the lookup table lowercase. Shared by
/// `writeHead`'s bulk-header path.
fn apply_headers_json(sr: &mut ServerResponse, json: &str) {
    if json.is_empty() || json == "null" || json == "undefined" {
        return;
    }
    if let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(json) {
        for (k, v) in obj {
            let lower = k.to_lowercase();
            // Array values (e.g. Set-Cookie) emit one wire line per element.
            // Node coerces the scalar `getHeader`/lookup value to the
            // elements joined with `, `, and keeps the per-element list so
            // the response serializer can emit each on its own line (#4826).
            if let serde_json::Value::Array(items) = v {
                let elems: Vec<String> = items
                    .into_iter()
                    .map(|item| match item {
                        serde_json::Value::String(s) => s,
                        other => other.to_string(),
                    })
                    .collect();
                sr.headers.insert(lower.clone(), elems.join(", "));
                sr.header_value_lists.insert(lower.clone(), elems);
                sr.raw_header_names.insert(lower, k);
                continue;
            }
            let value = match v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            sr.headers.insert(lower.clone(), value);
            sr.header_value_lists.remove(&lower);
            sr.raw_header_names.insert(lower, k);
        }
    }
}

/// `res.writeHead(statusCode[, statusMessage][, headers])` — set status +
/// optional status message + bulk headers.
///
/// #2132: `arg2`/`arg3` arrive as raw NaN-boxed JSValues (`NA_JSV`) so the
/// runtime can resolve Node's overloads — a string in slot 2 is the
/// `statusMessage`, an object in slot 2 or 3 is the bulk `headers`. Headers
/// objects are serialized here via `js_json_stringify`. The previous wiring
/// typed both slots `NA_STR`, which coerced a headers *object* to the literal
/// `"[object Object]"` and silently dropped every header set through
/// `writeHead` (visible as missing `Content-Type` etc. on the wire).
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_write_head(
    handle: i64,
    status: f64,
    arg2: i64,
    arg3: i64,
) {
    let v2 = JsValue::from_bits(arg2 as u64);
    let v3 = JsValue::from_bits(arg3 as u64);

    // Resolve the (statusMessage?, headers?) overload. `is_pointer()` is the
    // POINTER_TAG heap-object test — exactly the shape of a headers object
    // literal; strings (STRING_TAG) and primitives are excluded.
    let mut status_message: Option<String> = None;
    let mut headers_value: Option<f64> = None;
    if v3.is_pointer() {
        headers_value = Some(f64::from_bits(arg3 as u64));
        if v2.is_string() {
            status_message = read_string_header(v2.as_string_ptr());
        }
    } else if v2.is_pointer() {
        headers_value = Some(f64::from_bits(arg2 as u64));
    } else if v2.is_string() {
        status_message = read_string_header(v2.as_string_ptr());
    }

    let headers_json = headers_value.and_then(|hv| {
        let ptr = js_json_stringify(hv, 0);
        if ptr.is_null() {
            None
        } else {
            read_string_header(ptr)
        }
    });

    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if sr.headers_sent {
            return;
        }
        if status.is_finite() && status > 0.0 {
            sr.status_code = status as u16;
        }
        if let Some(m) = status_message {
            if !m.is_empty() {
                sr.status_message = Some(m);
            }
        }
        if let Some(json) = headers_json {
            // Node's `writeHead` accepts the headers as an object OR as a flat
            // array `[name, value, name, value, …]` (even offsets are names,
            // odd are values — NOT a list of tuples). Route the array form to
            // the pairwise applier; objects keep the original path (#4965).
            if json.trim_start().starts_with('[') {
                apply_headers_flat_array(sr, &json);
            } else {
                apply_headers_json(sr, &json);
            }
        }
        // Mark the head committed (Node's `_header`) so a later
        // `res.setHeaders(...)` throws `ERR_HTTP_HEADERS_SENT`. The actual wire
        // flush still happens lazily at `write`/`end` (`headers_sent`), so the
        // deferred-send path is unchanged (#4965).
        sr.header_committed = true;
    }
}

/// Apply a Node `writeHead` flat-array headers value (`[name, value, …]`).
/// Even offsets are header names, odd offsets the associated values; an array
/// element may itself be an array (multi-valued header). Mirrors
/// `apply_headers_json`'s lowercase-key / original-case / array-list handling.
fn apply_headers_flat_array(sr: &mut ServerResponse, json: &str) {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(json)
    else {
        return;
    };
    let mut it = items.into_iter();
    while let (Some(name_v), Some(value_v)) = (it.next(), it.next()) {
        let name = match name_v {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        };
        if name.is_empty() {
            continue;
        }
        let lower = name.to_lowercase();
        if let serde_json::Value::Array(elems) = value_v {
            let elems: Vec<String> = elems
                .into_iter()
                .map(|item| match item {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                })
                .collect();
            sr.headers.insert(lower.clone(), elems.join(", "));
            sr.header_value_lists.insert(lower.clone(), elems);
        } else {
            let value = match value_v {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            sr.headers.insert(lower.clone(), value);
            sr.header_value_lists.remove(&lower);
        }
        sr.raw_header_names.insert(lower, name);
    }
}

/// Send `bytes` over the live stream channel, charging the in-flight
/// counter. Returns `Some(below_hwm)` when the response is streaming
/// (`begin_streaming` succeeded now or earlier), `None` when it isn't —
/// the caller falls back to the legacy buffered path.
fn stream_write(handle: i64, bytes: &[u8]) -> Option<bool> {
    stream_write_with_cb(handle, bytes, 0)
}

/// `stream_write`, but also enqueues `callback` (if non-zero) into
/// `pending_write_callbacks` BEFORE the data frame is published on the channel.
///
/// Ordering matters: the stream-frame send makes the bytes visible to the
/// reader task, which may drain and run pending write callbacks immediately. If
/// we pushed the callback AFTER `tx.send(...)` (as the call site used to), a
/// receiver that drains right away could run this write's callback out of order
/// — after later writes' callbacks or `.end()`. Registering it first keeps the
/// callback ordered relative to the frame it belongs to and to later writes.
fn stream_write_with_cb(handle: i64, bytes: &[u8], callback: i64) -> Option<bool> {
    if !begin_streaming(handle) {
        return None;
    }
    let sr = get_handle_mut::<ServerResponse>(handle)?;
    // Clone the channel handles so the immutable borrow of `sr` ends before we
    // mutate `pending_write_callbacks` / `needs_drain` (the sender + Arc are
    // cheap to clone). The `fetch_add` reserves this chunk's byte count.
    let tx = sr.stream_tx.as_ref()?.clone();
    let in_flight = sr.stream_in_flight.as_ref()?.clone();
    let queued =
        in_flight.fetch_add(bytes.len(), std::sync::atomic::Ordering::AcqRel) + bytes.len();
    // Register the write callback BEFORE publishing the frame (see doc comment).
    if callback != 0 {
        sr.pending_write_callbacks.push(callback);
    }
    let _ = tx.send(StreamFrame::Data(Bytes::copy_from_slice(bytes)));
    let below_hwm = queued <= DEFAULT_HIGH_WATER_MARK;
    if !below_hwm {
        sr.needs_drain = true;
    }
    Some(below_hwm)
}

/// `res.write(chunk)` — flush the head on first write (Node's behavior)
/// and stream the chunk to the wire; falls back to buffering for handle
/// flavors that can't stream. Returns 0 (backpressure: "wait for drain")
/// once the queued-but-unsent bytes pass the HWM, else 1.
#[no_mangle]
pub extern "C" fn js_node_http_res_write(handle: i64, chunk: f64) -> i32 {
    let bytes = match jsvalue_to_body_bytes(chunk) {
        Some(b) => b,
        None => return 1,
    };
    let ended = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.writable_ended)
        .unwrap_or(true);
    if ended {
        return 1;
    }
    if let Some(below_hwm) = stream_write(handle, &bytes) {
        return below_hwm as i32;
    }
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.writable_ended {
            sr.headers_sent = true;
            sr.buffered_body.extend_from_slice(&bytes);
        }
    }
    1
}

/// Return the closure pointer carried by `value_bits` if it is a real callable
/// (POINTER_TAG + CLOSURE_MAGIC), else 0. Uses `js_value_is_closure` so a
/// `Buffer`/object chunk — also POINTER_TAG — is never mistaken for a callback.
fn callback_from_bits(value_bits: i64) -> i64 {
    if unsafe { js_value_is_closure(value_bits) } != 0 {
        (value_bits as u64 & PTR_MASK) as i64
    } else {
        0
    }
}

/// Pick the callback from a `(encoding?, callback?)` trailing arg pair, the
/// later slot first — mirroring Node's `(chunk, encoding, callback)` rule. A
/// string encoding is not callable, so it is skipped.
fn pick_trailing_callback(arg2: i64, arg3: i64) -> i64 {
    let c3 = callback_from_bits(arg3);
    if c3 != 0 {
        c3
    } else {
        callback_from_bits(arg2)
    }
}

/// `res.write(chunk[, encoding][, callback])` — the full Node surface routed
/// from the static native dispatch table. The trailing `(encoding?,
/// callback?)` args arrive as raw NaN-boxed JSValues (`NA_JSV`); the callback
/// is queued (it fires in order at `.end()`, #4904) and the encoding string is
/// ignored for the buffered body. Returns a NaN-boxed boolean: `false` once
/// the buffered body passes the 16 KiB high-water mark (Node's backpressure
/// signal, which terminates `while (res.write(buf))` producer loops), else
/// `true` (#4909).
#[no_mangle]
pub extern "C" fn js_node_http_res_write_full(
    handle: i64,
    chunk: f64,
    arg2: i64,
    arg3: i64,
) -> f64 {
    let callback = pick_trailing_callback(arg2, arg3);
    let bytes = jsvalue_to_body_bytes(chunk);
    let ended = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.writable_ended)
        .unwrap_or(true);
    if ended {
        return f64::from_bits(TAG_TRUE);
    }
    if let Some(b) = &bytes {
        if let Some(below_hwm) = stream_write(handle, b) {
            // Streaming: the chunk is on its way to the wire, so the write
            // callback fires now rather than queueing for `.end()`.
            if callback != 0 {
                call_closure0(callback);
            }
            return f64::from_bits(if below_hwm { TAG_TRUE } else { TAG_FALSE });
        }
    }
    let mut below_hwm = true;
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.writable_ended {
            sr.headers_sent = true;
            if let Some(b) = &bytes {
                sr.buffered_body.extend_from_slice(b);
            }
            if callback != 0 {
                sr.pending_write_callbacks.push(callback);
            }
            below_hwm = sr.buffered_body.len() <= DEFAULT_HIGH_WATER_MARK;
        }
    }
    f64::from_bits(if below_hwm { TAG_TRUE } else { TAG_FALSE })
}

/// `res.end([chunk][, encoding][, callback])` — the full Node surface routed
/// from the static native dispatch table. Handles the `end(cb)` form (callback
/// in the first slot) as well as `end(chunk[, encoding][, callback])`. Queued
/// write callbacks fire first (in order), then the end callback, then the
/// `'finish'`/`'close'` listeners — Node's ordering where `'finish'` never
/// precedes the end callback (#4909).
///
/// # Safety
/// FFI entry; `handle` must be a live `ServerResponse` handle (or absent).
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_end_full(handle: i64, chunk: f64, arg2: i64, arg3: i64) {
    // `end(cb)` passes the callback as the first arg; otherwise it trails.
    let first_cb = callback_from_bits(chunk.to_bits() as i64);
    let (real_chunk, callback) = if first_cb != 0 {
        (f64::from_bits(TAG_UNDEFINED), first_cb)
    } else {
        (chunk, pick_trailing_callback(arg2, arg3))
    };

    let is_standalone = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.standalone)
        .unwrap_or(false);
    if is_standalone {
        // standalone_end already runs write cbs → end cb → listeners in order.
        standalone_end(handle, real_chunk, callback);
        return;
    }

    let listeners = finalize_buffered_end(handle, real_chunk);
    let write_cbs = get_handle_mut::<ServerResponse>(handle)
        .map(|sr| std::mem::take(&mut sr.pending_write_callbacks))
        .unwrap_or_default();
    for cb in write_cbs {
        call_closure0(cb);
    }
    // Node order: queued write callbacks flush, then `'finish'` listeners, then
    // the end callback, then `'close'`. The end cb fires *after* `'finish'` so
    // a `res.on('finish')` handler that inspects end-callback state sees the
    // same interleaving as Node.
    let (finish_listeners, close_listeners) = listeners.unwrap_or_default();
    emit_no_arg_to_listeners(&finish_listeners);
    if callback != 0 {
        call_closure0(callback);
    }
    emit_no_arg_to_listeners(&close_listeners);
}

/// `res.addTrailers(headers)` — store HTTP trailers emitted after the
/// response body, per Node's `ServerResponse.addTrailers`. Trailers carry
/// metadata that isn't known until the body has been produced.
#[no_mangle]
pub extern "C" fn js_node_http_res_add_trailers(handle: i64, headers_value: f64) {
    let v = JsValue::from_bits(headers_value.to_bits());
    if v.is_undefined() || v.is_null() {
        return;
    }
    let json = match perry_ffi::json_stringify(v) {
        Some(j) => j,
        None => return,
    };
    let parsed: serde_json::Value = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(_) => return,
    };
    let Some(obj) = parsed.as_object() else {
        return;
    };
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if sr.writable_ended {
            return;
        }
        for (k, v) in obj {
            let lower = k.to_lowercase();
            let value = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            sr.trailers.insert(lower.clone(), value);
            sr.raw_trailer_names.insert(lower, k.clone());
        }
    }
}

/// Finalize a buffered response: append the final chunk, flush it back to
/// hyper through the oneshot channel, and return the `(finish, close)`
/// listener lists **without** firing them — the caller controls ordering so
/// that `res.end(cb)` can run write/end callbacks before `'finish'` (Node's
/// contract, where `'finish'` never precedes the end callback). Returns
/// `None` if the response was already ended or the handle is gone.
fn finalize_buffered_end(handle: i64, chunk: f64) -> Option<(Vec<i64>, Vec<i64>)> {
    let v = JsValue::from_bits(chunk.to_bits());
    let final_chunk = if v.is_undefined() || v.is_null() {
        None
    } else {
        jsvalue_to_body_bytes(chunk)
    };

    let sr = get_handle_mut::<ServerResponse>(handle)?;
    if sr.writable_ended {
        return None;
    }

    // Streaming mode: the head already went to the wire. Send the final
    // chunk + trailer block as frames and close the channel — hyper ends
    // the (chunked) body when the sender drops.
    if let Some(tx) = sr.stream_tx.take() {
        if let Some(c) = final_chunk {
            if let Some(in_flight) = sr.stream_in_flight.as_ref() {
                in_flight.fetch_add(c.len(), std::sync::atomic::Ordering::AcqRel);
            }
            let _ = tx.send(StreamFrame::Data(Bytes::from(c)));
        }
        let trailers = sr.snapshot_trailers();
        if !trailers.is_empty() {
            let mut map = HeaderMap::new();
            for (name, value) in trailers {
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(&value),
                ) {
                    map.insert(name, value);
                }
            }
            let _ = tx.send(StreamFrame::Trailers(map));
        }
        sr.writable_ended = true;
        sr.writable_finished = true;
        sr.needs_drain = false;
        let finish_listeners = sr.listeners.get("finish").cloned().unwrap_or_default();
        let close_listeners = sr.listeners.get("close").cloned().unwrap_or_default();
        return Some((finish_listeners, close_listeners));
    }

    if let Some(c) = final_chunk {
        sr.buffered_body.extend_from_slice(&c);
    }
    sr.headers_sent = true;
    sr.writable_ended = true;
    sr.ensure_content_length();
    let body = std::mem::take(&mut sr.buffered_body);
    let headers = sr.snapshot_headers();
    let trailers = sr.snapshot_trailers();
    let shape = HyperResponseShape {
        status: sr.status_code,
        status_message: sr.status_message.clone(),
        headers,
        trailers,
        body: ShapeBody::Full(body),
    };
    let finish_listeners = sr.listeners.get("finish").cloned().unwrap_or_default();
    let close_listeners = sr.listeners.get("close").cloned().unwrap_or_default();
    if let Some(tx) = sr.response_tx.take() {
        let _ = tx.send(shape);
    }
    sr.writable_finished = true;
    Some((finish_listeners, close_listeners))
}

/// `res.end(chunk?)` — append final chunk + flush the response back
/// to hyper through the oneshot channel + fire `'finish'` and
/// `'close'` listeners.
#[no_mangle]
pub extern "C" fn js_node_http_res_end(handle: i64, chunk: f64) {
    if let Some((finish_listeners, close_listeners)) = finalize_buffered_end(handle, chunk) {
        emit_no_arg_to_listeners(&finish_listeners);
        emit_no_arg_to_listeners(&close_listeners);
    }
}

/// Flush the response head to the wire now and switch the response into
/// streaming mode: the status line + headers go back to hyper immediately
/// with a channel-backed body, and subsequent `res.write(...)` chunks flow
/// straight to the client (chunked transfer-encoding unless the handler set
/// Content-Length). This is what makes Node shapes like "send headers, keep
/// the response open, write later" (SSE, long-poll, `res.flushHeaders()`,
/// `res.write()` before an async gap) observable client-side before
/// `.end()`.
///
/// Returns `true` when the response is in streaming mode after the call.
/// Standalone (`assignSocket`) and bare `OutgoingMessage` handles keep the
/// buffered path, as does a response whose connection already died.
pub(crate) fn begin_streaming(handle: i64) -> bool {
    let Some(sr) = get_handle_mut::<ServerResponse>(handle) else {
        return false;
    };
    if sr.writable_ended {
        return false;
    }
    if sr.stream_tx.is_some() {
        return true;
    }
    if sr.standalone || sr.outgoing_message_only {
        return false;
    }
    let receiver_alive = sr
        .response_tx
        .as_ref()
        .map(|tx| !tx.is_closed())
        .unwrap_or(false);
    if !receiver_alive {
        return false;
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let in_flight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let shape = HyperResponseShape {
        status: sr.status_code,
        status_message: sr.status_message.clone(),
        headers: sr.snapshot_headers(),
        trailers: Vec::new(),
        body: ShapeBody::Stream {
            rx,
            in_flight: in_flight.clone(),
        },
    };
    sr.headers_sent = true;
    let oneshot_tx = sr.response_tx.take().expect("checked above");
    if oneshot_tx.send(shape).is_err() {
        return false;
    }
    if !sr.buffered_body.is_empty() {
        let first = std::mem::take(&mut sr.buffered_body);
        in_flight.fetch_add(first.len(), std::sync::atomic::Ordering::AcqRel);
        let _ = tx.send(StreamFrame::Data(Bytes::from(first)));
    }
    sr.stream_tx = Some(tx);
    sr.stream_in_flight = Some(in_flight);
    true
}

/// If a streaming response previously hit backpressure (`res.write()`
/// returned `false`) and its queued bytes have since drained below the
/// HWM, clear the flag and return its `'drain'` listeners for the caller
/// (the pump) to fire. Empty otherwise.
pub(crate) fn take_drain_listeners_if_ready(handle: i64) -> Vec<i64> {
    let Some(sr) = get_handle_mut::<ServerResponse>(handle) else {
        return Vec::new();
    };
    if !sr.needs_drain || sr.writable_ended {
        return Vec::new();
    }
    let below = sr
        .stream_in_flight
        .as_ref()
        .map(|c| c.load(std::sync::atomic::Ordering::Acquire) <= DEFAULT_HIGH_WATER_MARK)
        .unwrap_or(false);
    if !below {
        return Vec::new();
    }
    sr.needs_drain = false;
    sr.listeners.get("drain").cloned().unwrap_or_default()
}

/// True when a streaming response's connection died under it (hyper
/// dropped the body receiver — client disconnect / server close).
pub(crate) fn stream_receiver_gone(handle: i64) -> bool {
    get_handle::<ServerResponse>(handle)
        .and_then(|sr| sr.stream_tx.as_ref().map(|tx| tx.is_closed()))
        .unwrap_or(false)
}

/// `res.flushHeaders()` — Node sends headers immediately even before
/// any body. Flushes the head to the wire and switches to the streaming
/// body path; falls back to marking headers-sent for handle flavors that
/// can't stream (standalone / bare OutgoingMessage).
#[no_mangle]
pub extern "C" fn js_node_http_res_flush_headers(handle: i64) {
    if !begin_streaming(handle) {
        if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
            sr.headers_sent = true;
        }
    }
}

/// `res.cork()` — buffered responses are already corked until `.end()`.
#[no_mangle]
pub extern "C" fn js_node_http_res_cork(_handle: i64) {}

/// `res.uncork()` — no-op counterpart to `cork()`.
#[no_mangle]
pub extern "C" fn js_node_http_res_uncork(_handle: i64) {}

/// `res.setTimeout(msecs[, callback])` — expose Node's chainable control
/// surface; actual transport timeout scheduling is handled at the server.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_timeout(handle: i64, _msecs: f64, _callback: i64) -> i64 {
    handle
}

/// `res.writeEarlyHints(hints[, cb])` — interim 103 response is not yet sent
/// (accepted no-op), but the `hints` argument is validated to match Node
/// (#4907): a non-object throws `ERR_INVALID_ARG_TYPE`, and a malformed
/// `link` value throws `ERR_INVALID_ARG_VALUE`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_write_early_hints(
    _handle: i64,
    hints: f64,
    _callback: i64,
) {
    // Serialize the hints object and inspect it. Node requires `typeof hints
    // === 'object'` (and non-null); a string / number / null throws
    // `ERR_INVALID_ARG_TYPE`.
    let json = {
        let ptr = js_json_stringify(hints, 0);
        if ptr.is_null() {
            None
        } else {
            read_string_header(ptr)
        }
    };
    let value: Option<serde_json::Value> = json.and_then(|j| serde_json::from_str(&j).ok());
    match value {
        // Arrays are objects in JS — `hints.link` is simply undefined, so no
        // validation fires.
        Some(serde_json::Value::Object(map)) => {
            if let Some(link) = map.get("link") {
                let invalid = match link {
                    serde_json::Value::Null => false,
                    serde_json::Value::String(s) => !is_valid_link_header(s),
                    serde_json::Value::Array(items) => items
                        .iter()
                        .any(|i| i.as_str().map(|s| !is_valid_link_header(s)).unwrap_or(true)),
                    _ => true,
                };
                if invalid {
                    perry_ffi::throw_with_code(
                        "The property 'hints.link' must be an array or string of format \"</styles.css>; rel=preload; as=style\".",
                        "ERR_INVALID_ARG_VALUE",
                        perry_ffi::ErrorKind::TypeError,
                    );
                }
            }
        }
        Some(serde_json::Value::Array(_)) => {}
        _ => {
            perry_ffi::throw_with_code(
                "The \"hints\" argument must be of type object.",
                "ERR_INVALID_ARG_TYPE",
                perry_ffi::ErrorKind::TypeError,
            );
        }
    }
}

/// `res.writeContinue()` — acknowledge an `Expect: 100-continue` request.
///
/// #5080: the interim `HTTP/1.1 100 Continue` is written by hyper the moment
/// the request body is polled (`req.collect()` in the service fn), which is
/// what unblocks the client's withheld body before `'checkContinue'` even
/// fires on the main thread. So by the time the handler calls
/// `writeContinue()` the 100 is already on the wire; this entry point exists
/// for API parity (the canonical `checkContinue` handler calls it) and is a
/// confirmation no-op rather than a second 100 line.
#[no_mangle]
pub extern "C" fn js_node_http_res_write_continue(_handle: i64) {
    // Interim 100 already flushed by hyper on first body poll — see above.
}

/// `res.writeProcessing()` — emits an HTTP/1.1 102-Processing. Stub.
#[no_mangle]
pub extern "C" fn js_node_http_res_write_processing(_handle: i64) {
    // No-op stub.
}

/// `res.on(event, cb)` — register a listener.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback: i64,
) -> f64 {
    let event = read_string_header(event_name_ptr as *mut _).unwrap_or_default();
    let mut should_fire_now = false;
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.listeners
            .entry(event.clone())
            .or_default()
            .push(callback);
        // If `.end()` already fired, late listeners for `'finish'` /
        // `'close'` should still see them (Node fires them
        // asynchronously, so a late `on` registration is racy but
        // observed; our synchronous emit means we fire on
        // registration if already done).
        if sr.writable_finished && (event == "finish" || event == "close") {
            should_fire_now = true;
        }
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if should_fire_now && callback != 0 {
        let raw = callback as *const RawClosureHeader;
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
    handle_to_pointer_f64(handle)
}

// ============================================================================
// Allocation helper used by server.rs
// ============================================================================

#[no_mangle]
pub extern "C" fn js_node_http_outgoing_message_new() -> i64 {
    register_handle(ServerResponse::outgoing_message())
}

// ============================================================================
// #4904: standalone `new http.ServerResponse(req)` + `assignSocket` support
// ============================================================================

/// `new http.ServerResponse(req)` — a response not bound to a live hyper
/// exchange. `req` contributes only the method (Node skips the body on
/// flush when it was a HEAD request); writes buffer until `.end()`, which
/// flushes through the socket assigned via `res.assignSocket(socket)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_response_standalone_new(req: f64) -> i64 {
    crate::ensure_gc_scanner_registered();
    let (tx, _rx) = oneshot::channel::<HyperResponseShape>();
    let mut sr = ServerResponse::new(tx);
    sr.standalone = true;
    sr.send_date = false;
    if JsValue::from_bits(req.to_bits()).is_pointer() {
        extern "C" {
            fn js_object_get_field_by_name(
                obj: *const perry_ffi::ObjectHeader,
                key: *const StringHeader,
            ) -> JsValue;
        }
        let key = alloc_string("method");
        let m = js_object_get_field_by_name(
            (req.to_bits() & PTR_MASK) as *const perry_ffi::ObjectHeader,
            key.as_raw(),
        );
        if JsValue::from_bits(m.bits()).is_string() {
            sr.standalone_req_method = read_string_header((m.bits() & PTR_MASK) as *mut _);
        }
    }
    register_handle(sr)
}

/// `res.assignSocket(socket)` — wire a (possibly userland) Writable as the
/// flush target. Node throws `ERR_HTTP_SOCKET_ASSIGNED` on a second call.
#[no_mangle]
pub extern "C" fn js_node_http_res_assign_socket(handle: i64, socket: f64) {
    let already = get_handle::<ServerResponse>(handle)
        .map(|sr| !JsValue::from_bits(sr.standalone_socket.to_bits()).is_undefined())
        .unwrap_or(false);
    if already {
        perry_ffi::throw_with_code(
            "Socket already assigned",
            "ERR_HTTP_SOCKET_ASSIGNED",
            perry_ffi::ErrorKind::Error,
        );
    }
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.standalone_socket = socket;
        sr.standalone = true;
    }
}

/// `res.detachSocket(socket)` — counterpart of `assignSocket`.
#[no_mangle]
pub extern "C" fn js_node_http_res_detach_socket(handle: i64, _socket: f64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.standalone_socket = f64::from_bits(TAG_UNDEFINED);
    }
}

/// `res.write(chunk[, encoding][, callback])` — callback-aware variant of
/// `js_node_http_res_write`. The callback queues until the buffered body
/// flushes on `.end()`, preserving call order (#4904).
#[no_mangle]
pub extern "C" fn js_node_http_res_write_with_cb(handle: i64, chunk: f64, callback: i64) -> i32 {
    let bytes = jsvalue_to_body_bytes(chunk);
    // Honor streaming mode (after `res.flushHeaders()` / a prior streamed
    // `res.write`) exactly like `js_node_http_res_write`: the chunk must go down
    // the stream channel, NOT into `buffered_body`. Without this, a streamed
    // response whose chunks arrive via the callback-aware dispatch arm
    // (`res.write(chunk)` from Next's `pipeToNodeResponse` WritableStream)
    // buffered every chunk while `.end()` took the stream-finalize path — which
    // only sends the final `.end(chunk)` arg and drops `buffered_body` entirely,
    // so the JSON API-route body never reached the wire (HTTP 200, 0 bytes).
    // #5437 (Next.js app-route response pipe).
    if let Some(b) = &bytes {
        let ended = get_handle::<ServerResponse>(handle)
            .map(|sr| sr.writable_ended)
            .unwrap_or(true);
        if !ended {
            // Register the write callback BEFORE the data frame is published so
            // a receiver that drains immediately can't run it out of order
            // relative to later writes / `.end()` (Node fires it once the chunk
            // is flushed; queued, it drains in order, #4904). `stream_write_with_cb`
            // enqueues the callback ahead of the `tx.send`.
            if let Some(below_hwm) = stream_write_with_cb(handle, b, callback) {
                return below_hwm as i32;
            }
        }
    }
    // #4909 — real backpressure boolean (mirrors `js_node_http_res_write_full`
    // on the static path): `false` past the 16 KiB high-water mark, so dynamic
    // `while (res.write(buf, cb))` producer loops terminate.
    let mut below_hwm = true;
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.writable_ended {
            sr.headers_sent = true;
            if let Some(b) = &bytes {
                sr.buffered_body.extend_from_slice(b);
            }
            if callback != 0 {
                sr.pending_write_callbacks.push(callback);
            }
            below_hwm = sr.buffered_body.len() <= DEFAULT_HIGH_WATER_MARK;
        }
    }
    if below_hwm {
        1
    } else {
        0
    }
}

/// `res.end([chunk][, callback])` — callback-aware variant. Standalone
/// responses flush through the assigned socket; everything else takes the
/// existing hyper-oneshot path. Queued write callbacks run first, in
/// order, then the end callback (#4904).
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_end_with_cb(handle: i64, chunk: f64, callback: i64) {
    let is_standalone = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.standalone)
        .unwrap_or(false);
    if is_standalone {
        standalone_end(handle, chunk, callback);
        return;
    }
    // #4909 — Node's flush ordering, matching `js_node_http_res_end_full`:
    // queued write callbacks → `'finish'` → end callback → `'close'`. The
    // previous code fired `'finish'`/`'close'` (via `js_node_http_res_end`)
    // before any callback ran.
    let listeners = finalize_buffered_end(handle, chunk);
    let write_cbs = get_handle_mut::<ServerResponse>(handle)
        .map(|sr| std::mem::take(&mut sr.pending_write_callbacks))
        .unwrap_or_default();
    for cb in write_cbs {
        call_closure0(cb);
    }
    let (finish_listeners, close_listeners) = listeners.unwrap_or_default();
    emit_no_arg_to_listeners(&finish_listeners);
    if callback != 0 {
        call_closure0(callback);
    }
    emit_no_arg_to_listeners(&close_listeners);
}

/// Flush a standalone response: serialize the head + buffered body and
/// write them through the assigned socket's JS `write` method — one write
/// for head+body, then the zero-length finish chunk Node's corked flush
/// emits. The body is suppressed for HEAD requests.
unsafe fn standalone_end(handle: i64, chunk: f64, callback: i64) {
    let v = JsValue::from_bits(chunk.to_bits());
    let final_chunk = if v.is_undefined() || v.is_null() {
        None
    } else {
        jsvalue_to_body_bytes(chunk)
    };

    let (socket, payload, write_cbs, finish_listeners, close_listeners);
    {
        let sr = match get_handle_mut::<ServerResponse>(handle) {
            Some(s) => s,
            None => return,
        };
        if sr.writable_ended {
            return;
        }
        if let Some(c) = final_chunk {
            sr.buffered_body.extend_from_slice(&c);
        }
        sr.headers_sent = true;
        sr.writable_ended = true;
        sr.ensure_content_length();
        let body = std::mem::take(&mut sr.buffered_body);
        // Fast path: with no custom `statusMessage`, a common status code has a
        // precomputed `HTTP/1.1 <code> <canonical reason>\r\n` status line,
        // skipping the per-response `format!`. The interned bytes equal exactly
        // what the `format!` produced for `(code, canonical reason)`. A custom
        // message, or an uncommon code, falls back so its reason still reaches
        // the wire byte-for-byte.
        let mut head = match sr.status_message.as_deref() {
            None => crate::response_fast::status_line_bytes(sr.status_code).map(str::to_string),
            Some(_) => None,
        }
        .unwrap_or_else(|| {
            let reason = sr.status_message.clone().unwrap_or_else(|| {
                StatusCode::from_u16(sr.status_code)
                    .ok()
                    .and_then(|s| s.canonical_reason())
                    .unwrap_or("")
                    .to_string()
            });
            format!("HTTP/1.1 {} {}\r\n", sr.status_code, reason)
        });
        for (k, v) in sr.snapshot_headers() {
            head.push_str(&k);
            head.push_str(": ");
            head.push_str(&v);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        if sr.standalone_req_method.as_deref() != Some("HEAD") {
            bytes.extend_from_slice(&body);
        }
        payload = bytes;
        socket = sr.standalone_socket;
        write_cbs = std::mem::take(&mut sr.pending_write_callbacks);
        finish_listeners = sr.listeners.get("finish").cloned().unwrap_or_default();
        close_listeners = sr.listeners.get("close").cloned().unwrap_or_default();
        sr.writable_finished = true;
    }
    if !JsValue::from_bits(socket.to_bits()).is_undefined() {
        socket_write_str(socket, &String::from_utf8_lossy(&payload));
        socket_write_str(socket, "");
    }
    for cb in write_cbs {
        call_closure0(cb);
    }
    if callback != 0 {
        call_closure0(callback);
    }
    emit_no_arg_to_listeners(&finish_listeners);
    emit_no_arg_to_listeners(&close_listeners);
}

/// Invoke `socket.write(chunk)` on an arbitrary JS value through the
/// runtime's dynamic method-call path.
unsafe fn socket_write_str(socket: f64, chunk: &str) {
    extern "C" {
        fn js_native_call_method_str_key(
            object: f64,
            name_handle: i64,
            args_ptr: *const f64,
            args_len: usize,
        ) -> f64;
    }
    let name = alloc_string("write");
    let chunk_val = f64::from_bits(JsValue::from_string_ptr(alloc_string(chunk).as_raw()).bits());
    let args = [chunk_val];
    let _ = js_native_call_method_str_key(socket, name.as_raw() as i64, args.as_ptr(), 1);
}

/// Call a closure pointer with no args, ignoring the result.
fn call_closure0(callback: i64) {
    if callback == 0 {
        return;
    }
    unsafe {
        let closure = JsClosure::from_raw(callback as *const RawClosureHeader);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
}

pub(crate) fn alloc_server_response_for_request(
    response_tx: oneshot::Sender<HyperResponseShape>,
    req_handle: i64,
) -> i64 {
    register_handle(ServerResponse::new(response_tx).with_request_handle(req_handle))
}

fn jsvalue_truthy(value: f64) -> bool {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_bool() {
        v.to_bool()
    } else if v.is_undefined() || v.is_null() {
        false
    } else if v.is_number() {
        v.to_number() != 0.0
    } else {
        true
    }
}

#[allow(dead_code)]
pub(crate) fn _force_link_helpers(v: f64) -> bool {
    f64::from_bits(TAG_NULL) == v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_response() -> ServerResponse {
        let (tx, _rx) = oneshot::channel::<HyperResponseShape>();
        ServerResponse::new(tx)
    }

    #[test]
    fn write_head_headers_json_merges_and_preserves_case() {
        // #2132: a `writeHead(status, headers)` object is JSON-serialized and
        // merged here; the lookup key is lowercase but the original case is
        // retained for `getHeaderNames()`.
        let mut sr = empty_response();
        apply_headers_json(&mut sr, r#"{"Content-Type":"text/plain","X-Custom":"abc"}"#);
        assert_eq!(
            sr.headers.get("content-type").map(String::as_str),
            Some("text/plain")
        );
        assert_eq!(sr.headers.get("x-custom").map(String::as_str), Some("abc"));
        assert_eq!(
            sr.raw_header_names.get("content-type").map(String::as_str),
            Some("Content-Type")
        );
        assert_eq!(
            sr.raw_header_names.get("x-custom").map(String::as_str),
            Some("X-Custom")
        );
    }

    #[test]
    fn write_head_headers_json_stringifies_non_string_values() {
        let mut sr = empty_response();
        apply_headers_json(&mut sr, r#"{"Content-Length":42,"X-Flag":true}"#);
        assert_eq!(
            sr.headers.get("content-length").map(String::as_str),
            Some("42")
        );
        assert_eq!(sr.headers.get("x-flag").map(String::as_str), Some("true"));
    }

    #[test]
    fn write_head_headers_json_ignores_empty_and_sentinels() {
        let mut sr = empty_response();
        apply_headers_json(&mut sr, "");
        apply_headers_json(&mut sr, "null");
        apply_headers_json(&mut sr, "undefined");
        assert!(sr.headers.is_empty());
    }

    // #4965 — `setHeaders` entries normalizer output.

    #[test]
    fn apply_headers_entries_lowercases_and_preserves_case() {
        let mut sr = empty_response();
        apply_headers_entries(&mut sr, r#"[["Foo","1"],["Bar","2"]]"#);
        assert_eq!(sr.headers.get("foo").map(String::as_str), Some("1"));
        assert_eq!(sr.headers.get("bar").map(String::as_str), Some("2"));
        assert_eq!(
            sr.raw_header_names.get("foo").map(String::as_str),
            Some("Foo")
        );
    }

    #[test]
    fn apply_headers_entries_set_cookie_array_keeps_per_element_list() {
        let mut sr = empty_response();
        apply_headers_entries(&mut sr, r#"[["set-cookie",["a=b","c=d"]]]"#);
        assert_eq!(
            sr.header_value_lists.get("set-cookie").map(Vec::as_slice),
            Some(["a=b".to_string(), "c=d".to_string()].as_slice())
        );
        assert_eq!(
            sr.headers.get("set-cookie").map(String::as_str),
            Some("a=b, c=d")
        );
    }

    #[test]
    fn apply_headers_entries_ignores_non_array_and_short_pairs() {
        let mut sr = empty_response();
        apply_headers_entries(&mut sr, r#"[{"foo":"1"},["only-name"],["k","v"]]"#);
        assert_eq!(sr.headers.get("k").map(String::as_str), Some("v"));
        assert_eq!(sr.headers.len(), 1);
    }

    #[test]
    fn write_head_flat_array_applies_pairs_and_overrides() {
        let mut sr = empty_response();
        sr.headers.insert("foo".into(), "1".into());
        apply_headers_flat_array(&mut sr, r#"["foo","3","X-New","z"]"#);
        // even/odd offsets are name/value; `foo` overrides the prior value.
        assert_eq!(sr.headers.get("foo").map(String::as_str), Some("3"));
        assert_eq!(sr.headers.get("x-new").map(String::as_str), Some("z"));
    }
}
