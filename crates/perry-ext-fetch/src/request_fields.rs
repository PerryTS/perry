//! Request field accessors, split from `lib.rs` for the 2000-line file cap
//! (#9682 added the shared TLS environment plumbing).

use super::*;

pub(crate) fn request_string_field(
    handle: f64,
    f: impl FnOnce(&RequestData) -> &str,
) -> *mut StringHeader {
    let id = handle_id(handle);
    let value = {
        let g = REQUEST_HANDLES.lock().unwrap();
        g.get(&id).map(|r| f(r).to_string())
    };
    match value {
        Some(value) => alloc_string(&value).as_raw(),
        None => alloc_string("").as_raw(),
    }
}

#[no_mangle]
pub extern "C" fn js_request_get_destination(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.destination)
}

#[no_mangle]
pub extern "C" fn js_request_get_referrer(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.referrer)
}

#[no_mangle]
pub extern "C" fn js_request_get_referrer_policy(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.referrer_policy)
}

#[no_mangle]
pub extern "C" fn js_request_get_mode(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.mode)
}

#[no_mangle]
pub extern "C" fn js_request_get_credentials(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.credentials)
}

#[no_mangle]
pub extern "C" fn js_request_get_cache(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.cache)
}

#[no_mangle]
pub extern "C" fn js_request_get_redirect(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.redirect)
}

#[no_mangle]
pub extern "C" fn js_request_get_integrity(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.integrity)
}

#[no_mangle]
pub extern "C" fn js_request_get_duplex(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |r| &r.duplex)
}

#[no_mangle]
pub extern "C" fn js_request_get_keepalive(handle: f64) -> f64 {
    let id = handle_id(handle);
    let g = REQUEST_HANDLES.lock().unwrap();
    tagged_bool(g.get(&id).map(|r| r.keepalive).unwrap_or(false))
}

#[no_mangle]
pub extern "C" fn js_request_get_signal(handle: f64) -> f64 {
    let id = handle_id(handle);
    let g = REQUEST_HANDLES.lock().unwrap();
    g.get(&id)
        .map(|r| r.signal)
        .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_request_get_headers(handle: f64) -> f64 {
    let id = handle_id(handle);
    let headers = REQUEST_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| r.headers.clone())
        .unwrap_or_default();
    store_headers(headers) as f64
}

#[no_mangle]
pub extern "C" fn js_request_get_body(handle: f64) -> f64 {
    let id = handle_id(handle);
    let body = {
        let g = REQUEST_HANDLES.lock().unwrap();
        g.get(&id).and_then(|r| r.body.clone())
    };
    match body {
        Some(b) => {
            let ptr = alloc_string(&String::from_utf8_lossy(&b)).as_raw();
            f64::from_bits(STRING_TAG | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        None => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Read a request's stored body as text (empty string for a bodiless
/// request), or `None` for an invalid handle. For the text-oriented
/// accessors (`text`/`json`/`formData`); the binary accessors
/// (`arrayBuffer`/`bytes`) read the raw bytes via `request_body_bytes`. (#1688)
pub(crate) fn request_body_string(handle: f64) -> Option<String> {
    request_body_bytes(handle).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Read a request's stored body as raw bytes (empty for a bodiless request),
/// or `None` for an invalid handle. Byte-exact: never routed through a
/// `String`, so a binary body survives `arrayBuffer()`/`bytes()` intact.
pub(crate) fn request_body_bytes(handle: f64) -> Option<Vec<u8>> {
    let id = handle_id(handle);
    REQUEST_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| r.body.clone().unwrap_or_default())
}

/// request.text() -> Promise<string>. Mirrors `js_fetch_response_text`. (#1688)
///
/// # Safety
/// `handle` must come from a previous `js_request_new`.
#[no_mangle]
pub unsafe extern "C" fn js_request_text(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    match request_body_string(handle) {
        Some(s) => promise.resolve(JsValue::from_string_ptr(alloc_string(&s).as_raw())),
        None => promise.reject_string("Invalid request handle"),
    }
    raw
}

/// request.json() -> Promise<string>. Returns the body as a JSON string —
/// callers JSON.parse on the JS side, matching `js_fetch_response_json`. (#1688)
///
/// # Safety
/// `handle` must come from a previous `js_request_new`.
#[no_mangle]
pub unsafe extern "C" fn js_request_json(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    match request_body_string(handle) {
        Some(s) => promise.resolve(JsValue::from_string_ptr(alloc_string(&s).as_raw())),
        None => promise.reject_string("Invalid request handle"),
    }
    raw
}

/// request.arrayBuffer() -> Promise<ArrayBuffer>. Resolves with a real Buffer
/// over the raw body bytes (caller wraps in Uint8Array), matching
/// `js_response_array_buffer`. Byte-exact for binary bodies. (#1688)
///
/// # Safety
/// `handle` must come from a previous `js_request_new`.
#[no_mangle]
pub unsafe extern "C" fn js_request_array_buffer(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    match request_body_bytes(handle) {
        Some(b) => promise.resolve(body_to_buffer_value(&b)),
        None => promise.reject_string("Invalid request handle"),
    }
    raw
}

/// # Safety
/// `handle` must come from a previous `js_request_new`.
#[no_mangle]
pub unsafe extern "C" fn js_request_blob(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let id = handle_id(handle);
    let data = REQUEST_HANDLES.lock().unwrap().get(&id).cloned();
    match data {
        Some(r) => {
            let content_type = r.headers.get("content-type").unwrap_or_default();
            let blob_id = store_blob(BlobData {
                bytes: r.body.unwrap_or_default(),
                content_type,
            });
            promise.resolve(JsValue::from_number(blob_id as f64));
        }
        None => promise.reject_string("Invalid request handle"),
    }
    raw
}

/// # Safety
/// `handle` must come from a previous `js_request_new`.
#[no_mangle]
pub unsafe extern "C" fn js_request_bytes(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    match request_body_bytes(handle) {
        Some(b) => promise.resolve(body_to_buffer_value(&b)),
        None => promise.reject_string("Invalid request handle"),
    }
    raw
}

/// # Safety
/// `handle` must come from a previous `js_request_new`.
#[no_mangle]
pub unsafe extern "C" fn js_request_form_data(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    match request_body_string(handle) {
        Some(s) => {
            let form_id = store_form_data(form_data_from_urlencoded(s.as_bytes()));
            promise.resolve(JsValue::from_number(form_id as f64));
        }
        None => promise.reject_string("Invalid request handle"),
    }
    raw
}

/// # Safety
/// `name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_form_data_get(handle: f64, name_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let Some(name) = read_str(name_ptr) else {
        return f64::from_bits(TAG_NULL);
    };
    let g = FORM_DATA_HANDLES.lock().unwrap();
    match g.get(&id).and_then(|f| f.get(&name)) {
        Some(v) => f64::from_bits(JsValue::from_string_ptr(alloc_string(&v).as_raw()).bits()),
        None => f64::from_bits(TAG_NULL),
    }
}
