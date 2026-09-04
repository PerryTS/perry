//! Body/blob clone + response stream helpers, split from `fetch/mod.rs`
//! for the 2000-line file cap (#9682 added the shared TLS plumbing).

use super::*;

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
pub(crate) fn response_body_stream(resp_id: usize) -> f64 {
    if let Some(id) = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&resp_id)
        .and_then(|r| r.body_stream_id)
    {
        return id as f64;
    }
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
