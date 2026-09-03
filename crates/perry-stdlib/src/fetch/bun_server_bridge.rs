//! Registry bridge for the `Bun.serve` adapter in `perry-ext-http`.
//!
//! Fetch `Request` and `Response` values are numeric handles owned by this
//! module, so the external HTTP provider cannot safely inspect their private
//! registries. These two FFI helpers exchange copied request/response snapshots
//! as JSON while keeping registry access here.

use super::*;

#[derive(serde::Deserialize)]
struct BunHttpRequestSnapshot {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

#[derive(serde::Serialize)]
struct BunHttpResponseSnapshot {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Construct a Fetch `Request` from a server-side HTTP request snapshot.
///
/// Returns `undefined` when `snapshot_ptr` is null or malformed.
///
/// # Safety
/// `snapshot_ptr` must be null or a live Perry `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_bun_http_request_from_json(snapshot_ptr: *const StringHeader) -> f64 {
    let Some(snapshot_json) = string_from_header(snapshot_ptr) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let Ok(snapshot) = serde_json::from_str::<BunHttpRequestSnapshot>(&snapshot_json) else {
        return f64::from_bits(TAG_UNDEFINED);
    };

    let mut headers = HeadersStore::default();
    for (name, value) in snapshot.headers {
        headers.append(&name, &value);
    }

    // Allocating the default AbortSignal can collect, so do it before taking
    // the request-registry lock (see fetch::gc's locking contract).
    let signal = body_metadata::signal_or_default(f64::from_bits(TAG_UNDEFINED));
    let id = alloc_fetch_handle_id();
    let record = RequestRecord {
        url: snapshot.url,
        method: snapshot.method,
        body: snapshot.body,
        body_used: false,
        headers,
        destination: String::new(),
        referrer: "about:client".to_string(),
        referrer_policy: String::new(),
        mode: "cors".to_string(),
        credentials: "same-origin".to_string(),
        cache: "default".to_string(),
        redirect: "follow".to_string(),
        integrity: String::new(),
        keepalive: false,
        duplex: "half".to_string(),
        signal,
        cached_headers_id: None,
    };
    gc::ensure_gc_registered();
    REQUEST_REGISTRY.lock().unwrap().insert(id, record);
    handle_to_f64(id)
}

/// Consume a Fetch `Response` and return its observable wire snapshot as JSON.
///
/// Returns null for a non-Response handle or an already-consumed body. Header
/// mutations made through `response.headers` are included.
#[no_mangle]
pub extern "C" fn js_bun_http_response_snapshot_json(response_handle: f64) -> *mut StringHeader {
    let response_id = handle_id(response_handle);
    if !FETCH_RESPONSES.lock().unwrap().contains_key(&response_id) {
        return std::ptr::null_mut();
    }
    let Ok(body) = consume_response_body(response_handle) else {
        return std::ptr::null_mut();
    };
    let snapshot = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        let Some(response) = guard.get(&response_id) else {
            return std::ptr::null_mut();
        };
        let headers = response_headers_snapshot(response);
        BunHttpResponseSnapshot {
            status: response.status,
            status_text: response.status_text.clone(),
            headers: headers.entries,
            body,
        }
    };
    let Ok(json) = serde_json::to_string(&snapshot) else {
        return std::ptr::null_mut();
    };
    js_string_from_bytes(json.as_ptr(), json.len() as u32)
}
