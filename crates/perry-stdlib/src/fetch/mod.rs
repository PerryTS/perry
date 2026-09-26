//! HTTP Fetch module (node-fetch compatible)
//!
//! Native implementation of the 'node-fetch' npm package on the turnloop
//! client engine (`crate::turnloop_client`).
//! Provides fetch() function for making HTTP requests.

use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::common::async_bridge::queue_promise_resolution;

unsafe extern "C" {
    fn js_fetch_take_pending_redirect() -> i32;
}

/// `RequestInit.redirect`, as the fetch surface sees it.
/// `turnloop_bridge::engine_redirect` translates it for the transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FetchRedirectMode {
    Follow,
    Error,
    Manual,
}

// Web Fetch `Headers` FFI — split out to keep this file under the 2,000-line
// lint gate (#1649). The child module sees mod.rs's private items via its
// `use super::*`.
mod abort_bridge;
pub use abort_bridge::*;

// turnloop P6: the outbound transport. Every transport-bearing entry point
// below hands its request to it; a request it cannot build rejects (see the
// bridge's module note).
mod headers;
mod request_handle;
mod transport_error;
#[path = "turnloop_bridge.rs"]
mod turnloop_bridge;
pub use headers::*;

// Cached bound-method values for Fetch `Headers` handles — split out to keep
// this file under the 2,000-line lint gate. Same child-module/`use super::*`
// contract as `headers`.
mod headers_method_value;
pub(crate) use headers_method_value::headers_bound_method_value;

// Untyped handle dispatch helpers (`dispatch_request_method`,
// `dispatch_response_property`, …) — split out to keep this file under the
// 2,000-line lint gate (#1698). Same child-module/`use super::*` contract as
// `headers`.
mod dispatch;
pub use dispatch::*;

mod body_metadata;
pub use body_metadata::*;

// Bridge used by perry-ext-http's Bun.serve adapter. The ext crate owns the
// listener, while this module owns the Fetch Request/Response registries; the
// small JSON ABI keeps those ownership boundaries intact.
mod bun_server_bridge;
pub use bun_server_bridge::*;

// GC root scanner for the heap values the Fetch registries hold (#8163):
// the two bound-method caches and `RequestRecord::signal`. Same
// child-module/`use super::*` contract as `headers`.
mod gc;
mod lifecycle;

// Web Fetch `Request` constructors (`js_request_new` /
// `js_request_new_from_init`) — split out to keep this file under the
// 2,000-line lint gate (#5458). Same child-module/`use super::*` contract as
// `headers`.
mod request_copy;
pub use request_copy::js_request_new_from_input;
mod request_ctor;
pub use request_ctor::*;

// Web Fetch `Response` constructor/accessors — split out to keep this file
// under the 2,000-line lint gate.
mod response_ctor;
use response_ctor::alloc_response;
pub use response_ctor::{js_response_clone, js_response_get_headers, js_response_new};

// Web Fetch constructor validation helpers (#2640 / #2643) — split out to
// keep this file under the 2,000-line lint gate.
mod validation;
use validation::{
    is_forbidden_method, is_null_body_status, is_redirect_status, is_valid_status_text,
    normalize_method, parse_redirect_location, redirect_status_from_value,
};

// Web Fetch handles must stay below the small-handle cutoff while avoiding
// the low native-id range exposed by `node:http` (#3973/#3974 via #4004). The
// band boundaries are owned by `perry_runtime::value::addr_class` (the
// runtime's magnitude checks classify against them).
pub(crate) const FETCH_HANDLE_ID_START: usize =
    perry_runtime::value::addr_class::FETCH_HANDLE_BAND_START;
pub(crate) const FETCH_HANDLE_ID_END: usize =
    perry_runtime::value::addr_class::FETCH_HANDLE_BAND_END;

// Response handle storage

static FETCH_RESPONSES: std::sync::LazyLock<Mutex<HashMap<usize, FetchResponse>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
/// One shared allocator for the whole Web Fetch handle family —
/// Response, Request, Headers, and Blob. Their registries stay separate
/// HashMaps, but a unified allocator guarantees an id belongs to exactly one
/// of them (no more "Request id 1 == Response id 1"). This is what lets the
/// runtime handle-dispatch arms (`dispatch_request_method` /
/// `dispatch_response_method` / …) distinguish handle types by
/// registry-membership alone for any-typed / computed-key calls, where the
/// static type was lost. The counter starts in a high subrange to avoid
/// colliding with perry-ffi handles exposed by `node:http`.
static FREE_FETCH_HANDLE_IDS: std::sync::LazyLock<Mutex<Vec<usize>>> =
    std::sync::LazyLock::new(|| Mutex::new(Vec::new()));
static NEXT_FETCH_HANDLE_ID: std::sync::LazyLock<Mutex<usize>> =
    std::sync::LazyLock::new(|| Mutex::new(FETCH_HANDLE_ID_START));
static STREAM_HANDLES: std::sync::LazyLock<Mutex<HashMap<usize, StreamState>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_STREAM_ID: std::sync::LazyLock<Mutex<usize>> =
    std::sync::LazyLock::new(|| Mutex::new(1));

/// Global proxy override installed by `undici.setGlobalDispatcher(new
/// ProxyAgent(...))` via `js_fetch_set_global_proxy` (perry-ext-undici),
/// as `(uri, token)`. `None` = direct connections. The turnloop engine
/// reads it per request (`turnloop_client::proxy_for`) and runs the
/// CONNECT tunnel itself.
static GLOBAL_PROXY_URI: std::sync::LazyLock<std::sync::RwLock<Option<(String, Option<String>)>>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(None));

/// Normalize and validate an undici `ProxyAgent` URI and token, with the
/// acceptance rules the reqwest client this replaced applied at install time
/// (`reqwest::Proxy::all` + `HeaderValue::from_str`), so
/// `js_fetch_set_global_proxy`'s 0.0/1.0 answer is unchanged:
///
/// * a URI without a scheme (or without a host) is read as `http://<uri>`;
/// * the scheme must be `http` or `https`, and there must be a host;
/// * the token must be a valid header value (it is the literal
///   `Proxy-Authorization` value).
///
/// The normalized URI is what is stored, so a bare `host:port` reaches the
/// engine as the `http://` proxy it always meant. An `https://` proxy is
/// accepted here, as before, and refused per request by the engine — see
/// `turnloop_client::Declined::Proxy`.
fn normalize_proxy(uri: &str, token: Option<&str>) -> Result<String, String> {
    // reqwest's `IntoProxyScheme`: a parse that failed for want of a base, or
    // that produced no host (`localhost:8080` reads as scheme `localhost`), is
    // retried as `http://<uri>`; any other parse failure is final.
    let parsed = match url::Url::parse(uri) {
        Ok(parsed) if parsed.has_host() => parsed,
        Ok(_) | Err(url::ParseError::RelativeUrlWithoutBase) => {
            url::Url::parse(&format!("http://{uri}"))
                .map_err(|e| format!("Invalid proxy URI \"{uri}\": {e}"))?
        }
        Err(e) => return Err(format!("Invalid proxy URI \"{uri}\": {e}")),
    };
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(format!("Invalid proxy URI \"{uri}\": unsupported proxy"));
    }
    if let Some(token) = token {
        http::HeaderValue::from_str(token).map_err(|e| format!("Invalid proxy token: {e}"))?;
    }
    Ok(parsed.to_string())
}

/// Install (or clear) the process-wide fetch proxy. Called by
/// perry-ext-undici's `setGlobalDispatcher` glue; also exported so any
/// other binding can reuse it. A null `uri_ptr` clears the proxy
/// (an undici `Agent` dispatcher = direct connections). Returns 1.0 on
/// success, 0.0 when the proxy URI/token is invalid (the current proxy
/// state is left unchanged in that case).
///
/// # Safety
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_set_global_proxy(
    uri_ptr: *const StringHeader,
    token_ptr: *const StringHeader,
) -> f64 {
    if uri_ptr.is_null() {
        if let Ok(mut uri) = GLOBAL_PROXY_URI.write() {
            *uri = None;
            return 1.0;
        }
        return 0.0;
    }
    let Some(uri) = string_from_header(uri_ptr).filter(|uri| !uri.is_empty()) else {
        return 0.0;
    };
    let token = string_from_header(token_ptr).filter(|t| !t.is_empty());
    match normalize_proxy(&uri, token.as_deref()) {
        Ok(uri) => {
            if let Ok(mut stored) = GLOBAL_PROXY_URI.write() {
                *stored = Some((uri, token));
                1.0
            } else {
                0.0
            }
        }
        Err(_) => 0.0,
    }
}

/// The installed dispatcher proxy as `(uri, token)`, for the turnloop engine.
pub(crate) fn global_dispatcher_proxy() -> Option<(String, Option<String>)> {
    GLOBAL_PROXY_URI.read().ok()?.clone()
}

/// Request headers `fetch` refuses outright, and the failure to reject with.
///
/// The Fetch standard's forbidden-header list is mostly *ignored* — a value is
/// dropped and the request goes out — but `Expect` is different in undici: it
/// throws, so `fetch()` rejects. Perry did neither thing consistently. On the
/// reqwest path the header went on the wire and the request SUCCEEDED, which is
/// a silent divergence from Node. On the turnloop path it reached
/// `Http1Connection::start`, which read `expect: 100-continue` on a non-empty
/// body as "park the upload until the server says 100" — `can_send_body()` went
/// false and the very next `send_body` failed `UND_ERR_INVALID_ARG "request
/// body is not writable"`, so the POST never left the process and the promise
/// rejected with a message naming the body rather than the header.
///
/// So the same program gave three different answers depending on transport, and
/// none of them was Node's. Deciding it here, before dispatch, is what makes the
/// answer transport-independent.
fn forbidden_header_failure(
    headers: &HashMap<String, String>,
) -> Option<transport_error::FetchFailure> {
    headers
        .keys()
        .find(|name| name.eq_ignore_ascii_case("expect"))
        .map(|_| transport_error::FetchFailure::forbidden_header("expect"))
}

fn alloc_fetch_handle_id() -> usize {
    let recycled = FREE_FETCH_HANDLE_IDS.lock().unwrap().pop();
    let id = match recycled {
        Some(id) => id,
        None => {
            let mut id_guard = NEXT_FETCH_HANDLE_ID.lock().unwrap();
            let id = *id_guard;
            if id >= FETCH_HANDLE_ID_END {
                panic!("Web Fetch handle id range exhausted");
            }
            *id_guard += 1;
            drop(id_guard);
            lifecycle::note_fresh_band_id(id);
            id
        }
    };
    lifecycle::register(id);
    if perry_runtime::hot_diag::receiver_repr_on() {
        perry_runtime::hot_diag::receiver_repr_note_constructed(
            perry_runtime::hot_diag::ReceiverReprFamily::Fetch,
        );
    }
    id
}

/// Serializes the unit tests that allocate or release Fetch handle ids
/// (#11417).
///
/// The registries and `FREE_FETCH_HANDLE_IDS` are process-global and the free
/// list is LIFO, so an id one test releases is handed straight to whichever
/// concurrent test allocates next. A test asserting "the full trace released
/// my id" (`!HEADERS_REGISTRY.contains_key(&id)`) then sees ANOTHER test's
/// record under the same number and fails although reclamation worked. The
/// id band is shared by design; the tests take turns on it.
#[cfg(test)]
static HANDLE_BAND_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn handle_band_test_lock() -> std::sync::MutexGuard<'static, ()> {
    HANDLE_BAND_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod headers_json_test;

#[cfg(test)]
mod tests;

struct StreamState {
    status: u8, // 0=connecting, 1=streaming, 2=done, 3=error
    pending_lines: Vec<String>,
    partial: String,
    #[allow(dead_code)]
    http_status: u16,
    #[allow(dead_code)]
    error: String,
}

impl StreamState {
    /// The line splitter, shared by both transports so the poll surface cannot
    /// observe which one carried the stream. Bytes accumulate in `partial`
    /// until a `\n`; empty lines are dropped, which is what the poll contract
    /// has always done (an empty return from `js_fetch_stream_poll` means
    /// "nothing pending", so an empty line could not be represented).
    fn push_text(&mut self, text: &str) {
        self.partial.push_str(text);
        while let Some(pos) = self.partial.find('\n') {
            let line = self.partial[..pos].to_string();
            self.partial = self.partial[pos + 1..].to_string();
            if !line.is_empty() {
                self.pending_lines.push(line);
            }
        }
    }

    /// End of body: flush a trailing unterminated line and mark the stream
    /// complete.
    fn finish(&mut self) {
        if !self.partial.is_empty() {
            let rest = std::mem::take(&mut self.partial);
            self.pending_lines.push(rest);
        }
        self.status = 2;
    }
}

/// Run `f` against one live stream's state. A miss is a no-op: the JS side may
/// have called `js_fetch_stream_close` while bytes were still arriving.
fn with_stream(id: usize, f: impl FnOnce(&mut StreamState)) {
    if let Ok(mut guard) = STREAM_HANDLES.lock() {
        if let Some(state) = guard.get_mut(&id) {
            f(state);
        }
    }
}

struct FetchResponse {
    status: u16,
    status_text: String,
    headers: HeadersStore,
    body: Vec<u8>,
    body_present: bool,
    body_used: bool,
    type_name: String,
    url: String,
    redirected: bool,
    /// Cached Headers handle id, allocated on first `response.headers`
    /// access. None until a property/method dispatcher needs to expose
    /// the headers as a Headers instance. Subsequent reads of `.headers`
    /// return the same id (preserves `res.headers === res.headers`).
    cached_headers_id: Option<usize>,
    /// Cached ReadableStream handle id for `response.body`, allocated on
    /// first read so repeat `.body` reads return the same stream (the spec
    /// requires a stable `ReadableStream`; getting a fresh unlocked stream
    /// each time would silently un-lock a reader). None for an empty body —
    /// `Response.body` is `ReadableStream | null` (#1650).
    cached_body_stream_id: Option<usize>,
    /// Original ReadableStream body passed to `new Response(stream, init)`.
    /// Unlike buffered bodies, this must stay lazy: constructing a Response
    /// must not synchronously drain a producer whose chunks appear only after
    /// downstream pulls (TanStack Start / React SSR relies on that).
    body_stream_id: Option<usize>,
}

/// Return the one `Headers` registry handle that backs `response.headers`.
///
/// Both typed property lowering (`js_response_get_headers`) and untyped handle
/// dispatch must come through this helper.  Allocating a fresh snapshot in the
/// typed path made repeated reads disagree and, more importantly, discarded
/// mutations made through an earlier view.  `NextResponse.cookies` mutates the
/// response through that Headers view, so losing the handle also lost its
/// `Set-Cookie` header when the response crossed a module boundary.
fn response_headers_handle(resp_id: usize) -> f64 {
    let mut responses = FETCH_RESPONSES.lock().unwrap();
    let Some(response) = responses.get_mut(&resp_id) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if let Some(id) = response.cached_headers_id {
        return handle_to_f64(id);
    }
    let id = alloc_headers(response.headers.clone());
    response.cached_headers_id = Some(id);
    handle_to_f64(id)
}

/// Snapshot the observable Headers backing, including mutations made through
/// `response.headers`, for operations such as `Response.clone()`.
fn response_headers_snapshot(response: &FetchResponse) -> HeadersStore {
    response
        .cached_headers_id
        .and_then(|id| {
            HEADERS_REGISTRY
                .lock()
                .unwrap()
                .get(&id)
                .map(|record| record.store.clone())
        })
        .unwrap_or_else(|| response.headers.clone())
}

/// Snapshot the observable Request headers, including mutations made through
/// the lazily cached `request.headers` object.
fn request_headers_snapshot(request: &RequestRecord) -> HeadersStore {
    request
        .cached_headers_id
        .and_then(|id| {
            HEADERS_REGISTRY
                .lock()
                .unwrap()
                .get(&id)
                .map(|record| record.store.clone())
        })
        .unwrap_or_else(|| request.headers.clone())
}

thread_local! {
    static PENDING_FETCH_BODY_STREAM_ID: Cell<usize> = const { Cell::new(0) };
    // Codegen coerces BodyInit to a StringHeader before js_response_new, so
    // preserve whether the original value was a string long enough for the
    // Response constructor to install Fetch's default Content-Type.
    static PENDING_FETCH_BODY_CONTENT_TYPE: Cell<Option<&'static str>> =
        const { Cell::new(None) };
}

pub(super) const BODY_CONTENT_TYPE_TEXT_PLAIN: &str = "text/plain;charset=UTF-8";

pub(super) fn set_pending_fetch_body_content_type(content_type: Option<&'static str>) {
    PENDING_FETCH_BODY_CONTENT_TYPE.with(|pending| pending.set(content_type));
}

pub(super) fn reset_pending_fetch_body_init() {
    PENDING_FETCH_BODY_STREAM_ID.with(|pending| pending.set(0));
    PENDING_FETCH_BODY_CONTENT_TYPE.with(|pending| pending.set(None));
}

fn take_pending_fetch_body_content_type() -> Option<&'static str> {
    PENDING_FETCH_BODY_CONTENT_TYPE.with(|pending| pending.replace(None))
}

fn take_pending_fetch_body_stream_id() -> Option<usize> {
    PENDING_FETCH_BODY_STREAM_ID.with(|pending| {
        let id = pending.get();
        pending.set(0);
        if id != 0 && crate::streams::js_stream_handle_kind(id) == 1 {
            Some(id)
        } else {
            None
        }
    })
}

/// Extract the registry id from a Web Fetch handle f64 value.
///
/// Web Fetch handles (Request / Response / Headers / Blob) are returned by
/// constructors as NaN-boxed POINTER_TAG values via `js_nanbox_pointer`, so
/// they look like pointers to the runtime's dispatchers (`js_object_get_field_by_name`,
/// `js_native_call_method`) and route through `HANDLE_PROPERTY/METHOD_DISPATCH`
/// for untyped property access — fixing the hono `request.url` blocker (#421).
///
/// This helper is tolerant during the cross-subsystem migration: it accepts both
/// the canonical NaN-boxed form (top16 ≥ 0x7FF8) AND the legacy raw-float form
/// (`1.0` = id 1, denormal bits, etc.). Other handle subsystems (streams / ws /
/// net / DB) still use the legacy form until Phase 2 of the unification migrates
/// them to NaN-boxed too.
#[inline]
pub(crate) fn handle_id(value: f64) -> usize {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    if top16 >= 0x7FF8 {
        // NaN-boxed (POINTER_TAG / STRING_TAG / etc.): extract lower 48 bits.
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && bits != 0 {
        // Raw integer bits as f64 (denormal-encoded handle id): use bits directly.
        bits as usize
    } else {
        // Legacy float form (1.0 → 1): float-to-int truncation.
        value as usize
    }
}

/// NaN-box a Web Fetch handle id (registry index) into a POINTER_TAG f64
/// for return across the FFI boundary. Pairs with `handle_id` on accessor entry.
#[inline]
pub(crate) fn handle_to_f64(id: usize) -> f64 {
    perry_runtime::value::js_nanbox_pointer(id as i64)
}

pub(crate) use crate::common::string_from_header;

/// Diagnostic: return the number of FETCH_RESPONSES entries.
/// Useful for detecting response handle leaks in long-running services.
#[no_mangle]
pub extern "C" fn js_fetch_response_count() -> i64 {
    FETCH_RESPONSES.lock().map(|g| g.len() as i64).unwrap_or(-1)
}

/// Build a NaN-boxed JSValue holding a real `Error` object for promise
/// rejection. Pre-fix (#236) every fetch error site NaN-boxed a bare
/// `*StringHeader` with `POINTER_TAG` (0x7FFD), which the uncaught-exception
/// printer in `perry-runtime/src/exception.rs` then read as an
/// the first `ObjectHeader` u32 (`class_id` since #8113) — `byte_len` of the message string is
/// neither `OBJECT_TYPE_ERROR` (2) nor `OBJECT_TYPE_REGULAR` (1), so the
/// printer fell through to the generic stringifier which printed
/// `Uncaught exception: [object Object]`. Allocating a real
/// `ErrorHeader` makes the printer take the dedicated Error arm and emit
/// `Uncaught exception: Error: <message>` with a stack frame.
unsafe fn fetch_error_bits<S: AsRef<str>>(msg: S) -> u64 {
    let s = msg.as_ref();
    let msg_str = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    let err = perry_runtime::error::js_error_new_with_message(msg_str);
    JSValue::pointer(err as *const u8).bits()
}

const BODY_ALREADY_USED_MESSAGE: &str = "Body is unusable: Body has already been read";

unsafe fn fetch_type_error_bits<S: AsRef<str>>(msg: S) -> u64 {
    let s = msg.as_ref();
    let msg_str = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    let err = perry_runtime::error::js_typeerror_new(msg_str);
    JSValue::pointer(err as *const u8).bits()
}

unsafe fn reject_fetch_type_error(promise: *mut perry_runtime::Promise, msg: &str) {
    perry_runtime::js_promise_reject(promise, f64::from_bits(fetch_type_error_bits(msg)));
}

unsafe fn throw_fetch_type_error(msg: &str) -> ! {
    perry_runtime::exception::js_throw(f64::from_bits(fetch_type_error_bits(msg)))
}

unsafe fn throw_fetch_range_error(msg: &str) -> ! {
    let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = perry_runtime::error::js_rangeerror_new(msg_str);
    perry_runtime::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()))
}

fn tagged_bool(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

/// Perform a GET request
/// fetch(url) -> Promise<Response>
#[no_mangle]
pub unsafe extern "C" fn js_fetch_get(url_ptr: *const StringHeader) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            let err_msg = "Invalid URL";
            let err_bits = fetch_error_bits(err_msg);
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    turnloop_bridge::dispatch(
        turnloop_bridge::FetchDispatch {
            url,
            method: "GET".to_string(),
            headers: Vec::new(),
            body: None,
            abort_key: None,
            redirect: FetchRedirectMode::Follow,
        },
        promise_ptr,
    );

    promise
}

/// Perform a GET request with Authorization header
/// Used when fetch(url, { headers: { Authorization: "Bearer ..." } }) is needed
#[no_mangle]
pub unsafe extern "C" fn js_fetch_get_with_auth(
    url_ptr: *const StringHeader,
    auth_header_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            let err_msg = "Invalid URL";
            let err_bits = fetch_error_bits(err_msg);
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let auth_header = string_from_header(auth_header_ptr).unwrap_or_default();

    turnloop_bridge::dispatch(
        turnloop_bridge::FetchDispatch {
            url,
            method: "GET".to_string(),
            headers: auth_header
                .is_empty()
                .then(Vec::new)
                .unwrap_or_else(|| vec![("authorization".to_string(), auth_header)]),
            body: None,
            abort_key: None,
            redirect: FetchRedirectMode::Follow,
        },
        promise_ptr,
    );

    promise
}

/// Perform a POST request with Authorization header and JSON body
/// fetchPostWithAuth(url, authHeader, body) -> Promise<Response>
#[no_mangle]
pub unsafe extern "C" fn js_fetch_post_with_auth(
    url_ptr: *const StringHeader,
    auth_header_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            let err_msg = "Invalid URL";
            let err_bits = fetch_error_bits(err_msg);
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let auth_header = string_from_header(auth_header_ptr).unwrap_or_default();
    let body = string_from_header(body_ptr).unwrap_or_default();

    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    if !auth_header.is_empty() {
        headers.push(("authorization".to_string(), auth_header));
    }
    turnloop_bridge::dispatch(
        turnloop_bridge::FetchDispatch {
            url,
            method: "POST".to_string(),
            headers,
            body: Some(body.into_bytes()),
            abort_key: None,
            redirect: FetchRedirectMode::Follow,
        },
        promise_ptr,
    );

    promise
}

/// Perform a POST request with body
/// fetch(url, { method: 'POST', body: '...' }) -> Promise<Response>
#[no_mangle]
pub unsafe extern "C" fn js_fetch_post(
    url_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    content_type_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            let err_msg = "Invalid URL";
            let err_bits = fetch_error_bits(err_msg);
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    // Probe the buffer/typed-array registry before falling back to a string read
    // so a binary body (Buffer / Uint8Array / typed array / ArrayBuffer) is sent
    // byte-for-byte instead of being shifted left 12 bytes by the StringHeader
    // data offset (#5757).
    let form_data_body = body_metadata::serialize_form_data(body_ptr as usize);
    let form_data_content_type = form_data_body
        .as_ref()
        .map(|(_, content_type)| content_type.clone());
    let body = form_data_body
        .map(|(body, _)| body)
        .or_else(|| fetch_request_body_bytes(body_ptr))
        .unwrap_or_default();
    let content_type = string_from_header(content_type_ptr)
        .or(form_data_content_type)
        .unwrap_or_else(|| "application/json".to_string());

    turnloop_bridge::dispatch(
        turnloop_bridge::FetchDispatch {
            url,
            method: "POST".to_string(),
            headers: vec![("content-type".to_string(), content_type)],
            body: Some(body),
            abort_key: None,
            redirect: FetchRedirectMode::Follow,
        },
        promise_ptr,
    );

    promise
}

/// Perform a fetch request with full options (method, headers, body)
/// This is the most flexible fetch function
#[no_mangle]
pub unsafe extern "C" fn js_fetch_with_options(
    url_ptr: *const StringHeader,
    method_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    headers_json_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    // Consume the pending `AbortSignal` (stashed by the fetch thunk / codegen)
    // on the main thread BEFORE allocating the promise, so a GC during the
    // allocation can't move the still-TLS-stashed signal before we read it.
    let abort_state = abort_bridge::take_pending_signal_watch();
    let pending_redirect = js_fetch_take_pending_redirect();

    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;

    // An already-aborted signal rejects the request up front; otherwise keep the
    // optional watch to race the request against.
    let abort_watch = match abort_bridge::watch_or_reject(abort_state, promise_ptr) {
        Some(watch) => watch,
        None => return promise,
    };

    // `fetch(Request)` form: callers (axios/gaxios / any WHATWG-fetch) build a
    // `Request` object and call `fetch(request, init)`; its handle id lands in
    // the `url_ptr` slot. Recover url/method/body/headers from the Request
    // registry so the request is dispatched (`init` members override).
    let form_data_body = body_metadata::serialize_form_data(body_ptr as usize);
    let form_data_content_type = form_data_body
        .as_ref()
        .map(|(_, content_type)| content_type.clone());
    let body_bytes = form_data_body
        .map(|(body, _)| body)
        .or_else(|| fetch_request_body_bytes(body_ptr));
    let mut inputs = match request_handle::resolve_fetch_inputs(
        string_from_header(url_ptr),
        string_from_header(method_ptr),
        // Read the body as raw bytes (binary bodies probe the buffer/typed-array
        // registry first) so a Buffer/Uint8Array body isn't corrupted by a lossy
        // StringHeader read (#5757).
        body_bytes,
        string_from_header(headers_json_ptr),
        url_ptr as usize,
        pending_redirect,
    ) {
        Ok(inputs) => inputs,
        Err(err_bits) => {
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };
    if let Some(content_type) = form_data_content_type {
        inputs
            .custom_headers
            .entry("content-type".to_string())
            .or_insert(content_type);
    }

    // Refused before either transport is chosen, so the answer does not depend
    // on which one would have carried it — which was the bug. See
    // `forbidden_header_failure`.
    if let Some(failure) = forbidden_header_failure(&inputs.custom_headers) {
        queue_promise_resolution(promise_ptr, false, failure.into_js_bits());
        return promise;
    }

    // The watch's only job from here on is its key: the engine owns
    // cancellation, and `js_fetch_notify_signal_aborted` reaches it directly.
    let abort_key = abort_watch
        .as_ref()
        .map(abort_bridge::FetchAbortWatch::signal_ptr);
    turnloop_bridge::dispatch_inputs(inputs, abort_key, promise_ptr);

    promise
}

/// Get response status code
/// response.status -> number
#[no_mangle]
pub extern "C" fn js_fetch_response_status(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let response_id = handle_id(handle);
    let guard = FETCH_RESPONSES.lock().unwrap();
    match guard.get(&response_id) {
        Some(resp) => resp.status as f64,
        None => 0.0,
    }
}

/// Get response status text
/// response.statusText -> string
#[no_mangle]
pub extern "C" fn js_fetch_response_status_text(handle: f64) -> *mut StringHeader {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let response_id = handle_id(handle);
    // Copy out and drop the guard before allocating (see `lifecycle`).
    let status_text = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&response_id)
        .map(|resp| resp.status_text.clone());
    match status_text {
        Some(text) => js_string_from_bytes(text.as_ptr(), text.len() as u32),
        None => std::ptr::null_mut(),
    }
}

/// Check if response was successful (status 200-299)
/// response.ok -> boolean
#[no_mangle]
pub extern "C" fn js_fetch_response_ok(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let response_id = handle_id(handle);
    let guard = FETCH_RESPONSES.lock().unwrap();
    match guard.get(&response_id) {
        Some(resp) => {
            if resp.status >= 200 && resp.status < 300 {
                1.0
            } else {
                0.0
            }
        }
        None => 0.0,
    }
}

/// response.bodyUsed -> boolean
#[no_mangle]
pub extern "C" fn js_response_body_used(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let response_id = handle_id(handle);
    let guard = FETCH_RESPONSES.lock().unwrap();
    tagged_bool(
        guard
            .get(&response_id)
            .map(|resp| resp.body_used)
            .unwrap_or(false),
    )
}

fn consume_response_body(handle: f64) -> Result<Vec<u8>, &'static str> {
    let response_id = handle_id(handle);
    let (body, stream_id) = {
        let mut guard = FETCH_RESPONSES.lock().unwrap();
        let resp = guard
            .get_mut(&response_id)
            .ok_or("Invalid response handle")?;
        if !resp.body_present {
            return Ok(Vec::new());
        }
        if resp.body_used {
            return Err(BODY_ALREADY_USED_MESSAGE);
        }
        resp.body_used = true;
        (resp.body.clone(), resp.body_stream_id)
    };
    if let Some(stream_id) = stream_id {
        return Ok(crate::streams::drain_readable_into_bytes(stream_id));
    }
    Ok(body)
}

/// Get response body as text
/// response.text() -> Promise<string>
///
/// The body is already in-memory at the point of the call, so resolve
/// the promise synchronously via `js_promise_resolve` rather than
/// routing through the deferred `PENDING_RESOLUTIONS` queue. This
/// avoids a hang in the LLVM backend's await loop (which does not
/// drain the pump — see `crates/perry-codegen/src/expr.rs`
/// `Expr::Await` for the rationale).
#[no_mangle]
pub unsafe extern "C" fn js_fetch_response_text(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let promise = perry_runtime::js_promise_new_cross_thread();
    let body = match consume_response_body(handle) {
        Ok(body) => body,
        Err(err_msg) if err_msg == BODY_ALREADY_USED_MESSAGE => {
            reject_fetch_type_error(promise, BODY_ALREADY_USED_MESSAGE);
            return promise;
        }
        Err(err_msg) => {
            let err_nan = f64::from_bits(fetch_error_bits(err_msg));
            perry_runtime::js_promise_reject(promise, err_nan);
            return promise;
        }
    };

    // Convert body to string and resolve synchronously.
    let text = String::from_utf8_lossy(&body).to_string();
    let result_str = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let result_nan = f64::from_bits(JSValue::string_ptr(result_str).bits());
    perry_runtime::js_promise_resolve(promise, result_nan);
    promise
}

/// Parse a Fetch body with the runtime's `JSON.parse` implementation. Besides
/// matching JavaScript number and error semantics, this preserves document
/// order for non-index object keys; `serde_json::Value` uses a sorted map in
/// this build and silently reordered them (#10392).
unsafe fn parse_json_body(body: &[u8]) -> Result<JSValue, f64> {
    let text = String::from_utf8_lossy(body);
    let text_ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    perry_runtime::json::js_json_parse_result(text_ptr)
}

/// Get response body as JSON (parses and returns proper JS object)
/// response.json() -> Promise<object>
#[no_mangle]
pub unsafe extern "C" fn js_fetch_response_json(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let promise = perry_runtime::js_promise_new_cross_thread();
    let body = match consume_response_body(handle) {
        Ok(body) => body,
        Err(err_msg) if err_msg == BODY_ALREADY_USED_MESSAGE => {
            reject_fetch_type_error(promise, BODY_ALREADY_USED_MESSAGE);
            return promise;
        }
        Err(err_msg) => {
            let err_nan = f64::from_bits(fetch_error_bits(err_msg));
            perry_runtime::js_promise_reject(promise, err_nan);
            return promise;
        }
    };

    // Parse and resolve synchronously — see comment on
    // `js_fetch_response_text`.
    match parse_json_body(&body) {
        Ok(js_value) => {
            let result_nan = f64::from_bits(js_value.bits());
            perry_runtime::js_promise_resolve(promise, result_nan);
        }
        Err(error) => {
            perry_runtime::js_promise_reject(promise, error);
        }
    }

    promise
}

/// Simple fetch that returns text directly (convenience function)
/// fetchText(url) -> Promise<string>
#[no_mangle]
pub unsafe extern "C" fn js_fetch_text(
    url_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            let err_msg = "Invalid URL";
            let err_bits = fetch_error_bits(err_msg);
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    turnloop_bridge::dispatch_text(url, promise_ptr);

    promise
}

// ========================================================================
// SSE Streaming Functions
// ========================================================================

#[no_mangle]
pub unsafe extern "C" fn js_fetch_stream_start(
    url_ptr: *const StringHeader,
    method_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    headers_json_ptr: *const StringHeader,
) -> f64 {
    let url = string_from_header(url_ptr).unwrap_or_default();
    let method = string_from_header(method_ptr).unwrap_or_else(|| "POST".to_string());
    let body = string_from_header(body_ptr);
    let headers_json = string_from_header(headers_json_ptr).unwrap_or_else(|| "{}".to_string());
    let custom_headers: HashMap<String, String> =
        serde_json::from_str(&headers_json).unwrap_or_default();
    let mut id_guard = NEXT_STREAM_ID.lock().unwrap();
    let stream_id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    STREAM_HANDLES.lock().unwrap().insert(
        stream_id,
        StreamState {
            status: 0,
            pending_lines: Vec::new(),
            partial: String::new(),
            http_status: 0,
            error: String::new(),
        },
    );
    let sid = stream_id;
    // turnloop P6's streaming sink. Until this lane the engine's
    // `Sink::on_head` / `Sink::on_chunk` hooks existed and NOTHING called them
    // — an unexercised mode, which CLAUDE.md's GC-knob kill policy calls a
    // decision nobody has made. This is the caller.
    turnloop_bridge::dispatch_stream(
        sid,
        url,
        method,
        custom_headers.into_iter().collect(),
        body.map(String::into_bytes),
    );
    stream_id as f64
}

#[no_mangle]
pub extern "C" fn js_fetch_stream_poll(handle: f64) -> *mut StringHeader {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle as usize;
    let mut g = STREAM_HANDLES.lock().unwrap();
    if let Some(s) = g.get_mut(&id) {
        if !s.pending_lines.is_empty() {
            let line = s.pending_lines.remove(0);
            return js_string_from_bytes(line.as_ptr(), line.len() as u32);
        }
    }
    js_string_from_bytes("".as_ptr(), 0)
}

#[no_mangle]
pub extern "C" fn js_fetch_stream_status(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle as usize;
    let g = STREAM_HANDLES.lock().unwrap();
    if let Some(s) = g.get(&id) {
        s.status as f64
    } else {
        3.0
    }
}

#[no_mangle]
pub extern "C" fn js_fetch_stream_close(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle as usize;
    let mut g = STREAM_HANDLES.lock().unwrap();
    if g.remove(&id).is_some() {
        1.0
    } else {
        0.0
    }
}

// ========================================================================
// Web Fetch API: Headers, Request, Response constructors and methods
// ========================================================================

pub(crate) const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

mod headers_store;
use headers_store::{HeadersRecord, HeadersStore};
#[derive(Clone)]
struct RequestRecord {
    url: String,
    method: String,
    /// Raw body bytes, stored verbatim so a binary (Buffer/Uint8Array) body
    /// survives byte-for-byte through `arrayBuffer()`/`text()` (#5483). `text()`
    /// / `json()` still decode lossily via `from_utf8_lossy`, matching Node.
    body: Option<Vec<u8>>,
    body_used: bool,
    headers: HeadersStore,
    destination: String,
    referrer: String,
    referrer_policy: String,
    mode: String,
    credentials: String,
    cache: String,
    redirect: String,
    integrity: String,
    keepalive: bool,
    duplex: String,
    signal: f64,
    /// Cached Headers handle id, allocated on first `request.headers` read so
    /// repeat reads return the same handle (preserves `req.headers ===
    /// req.headers`). Mirrors `FetchResponse::cached_headers_id` (#1649).
    cached_headers_id: Option<usize>,
}

static HEADERS_REGISTRY: std::sync::LazyLock<Mutex<HashMap<usize, HeadersRecord>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static REQUEST_REGISTRY: std::sync::LazyLock<Mutex<HashMap<usize, RequestRecord>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
pub(crate) static BLOB_REGISTRY: std::sync::LazyLock<Mutex<HashMap<usize, BlobData>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
pub(crate) struct BlobData {
    pub(crate) body: Vec<u8>,
    pub(crate) content_type: String,
    // Issue #1211: when the handle was created via `new File(parts, name, opts)`
    // these mirror the spec File interface — Blobs leave both at None.
    pub(crate) file_name: Option<String>,
    pub(crate) last_modified_ms: Option<f64>,
}

impl BlobData {
    pub(crate) fn blob(body: Vec<u8>, content_type: String) -> Self {
        BlobData {
            body,
            content_type,
            file_name: None,
            last_modified_ms: None,
        }
    }
}

pub(crate) fn alloc_blob(data: BlobData) -> usize {
    let id = alloc_fetch_handle_id();
    BLOB_REGISTRY.lock().unwrap().insert(id, data);
    id
}

fn alloc_headers(store: HeadersStore) -> usize {
    let id = alloc_fetch_handle_id();
    HEADERS_REGISTRY
        .lock()
        .unwrap()
        .insert(id, HeadersRecord::from(store));
    id
}

// ----------------- Headers FFI -----------------
// Moved to the `headers` sub-module (#1649 pushed fetch.rs past the 2,000-line
// lint gate; mirrors the earlier fetch_blob.rs extraction). Re-exported below.

// ----------------- Response FFI (constructor + extra methods) -----------------

/// response.arrayBuffer() — returns a real BufferHeader holding the body bytes,
/// NaN-boxed as POINTER_TAG so that `new Uint8Array(buf)` and `Buffer.from(buf)`
/// see the actual byte contents. `.byteLength` / `.length` access routes through
/// the BufferHeader property dispatch in `value.rs`. Resolved synchronously so
/// the LLVM backend's await loop (which doesn't pump deferred resolutions)
/// doesn't hang. See `js_fetch_response_text` for rationale.
#[no_mangle]
pub unsafe extern "C" fn js_response_array_buffer(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let promise = perry_runtime::js_promise_new_cross_thread();
    let body = match consume_response_body(handle) {
        Ok(body) => body,
        Err(err_msg) if err_msg == BODY_ALREADY_USED_MESSAGE => {
            reject_fetch_type_error(promise, BODY_ALREADY_USED_MESSAGE);
            return promise;
        }
        Err(err_msg) => {
            let err_nan = f64::from_bits(fetch_error_bits(err_msg));
            perry_runtime::js_promise_reject(promise, err_nan);
            return promise;
        }
    };
    let buf = perry_runtime::buffer::buffer_alloc(body.len() as u32);
    (*buf).length = body.len() as u32;
    if !body.is_empty() {
        std::ptr::copy_nonoverlapping(
            body.as_ptr(),
            perry_runtime::buffer::buffer_data_mut(buf),
            body.len(),
        );
    }
    let val = JSValue::object_ptr(buf as *mut u8);
    perry_runtime::js_promise_resolve(promise, f64::from_bits(val.bits()));
    promise
}

/// response.blob() — registers a real Blob in BLOB_REGISTRY (cloning body
/// bytes + content-type) and resolves with the numeric blob handle as f64.
/// Resolved synchronously; see `js_fetch_response_text`.
///
/// Closes #234 (followup of #232 / #227): pre-fix this returned a
/// metadata-only stub `{size, type}` and silently dropped `resp.body`. The
/// codegen-side dispatch arm at `crates/perry-codegen/src/lower_call.rs`
/// (module=="blob") routes `.arrayBuffer()` / `.text()` / `.bytes()` /
/// `.slice()` / `.size` / `.type` to the FFIs below.
#[no_mangle]
pub unsafe extern "C" fn js_response_blob(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let promise = perry_runtime::js_promise_new_cross_thread();
    let id = handle_id(handle);
    let content_type = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        guard
            .get(&id)
            .and_then(|resp| response_headers_snapshot(resp).get("content-type"))
            .unwrap_or_default()
    };
    let body = match consume_response_body(handle) {
        Ok(body) => body,
        Err(err_msg) if err_msg == BODY_ALREADY_USED_MESSAGE => {
            reject_fetch_type_error(promise, BODY_ALREADY_USED_MESSAGE);
            return promise;
        }
        Err(err_msg) => {
            let err_nan = f64::from_bits(fetch_error_bits(err_msg));
            perry_runtime::js_promise_reject(promise, err_nan);
            return promise;
        }
    };
    let data = BlobData::blob(body, content_type);
    let blob_id = alloc_blob(data);
    perry_runtime::js_promise_resolve(promise, handle_to_f64(blob_id));
    promise
}

// ----------------- Blob FFI -----------------
//
// Blob handles flow as NaN-boxed POINTER_TAG f64 values (registry IDs into
// BLOB_REGISTRY), matching the migrated Response/Request/Headers handle ABI
// (Phase 1 of the handle-NaN-boxing unification). Constructors wrap return
// values via `handle_to_f64`; accessors unbox via `handle_id` (tolerant of
// the legacy raw-float form during the cross-subsystem transition).
// Codegen passes them through as DOUBLE arg kinds — no `fptosi` needed.
// See `lower_call.rs::module=="blob"` arm.

/// blob.size — body byte length as f64.
#[no_mangle]
pub extern "C" fn js_blob_size(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle_id(handle);
    BLOB_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|b| b.body.len() as f64)
        .unwrap_or(0.0)
}

/// blob.type — content_type as `*mut StringHeader` (codegen NaN-boxes with STRING_TAG).
#[no_mangle]
pub unsafe extern "C" fn js_blob_type(handle: f64) -> *mut StringHeader {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle_id(handle);
    let ct = BLOB_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|b| b.content_type.clone())
        .unwrap_or_default();
    js_string_from_bytes(ct.as_ptr(), ct.len() as u32)
}

/// blob.arrayBuffer() — allocates a `BufferHeader` holding the body bytes,
/// resolves the promise with it NaN-boxed as POINTER_TAG. Mirrors
/// `js_response_array_buffer` (closes #227 path). `new Uint8Array(buf)` and
/// `Buffer.from(buf)` see the actual byte contents via the BufferHeader
/// property dispatch in `value.rs`. Resolved synchronously.
#[no_mangle]
pub unsafe extern "C" fn js_blob_array_buffer(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let result = perry_runtime::async_hooks::run_provider_completion("BLOBREADER", || {
        perry_runtime::value::js_nanbox_pointer(blob_array_buffer_impl(handle) as i64)
    });
    perry_runtime::value::js_nanbox_get_pointer(result) as *mut perry_runtime::Promise
}

unsafe fn blob_array_buffer_impl(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let id = handle_id(handle);
    let body: Vec<u8> = BLOB_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|b| b.body.clone())
        .unwrap_or_default();
    let buf = perry_runtime::buffer::buffer_alloc(body.len() as u32);
    (*buf).length = body.len() as u32;
    if !body.is_empty() {
        std::ptr::copy_nonoverlapping(
            body.as_ptr(),
            perry_runtime::buffer::buffer_data_mut(buf),
            body.len(),
        );
    }
    let val = JSValue::object_ptr(buf as *mut u8);
    perry_runtime::js_promise_resolve(promise, f64::from_bits(val.bits()));
    promise
}

/// blob.bytes() — alias for arrayBuffer() (the BufferHeader is already
/// byte-array-shaped; users wrap in Uint8Array via `new Uint8Array(buf)` which
/// hits the `is_registered_buffer` path from #227).
#[no_mangle]
pub unsafe extern "C" fn js_blob_bytes(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let result = perry_runtime::async_hooks::run_provider_completion("BLOBREADER", || {
        perry_runtime::value::js_nanbox_pointer(blob_array_buffer_impl(handle) as i64)
    });
    perry_runtime::value::js_nanbox_get_pointer(result) as *mut perry_runtime::Promise
}

/// blob.text() — UTF-8-decodes the body bytes into a `StringHeader` and
/// resolves the promise with it NaN-boxed as STRING_TAG. Lossy decode for
/// invalid sequences (matches WHATWG Blob.text() spec which uses replacement
/// characters; lossy_utf8 produces U+FFFD identically).
#[no_mangle]
pub unsafe extern "C" fn js_blob_text(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let result = perry_runtime::async_hooks::run_provider_completion("BLOBREADER", || {
        perry_runtime::value::js_nanbox_pointer(blob_text_impl(handle) as i64)
    });
    perry_runtime::value::js_nanbox_get_pointer(result) as *mut perry_runtime::Promise
}

unsafe fn blob_text_impl(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let id = handle_id(handle);
    let body: Vec<u8> = BLOB_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|b| b.body.clone())
        .unwrap_or_default();
    let s = String::from_utf8_lossy(&body).into_owned();
    let str_ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    let val = JSValue::string_ptr(str_ptr);
    perry_runtime::js_promise_resolve(promise, f64::from_bits(val.bits()));
    promise
}

/// blob.slice(start?, end?, type?) — returns a NEW blob handle covering
/// [start, end) of the body. `f64::NAN` sentinel for missing numeric args.
/// `type_ptr` may be null to inherit the original content-type. Negative
/// indices count from the end; out-of-range values clamp to [0, len] per
/// WHATWG Blob spec.
#[no_mangle]
pub unsafe extern "C" fn js_blob_slice(
    handle: f64,
    start: f64,
    end: f64,
    type_ptr: *const StringHeader,
) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle, start, end]);
    let id = handle_id(handle);
    let body: Vec<u8> = {
        let guard = BLOB_REGISTRY.lock().unwrap();
        guard.get(&id).map(|b| b.body.clone()).unwrap_or_default()
    };
    let len = body.len() as i64;
    let normalize = |v: f64, default: i64| -> i64 {
        if v.is_nan() {
            return default;
        }
        let n = v as i64;
        if n < 0 {
            (len + n).max(0)
        } else {
            n.min(len)
        }
    };
    let s = normalize(start, 0);
    let e = normalize(end, len);
    let (lo, hi) = if e < s { (s, s) } else { (s, e) };
    let slice = body[lo as usize..hi as usize].to_vec();
    // Per WHATWG Blob spec: when `contentType` is absent, the new blob's
    // type is the empty string — NOT inherited from the original. Same
    // applies when the caller's type string fails to decode.
    let new_type = if type_ptr.is_null() {
        String::new()
    } else {
        string_from_header(type_ptr).unwrap_or_default()
    };
    handle_to_f64(alloc_blob(BlobData::blob(slice, new_type)))
}

// Issue #1211: Blob / File constructors + object-URL registry now
// live in sibling `fetch_blob.rs` to keep this file under the
// 2,000-line size gate.  See that module for the FFI entry points.

// ----------------- Web Streams bridge helpers (issue #237) -----------------
//
// `streams.rs` reaches in here for the bytes backing `blob.stream()` and
// `response.body`. Going through these `pub(crate)` shims (rather than
// re-implementing the `BLOB_REGISTRY` / `FETCH_RESPONSES` lookups in
// `streams.rs`) keeps the registry types private to fetch.rs.

mod body_clone;
pub use body_clone::*;

/// Shared `request.headers` resolver used by both the typed codegen path
/// (`js_request_get_headers`) and the untyped property dispatcher. Lazily
/// allocates a Headers registry entry from the request's stored header map
/// and caches the id so `req.headers === req.headers`. Caller must have
/// already verified `req_id` is a live Request. (#1649)
fn request_headers_handle(req_id: usize) -> f64 {
    if let Some(id) = REQUEST_REGISTRY
        .lock()
        .unwrap()
        .get(&req_id)
        .and_then(|r| r.cached_headers_id)
    {
        return handle_to_f64(id);
    }
    let store = match REQUEST_REGISTRY.lock().unwrap().get(&req_id) {
        Some(r) => r.headers.clone(),
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    let new_id = alloc_headers(store);
    if let Some(req) = REQUEST_REGISTRY.lock().unwrap().get_mut(&req_id) {
        req.cached_headers_id = Some(new_id);
    }
    handle_to_f64(new_id)
}

/// `request.headers` — returns a NaN-boxed `Headers` handle (Web Fetch spec).
/// Without this the typed codegen path fell through to a raw numeric handle
/// and `req.headers.get(...)` threw "(number).get is not a function", which
/// broke every Hono adapter on the first request (#1649).
#[no_mangle]
pub extern "C" fn js_request_get_headers(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle_id(handle);
    if REQUEST_REGISTRY.lock().unwrap().get(&id).is_none() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    request_headers_handle(id)
}

#[no_mangle]
pub extern "C" fn js_request_get_url(handle: f64) -> *mut StringHeader {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle_id(handle);
    // #8163: snapshot under the guard, allocate after it is dropped. The Fetch
    // root scanner takes this same lock during a collection on this thread, so
    // a `js_string_from_bytes` inside the guard's scope self-deadlocks the
    // moment that allocation triggers one. See `fetch::gc`.
    let url = match REQUEST_REGISTRY.lock().unwrap().get(&id) {
        Some(req) => req.url.clone(),
        None => return std::ptr::null_mut(),
    };
    js_string_from_bytes(url.as_ptr(), url.len() as u32)
}

/// Coerce the first argument of `new Request(input, init)` to its URL string.
/// The spec runs `ToString(input)` unless `input` is already a `Request`, in
/// which case the constructor clones it and keeps its url. A `Request` handle
/// has no custom `toString` (`String(request)` is `"[object Request]"`), so
/// running ToString on it would store that literal as the url — this returns the
/// cloned request's real url instead. Any other value (a URL object, a string)
/// falls through to `js_jsvalue_to_string`, which invokes `toString` (URL → href)
/// or passes a string straight through.
#[no_mangle]
pub extern "C" fn js_request_input_to_url(value: f64) -> *mut StringHeader {
    let _fetch_roots = lifecycle::pin_handles(&[value]);
    let id = handle_id(value);
    // #8163: snapshot under the guard, allocate after (see `js_request_get_url`).
    let url = REQUEST_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|req| req.url.clone());
    if let Some(url) = url {
        return js_string_from_bytes(url.as_ptr(), url.len() as u32);
    }
    perry_runtime::value::js_jsvalue_to_string(value) as *mut StringHeader
}

#[no_mangle]
pub extern "C" fn js_request_get_method(handle: f64) -> *mut StringHeader {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle_id(handle);
    // #8163: snapshot under the guard, allocate after (see `js_request_get_url`).
    let method = match REQUEST_REGISTRY.lock().unwrap().get(&id) {
        Some(req) => req.method.clone(),
        None => return std::ptr::null_mut(),
    };
    js_string_from_bytes(method.as_ptr(), method.len() as u32)
}

/// req.body — returns a string body or null. NaN-boxed return.
#[no_mangle]
pub extern "C" fn js_request_get_body(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle_id(handle);
    // #8163: snapshot under the guard, allocate after (see `js_request_get_url`).
    let body = match REQUEST_REGISTRY.lock().unwrap().get(&id) {
        Some(req) => match &req.body {
            Some(b) => b.clone(),
            None => return f64::from_bits(TAG_NULL),
        },
        None => return f64::from_bits(TAG_NULL),
    };
    let s = js_string_from_bytes(body.as_ptr(), body.len() as u32);
    f64::from_bits(JSValue::string_ptr(s).bits())
}

/// request.bodyUsed -> boolean
#[no_mangle]
pub extern "C" fn js_request_body_used(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle_id(handle);
    let guard = REQUEST_REGISTRY.lock().unwrap();
    tagged_bool(guard.get(&id).map(|req| req.body_used).unwrap_or(false))
}

/// request.clone() — duplicates the request unless its body was consumed.
#[no_mangle]
pub extern "C" fn js_request_clone(handle: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let id = handle_id(handle);
    // #8163: the `unusable` throw MUST NOT happen under the guard. It allocates
    // an Error (a collection point, and the scanner takes this same lock) and
    // then unwinds through this frame without running `Drop`, so the registry
    // mutex would stay locked for the life of the process. Decide under the
    // guard, throw after it is dropped. `Err(())` is "unusable".
    #[allow(clippy::result_unit_err)]
    let cloned: Option<Result<RequestRecord, ()>> = {
        let guard = REQUEST_REGISTRY.lock().unwrap();
        guard.get(&id).map(|req| {
            if req.body.is_some() && req.body_used {
                return Err(());
            }
            Ok(RequestRecord {
                url: req.url.clone(),
                method: req.method.clone(),
                body: req.body.clone(),
                body_used: false,
                headers: req.headers.clone(),
                destination: req.destination.clone(),
                referrer: req.referrer.clone(),
                referrer_policy: req.referrer_policy.clone(),
                mode: req.mode.clone(),
                credentials: req.credentials.clone(),
                cache: req.cache.clone(),
                redirect: req.redirect.clone(),
                integrity: req.integrity.clone(),
                keepalive: req.keepalive,
                duplex: req.duplex.clone(),
                signal: req.signal,
                cached_headers_id: None,
            })
        })
    };
    match cloned {
        Some(Err(())) => unsafe { throw_fetch_type_error("unusable") },
        Some(Ok(new_req)) => {
            let new_id = alloc_fetch_handle_id();
            gc::ensure_gc_registered();
            REQUEST_REGISTRY.lock().unwrap().insert(new_id, new_req);
            handle_to_f64(new_id)
        }
        None => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Read and consume a request's stored body. Bodiless requests are reusable and
/// resolve to an empty body, matching Node's Fetch Body behavior.
fn consume_request_body(handle: f64) -> Result<Vec<u8>, &'static str> {
    let id = handle_id(handle);
    let mut guard = REQUEST_REGISTRY.lock().unwrap();
    let req = guard.get_mut(&id).ok_or("Invalid request handle")?;
    let body = match &req.body {
        Some(body) => body.clone(),
        None => return Ok(Vec::new()),
    };
    if req.body_used {
        return Err(BODY_ALREADY_USED_MESSAGE);
    }
    req.body_used = true;
    Ok(body)
}

/// request.text() -> Promise<string>. Mirrors `js_fetch_response_text`: the
/// body is in-memory, so resolve the promise synchronously (the LLVM await
/// loop doesn't drain the deferred pump). A bodiless request resolves to "".
/// (#1688)
#[no_mangle]
pub unsafe extern "C" fn js_request_text(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let promise = perry_runtime::js_promise_new_cross_thread();
    match consume_request_body(handle) {
        Ok(body) => {
            let text = String::from_utf8_lossy(&body).to_string();
            let result_str = js_string_from_bytes(text.as_ptr(), text.len() as u32);
            let result_nan = f64::from_bits(JSValue::string_ptr(result_str).bits());
            perry_runtime::js_promise_resolve(promise, result_nan);
        }
        Err(err_msg) if err_msg == BODY_ALREADY_USED_MESSAGE => {
            reject_fetch_type_error(promise, BODY_ALREADY_USED_MESSAGE);
        }
        Err(err_msg) => {
            let err_nan = f64::from_bits(fetch_error_bits(err_msg));
            perry_runtime::js_promise_reject(promise, err_nan);
        }
    }
    promise
}

/// request.json() -> Promise<object>. Parses the stored body as JSON, mirroring
/// `js_fetch_response_json`. (#1688)
#[no_mangle]
pub unsafe extern "C" fn js_request_json(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let promise = perry_runtime::js_promise_new_cross_thread();
    let body = match consume_request_body(handle) {
        Ok(b) => b,
        Err(err_msg) if err_msg == BODY_ALREADY_USED_MESSAGE => {
            reject_fetch_type_error(promise, BODY_ALREADY_USED_MESSAGE);
            return promise;
        }
        Err(err_msg) => {
            let err_nan = f64::from_bits(fetch_error_bits(err_msg));
            perry_runtime::js_promise_reject(promise, err_nan);
            return promise;
        }
    };
    match parse_json_body(&body) {
        Ok(js_value) => {
            perry_runtime::js_promise_resolve(promise, f64::from_bits(js_value.bits()));
        }
        Err(error) => {
            perry_runtime::js_promise_reject(promise, error);
        }
    }
    promise
}

/// request.arrayBuffer() -> Promise<ArrayBuffer>. Resolves with a real
/// BufferHeader over the body bytes, mirroring `js_response_array_buffer`. (#1688)
#[no_mangle]
pub unsafe extern "C" fn js_request_array_buffer(handle: f64) -> *mut perry_runtime::Promise {
    let _fetch_roots = lifecycle::pin_handles(&[handle]);
    let promise = perry_runtime::js_promise_new_cross_thread();
    let body = match consume_request_body(handle) {
        Ok(b) => b,
        Err(err_msg) if err_msg == BODY_ALREADY_USED_MESSAGE => {
            reject_fetch_type_error(promise, BODY_ALREADY_USED_MESSAGE);
            return promise;
        }
        Err(err_msg) => {
            let err_nan = f64::from_bits(fetch_error_bits(err_msg));
            perry_runtime::js_promise_reject(promise, err_nan);
            return promise;
        }
    };
    let buf = perry_runtime::buffer::buffer_alloc(body.len() as u32);
    (*buf).length = body.len() as u32;
    if !body.is_empty() {
        std::ptr::copy_nonoverlapping(
            body.as_ptr(),
            perry_runtime::buffer::buffer_data_mut(buf),
            body.len(),
        );
    }
    let val = JSValue::object_ptr(buf as *mut u8);
    perry_runtime::js_promise_resolve(promise, f64::from_bits(val.bits()));
    promise
}
