//! Native bindings for the npm `node-fetch` package — Web Fetch API
//! surface via `reqwest`. Uses only perry-ffi.
//!
//! Async via `spawn_blocking + JsPromise + tokio::Handle::current().block_on`.
//! Mirrors perry-stdlib's existing surface byte-for-byte: lazy
//! per-process `reqwest::Client` (connection pool + DNS cache + TLS
//! session cache reused across calls), default `User-Agent` header
//! (closes #236), per-handle Response / Headers / Blob / Request /
//! Stream pools.
//!
//! 41 `js_*` exports across the Fetch API:
//!   - fetch core: get / get_with_auth / post / post_with_auth /
//!     with_options / text + response_count debug counter
//!   - response: status / statusText / ok / text / json /
//!     array_buffer / blob / get_headers / clone / body /
//!     static_json / static_redirect
//!   - headers: new / set / get / has / delete / for_each
//!   - stream: start / poll / status / close
//!   - blob: size / type / text / array_buffer / bytes / slice / stream
//!   - request: new / get_url / get_method / get_body

use lazy_static::lazy_static;
use perry_ffi::{
    alloc_string, get_handle, register_handle, spawn_blocking, JsClosure, JsPromise, JsString,
    JsValue, Promise, RawClosureHeader, StringHeader,
};
use std::collections::HashMap;
use std::sync::Mutex;

const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let h = JsString::from_raw(ptr as *mut StringHeader);
    perry_ffi::read_string(h).map(String::from)
}

/// Decode a Web Fetch handle (Response / Headers / Request / Blob) from
/// the f64 the codegen passes across the FFI. Mirrors perry-stdlib's
/// `handle_id` (refs #421 Phase 1 + #589 follow-up): accepts both the
/// NaN-boxed POINTER_TAG form (top-16 ≥ 0x7FF8) and the legacy raw-float
/// form (`1.0` = id 1) so callers see the same id regardless of which
/// staticlib produced the handle.
#[inline]
fn handle_id(value: f64) -> usize {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    if top16 >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && bits != 0 {
        bits as usize
    } else {
        value as usize
    }
}

// ── Response storage ──────────────────────────────────────────────

#[derive(Clone)]
struct FetchResponse {
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

lazy_static! {
    static ref FETCH_RESPONSES: Mutex<HashMap<usize, FetchResponse>> =
        Mutex::new(HashMap::new());
    static ref NEXT_RESPONSE_ID: Mutex<usize> = Mutex::new(1);

    static ref HEADERS_HANDLES: Mutex<HashMap<usize, HashMap<String, String>>> =
        Mutex::new(HashMap::new());
    static ref NEXT_HEADERS_ID: Mutex<usize> = Mutex::new(1);

    static ref BLOB_HANDLES: Mutex<HashMap<usize, BlobData>> = Mutex::new(HashMap::new());
    static ref NEXT_BLOB_ID: Mutex<usize> = Mutex::new(1);

    static ref REQUEST_HANDLES: Mutex<HashMap<usize, RequestData>> = Mutex::new(HashMap::new());
    static ref NEXT_REQUEST_ID: Mutex<usize> = Mutex::new(1);

    static ref STREAM_HANDLES: Mutex<HashMap<usize, StreamState>> = Mutex::new(HashMap::new());
    static ref NEXT_STREAM_ID: Mutex<usize> = Mutex::new(1);

    /// Shared HTTP client — reuses connection pool, DNS cache, and TLS
    /// session cache. Without this, each fetch allocs a fresh
    /// reqwest::Client (~250 KB) and the memory never gets reused.
    /// Sets a default User-Agent so endpoints that reject anonymous
    /// requests (api.github.com etc.) work out of the box.
    static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder()
        .user_agent(concat!("perry/", env!("CARGO_PKG_VERSION")))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(16)
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
}

#[derive(Clone)]
struct BlobData {
    bytes: Vec<u8>,
    content_type: String,
}

#[derive(Clone, Default)]
struct RequestData {
    url: String,
    method: String,
    body: Option<String>,
}

struct StreamState {
    rx: tokio::sync::mpsc::UnboundedReceiver<StreamMsg>,
    status: i32, // 0 = active, 1 = done, 2 = error
}

enum StreamMsg {
    Chunk(String),
    Done,
    Error(String),
}

fn store_response(resp: FetchResponse) -> usize {
    let mut id_guard = NEXT_RESPONSE_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    FETCH_RESPONSES.lock().unwrap().insert(id, resp);
    id
}

fn store_headers(headers: HashMap<String, String>) -> usize {
    let mut id_guard = NEXT_HEADERS_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    HEADERS_HANDLES.lock().unwrap().insert(id, headers);
    id
}

fn store_blob(data: BlobData) -> usize {
    let mut id_guard = NEXT_BLOB_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    BLOB_HANDLES.lock().unwrap().insert(id, data);
    id
}

fn store_request(data: RequestData) -> usize {
    let mut id_guard = NEXT_REQUEST_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    REQUEST_HANDLES.lock().unwrap().insert(id, data);
    id
}

#[no_mangle]
pub extern "C" fn js_fetch_response_count() -> i64 {
    FETCH_RESPONSES.lock().unwrap().len() as i64
}

// ── do_fetch helper — every variant funnels through here ──────────

fn do_fetch(
    method: String,
    url: String,
    custom_headers: HashMap<String, String>,
    body: Option<String>,
    promise: JsPromise,
) {
    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            let mut req = match method.to_uppercase().as_str() {
                "POST" => HTTP_CLIENT.post(&url),
                "PUT" => HTTP_CLIENT.put(&url),
                "DELETE" => HTTP_CLIENT.delete(&url),
                "PATCH" => HTTP_CLIENT.patch(&url),
                "HEAD" => HTTP_CLIENT.head(&url),
                _ => HTTP_CLIENT.get(&url),
            };
            for (k, v) in &custom_headers {
                req = req.header(k.as_str(), v.as_str());
            }
            if let Some(b) = body {
                req = req.body(b);
            }
            match req.send().await {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let status_text = response
                        .status()
                        .canonical_reason()
                        .unwrap_or("")
                        .to_string();
                    let mut headers = HashMap::new();
                    for (key, value) in response.headers() {
                        if let Ok(v) = value.to_str() {
                            headers.insert(key.to_string(), v.to_string());
                        }
                    }
                    let body = response.bytes().await.unwrap_or_default().to_vec();
                    Ok(FetchResponse {
                        status,
                        status_text,
                        headers,
                        body,
                    })
                }
                Err(e) => Err(format!("Fetch error: {}", e)),
            }
        });
        match outcome {
            Ok(resp) => {
                let id = store_response(resp);
                promise.resolve(JsValue::from_number(id as f64));
            }
            Err(e) => promise.reject_string(&e),
        }
    });
}

// ── fetch core ────────────────────────────────────────────────────

/// # Safety
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_get(url_ptr: *const StringHeader) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(url) = read_str(url_ptr) else {
        promise.reject_string("Invalid URL");
        return raw;
    };
    do_fetch("GET".to_string(), url, HashMap::new(), None, promise);
    raw
}

/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_get_with_auth(
    url_ptr: *const StringHeader,
    auth_header_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(url) = read_str(url_ptr) else {
        eprintln!(
            "[ext-fetch GET-auth] url_ptr null/invalid (ptr={:?})",
            url_ptr
        );
        promise.reject_string("Invalid URL");
        return raw;
    };
    let mut headers = HashMap::new();
    if let Some(auth) = read_str(auth_header_ptr) {
        if !auth.is_empty() {
            headers.insert("Authorization".to_string(), auth);
        }
    }
    eprintln!(
        "[ext-fetch GET-auth] url='{}' headers.len={}",
        &url[..url.len().min(80)],
        headers.len()
    );
    do_fetch("GET".to_string(), url, headers, None, promise);
    raw
}

/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_post(
    url_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(url) = read_str(url_ptr) else {
        promise.reject_string("Invalid URL");
        return raw;
    };
    let body = read_str(body_ptr);
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    do_fetch("POST".to_string(), url, headers, body, promise);
    raw
}

/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_post_with_auth(
    url_ptr: *const StringHeader,
    auth_header_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(url) = read_str(url_ptr) else {
        promise.reject_string("Invalid URL");
        return raw;
    };
    let body = read_str(body_ptr);
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    if let Some(auth) = read_str(auth_header_ptr) {
        if !auth.is_empty() {
            headers.insert("Authorization".to_string(), auth);
        }
    }
    do_fetch("POST".to_string(), url, headers, body, promise);
    raw
}

/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_with_options(
    url_ptr: *const StringHeader,
    method_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    headers_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(url) = read_str(url_ptr) else {
        promise.reject_string("Invalid URL");
        return raw;
    };
    let method = read_str(method_ptr).unwrap_or_else(|| "GET".to_string());
    let body = read_str(body_ptr);
    let headers_json = read_str(headers_json_ptr).unwrap_or_else(|| "{}".to_string());
    let custom_headers: HashMap<String, String> =
        serde_json::from_str(&headers_json).unwrap_or_default();
    do_fetch(method, url, custom_headers, body, promise);
    raw
}

// ── Response handle accessors ─────────────────────────────────────
//
// All response/headers/request accessors take `handle: f64` to match
// perry-stdlib's signature. The codegen-side dispatch (declared in
// `crates/perry-codegen/src/runtime_decls.rs:1072-1082`) passes DOUBLE
// for these calls; an `i64` Rust signature would put the bits in a
// general register (x0 on aarch64) while the call site put them in a
// floating-point register (d0), and the function would read garbage —
// the `Invalid response handle` symptom of #589's runtime path.

#[no_mangle]
pub extern "C" fn js_fetch_response_status(handle: f64) -> f64 {
    let id = handle_id(handle);
    let map = FETCH_RESPONSES.lock().unwrap();
    let result = map.get(&id).map(|r| r.status as f64).unwrap_or(0.0);
    eprintln!(
        "[ext-fetch resp.status] handle={} bits=0x{:016x} id={} keys={:?} -> {}",
        handle,
        handle.to_bits(),
        id,
        map.keys().collect::<Vec<_>>(),
        result
    );
    result
}

#[no_mangle]
pub extern "C" fn js_fetch_response_status_text(handle: f64) -> *mut StringHeader {
    let id = handle_id(handle);
    let g = FETCH_RESPONSES.lock().unwrap();
    match g.get(&id) {
        Some(r) => alloc_string(&r.status_text).as_raw(),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn js_fetch_response_ok(handle: f64) -> f64 {
    let id = handle_id(handle);
    let g = FETCH_RESPONSES.lock().unwrap();
    match g.get(&id) {
        Some(r) if (200..300).contains(&r.status) => 1.0,
        _ => 0.0,
    }
}

/// # Safety
/// `handle` must come from a previous `js_fetch_*` resolution.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_response_text(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let id = handle_id(handle);
    let body = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| r.body.clone());
    match body {
        Some(b) => {
            let s = String::from_utf8_lossy(&b).to_string();
            promise.resolve(JsValue::from_string_ptr(alloc_string(&s).as_raw()));
        }
        None => promise.reject_string("Invalid response handle"),
    }
    raw
}

/// # Safety
/// `handle` must come from a previous `js_fetch_*` resolution.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_response_json(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let id = handle_id(handle);
    let body = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| r.body.clone());
    match body {
        Some(b) => {
            // Return the body as a JSON string — user code does
            // JSON.parse(text) on the JS side. Same shape as
            // perry-stdlib's existing copy.
            let s = String::from_utf8_lossy(&b).to_string();
            promise.resolve(JsValue::from_string_ptr(alloc_string(&s).as_raw()));
        }
        None => promise.reject_string("Invalid response handle"),
    }
    raw
}

/// `fetch.text(url)` — convenience that fetches + reads body in one call.
///
/// # Safety
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_text(url_ptr: *const StringHeader) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let Some(url) = read_str(url_ptr) else {
        promise.reject_string("Invalid URL");
        return raw;
    };
    spawn_blocking(move || {
        let result = tokio::runtime::Handle::current().block_on(async move {
            HTTP_CLIENT
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("fetch.text: {}", e))?
                .text()
                .await
                .map_err(|e| format!("fetch.text body: {}", e))
        });
        match result {
            Ok(body) => promise.resolve(JsValue::from_string_ptr(alloc_string(&body).as_raw())),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

// ── Streaming ─────────────────────────────────────────────────────

/// `fetch.streamStart(url) -> handle` — start a streaming fetch.
///
/// # Safety
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_fetch_stream_start(
    url_ptr: *const StringHeader,
    method_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    headers_json_ptr: *const StringHeader,
) -> f64 {
    let Some(url) = read_str(url_ptr) else {
        return 0.0;
    };
    let method = read_str(method_ptr).unwrap_or_else(|| "GET".to_string());
    let body = read_str(body_ptr);
    let headers_json = read_str(headers_json_ptr).unwrap_or_else(|| "{}".to_string());
    let custom_headers: HashMap<String, String> =
        serde_json::from_str(&headers_json).unwrap_or_default();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<StreamMsg>();

    let mut id_guard = NEXT_STREAM_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    STREAM_HANDLES
        .lock()
        .unwrap()
        .insert(id, StreamState { rx, status: 0 });

    spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async move {
            let mut req = match method.to_uppercase().as_str() {
                "POST" => HTTP_CLIENT.post(&url),
                "PUT" => HTTP_CLIENT.put(&url),
                "DELETE" => HTTP_CLIENT.delete(&url),
                "PATCH" => HTTP_CLIENT.patch(&url),
                _ => HTTP_CLIENT.get(&url),
            };
            for (k, v) in &custom_headers {
                req = req.header(k.as_str(), v.as_str());
            }
            if let Some(b) = body {
                req = req.body(b);
            }
            match req.send().await {
                Ok(mut response) => {
                    while let Ok(Some(chunk)) = response.chunk().await {
                        let s = String::from_utf8_lossy(&chunk).to_string();
                        if tx.send(StreamMsg::Chunk(s)).is_err() {
                            return;
                        }
                    }
                    let _ = tx.send(StreamMsg::Done);
                }
                Err(e) => {
                    let _ = tx.send(StreamMsg::Error(format!("Stream error: {}", e)));
                }
            }
        });
    });

    id as f64
}

#[no_mangle]
pub extern "C" fn js_fetch_stream_poll(handle: f64) -> *mut StringHeader {
    let id = handle as usize;
    let mut g = STREAM_HANDLES.lock().unwrap();
    let Some(state) = g.get_mut(&id) else {
        return std::ptr::null_mut();
    };
    match state.rx.try_recv() {
        Ok(StreamMsg::Chunk(s)) => alloc_string(&s).as_raw(),
        Ok(StreamMsg::Done) => {
            state.status = 1;
            std::ptr::null_mut()
        }
        Ok(StreamMsg::Error(e)) => {
            state.status = 2;
            alloc_string(&format!("[error]{}", e)).as_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn js_fetch_stream_status(handle: f64) -> f64 {
    let id = handle as usize;
    STREAM_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| s.status as f64)
        .unwrap_or(2.0)
}

#[no_mangle]
pub extern "C" fn js_fetch_stream_close(handle: f64) -> f64 {
    let id = handle as usize;
    let removed = STREAM_HANDLES.lock().unwrap().remove(&id).is_some();
    if removed {
        1.0
    } else {
        0.0
    }
}

// ── Headers ───────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn js_headers_new() -> f64 {
    store_headers(HashMap::new()) as f64
}

/// # Safety
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_headers_set(
    handle: f64,
    key_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> f64 {
    let id = handle_id(handle);
    let Some(key) = read_str(key_ptr) else {
        return 0.0;
    };
    let value = read_str(value_ptr).unwrap_or_default();
    let mut g = HEADERS_HANDLES.lock().unwrap();
    if let Some(h) = g.get_mut(&id) {
        h.insert(key.to_lowercase(), value);
        1.0
    } else {
        0.0
    }
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_headers_get(
    handle: f64,
    key_ptr: *const StringHeader,
) -> *mut StringHeader {
    let id = handle_id(handle);
    let Some(key) = read_str(key_ptr) else {
        return std::ptr::null_mut();
    };
    let g = HEADERS_HANDLES.lock().unwrap();
    match g.get(&id).and_then(|h| h.get(&key.to_lowercase())) {
        Some(v) => alloc_string(v).as_raw(),
        None => std::ptr::null_mut(),
    }
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_headers_has(handle: f64, key_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let Some(key) = read_str(key_ptr) else {
        return 0.0;
    };
    let g = HEADERS_HANDLES.lock().unwrap();
    if g.get(&id)
        .map(|h| h.contains_key(&key.to_lowercase()))
        .unwrap_or(false)
    {
        1.0
    } else {
        0.0
    }
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_headers_delete(handle: f64, key_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let Some(key) = read_str(key_ptr) else {
        return 0.0;
    };
    let mut g = HEADERS_HANDLES.lock().unwrap();
    if let Some(h) = g.get_mut(&id) {
        if h.remove(&key.to_lowercase()).is_some() {
            return 1.0;
        }
    }
    0.0
}

/// Snapshot the headers under `handle` as a sorted-by-key vec. WHATWG
/// Fetch spec calls for iteration order to be sorted lexicographically
/// by header name; perry-stdlib's matching helper does the same (refs
/// #576 in CLAUDE.md). Used by forEach / keys / values / entries.
fn snapshot_sorted(handle: f64) -> Vec<(String, String)> {
    let id = handle_id(handle);
    let mut entries: Vec<(String, String)> = HEADERS_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|h| h.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// `headers.forEach(callback)` — invoke callback(value, key) for each
/// entry. Iteration order is WHATWG-Fetch sorted-by-key (spec-compliant
/// — perry-stdlib's copy applies the same sort, refs #576).
#[no_mangle]
pub extern "C" fn js_headers_for_each(handle: f64, callback: f64) -> f64 {
    let cb_bits = callback.to_bits();
    let cb_ptr = (cb_bits & 0x0000_FFFF_FFFF_FFFF) as *const RawClosureHeader;
    if cb_ptr.is_null() {
        return 0.0;
    }
    for (key, value) in snapshot_sorted(handle) {
        let key_str = alloc_string(&key);
        let value_str = alloc_string(&value);
        let key_v = JsValue::from_string_ptr(key_str.as_raw());
        let value_v = JsValue::from_string_ptr(value_str.as_raw());
        let closure = unsafe { JsClosure::from_raw(cb_ptr) };
        // Web Fetch order is (value, key) per the spec.
        let _ =
            unsafe { closure.call2(f64::from_bits(value_v.bits()), f64::from_bits(key_v.bits())) };
        let _ = (key_v, value_v);
    }
    1.0
}

/// NaN-box a perry-ffi ArrayHeader pointer as a POINTER_TAG f64.
/// Mirrors perry-stdlib's `nanbox_array_pointer`; codegen unboxes via
/// `js_nanbox_get_pointer` on the consuming side.
#[inline]
fn nanbox_array_ptr(arr: *mut perry_ffi::ArrayHeader) -> f64 {
    let bits = POINTER_TAG | ((arr as u64) & 0x0000_FFFF_FFFF_FFFF);
    f64::from_bits(bits)
}

/// Build a JsValue holding a NaN-boxed string pointer (STRING_TAG).
#[inline]
fn js_string_value(s: &str) -> JsValue {
    JsValue::from_string_ptr(alloc_string(s).as_raw())
}

/// `headers.keys()` — sorted-by-key string array. Matches perry-stdlib's
/// equivalent; refs #576 (`for…of headers.keys()` direct iteration plus
/// spread / Array.from work via the array's own iterator).
#[no_mangle]
pub extern "C" fn js_headers_keys(handle: f64) -> f64 {
    let entries = snapshot_sorted(handle);
    unsafe {
        let mut arr = perry_ffi::js_array_alloc(entries.len() as u32);
        for (k, _) in entries {
            arr = perry_ffi::js_array_push(arr, js_string_value(&k));
        }
        nanbox_array_ptr(arr)
    }
}

/// `headers.values()` — sorted-by-key value array. See `js_headers_keys`.
#[no_mangle]
pub extern "C" fn js_headers_values(handle: f64) -> f64 {
    let entries = snapshot_sorted(handle);
    unsafe {
        let mut arr = perry_ffi::js_array_alloc(entries.len() as u32);
        for (_, v) in entries {
            arr = perry_ffi::js_array_push(arr, js_string_value(&v));
        }
        nanbox_array_ptr(arr)
    }
}

/// `headers.entries()` — sorted-by-key array of `[key, value]` pair
/// arrays. `for (const [k, v] of headers.entries())` and the bare
/// `for (const [k, v] of h)` direct-iteration shape both route here
/// (the latter via the codegen Symbol.iterator alias added in #576).
#[no_mangle]
pub extern "C" fn js_headers_entries(handle: f64) -> f64 {
    let entries = snapshot_sorted(handle);
    unsafe {
        let mut arr = perry_ffi::js_array_alloc(entries.len() as u32);
        for (k, v) in entries {
            let mut pair = perry_ffi::js_array_alloc(2);
            pair = perry_ffi::js_array_push(pair, js_string_value(&k));
            pair = perry_ffi::js_array_push(pair, js_string_value(&v));
            // Push the inner pair as a NaN-boxed pointer JsValue so
            // the outer array's element reads as a real array.
            let pair_v = JsValue::from_bits(nanbox_array_ptr(pair).to_bits());
            arr = perry_ffi::js_array_push(arr, pair_v);
        }
        nanbox_array_ptr(arr)
    }
}

// ── Response advanced ─────────────────────────────────────────────

/// `new Response(body, init)` — stores body string + status + statusText
/// + headers. The `headers_handle` arg matches perry-stdlib's 4-arg shape
/// (declared in `crates/perry-codegen/src/runtime_decls.rs:1045`); a
/// 3-arg version dropped the codegen-supplied headers handle on the
/// floor — `fetchRes.headers.forEach(...)` then iterated an empty map.
///
/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s;
/// `headers_handle` must be 0.0 / TAG_UNDEFINED or a valid handle id
/// returned by `js_headers_new`.
#[no_mangle]
pub unsafe extern "C" fn js_response_new(
    body_ptr: *const StringHeader,
    status: f64,
    status_text_ptr: *const StringHeader,
    headers_handle: f64,
) -> f64 {
    let body = read_str(body_ptr).unwrap_or_default().into_bytes();
    let status = if status.is_nan() || status == 0.0 {
        200
    } else {
        status as u16
    };
    let status_text = read_str(status_text_ptr).unwrap_or_else(|| "OK".to_string());
    let headers_id = handle_id(headers_handle);
    let headers = if headers_id != 0 {
        HEADERS_HANDLES
            .lock()
            .unwrap()
            .get(&headers_id)
            .cloned()
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    store_response(FetchResponse {
        status,
        status_text,
        headers,
        body,
    }) as f64
}

#[no_mangle]
pub extern "C" fn js_response_get_headers(handle: f64) -> f64 {
    let id = handle_id(handle);
    let headers = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| r.headers.clone())
        .unwrap_or_default();
    store_headers(headers) as f64
}

#[no_mangle]
pub extern "C" fn js_response_clone(handle: f64) -> f64 {
    let id = handle_id(handle);
    let cloned = FETCH_RESPONSES.lock().unwrap().get(&id).cloned();
    match cloned {
        Some(r) => store_response(r) as f64,
        None => 0.0,
    }
}

/// # Safety
/// `handle` must come from a previous fetch.
#[no_mangle]
pub unsafe extern "C" fn js_response_array_buffer(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let id = handle_id(handle);
    let body = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| r.body.clone());
    match body {
        Some(b) => {
            // Resolve with the bytes as a string (caller wraps in
            // Uint8Array on JS side).
            let s = unsafe { std::str::from_utf8_unchecked(&b) }.to_string();
            promise.resolve(JsValue::from_string_ptr(alloc_string(&s).as_raw()));
        }
        None => promise.reject_string("Invalid response handle"),
    }
    raw
}

/// # Safety
/// `handle` must come from a previous fetch.
#[no_mangle]
pub unsafe extern "C" fn js_response_blob(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let id = handle_id(handle);
    let cloned = FETCH_RESPONSES.lock().unwrap().get(&id).cloned();
    match cloned {
        Some(r) => {
            let content_type = r
                .headers
                .get("content-type")
                .cloned()
                .unwrap_or_else(|| "application/octet-stream".to_string());
            let blob_id = store_blob(BlobData {
                bytes: r.body,
                content_type,
            });
            promise.resolve(JsValue::from_number(blob_id as f64));
        }
        None => promise.reject_string("Invalid response handle"),
    }
    raw
}

#[no_mangle]
pub extern "C" fn js_response_body(handle: f64) -> f64 {
    let id = handle_id(handle);
    if FETCH_RESPONSES.lock().unwrap().contains_key(&id) {
        // Return the same handle as a stub stream id; fully wiring
        // ReadableStream is a followup (matches perry-stdlib's
        // existing minimum: returns the response handle itself).
        handle
    } else {
        0.0
    }
}

/// `Response.json(value)` — static; constructs a Response with a
/// JSON-encoded body. We accept the JSValue f64 and assume the
/// caller has already JSON-stringified it (perry-stdlib's existing
/// convention — the codegen-side wrapper does the stringify).
///
/// # Safety
/// `value` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_response_static_json(value: f64) -> f64 {
    let v = JsValue::from_bits(value.to_bits());
    let body = perry_ffi::json_stringify(v).unwrap_or_default();
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    store_response(FetchResponse {
        status: 200,
        status_text: "OK".to_string(),
        headers,
        body: body.into_bytes(),
    }) as f64
}

/// `Response.redirect(url, status)` — static.
///
/// # Safety
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_response_static_redirect(
    url_ptr: *const StringHeader,
    status: f64,
) -> f64 {
    let url = read_str(url_ptr).unwrap_or_default();
    let status = status as u16;
    let mut headers = HashMap::new();
    headers.insert("location".to_string(), url);
    store_response(FetchResponse {
        status,
        status_text: "Found".to_string(),
        headers,
        body: Vec::new(),
    }) as f64
}

// ── Blob ──────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn js_blob_size(handle: f64) -> f64 {
    let id = handle as usize;
    BLOB_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|b| b.bytes.len() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_blob_type(handle: f64) -> *mut StringHeader {
    let id = handle as usize;
    let g = BLOB_HANDLES.lock().unwrap();
    match g.get(&id) {
        Some(b) => alloc_string(&b.content_type).as_raw(),
        None => alloc_string("").as_raw(),
    }
}

/// # Safety
/// `handle` must come from a previous blob alloc.
#[no_mangle]
pub unsafe extern "C" fn js_blob_array_buffer(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let id = handle as usize;
    let bytes = BLOB_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|b| b.bytes.clone());
    match bytes {
        Some(b) => {
            let s = unsafe { std::str::from_utf8_unchecked(&b) }.to_string();
            promise.resolve(JsValue::from_string_ptr(alloc_string(&s).as_raw()));
        }
        None => promise.reject_string("Invalid blob handle"),
    }
    raw
}

/// # Safety
/// `handle` must come from a previous blob alloc.
#[no_mangle]
pub unsafe extern "C" fn js_blob_bytes(handle: f64) -> *mut Promise {
    js_blob_array_buffer(handle)
}

/// # Safety
/// `handle` must come from a previous blob alloc.
#[no_mangle]
pub unsafe extern "C" fn js_blob_text(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let id = handle as usize;
    let bytes = BLOB_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|b| b.bytes.clone());
    match bytes {
        Some(b) => {
            let s = String::from_utf8_lossy(&b).to_string();
            promise.resolve(JsValue::from_string_ptr(alloc_string(&s).as_raw()));
        }
        None => promise.reject_string("Invalid blob handle"),
    }
    raw
}

/// `blob.slice(start, end, contentType)` — returns a new Blob
/// covering `[start, end)`. Negative indices wrap; if `end < start`
/// returns an empty blob (matches `Blob.slice` spec).
///
/// # Safety
/// `content_type_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_blob_slice(
    handle: f64,
    start: f64,
    end: f64,
    content_type_ptr: *const StringHeader,
) -> f64 {
    let id = handle as usize;
    let g = BLOB_HANDLES.lock().unwrap();
    let Some(orig) = g.get(&id) else { return 0.0 };
    let len = orig.bytes.len() as f64;
    let s = if start < 0.0 {
        (len + start).max(0.0)
    } else {
        start.min(len)
    } as usize;
    let e = if end < 0.0 {
        (len + end).max(0.0)
    } else {
        end.min(len)
    } as usize;
    let slice_bytes = if e > s {
        orig.bytes[s..e].to_vec()
    } else {
        Vec::new()
    };
    let content_type = read_str(content_type_ptr).unwrap_or_else(|| orig.content_type.clone());
    drop(g);
    store_blob(BlobData {
        bytes: slice_bytes,
        content_type,
    }) as f64
}

#[no_mangle]
pub extern "C" fn js_blob_stream(handle: f64) -> f64 {
    // Stub — return the handle so user code can call it; full
    // ReadableStream wiring is a followup (matches perry-stdlib's
    // existing minimum behavior).
    handle
}

// ── Request ───────────────────────────────────────────────────────

/// `new Request(url, init)` — stores url/method/body. The `headers_handle`
/// arg matches perry-stdlib's 4-arg shape (declared in
/// `crates/perry-codegen/src/runtime_decls.rs:1064`); it's currently ignored
/// at the storage level (perry-ext-fetch's RequestData has no headers field
/// yet), but accepting the arg keeps the ABI in sync with codegen so the f64
/// arg lands in the right register.
///
/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s;
/// `headers_handle` must be 0.0 / TAG_UNDEFINED or a valid handle id.
#[no_mangle]
pub unsafe extern "C" fn js_request_new(
    url_ptr: *const StringHeader,
    method_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    _headers_handle: f64,
) -> f64 {
    let url = read_str(url_ptr).unwrap_or_default();
    let method = read_str(method_ptr).unwrap_or_else(|| "GET".to_string());
    let body = read_str(body_ptr);
    store_request(RequestData { url, method, body }) as f64
}

#[no_mangle]
pub extern "C" fn js_request_get_url(handle: f64) -> *mut StringHeader {
    let id = handle_id(handle);
    let g = REQUEST_HANDLES.lock().unwrap();
    match g.get(&id) {
        Some(r) => alloc_string(&r.url).as_raw(),
        None => alloc_string("").as_raw(),
    }
}

#[no_mangle]
pub extern "C" fn js_request_get_method(handle: f64) -> *mut StringHeader {
    let id = handle_id(handle);
    let g = REQUEST_HANDLES.lock().unwrap();
    match g.get(&id) {
        Some(r) => alloc_string(&r.method).as_raw(),
        None => alloc_string("GET").as_raw(),
    }
}

#[no_mangle]
pub extern "C" fn js_request_get_body(handle: f64) -> f64 {
    let id = handle_id(handle);
    let g = REQUEST_HANDLES.lock().unwrap();
    match g.get(&id).and_then(|r| r.body.as_ref()) {
        Some(s) => {
            let ptr = alloc_string(s).as_raw();
            f64::from_bits(STRING_TAG | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        None => f64::from_bits(TAG_UNDEFINED),
    }
}

// `get_handle` / `register_handle` referenced for future surface;
// silence unused-import warnings without dropping them.
#[allow(dead_code)]
fn _ensure_handle_imports() -> Option<()> {
    let _: Option<&i64> = get_handle::<i64>(0);
    let _: i64 = register_handle(0i64);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_count_starts_at_zero() {
        let initial = js_fetch_response_count();
        // Other tests may have populated, but it can't be negative.
        assert!(initial >= 0);
    }

    #[test]
    fn response_status_invalid_handle() {
        assert_eq!(js_fetch_response_status(99_999_999.0), 0.0);
    }

    #[test]
    fn headers_round_trip() {
        let h = js_headers_new();
        let key = alloc_string("Content-Type");
        let value = alloc_string("application/json");
        let set = unsafe { js_headers_set(h, key.as_raw(), value.as_raw()) };
        assert_eq!(set, 1.0);
        let got_ptr = unsafe { js_headers_get(h, key.as_raw()) };
        let got = perry_ffi::read_string(unsafe { JsString::from_raw(got_ptr) }).expect("non-null");
        assert_eq!(got, "application/json");
        let has = unsafe { js_headers_has(h, key.as_raw()) };
        assert_eq!(has, 1.0);
        let del = unsafe { js_headers_delete(h, key.as_raw()) };
        assert_eq!(del, 1.0);
        let has2 = unsafe { js_headers_has(h, key.as_raw()) };
        assert_eq!(has2, 0.0);
    }

    #[test]
    fn blob_slice_basic() {
        let id = store_blob(BlobData {
            bytes: b"hello, world".to_vec(),
            content_type: "text/plain".to_string(),
        });
        let null = std::ptr::null::<StringHeader>();
        let sliced = unsafe { js_blob_slice(id as f64, 7.0, 12.0, null) };
        assert!(sliced > 0.0);
        let size = js_blob_size(sliced);
        assert_eq!(size, 5.0);
    }

    #[test]
    fn request_round_trip() {
        let url = alloc_string("https://example.com");
        let method = alloc_string("POST");
        let body = alloc_string(r#"{"x":1}"#);
        let h = unsafe { js_request_new(url.as_raw(), method.as_raw(), body.as_raw(), 0.0) };
        assert!(h > 0.0);
        let url_ptr = js_request_get_url(h);
        let url_str =
            perry_ffi::read_string(unsafe { JsString::from_raw(url_ptr) }).expect("non-null");
        assert_eq!(url_str, "https://example.com");
        let method_ptr = js_request_get_method(h);
        let method_str =
            perry_ffi::read_string(unsafe { JsString::from_raw(method_ptr) }).expect("non-null");
        assert_eq!(method_str, "POST");
    }

    #[test]
    fn response_static_json() {
        let v = JsValue::from_string_ptr(alloc_string("hello").as_raw());
        let resp = unsafe { js_response_static_json(f64::from_bits(v.bits())) };
        assert!(resp > 0.0);
        let status = js_fetch_response_status(resp);
        assert_eq!(status, 200.0);
    }
}
