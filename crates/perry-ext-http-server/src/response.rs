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
    js_json_stringify, jsvalue_to_body_bytes, jsvalue_to_owned_string, read_string_header,
    PTR_MASK, STRING_TAG, TAG_NULL, TAG_UNDEFINED,
};

pub type ResponseBody = BoxBody<Bytes, Infallible>;

struct TrailerBody {
    body: Option<Bytes>,
    trailers: Option<HeaderMap>,
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
    /// One-shot back to hyper's service fn — taken on `.end()`.
    pub response_tx: Option<oneshot::Sender<HyperResponseShape>>,
    /// Event-name → list of registered listener closure pointers.
    pub listeners: HashMap<String, Vec<i64>>,
}

/// Owned shape produced by `.end()` — the per-request oneshot channel
/// drops back to hyper carrying this.
pub struct HyperResponseShape {
    pub status: u16,
    pub status_message: Option<String>,
    pub headers: Vec<(String, String)>,
    pub trailers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HyperResponseShape {
    /// Build a hyper `Response<BoxBody<Bytes, Infallible>>` ready to return from the
    /// service fn.
    pub fn into_hyper(self) -> Response<ResponseBody> {
        let mut builder =
            Response::builder().status(StatusCode::from_u16(self.status).unwrap_or(StatusCode::OK));
        for (k, v) in self.headers {
            builder = builder.header(k, v);
        }
        let trailers = self.trailers;
        let body = if trailers.is_empty() {
            Full::new(Bytes::from(self.body)).boxed()
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
                body: Some(Bytes::from(self.body)),
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
            self.headers
                .push(("Keep-Alive".to_string(), format!("timeout={}", secs)));
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
            listeners: HashMap::new(),
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
    if name.is_empty() {
        return;
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

/// `res.setHeaders(headers)` — accepts any JSON-stringifiable object shape
/// Perry can inspect and returns the receiver. Native Node also accepts Map
/// and Headers; those stringify to `{}` in the current runtime, so this remains
/// a deterministic no-op for those inputs until iterable extraction lands.
#[no_mangle]
pub extern "C" fn js_node_http_res_set_headers(handle: i64, headers_value: f64) -> i64 {
    let v = JsValue::from_bits(headers_value.to_bits());
    if v.is_undefined() || v.is_null() {
        return handle;
    }
    let Some(json) = perry_ffi::json_stringify(v) else {
        return handle;
    };
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            apply_headers_json(sr, &json);
        }
    }
    handle
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
            apply_headers_json(sr, &json);
        }
    }
}

/// `res.write(chunk)` — append to the buffered body. Returns 1
/// (always-flushed for the buffered MVP — Node's contract is "false
/// = call drain", which we sidestep by buffering).
#[no_mangle]
pub extern "C" fn js_node_http_res_write(handle: i64, chunk: f64) -> i32 {
    let bytes = match jsvalue_to_body_bytes(chunk) {
        Some(b) => b,
        None => return 1,
    };
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.writable_ended {
            sr.headers_sent = true;
            sr.buffered_body.extend_from_slice(&bytes);
        }
    }
    1
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

/// `res.end(chunk?)` — append final chunk + flush the response back
/// to hyper through the oneshot channel + fire `'finish'` and
/// `'close'` listeners.
#[no_mangle]
pub extern "C" fn js_node_http_res_end(handle: i64, chunk: f64) {
    let v = JsValue::from_bits(chunk.to_bits());
    let final_chunk = if v.is_undefined() || v.is_null() {
        None
    } else {
        jsvalue_to_body_bytes(chunk)
    };

    let (shape, finish_listeners, close_listeners);
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
        let headers = sr.snapshot_headers();
        let trailers = sr.snapshot_trailers();
        shape = HyperResponseShape {
            status: sr.status_code,
            status_message: sr.status_message.clone(),
            headers,
            trailers,
            body,
        };
        finish_listeners = sr.listeners.get("finish").cloned().unwrap_or_default();
        close_listeners = sr.listeners.get("close").cloned().unwrap_or_default();
        if let Some(tx) = sr.response_tx.take() {
            let _ = tx.send(shape);
        }
        sr.writable_finished = true;
    }
    emit_no_arg_to_listeners(&finish_listeners);
    emit_no_arg_to_listeners(&close_listeners);
}

/// `res.flushHeaders()` — Node sends headers immediately even before
/// any body. Phase 1 marks the response as headers-sent (our actual
/// flush is unified at `.end()` time since we buffer).
#[no_mangle]
pub extern "C" fn js_node_http_res_flush_headers(handle: i64) {
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        sr.headers_sent = true;
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

/// `res.writeEarlyHints(headers[, cb])` — accepted no-op until interim
/// response streaming is implemented.
#[no_mangle]
pub extern "C" fn js_node_http_res_write_early_hints(_handle: i64, _headers: f64, _callback: i64) {}

/// `res.writeContinue()` — emits an HTTP/1.1 100-continue. Phase 1
/// stores the intent only; the actual 100-continue sequence requires
/// a streaming body path that we'll wire up in a follow-up.
#[no_mangle]
pub extern "C" fn js_node_http_res_write_continue(_handle: i64) {
    // No-op stub. Acceptable per #577 — most modern clients don't
    // negotiate Expect: 100-continue against a server that buffers.
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
}
