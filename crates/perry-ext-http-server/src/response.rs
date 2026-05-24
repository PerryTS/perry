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
    jsvalue_to_body_bytes, read_string_header, PTR_MASK, STRING_TAG, TAG_NULL, TAG_UNDEFINED,
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
    /// Lowercase-keyed header map (the lookup table).
    pub headers: HashMap<String, String>,
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
}

impl ServerResponse {
    pub fn new(response_tx: oneshot::Sender<HyperResponseShape>) -> Self {
        Self {
            status_code: 200,
            status_message: None,
            headers: HashMap::new(),
            trailers: HashMap::new(),
            raw_header_names: HashMap::new(),
            raw_trailer_names: HashMap::new(),
            headers_sent: false,
            writable_ended: false,
            writable_finished: false,
            buffered_body: Vec::new(),
            response_tx: Some(response_tx),
            listeners: HashMap::new(),
        }
    }

    /// Snapshot the current header map as `Vec<(orig_name, value)>`
    /// preserving original case.
    fn snapshot_headers(&self) -> Vec<(String, String)> {
        let mut out = Vec::with_capacity(self.headers.len());
        for (lower_k, v) in &self.headers {
            let orig = self
                .raw_header_names
                .get(lower_k)
                .cloned()
                .unwrap_or_else(|| lower_k.clone());
            out.push((orig, v.clone()));
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
        .map(|sr| sr.status_code as f64)
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

/// `res.setHeader(name, value)` — string value form. Object/array
/// values get JSON-stringified by the TS-side wrapper before reaching
/// here so the FFI surface stays simple.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_set_header(
    handle: i64,
    name_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) {
    let name = read_string_header(name_ptr as *mut _).unwrap_or_default();
    let value = read_string_header(value_ptr as *mut _).unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let lower = name.to_lowercase();
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if !sr.headers_sent {
            sr.headers.insert(lower.clone(), value);
            sr.raw_header_names.insert(lower, name);
        }
    }
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
            let names: Vec<&String> = sr.headers.keys().collect();
            serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string())
        })
        .unwrap_or_else(|| "[]".to_string());
    alloc_string(&s).as_raw()
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

/// `res.writeHead(status, statusMessage?, headers?)` — set status +
/// optional status message + bulk headers. `headers_json` is the
/// JSON-stringified header object from the TS-side wrapper, or empty
/// for no bulk headers.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_write_head(
    handle: i64,
    status: f64,
    status_msg_ptr: *const StringHeader,
    headers_json_ptr: *const StringHeader,
) {
    let msg = read_string_header(status_msg_ptr as *mut _);
    let headers_json = read_string_header(headers_json_ptr as *mut _);
    if let Some(sr) = get_handle_mut::<ServerResponse>(handle) {
        if sr.headers_sent {
            return;
        }
        if status.is_finite() && status > 0.0 {
            sr.status_code = status as u16;
        }
        if let Some(m) = msg {
            if !m.is_empty() {
                sr.status_message = Some(m);
            }
        }
        if let Some(json) = headers_json {
            if !json.is_empty() && json != "null" && json != "undefined" {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
                    if let Some(obj) = parsed.as_object() {
                        for (k, v) in obj {
                            let lower = k.to_lowercase();
                            let value = match v {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            sr.headers.insert(lower.clone(), value);
                            sr.raw_header_names.insert(lower, k.clone());
                        }
                    }
                }
            }
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

pub(crate) fn alloc_server_response(response_tx: oneshot::Sender<HyperResponseShape>) -> i64 {
    register_handle(ServerResponse::new(response_tx))
}

#[allow(dead_code)]
pub(crate) fn _force_link_helpers(v: f64) -> bool {
    f64::from_bits(TAG_NULL) == v
}
