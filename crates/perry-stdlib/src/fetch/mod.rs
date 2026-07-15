//! HTTP Fetch module (node-fetch compatible)
//!
//! Native implementation of the 'node-fetch' npm package using reqwest.
//! Provides fetch() function for making HTTP requests.

use perry_runtime::{
    js_array_alloc, js_array_push, js_object_alloc, js_object_set_field, js_object_set_keys,
    js_string_from_bytes, JSValue, StringHeader,
};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::common::async_bridge::{queue_promise_resolution, spawn};

// Web Fetch `Headers` FFI — split out to keep this file under the 2,000-line
// lint gate (#1649). The child module sees mod.rs's private items via its
// `use super::*`.
mod abort_bridge;
pub use abort_bridge::*;
mod headers;
mod request_handle;
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

// Web Fetch `Request` constructors (`js_request_new` /
// `js_request_new_from_init`) — split out to keep this file under the
// 2,000-line lint gate (#5458). Same child-module/`use super::*` contract as
// `headers`.
mod request_ctor;
pub use request_ctor::*;

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
lazy_static::lazy_static! {
    static ref FETCH_RESPONSES: Mutex<HashMap<usize, FetchResponse>> = Mutex::new(HashMap::new());
    /// #1698: ONE shared id counter for the whole Web Fetch handle family —
    /// Response, Request, Headers, and Blob. Their registries stay separate
    /// HashMaps, but a unified counter guarantees an id belongs to exactly one
    /// of them (no more "Request id 1 == Response id 1"). This is what lets the
    /// runtime handle-dispatch arms (`dispatch_request_method` /
    /// `dispatch_response_method` / …) distinguish handle types by
    /// registry-membership alone for any-typed / computed-key calls, where the
    /// static type was lost. The counter starts in a high subrange to avoid
    /// colliding with perry-ffi handles exposed by `node:http`.
    static ref NEXT_FETCH_HANDLE_ID: Mutex<usize> = Mutex::new(FETCH_HANDLE_ID_START);
    static ref STREAM_HANDLES: Mutex<HashMap<usize, StreamState>> = Mutex::new(HashMap::new());
    static ref NEXT_STREAM_ID: Mutex<usize> = Mutex::new(1);

    /// Shared HTTP client — reuses connection pool, DNS cache, and TLS session cache
    /// across all fetch() calls. Without this, each fetch allocates a fresh
    /// reqwest::Client (~250KB of state per request) and the memory never gets
    /// reused, causing unbounded RSS growth in long-running services.
    ///
    /// Sets a default `User-Agent` so endpoints that reject anonymous requests
    /// (api.github.com being the canonical example — closes #236) work out of
    /// the box. Per-request `User-Agent` headers passed via `fetch(url, {
    /// headers: { "User-Agent": "..." } })` override this default; reqwest's
    /// `RequestBuilder::header` replaces the client-level value.
    static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder()
        .user_agent(concat!("perry/", env!("CARGO_PKG_VERSION")))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(16)
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
}

fn alloc_fetch_handle_id() -> usize {
    let mut id_guard = NEXT_FETCH_HANDLE_ID.lock().unwrap();
    let id = *id_guard;
    if id >= FETCH_HANDLE_ID_END {
        panic!("Web Fetch handle id range exhausted");
    }
    *id_guard += 1;
    id
}

#[cfg(test)]
mod headers_json_test;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_handle_ids_use_high_small_handle_range() {
        use perry_runtime::value::addr_class;
        assert!(FETCH_HANDLE_ID_START >= addr_class::COMMON_HANDLE_BAND_END);
        assert!(FETCH_HANDLE_ID_END <= addr_class::HANDLE_BAND_MAX);

        let native_id = crate::common::register_handle("native-request-marker".to_string());
        let id = alloc_fetch_handle_id();
        assert!((native_id as usize) < FETCH_HANDLE_ID_START);
        assert!((FETCH_HANDLE_ID_START..FETCH_HANDLE_ID_END).contains(&id));
        assert_ne!(native_id as usize, id);
        crate::common::drop_handle(native_id);
    }

    /// `string_from_header` must treat a handle-band value (a Fetch / native
    /// registry id, not a `StringHeader` pointer) as "not a string" and return
    /// `None` WITHOUT dereferencing it. Regression for the doctor / mcp-list
    /// startup SIGSEGV: `fetch()` called with a non-string first argument (a
    /// `Request`/`Headers` object) passed the bare handle id into the
    /// `url_ptr` `*StringHeader` slot, and reading `(*ptr).byte_len` at `id+4`
    /// dereferenced an unmapped low address.
    #[test]
    fn string_from_header_rejects_handle_band_ids() {
        use perry_runtime::value::addr_class;
        for &id in &[
            1usize,                                  // common native handle
            addr_class::FETCH_HANDLE_BAND_START,     // 0x40000
            addr_class::FETCH_HANDLE_BAND_START + 2, // a fetch handle id
            addr_class::HANDLE_BAND_MAX - 1,         // 0xFFFFF
        ] {
            assert!(addr_class::is_handle_band(id));
            // Must return None without dereferencing the bogus pointer.
            let r = unsafe { string_from_header(id as *const StringHeader) };
            assert!(
                r.is_none(),
                "handle-band id {id:#x} must be rejected, got {r:?}"
            );
        }
    }
}

struct StreamState {
    status: u8, // 0=connecting, 1=streaming, 2=done, 3=error
    pending_lines: Vec<String>,
    partial: String,
    #[allow(dead_code)]
    http_status: u16,
    #[allow(dead_code)]
    error: String,
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

/// Helper to extract string from StringHeader pointer
pub(crate) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    // NaN-boxed TAG_UNDEFINED (0x7FFC_0000_0000_0001) unboxes to 0x1
    // after POINTER_MASK. Treat any pointer below page size as invalid.
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    // A handle-band value (`< 0x100000`: Web Fetch Headers/Request/Response/Blob
    // ids, net/http small handles, zlib/proxy ids) is a registry id, NOT a
    // `StringHeader` pointer. It reaches here when `fetch()` is called with a
    // non-string first argument such as a `Request`/`Headers` object — the
    // codegen passes the bare handle id into the `url_ptr` `*StringHeader`
    // slot. Reading `(*ptr).byte_len` at `id + 4` then dereferences an
    // unmapped low address → SIGSEGV (the doctor / mcp-list startup crash at
    // the fetch-handle address). The `< 0x1000` floor above only catches the
    // TAG_UNDEFINED `0x1` remnant; widen it to the whole handle band so any
    // native handle is treated as "not a string" (`None`) rather than
    // dereferenced.
    if perry_runtime::value::addr_class::is_handle_band(ptr as usize) {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

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
/// `*ObjectHeader.object_type` u32 — `byte_len` of the message string is
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
    let promise = perry_runtime::js_promise_new();
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

    spawn(async move {
        match HTTP_CLIENT.get(&url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();

                let headers = headers_from_header_map(response.headers());

                let body = response.bytes().await.unwrap_or_default().to_vec();

                // Store response
                let response_id = alloc_fetch_handle_id();

                FETCH_RESPONSES.lock().unwrap().insert(
                    response_id,
                    FetchResponse {
                        status,
                        status_text,
                        headers,
                        body,
                        body_present: true,
                        body_used: false,
                        type_name: "basic".to_string(),
                        url: url.clone(),
                        redirected: false,
                        cached_headers_id: None,
                        cached_body_stream_id: None,
                    },
                );

                // Return response handle
                let result_bits = handle_to_f64(response_id).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_bits = fetch_error_bits(&err_msg);
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

    promise
}

/// Perform a GET request with Authorization header
/// Used when fetch(url, { headers: { Authorization: "Bearer ..." } }) is needed
#[no_mangle]
pub unsafe extern "C" fn js_fetch_get_with_auth(
    url_ptr: *const StringHeader,
    auth_header_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
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

    spawn(async move {
        let client = HTTP_CLIENT.clone();
        let mut request = client.get(&url);
        if !auth_header.is_empty() {
            request = request.header("Authorization", &auth_header);
        }
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();

                let headers = headers_from_header_map(response.headers());

                let body = response.bytes().await.unwrap_or_default().to_vec();

                let response_id = alloc_fetch_handle_id();

                FETCH_RESPONSES.lock().unwrap().insert(
                    response_id,
                    FetchResponse {
                        status,
                        status_text,
                        headers,
                        body,
                        body_present: true,
                        body_used: false,
                        type_name: "basic".to_string(),
                        url: url.clone(),
                        redirected: false,
                        cached_headers_id: None,
                        cached_body_stream_id: None,
                    },
                );

                let result_bits = handle_to_f64(response_id).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_bits = fetch_error_bits(&err_msg);
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

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
    let promise = perry_runtime::js_promise_new();
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

    spawn(async move {
        let client = HTTP_CLIENT.clone();
        let mut request = client.post(&url).header("Content-Type", "application/json");
        if !auth_header.is_empty() {
            request = request.header("Authorization", &auth_header);
        }
        request = request.body(body);
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();

                let headers = headers_from_header_map(response.headers());

                let body = response.bytes().await.unwrap_or_default().to_vec();

                let response_id = alloc_fetch_handle_id();

                FETCH_RESPONSES.lock().unwrap().insert(
                    response_id,
                    FetchResponse {
                        status,
                        status_text,
                        headers,
                        body,
                        body_present: true,
                        body_used: false,
                        type_name: "basic".to_string(),
                        url: url.clone(),
                        redirected: false,
                        cached_headers_id: None,
                        cached_body_stream_id: None,
                    },
                );

                let result_bits = handle_to_f64(response_id).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_bits = fetch_error_bits(&err_msg);
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

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
    let promise = perry_runtime::js_promise_new();
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
    // data offset (#5757). `reqwest::Body` accepts `Vec<u8>` directly.
    let body = fetch_request_body_bytes(body_ptr).unwrap_or_default();
    let content_type =
        string_from_header(content_type_ptr).unwrap_or_else(|| "application/json".to_string());

    spawn(async move {
        let client = HTTP_CLIENT.clone();
        match client
            .post(&url)
            .header("Content-Type", &content_type)
            .body(body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();

                let headers = headers_from_header_map(response.headers());

                let body = response.bytes().await.unwrap_or_default().to_vec();

                // Store response
                let response_id = alloc_fetch_handle_id();

                FETCH_RESPONSES.lock().unwrap().insert(
                    response_id,
                    FetchResponse {
                        status,
                        status_text,
                        headers,
                        body,
                        body_present: true,
                        body_used: false,
                        type_name: "basic".to_string(),
                        url: url.clone(),
                        redirected: false,
                        cached_headers_id: None,
                        cached_body_stream_id: None,
                    },
                );

                // Return response handle
                let result_bits = handle_to_f64(response_id).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_bits = fetch_error_bits(&err_msg);
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

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

    let promise = perry_runtime::js_promise_new();
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
    let inputs = match request_handle::resolve_fetch_inputs(
        string_from_header(url_ptr),
        string_from_header(method_ptr),
        // Read the body as raw bytes (binary bodies probe the buffer/typed-array
        // registry first) so a Buffer/Uint8Array body isn't corrupted by a lossy
        // StringHeader read (#5757).
        fetch_request_body_bytes(body_ptr),
        string_from_header(headers_json_ptr),
        url_ptr as usize,
    ) {
        Ok(inputs) => inputs,
        Err(err_bits) => {
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    // Dispatch + abort handling live in `abort_bridge::run_request` (keeps this
    // file under the line-size lint gate).
    spawn(abort_bridge::run_request(promise_ptr, abort_watch, inputs));

    promise
}

/// Get response status code
/// response.status -> number
#[no_mangle]
pub extern "C" fn js_fetch_response_status(handle: f64) -> f64 {
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
    let response_id = handle_id(handle);
    let guard = FETCH_RESPONSES.lock().unwrap();
    match guard.get(&response_id) {
        Some(resp) => {
            js_string_from_bytes(resp.status_text.as_ptr(), resp.status_text.len() as u32)
        }
        None => std::ptr::null_mut(),
    }
}

/// Check if response was successful (status 200-299)
/// response.ok -> boolean
#[no_mangle]
pub extern "C" fn js_fetch_response_ok(handle: f64) -> f64 {
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
    Ok(resp.body.clone())
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
    let promise = perry_runtime::js_promise_new();
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

/// Convert serde_json::Value to JSValue
unsafe fn json_value_to_jsvalue(value: &serde_json::Value) -> JSValue {
    match value {
        serde_json::Value::Null => JSValue::null(),
        serde_json::Value::Bool(b) => JSValue::bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                JSValue::number(f)
            } else if let Some(i) = n.as_i64() {
                JSValue::number(i as f64)
            } else {
                JSValue::number(0.0)
            }
        }
        serde_json::Value::String(s) => {
            let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            JSValue::string_ptr(ptr)
        }
        serde_json::Value::Array(arr) => {
            let js_arr = js_array_alloc(arr.len() as u32);
            for item in arr {
                js_array_push(js_arr, json_value_to_jsvalue(item));
            }
            JSValue::object_ptr(js_arr as *mut u8)
        }
        serde_json::Value::Object(obj) => {
            let js_obj = js_object_alloc(0, obj.len() as u32);
            // Create keys array for property names
            let keys_arr = js_array_alloc(obj.len() as u32);
            for (idx, (key, val)) in obj.iter().enumerate() {
                // Add key to keys array
                let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
                js_array_push(keys_arr, JSValue::string_ptr(key_ptr));
                // Set field value
                js_object_set_field(js_obj, idx as u32, json_value_to_jsvalue(val));
            }
            // Associate keys with object
            js_object_set_keys(js_obj, keys_arr);
            JSValue::object_ptr(js_obj as *mut u8)
        }
    }
}

/// Get response body as JSON (parses and returns proper JS object)
/// response.json() -> Promise<object>
#[no_mangle]
pub unsafe extern "C" fn js_fetch_response_json(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
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

    // Convert body to string and parse as JSON. Resolve the promise
    // synchronously — see comment on `js_fetch_response_text`.
    let text = String::from_utf8_lossy(&body).to_string();
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(json_value) => {
            let js_value = json_value_to_jsvalue(&json_value);
            let result_nan = f64::from_bits(js_value.bits());
            perry_runtime::js_promise_resolve(promise, result_nan);
        }
        Err(e) => {
            let err_msg = format!("JSON parse error: {}", e);
            let err_nan = f64::from_bits(fetch_error_bits(&err_msg));
            perry_runtime::js_promise_reject(promise, err_nan);
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
    let promise = perry_runtime::js_promise_new();
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

    spawn(async move {
        match HTTP_CLIENT.get(&url).send().await {
            Ok(response) => match response.text().await {
                Ok(text) => {
                    let result_str = js_string_from_bytes(text.as_ptr(), text.len() as u32);
                    let result_bits = JSValue::pointer(result_str as *const u8).bits();
                    queue_promise_resolution(promise_ptr, true, result_bits);
                }
                Err(e) => {
                    let err_msg = format!("Read error: {}", e);
                    let err_bits = fetch_error_bits(&err_msg);
                    queue_promise_resolution(promise_ptr, false, err_bits);
                }
            },
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_bits = fetch_error_bits(&err_msg);
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

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
    spawn(async move {
        let client = HTTP_CLIENT.clone();
        let mut request = match method.to_uppercase().as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "PATCH" => client.patch(&url),
            _ => client.get(&url),
        };
        for (key, value) in &custom_headers {
            request = request.header(key.as_str(), value.as_str());
        }
        if let Some(b) = body {
            request = request.body(b);
        }
        match request.send().await {
            Ok(mut response) => {
                let http_status = response.status().as_u16();
                {
                    let mut g = STREAM_HANDLES.lock().unwrap();
                    if let Some(s) = g.get_mut(&sid) {
                        s.http_status = http_status;
                        s.status = 1;
                    }
                }
                loop {
                    match response.chunk().await {
                        Ok(Some(chunk)) => {
                            let text = String::from_utf8_lossy(&chunk).to_string();
                            let mut g = STREAM_HANDLES.lock().unwrap();
                            if let Some(s) = g.get_mut(&sid) {
                                s.partial.push_str(&text);
                                loop {
                                    if let Some(pos) = s.partial.find('\n') {
                                        let line = s.partial[..pos].to_string();
                                        s.partial = s.partial[pos + 1..].to_string();
                                        if !line.is_empty() {
                                            s.pending_lines.push(line);
                                        }
                                    } else {
                                        break;
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                        Ok(None) => {
                            let mut g = STREAM_HANDLES.lock().unwrap();
                            if let Some(s) = g.get_mut(&sid) {
                                if !s.partial.is_empty() {
                                    let r = std::mem::take(&mut s.partial);
                                    s.pending_lines.push(r);
                                }
                                s.status = 2;
                            }
                            break;
                        }
                        Err(e) => {
                            let mut g = STREAM_HANDLES.lock().unwrap();
                            if let Some(s) = g.get_mut(&sid) {
                                s.error = format!("Stream error: {}", e);
                                s.status = 3;
                            }
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                let mut g = STREAM_HANDLES.lock().unwrap();
                if let Some(s) = g.get_mut(&sid) {
                    s.error = format!("Connection error: {}", e);
                    s.status = 3;
                }
            }
        }
    });
    stream_id as f64
}

#[no_mangle]
pub extern "C" fn js_fetch_stream_poll(handle: f64) -> *mut StringHeader {
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

#[derive(Clone, Default)]
struct HeadersStore {
    /// (lowercase_name, value) entries — insertion order preserved
    entries: Vec<(String, String)>,
}

impl HeadersStore {
    fn set(&mut self, key: &str, value: &str) {
        let lk = key.to_ascii_lowercase();
        self.entries.retain(|(k, _)| *k != lk);
        self.entries.push((lk, value.to_string()));
    }
    /// Web Fetch `Headers.append` — combines repeated normal headers with
    /// `", "`, but keeps `Set-Cookie` values as separate entries so
    /// `getSetCookie()` can return them individually.
    fn append(&mut self, key: &str, value: &str) {
        let lk = key.to_ascii_lowercase();
        if lk == "set-cookie" {
            self.entries.push((lk, value.to_string()));
            return;
        }
        for entry in self.entries.iter_mut() {
            if entry.0 == lk {
                entry.1.push_str(", ");
                entry.1.push_str(value);
                return;
            }
        }
        self.entries.push((lk, value.to_string()));
    }
    fn get(&self, key: &str) -> Option<String> {
        let lk = key.to_ascii_lowercase();
        if lk == "set-cookie" {
            let values: Vec<&str> = self
                .entries
                .iter()
                .filter(|(k, _)| *k == lk)
                .map(|(_, v)| v.as_str())
                .collect();
            if values.is_empty() {
                None
            } else {
                Some(values.join(", "))
            }
        } else {
            self.entries
                .iter()
                .find(|(k, _)| *k == lk)
                .map(|(_, v)| v.clone())
        }
    }
    fn has(&self, key: &str) -> bool {
        let lk = key.to_ascii_lowercase();
        self.entries.iter().any(|(k, _)| *k == lk)
    }
    fn delete(&mut self, key: &str) {
        let lk = key.to_ascii_lowercase();
        self.entries.retain(|(k, _)| *k != lk);
    }
    fn set_cookie_values(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(k, _)| k == "set-cookie")
            .map(|(_, v)| v.clone())
            .collect()
    }
}

fn headers_from_header_map(headers: &reqwest::header::HeaderMap) -> HeadersStore {
    let mut store = HeadersStore::default();
    for (key, value) in headers {
        if let Ok(v) = value.to_str() {
            store.append(key.as_str(), v);
        }
    }
    store
}

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

lazy_static::lazy_static! {
    static ref HEADERS_REGISTRY: Mutex<HashMap<usize, HeadersStore>> = Mutex::new(HashMap::new());
    static ref REQUEST_REGISTRY: Mutex<HashMap<usize, RequestRecord>> = Mutex::new(HashMap::new());
    pub(crate) static ref BLOB_REGISTRY: Mutex<HashMap<usize, BlobData>> = Mutex::new(HashMap::new());
}

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
    HEADERS_REGISTRY.lock().unwrap().insert(id, store);
    id
}

fn alloc_response(
    status: u16,
    status_text: String,
    headers: HeadersStore,
    body: Vec<u8>,
    body_present: bool,
) -> usize {
    let id = alloc_fetch_handle_id();
    FETCH_RESPONSES.lock().unwrap().insert(
        id,
        FetchResponse {
            status,
            status_text,
            headers,
            body,
            body_present,
            body_used: false,
            type_name: "default".to_string(),
            url: String::new(),
            redirected: false,
            cached_headers_id: None,
            cached_body_stream_id: None,
        },
    );
    id
}

// ----------------- Headers FFI -----------------
// Moved to the `headers` sub-module (#1649 pushed fetch.rs past the 2,000-line
// lint gate; mirrors the earlier fetch_blob.rs extraction). Re-exported below.

// ----------------- Response FFI (constructor + extra methods) -----------------

/// new Response(body, statusOpt, statusTextPtrOpt, headersHandleOpt)
/// - body_ptr: StringHeader for the body, or null for ""
/// - status: f64 (200 default)
/// - status_text_ptr: StringHeader for statusText, or null for ""
/// - headers_handle: f64 numeric handle from js_headers_new, or 0
#[no_mangle]
pub unsafe extern "C" fn js_response_new(
    body_ptr: *const StringHeader,
    status: f64,
    status_text_ptr: *const StringHeader,
    headers_handle: f64,
) -> f64 {
    // Lossless raw-byte read so binary bodies survive byte-for-byte (#5435).
    let body_opt = dispatch::body_bytes_from_header(body_ptr);
    let body_present = body_opt.is_some();
    let body = body_opt.unwrap_or_default();
    // NaN / 0.0 are the codegen "no status field" sentinels. Node defaults
    // missing status to 200; any explicit value is truncated toward zero
    // then range-checked against 200..=599 (199.9 → RangeError, 599.9 →
    // 599). Refs #2640.
    let status_u16 = if status.is_nan() || status == 0.0 {
        200
    } else {
        let truncated = status.trunc();
        if !(200.0..=599.0).contains(&truncated) {
            throw_fetch_range_error(
                "init[\"status\"] must be in the range of 200 to 599, inclusive.",
            );
        }
        truncated as u16
    };
    // Node defaults statusText to the empty string (NOT the canonical
    // reason phrase) and validates the reason-phrase token. Refs #2640.
    let status_text = match string_from_header(status_text_ptr) {
        Some(s) => {
            if !is_valid_status_text(&s) {
                throw_fetch_type_error("Invalid statusText");
            }
            s
        }
        None => String::new(),
    };
    if body_present && is_null_body_status(status_u16) {
        throw_fetch_type_error(&format!(
            "Response constructor: Invalid response status code {status_u16}"
        ));
    }
    let headers_id = handle_id(headers_handle);
    let headers = if headers_id != 0 {
        HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get(&headers_id)
            .cloned()
            .unwrap_or_default()
    } else {
        HeadersStore::default()
    };
    handle_to_f64(alloc_response(
        status_u16,
        status_text,
        headers,
        body,
        body_present,
    ))
}

/// response.headers — returns a Headers handle (f64). Lazily allocates a Headers entry
/// from the response's stored header HashMap if one doesn't exist yet.
#[no_mangle]
pub extern "C" fn js_response_get_headers(handle: f64) -> f64 {
    let id = handle_id(handle);
    let store = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        match guard.get(&id) {
            Some(resp) => resp.headers.clone(),
            None => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    handle_to_f64(alloc_headers(store))
}

/// response.clone() — duplicates the response (deep copy of body + headers)
#[no_mangle]
pub extern "C" fn js_response_clone(handle: f64) -> f64 {
    let id = handle_id(handle);
    let cloned = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        guard.get(&id).map(|resp| {
            if resp.body_present && resp.body_used {
                unsafe {
                    throw_fetch_type_error("Response.clone: Body has already been consumed.")
                };
            }
            FetchResponse {
                status: resp.status,
                status_text: resp.status_text.clone(),
                headers: resp.headers.clone(),
                body: resp.body.clone(),
                body_present: resp.body_present,
                body_used: false,
                type_name: resp.type_name.clone(),
                url: resp.url.clone(),
                redirected: resp.redirected,
                cached_headers_id: None,
                cached_body_stream_id: None,
            }
        })
    };
    if let Some(new_resp) = cloned {
        let new_id = alloc_fetch_handle_id();
        FETCH_RESPONSES.lock().unwrap().insert(new_id, new_resp);
        return handle_to_f64(new_id);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// response.arrayBuffer() — returns a real BufferHeader holding the body bytes,
/// NaN-boxed as POINTER_TAG so that `new Uint8Array(buf)` and `Buffer.from(buf)`
/// see the actual byte contents. `.byteLength` / `.length` access routes through
/// the BufferHeader property dispatch in `value.rs`. Resolved synchronously so
/// the LLVM backend's await loop (which doesn't pump deferred resolutions)
/// doesn't hang. See `js_fetch_response_text` for rationale.
#[no_mangle]
pub unsafe extern "C" fn js_response_array_buffer(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
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
    let promise = perry_runtime::js_promise_new();
    let id = handle_id(handle);
    let content_type = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        guard
            .get(&id)
            .and_then(|resp| resp.headers.get("content-type"))
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
    let promise = perry_runtime::js_promise_new();
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
    js_blob_array_buffer(handle)
}

/// blob.text() — UTF-8-decodes the body bytes into a `StringHeader` and
/// resolves the promise with it NaN-boxed as STRING_TAG. Lossy decode for
/// invalid sequences (matches WHATWG Blob.text() spec which uses replacement
/// characters; lossy_utf8 produces U+FFFD identically).
#[no_mangle]
pub unsafe extern "C" fn js_blob_text(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
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

/// Clone the bytes backing the given Blob handle. Returns `None` for an
/// unknown handle.
#[doc(hidden)]
pub fn blob_bytes_clone(blob_id: usize) -> Option<Vec<u8>> {
    BLOB_REGISTRY
        .lock()
        .unwrap()
        .get(&blob_id)
        .map(|b| b.body.clone())
}

/// Clone the body bytes of the given fetch Response handle. Returns
/// `None` for an unknown handle.
#[doc(hidden)]
pub fn response_bytes_clone(resp_id: usize) -> Option<Vec<u8>> {
    FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&resp_id)
        .map(|r| r.body.clone())
}

/// `blob.stream()` — returns a single-chunk ReadableStream handle (f64,
/// numeric registry id) over the blob's byte payload. Closes the stream
/// after the one chunk is delivered.
#[no_mangle]
pub unsafe extern "C" fn js_blob_stream(handle: f64) -> f64 {
    let id = handle_id(handle);
    let bytes = blob_bytes_clone(id).unwrap_or_default();
    // Streams are still a Phase 2 subsystem — keep their handle in the legacy
    // raw-float form (`as f64`) so streams.rs's accessors continue to round-trip.
    crate::streams::alloc_readable_from_bytes(bytes) as f64
}

/// Shared `response.body` resolver used by both the typed codegen path
/// (`js_response_body`) and the untyped property dispatcher
/// (`dispatch_response_property`). Returns a single-chunk ReadableStream
/// handle over the buffered body, or `null` when the response carries no
/// body (`Response.body: ReadableStream | null`). The handle is a raw
/// `id as f64` in the shared Web Streams id range (#1545), so the runtime
/// machinery answers `typeof` (`"object"`), `instanceof ReadableStream`,
/// and `.getReader()` / `reader.read()`. The stream id is cached on the
/// FetchResponse so `.body` is stable across reads — the spec mandates a
/// single stream, and a fresh one each call would silently unlock a held
/// reader (#1650).
fn response_body_stream(resp_id: usize) -> f64 {
    if let Some(id) = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&resp_id)
        .and_then(|r| r.cached_body_stream_id)
    {
        return id as f64;
    }
    let bytes = match FETCH_RESPONSES.lock().unwrap().get(&resp_id) {
        Some(r) if r.body_present => r.body.clone(),
        Some(_) => return f64::from_bits(TAG_NULL),
        None => return f64::from_bits(TAG_NULL),
    };
    let stream_id = crate::streams::alloc_readable_from_bytes(bytes);
    if let Some(resp) = FETCH_RESPONSES.lock().unwrap().get_mut(&resp_id) {
        resp.cached_body_stream_id = Some(stream_id);
    }
    stream_id as f64
}

/// `response.body` — `ReadableStream | null` per the Web Fetch spec. The
/// returned handle is a raw `id as f64` in the shared Web Streams id range
/// so the #1545 runtime machinery answers `typeof`/`instanceof`/method
/// dispatch; `null` when the response has no body. (#1650)
#[no_mangle]
pub unsafe extern "C" fn js_response_body(handle: f64) -> f64 {
    response_body_stream(handle_id(handle))
}

/// Response.json(value, init?) — static method. Allocates a Response with the
/// JSON-stringified body and `Content-Type: application/json`, honoring the
/// optional `init` (#2638): `init.status` (default 200), `init.statusText`
/// (default "" — Node does NOT derive it from the status code for this
/// factory) and `init.headers` (a Headers handle; user headers are applied
/// first, then `content-type` defaults to `application/json` only if the init
/// didn't already set it). The value is passed as NaN-boxed JSValue bits (f64).
#[no_mangle]
pub unsafe extern "C" fn js_response_static_json(
    value: f64,
    init_status: f64,
    init_status_text_ptr: *const StringHeader,
    headers_handle: f64,
) -> f64 {
    // Stringify via runtime (type_hint 1 = object)
    extern "C" {
        fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
    }
    let str_ptr = js_json_stringify(value, 1);
    let body_str = if str_ptr.is_null() {
        "null".to_string()
    } else {
        string_from_header(str_ptr).unwrap_or_else(|| "null".to_string())
    };
    let status_u16 = if init_status.is_nan() || init_status == 0.0 {
        200
    } else {
        init_status as u16
    };
    // Node's `Response.json` leaves statusText "" when not provided — it does
    // not fall back to the status reason phrase.
    let status_text = string_from_header(init_status_text_ptr).unwrap_or_default();
    // Start from any user-provided headers, then add the default content-type
    // only if the init headers didn't already set one.
    let headers_id = handle_id(headers_handle);
    let mut headers = if headers_id != 0 {
        HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get(&headers_id)
            .cloned()
            .unwrap_or_default()
    } else {
        HeadersStore::default()
    };
    if !headers.has("content-type") {
        headers.set("content-type", "application/json");
    }
    handle_to_f64(alloc_response(
        status_u16,
        status_text,
        headers,
        body_str.into_bytes(),
        true,
    ))
}

/// Response.redirect(url, status) — static method. Allocates a redirect response.
#[no_mangle]
pub unsafe extern "C" fn js_response_static_redirect(
    url_ptr: *const StringHeader,
    status: f64,
) -> f64 {
    let url = string_from_header(url_ptr).unwrap_or_default();
    let status_u16 = redirect_status_from_value(status);
    if !is_redirect_status(status_u16) {
        throw_fetch_range_error(&format!("Invalid status code {status_u16}"));
    }
    let location = match parse_redirect_location(&url) {
        Ok(location) => location,
        Err(_) => throw_fetch_type_error(&format!("Failed to parse URL from {url}")),
    };
    let mut headers = HeadersStore::default();
    headers.set("location", &location);
    handle_to_f64(alloc_response(
        status_u16 as u16,
        String::new(),
        headers,
        Vec::new(),
        false,
    ))
}

// ----------------- Request FFI -----------------
// The `Request` constructors (`js_request_new` / `js_request_new_from_init`)
// live in the `request_ctor` sibling module (re-exported below) to keep this
// file under the 2,000-line lint gate (#5458).

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
    let id = handle_id(handle);
    if REQUEST_REGISTRY.lock().unwrap().get(&id).is_none() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    request_headers_handle(id)
}

#[no_mangle]
pub extern "C" fn js_request_get_url(handle: f64) -> *mut StringHeader {
    let id = handle_id(handle);
    let guard = REQUEST_REGISTRY.lock().unwrap();
    match guard.get(&id) {
        Some(req) => js_string_from_bytes(req.url.as_ptr(), req.url.len() as u32),
        None => std::ptr::null_mut(),
    }
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
    let id = handle_id(value);
    if let Some(req) = REQUEST_REGISTRY.lock().unwrap().get(&id) {
        return js_string_from_bytes(req.url.as_ptr(), req.url.len() as u32);
    }
    perry_runtime::value::js_jsvalue_to_string(value) as *mut StringHeader
}

#[no_mangle]
pub extern "C" fn js_request_get_method(handle: f64) -> *mut StringHeader {
    let id = handle_id(handle);
    let guard = REQUEST_REGISTRY.lock().unwrap();
    match guard.get(&id) {
        Some(req) => js_string_from_bytes(req.method.as_ptr(), req.method.len() as u32),
        None => std::ptr::null_mut(),
    }
}

/// req.body — returns a string body or null. NaN-boxed return.
#[no_mangle]
pub extern "C" fn js_request_get_body(handle: f64) -> f64 {
    let id = handle_id(handle);
    let guard = REQUEST_REGISTRY.lock().unwrap();
    match guard.get(&id) {
        Some(req) => match &req.body {
            Some(b) => {
                let s = js_string_from_bytes(b.as_ptr(), b.len() as u32);
                f64::from_bits(JSValue::string_ptr(s).bits())
            }
            None => f64::from_bits(TAG_NULL),
        },
        None => f64::from_bits(TAG_NULL),
    }
}

/// request.bodyUsed -> boolean
#[no_mangle]
pub extern "C" fn js_request_body_used(handle: f64) -> f64 {
    let id = handle_id(handle);
    let guard = REQUEST_REGISTRY.lock().unwrap();
    tagged_bool(guard.get(&id).map(|req| req.body_used).unwrap_or(false))
}

/// request.clone() — duplicates the request unless its body was consumed.
#[no_mangle]
pub extern "C" fn js_request_clone(handle: f64) -> f64 {
    let id = handle_id(handle);
    let cloned = {
        let guard = REQUEST_REGISTRY.lock().unwrap();
        guard.get(&id).map(|req| {
            if req.body.is_some() && req.body_used {
                unsafe { throw_fetch_type_error("unusable") };
            }
            RequestRecord {
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
            }
        })
    };
    if let Some(new_req) = cloned {
        let new_id = alloc_fetch_handle_id();
        REQUEST_REGISTRY.lock().unwrap().insert(new_id, new_req);
        return handle_to_f64(new_id);
    }
    f64::from_bits(TAG_UNDEFINED)
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
    let promise = perry_runtime::js_promise_new();
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
    let promise = perry_runtime::js_promise_new();
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
    let text = String::from_utf8_lossy(&body).to_string();
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(json_value) => {
            let js_value = json_value_to_jsvalue(&json_value);
            perry_runtime::js_promise_resolve(promise, f64::from_bits(js_value.bits()));
        }
        Err(e) => {
            let err_nan = f64::from_bits(fetch_error_bits(&format!("JSON parse error: {}", e)));
            perry_runtime::js_promise_reject(promise, err_nan);
        }
    }
    promise
}

/// request.arrayBuffer() -> Promise<ArrayBuffer>. Resolves with a real
/// BufferHeader over the body bytes, mirroring `js_response_array_buffer`. (#1688)
#[no_mangle]
pub unsafe extern "C" fn js_request_array_buffer(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
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
