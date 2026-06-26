//! Request/Reply context — unified surface for both Fastify-style
//! `(request, reply)` handlers and Hono-style `(c)` handlers. The
//! same handle backs both: each call's signature picks which set of
//! methods is exposed to TS code.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use perry_ffi::{
    alloc_string, build_object_shape, get_handle, get_handle_mut, js_object_alloc_with_shape,
    js_object_set_field, ArrayHeader, Handle, JsValue, ObjectHeader, StringHeader,
};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

// Runtime symbols that perry-ffi hasn't yet wrapped — these are
// stable `extern "C"` exports from perry-runtime. Declaring them
// locally is the same pattern perry-ext-{net,http,ws} use for the
// few runtime symbols outside perry-ffi v0.5's surface.
extern "C" {
    /// Parse a JSON string into a NaN-boxed JsValue. Returns the
    /// resulting NaN-boxed bits as a `u64`.
    fn js_json_parse(text_ptr: *const StringHeader) -> u64;

    /// Pull the `*mut StringHeader` out of a NaN-boxed JsValue,
    /// regardless of whether the box is STRING_TAG or POINTER_TAG.
    /// Returns 0 if the value isn't a string.
    fn js_get_string_pointer_unified(value: f64) -> i64;

    /// Stringify any NaN-boxed JsValue (number, bool, etc.) — the
    /// runtime's general-purpose toString helper.
    fn js_jsvalue_to_string(value: f64) -> *mut StringHeader;

    /// JSON.stringify with type hint. Same as
    /// `perry_ffi::json_stringify` modulo the type-hint argument.
    fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;

    /// True if `ptr` (raw POINTER_TAG-masked address) points at a
    /// `BufferHeader` registered with the runtime's BUFFER_REGISTRY.
    /// Used to distinguish `Buffer` / `Uint8Array` payloads from
    /// `StringHeader`-shaped objects when building response bodies.
    /// Same C-exposed extern perry-ext-http-server uses (see
    /// `crates/perry-runtime/src/buffer.rs:601`).
    fn js_buffer_is_buffer(ptr: i64) -> i32;
}

/// `BufferHeader` mirror — must match `crates/perry-runtime/src/buffer.rs`'s
/// layout (`{ length: u32, capacity: u32 }`, data immediately after the
/// 8-byte header). Distinct from `StringHeader` (20-byte header), so a
/// Buffer must NEVER be read through the string-shaped path.
#[repr(C)]
struct BufferHeader {
    length: u32,
    capacity: u32,
}

/// Body type returned by `build_response_body` / `jsvalue_to_response_body`
/// when the caller needs to pick a sensible default `content-type`.
/// `Binary` is set when the handler returned a `Buffer` / `Uint8Array`;
/// the caller defaults its content-type to `application/octet-stream`
/// instead of `application/json` so binary assets (PNG, WASM, …) ship
/// untouched (#1120).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyKind {
    Binary,
    TextOrJson,
}

static CONTEXT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Unified request / reply context — backs both Fastify `(req, reply)`
/// pairs and Hono `(c)` handlers.
pub struct FastifyContext {
    pub id: u64,
    pub request_id: u64,
    pub method: String,
    pub url: String,
    pub query_string: String,
    pub params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,

    // Reply state
    pub status_code: u16,
    pub response_headers: Vec<(String, String)>,
    pub sent: bool,
    pub response_body: Option<Vec<u8>>,
    /// User data stashed by auth middleware — NaN-boxed bits.
    pub user_data: u64,
    /// Cached `request.params` JS object, built on first access by
    /// `js_fastify_req_params_object`. `0` means uncached; otherwise the
    /// NaN-boxed object-pointer bits (`POINTER_TAG` top 16 = `0x7FFD`), so `0`
    /// never collides with a real entry. `AtomicU64` (not `Cell`) because the
    /// handle registry hands out shared `&FastifyContext` and the type must stay
    /// `Send + Sync`. Each request gets a fresh context (reset to 0 in `new`), so
    /// the cache is naturally request-scoped — no inter-request leak. The GC root
    /// scanner visits this slot (`scan_fastify_roots`) so a copying GC between the
    /// first build and a later `req.params` read keeps the object alive *and*
    /// relocates the cached pointer.
    pub params_object_cache: AtomicU64,
    /// Cached `request.query` JS object. Same encoding and invariants as
    /// `params_object_cache`.
    pub query_object_cache: AtomicU64,
    /// Cached `request.headers` JS object. Same encoding and invariants as
    /// `params_object_cache`. Handlers commonly read several headers per request,
    /// so caching the constructed object after the first read makes later reads an
    /// atomic load.
    pub headers_object_cache: AtomicU64,
}

impl FastifyContext {
    pub fn new(
        request_id: u64,
        method: String,
        url: String,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
        params: HashMap<String, String>,
    ) -> Self {
        let (path, query_string) = match url.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (url.clone(), String::new()),
        };
        Self {
            id: CONTEXT_ID_COUNTER.fetch_add(1, Ordering::SeqCst),
            request_id,
            method,
            url: path,
            query_string,
            params,
            headers,
            body,
            status_code: 200,
            response_headers: Vec::new(),
            sent: false,
            response_body: None,
            user_data: TAG_UNDEFINED,
            params_object_cache: AtomicU64::new(0),
            query_object_cache: AtomicU64::new(0),
            headers_object_cache: AtomicU64::new(0),
        }
    }

    pub fn get_query_param(&self, name: &str) -> Option<String> {
        for pair in self.query_string.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                if key == name {
                    return Some(urlencoding_decode(value));
                }
            }
        }
        None
    }

    pub fn get_query_params(&self) -> HashMap<String, String> {
        let mut params = HashMap::new();
        for pair in self.query_string.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                params.insert(key.to_string(), urlencoding_decode(value));
            }
        }
        params
    }

    pub fn body_string(&self) -> Option<String> {
        self.body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).to_string())
    }

    pub fn set_status(&mut self, code: u16) {
        self.status_code = code;
    }

    pub fn add_header(&mut self, name: &str, value: &str) {
        self.response_headers
            .push((name.to_string(), value.to_string()));
    }
}

/// Minimal RFC-3986 percent-decoder + `+ → space` normalization for
/// query strings.
fn urlencoding_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// Read a `*const StringHeader` into an owned `String`.
pub(crate) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Pull a string out of a raw i64 NaN-boxed value.
pub(crate) unsafe fn string_from_nanboxed(value: i64) -> Option<String> {
    let ptr = js_get_string_pointer_unified(f64::from_bits(value as u64));
    if ptr == 0 {
        return None;
    }
    string_from_header(ptr as *const StringHeader)
}

/// Pull a string out of a NaN-boxed f64 JsValue.
unsafe fn extract_jsvalue_string(value: f64) -> Option<String> {
    let ptr = js_get_string_pointer_unified(value);
    if ptr == 0 {
        return None;
    }
    string_from_header(ptr as *const StringHeader)
}

// ============================================================================
// FFI: Request methods (Fastify + Hono)
// ============================================================================

/// `request.method` / `c.req.method`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_method(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        return alloc_string(&ctx.method).as_raw();
    }
    std::ptr::null_mut()
}

/// `request.url` / `c.req.url` — the path **with** its query string, matching
/// Node/Fastify where `req.url` is the raw request target (e.g. `/p?k=v`).
/// `FastifyContext` splits the incoming target into `url` (path) and
/// `query_string` for routing/`req.query`, so reconstruct the original here.
/// #1705: returning the bare path dropped `?k=v`, so `@hono/perry-server`'s
/// `new Request('http://' + host + req.url)` lost the query and
/// `c.req.query()` / `new URL(c.req.url).searchParams` came back empty.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_url(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if ctx.query_string.is_empty() {
            return alloc_string(&ctx.url).as_raw();
        }
        let full = format!("{}?{}", ctx.url, ctx.query_string);
        return alloc_string(&full).as_raw();
    }
    std::ptr::null_mut()
}

/// `request.params` — JSON-encoded params object as a string. Used by
/// codegen paths that don't yet support object returns.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_params(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Ok(json) = serde_json::to_string(&ctx.params) {
            return alloc_string(&json).as_raw();
        }
    }
    std::ptr::null_mut()
}

/// `request.params` returning a NaN-boxed object — built via
/// perry-ffi's shape-aware allocator.
///
/// Caches the constructed object on the `FastifyContext` on first access, so a
/// handler reading `req.params.a` then `req.params.b` builds the object once.
/// The cache holds the NaN-boxed object bits; the GC root scanner visits the
/// slot, so a copying GC between reads keeps it alive and relocates the pointer.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_params_object(ctx_handle: Handle) -> f64 {
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let ctx = match get_handle::<FastifyContext>(ctx_handle) {
        Some(c) => c,
        None => return undefined,
    };
    // Fast path: object already built earlier in this same request.
    let cached = ctx.params_object_cache.load(Ordering::Acquire);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    match build_string_map_object(&ctx.params) {
        // Release so a later Acquire load on this context sees the bits.
        Some(obj) => {
            ctx.params_object_cache
                .store(obj.to_bits(), Ordering::Release);
            obj
        }
        None => undefined,
    }
}

/// `c.req.param('id')` — single param accessor.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_param(ctx_handle: Handle, name: i64) -> *mut StringHeader {
    let name = match string_from_nanboxed(name) {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Some(value) = ctx.params.get(&name) {
            return alloc_string(value).as_raw();
        }
    }
    std::ptr::null_mut()
}

/// `request.query` — JSON-encoded query params.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_query(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        let params = ctx.get_query_params();
        if let Ok(json) = serde_json::to_string(&params) {
            return alloc_string(&json).as_raw();
        }
    }
    std::ptr::null_mut()
}

/// `request.query` returning a NaN-boxed object. Cached on the
/// `FastifyContext` on first access — same mechanism as
/// `js_fastify_req_params_object`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_query_object(ctx_handle: Handle) -> f64 {
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let ctx = match get_handle::<FastifyContext>(ctx_handle) {
        Some(c) => c,
        None => return undefined,
    };
    let cached = ctx.query_object_cache.load(Ordering::Acquire);
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let params = ctx.get_query_params();
    match build_string_map_object(&params) {
        Some(obj) => {
            ctx.query_object_cache
                .store(obj.to_bits(), Ordering::Release);
            obj
        }
        None => undefined,
    }
}

/// `request.body` — raw body as a string.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_body(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Some(body) = ctx.body_string() {
            return alloc_string(&body).as_raw();
        }
    }
    std::ptr::null_mut()
}

/// `await request.json()` — parse the body as JSON, returning a
/// NaN-boxed object.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_json(ctx_handle: Handle) -> f64 {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Some(body) = ctx.body_string() {
            // Round-trip through perry-runtime's JSON parser.
            let body_str = alloc_string(&body);
            if !body_str.is_null() {
                let bits = js_json_parse(body_str.as_raw() as *const StringHeader);
                return f64::from_bits(bits);
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `request.headers` — full headers as a NaN-boxed object. Cached on the
/// `FastifyContext` on first access — same mechanism as
/// `js_fastify_req_params_object`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_headers(ctx_handle: Handle) -> i64 {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        let cached = ctx.headers_object_cache.load(Ordering::Acquire);
        if cached != 0 {
            return cached as i64;
        }
        if let Some(obj_f64) = build_string_map_object(&ctx.headers) {
            ctx.headers_object_cache
                .store(obj_f64.to_bits(), Ordering::Release);
            return obj_f64.to_bits() as i64;
        }
    }
    TAG_UNDEFINED as i64
}

/// `request.headers['x-foo']` — single header lookup.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_header(ctx_handle: Handle, name: i64) -> *mut StringHeader {
    let name = match string_from_nanboxed(name) {
        Some(n) => n.to_lowercase(),
        None => return std::ptr::null_mut(),
    };
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Some(value) = ctx.headers.get(&name) {
            return alloc_string(value).as_raw();
        }
    }
    std::ptr::null_mut()
}

/// `request.user` — accessor for user data attached by middleware.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_get_user_data(ctx_handle: Handle) -> f64 {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        return f64::from_bits(ctx.user_data);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `request.user = data` — setter for middleware-attached state.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_set_user_data(ctx_handle: Handle, data: f64) {
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.user_data = data.to_bits();
    }
}

/// #1293 — membership probe for the well-known-flipped generic
/// dispatch path. Returns 1 iff `handle` is a live `FastifyContext`
/// in *this crate's* perry-ffi registry, 0 otherwise.
///
/// When the well-known flip routes `fastify` to perry-ext-fastify,
/// perry-stdlib's `js_handle_method_dispatch` / `js_handle_property_dispatch`
/// can no longer see the request/reply handle — it lives in perry-ffi's
/// registry, not perry-stdlib's. So `(request as any).json()` /
/// `(request as any).body` (where the `as any` cast erased the static
/// type and codegen emitted a generic dynamic dispatch rather than a
/// `NativeMethodCall`) silently returned NaN / undefined. perry-stdlib's
/// external-fastify dispatch arms call this to confirm the handle is ours
/// before forwarding to the `js_fastify_req_*` / `js_fastify_ctx_*`
/// exports. Mirrors `js_ext_net_is_socket_handle` (perry-ext-net).
#[no_mangle]
pub unsafe extern "C" fn js_ext_fastify_is_context_handle(ctx_handle: Handle) -> i32 {
    if get_handle::<FastifyContext>(ctx_handle).is_some() {
        1
    } else {
        0
    }
}

// ============================================================================
// FFI: Reply methods (Fastify style — `reply.status(...).send(...)`)
// ============================================================================

/// `reply.status(code)` — chainable, returns the same handle.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_reply_status(ctx_handle: Handle, code: f64) -> Handle {
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.status_code = code as u16;
    }
    ctx_handle
}

/// `reply.header(name, value)` — chainable.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_reply_header(
    ctx_handle: Handle,
    name: i64,
    value: i64,
) -> Handle {
    let name = match string_from_nanboxed(name) {
        Some(n) => n,
        None => return ctx_handle,
    };
    let value = match string_from_nanboxed(value) {
        Some(v) => v,
        None => return ctx_handle,
    };
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.response_headers.push((name, value));
    }
    ctx_handle
}

/// `reply.type(value)` — Fastify alias for `reply.header("content-type", value)`.
/// Returns the reply handle for chaining.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_reply_type(ctx_handle: Handle, value: i64) -> Handle {
    let value = match string_from_nanboxed(value) {
        Some(v) => v,
        None => return ctx_handle,
    };
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.response_headers
            .push(("content-type".to_string(), value));
    }
    ctx_handle
}

/// `reply.send(payload)` — finalize the response. Returns true if the
/// response was newly sent, false if `ctx.sent` was already true.
///
/// When the payload is a `Buffer` / `Uint8Array` and the handler
/// hasn't pinned a `content-type` yet (`reply.type(...)`), the
/// response defaults to `application/octet-stream` so binary assets
/// don't ship as JSON.toString'd bytes (#1120).
#[no_mangle]
pub unsafe extern "C" fn js_fastify_reply_send(ctx_handle: Handle, data: f64) -> bool {
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        if ctx.sent {
            return false;
        }
        let (body, kind) = jsvalue_to_response_body(data);
        if kind == BodyKind::Binary
            && !ctx
                .response_headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        {
            ctx.response_headers.push((
                "content-type".to_string(),
                "application/octet-stream".to_string(),
            ));
        }
        ctx.response_body = Some(body);
        ctx.sent = true;
        return true;
    }
    false
}

// ============================================================================
// FFI: Context methods (Hono style — `c.json(...)`, `c.text(...)`, …)
// ============================================================================

/// `c.json(data, status?)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_ctx_json(ctx_handle: Handle, data: f64, status: f64) -> f64 {
    // #1240 — `request.json()` (Fetch API) shares the dispatch slot with
    // `reply.json(data)` / `c.json(data, status)` because both receivers are
    // backed by the same FastifyContext handle. A zero-arg call from user code
    // arrives here with `data` padded to NaN-boxed undefined; in that case route
    // to `js_fastify_req_json` so existing Fetch-style codebases keep working
    // without touching `request.body`.
    if data.to_bits() == TAG_UNDEFINED {
        return js_fastify_req_json(ctx_handle);
    }

    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        if status > 0.0 {
            ctx.status_code = status as u16;
        }
        ctx.response_headers.push((
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        ));
        let body = jsvalue_to_json_string(data);
        ctx.response_body = Some(body.into_bytes());
        ctx.sent = true;
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `c.text(text, status?)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_ctx_text(ctx_handle: Handle, text: i64, status: f64) -> f64 {
    let text = string_from_nanboxed(text).unwrap_or_default();
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        if status > 0.0 {
            ctx.status_code = status as u16;
        }
        ctx.response_headers.push((
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ));
        ctx.response_body = Some(text.into_bytes());
        ctx.sent = true;
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `c.html(html, status?)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_ctx_html(ctx_handle: Handle, html: i64, status: f64) -> f64 {
    let html = string_from_nanboxed(html).unwrap_or_default();
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        if status > 0.0 {
            ctx.status_code = status as u16;
        }
        ctx.response_headers.push((
            "content-type".to_string(),
            "text/html; charset=utf-8".to_string(),
        ));
        ctx.response_body = Some(html.into_bytes());
        ctx.sent = true;
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `c.redirect(url, status?)`.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_ctx_redirect(ctx_handle: Handle, url: i64, status: f64) -> f64 {
    let url = string_from_nanboxed(url).unwrap_or_default();
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.status_code = if status > 0.0 { status as u16 } else { 302 };
        ctx.response_headers.push(("location".to_string(), url));
        ctx.response_body = Some(Vec::new());
        ctx.sent = true;
    }
    f64::from_bits(TAG_UNDEFINED)
}

// ============================================================================
// Helpers — shared with server.rs
// ============================================================================

/// Convert a NaN-boxed JsValue to response bytes. Strings pass through
/// as-is; `Buffer` / `Uint8Array` ships its raw bytes (no UTF-8
/// round-trip); everything else gets JSON-stringified. The caller
/// inspects the returned `BodyKind` to pick a default content-type
/// when the handler didn't set one.
///
/// Issue #1120: pre-fix, every non-string value funnelled through
/// `js_json_stringify`. A returned Buffer surfaced as Buffer.toJSON's
/// `{"type":"Buffer","data":[...]}` payload (~6× bloat) and the
/// caller still pushed `application/json` for the content-type.
pub(crate) unsafe fn jsvalue_to_response_body(value: f64) -> (Vec<u8>, BodyKind) {
    let jsv = JsValue::from_bits(value.to_bits());
    if jsv.is_string() {
        if let Some(s) = extract_jsvalue_string(value) {
            return (s.into_bytes(), BodyKind::TextOrJson);
        }
    }
    if let Some(bytes) = extract_buffer_bytes(value) {
        return (bytes, BodyKind::Binary);
    }
    (
        jsvalue_to_json_string(value).into_bytes(),
        BodyKind::TextOrJson,
    )
}

/// Probe BUFFER_REGISTRY for a POINTER_TAG'd value. Returns the raw
/// bytes of the underlying `BufferHeader` payload if `value` is a
/// `Buffer` / `Uint8Array`; `None` otherwise. Shared with `server.rs`'s
/// `build_response_body` (#1120).
pub(crate) unsafe fn extract_buffer_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JsValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return None;
    }
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
    if js_buffer_is_buffer(raw) == 0 {
        return None;
    }
    let buf = raw as *const BufferHeader;
    if buf.is_null() {
        return None;
    }
    let len = (*buf).length as usize;
    let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
    Some(std::slice::from_raw_parts(data, len).to_vec())
}

/// Serialize a NaN-boxed JsValue as a JSON string.
pub(crate) unsafe fn jsvalue_to_json_string(value: f64) -> String {
    let jsv = JsValue::from_bits(value.to_bits());
    if jsv.is_undefined() || jsv.is_null() {
        return "null".to_string();
    }
    if jsv.is_bool() {
        return if jsv.to_bool() {
            "true".to_string()
        } else {
            "false".to_string()
        };
    }
    if jsv.is_number() {
        return format!("{}", value);
    }
    if jsv.is_string() {
        if let Some(s) = extract_jsvalue_string(value) {
            return serde_json::to_string(&s).unwrap_or_else(|_| format!("\"{}\"", s));
        }
    }
    if jsv.is_pointer() {
        let str_ptr = js_json_stringify(value, 0);
        if !str_ptr.is_null() {
            if let Some(s) = string_from_header(str_ptr) {
                return s;
            }
        }
    }
    let str_ptr = js_jsvalue_to_string(value);
    if !str_ptr.is_null() {
        if let Some(s) = string_from_header(str_ptr) {
            return s;
        }
    }
    "null".to_string()
}

/// Build a NaN-boxed object from a `HashMap<String, String>`. Returns
/// the boxed pointer as f64. None on allocation failure / empty.
unsafe fn build_string_map_object(map: &HashMap<String, String>) -> Option<f64> {
    let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
    let (packed, shape_id) = build_object_shape(&keys);
    let count = keys.len() as u32;
    let obj: *mut ObjectHeader =
        js_object_alloc_with_shape(shape_id, count, packed.as_ptr(), packed.len() as u32);
    if obj.is_null() {
        return None;
    }
    for (i, key) in keys.iter().enumerate() {
        if let Some(val) = map.get(*key) {
            let s = alloc_string(val);
            let v = JsValue::from_string_ptr(s.as_raw());
            js_object_set_field(obj, i as u32, v);
        }
    }
    let v = JsValue::from_object_ptr(obj);
    Some(f64::from_bits(v.bits()))
}

// Suppress unused-import warnings for FFI types that may not appear
// in this module's signatures but need to remain in scope for the
// crate's exports.
#[allow(dead_code)]
fn _link_keepalive() -> Option<*mut ArrayHeader> {
    None
}
