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
use bytes::Bytes;
use lazy_static::lazy_static;
use perry_ffi::{
    alloc_string, get_handle, register_handle, spawn_blocking, JsClosure, JsPromise, JsString,
    JsValue, Promise, RawClosureHeader, StringHeader,
};
use std::collections::HashMap;
use std::sync::Mutex;

// Web Fetch constructor validation helpers (#2640 / #2643) — split out to
// keep lib.rs under the 2,000-line lint gate.
mod validation;
use validation::{
    is_forbidden_method, is_null_body_status, is_redirect_status, is_valid_status_text,
    normalize_method, parse_redirect_location, redirect_status_from_value,
};
use validation::{throw_range_error, throw_type_error};

#[cfg(test)]
mod test_async_shims;

const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

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
    headers: HeadersStore,
    // `Bytes` (not `Vec<u8>`): the body is filled once from `reqwest`'s
    // `response.bytes()` and then read by several accessors
    // (`text`/`json`/`arrayBuffer`/`bytes`/`blob`/`formData`). With a
    // `Vec<u8>` every accessor `.clone()`d the whole payload to drop the
    // registry lock before allocating into the arena — a full body copy
    // per read. `Bytes` is a refcounted slice, so that same `.clone()` is
    // an O(1) atomic bump (zero copy); the decode is what allocates, once.
    body: Bytes,
    type_name: String,
    url: String,
    redirected: bool,
}

#[derive(Clone, Default)]
struct HeadersStore {
    entries: Vec<(String, String)>,
}

impl HeadersStore {
    fn set(&mut self, key: &str, value: &str) {
        let lk = key.to_ascii_lowercase();
        self.entries.retain(|(k, _)| *k != lk);
        self.entries.push((lk, value.to_string()));
    }

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

    fn delete(&mut self, key: &str) -> bool {
        let lk = key.to_ascii_lowercase();
        let old_len = self.entries.len();
        self.entries.retain(|(k, _)| *k != lk);
        self.entries.len() != old_len
    }

    fn set_cookie_values(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(k, _)| k == "set-cookie")
            .map(|(_, v)| v.clone())
            .collect()
    }
}

lazy_static! {
    static ref FETCH_RESPONSES: Mutex<HashMap<usize, FetchResponse>> =
        Mutex::new(HashMap::new());
    static ref NEXT_RESPONSE_ID: Mutex<usize> = Mutex::new(1);

    static ref HEADERS_HANDLES: Mutex<HashMap<usize, HeadersStore>> =
        Mutex::new(HashMap::new());
    static ref NEXT_HEADERS_ID: Mutex<usize> = Mutex::new(1);

    static ref BLOB_HANDLES: Mutex<HashMap<usize, BlobData>> = Mutex::new(HashMap::new());
    static ref NEXT_BLOB_ID: Mutex<usize> = Mutex::new(1);

    static ref REQUEST_HANDLES: Mutex<HashMap<usize, RequestData>> = Mutex::new(HashMap::new());
    static ref NEXT_REQUEST_ID: Mutex<usize> = Mutex::new(1);

    static ref FORM_DATA_HANDLES: Mutex<HashMap<usize, FormDataStore>> =
        Mutex::new(HashMap::new());
    static ref NEXT_FORM_DATA_ID: Mutex<usize> = Mutex::new(1);

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
}

#[derive(Clone, Default)]
struct FormDataStore {
    entries: Vec<(String, String)>,
}

impl FormDataStore {
    fn append(&mut self, name: String, value: String) {
        self.entries.push((name, value));
    }

    fn get(&self, name: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    fn get_all(&self, name: &str) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }
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

fn store_headers(headers: HeadersStore) -> usize {
    let mut id_guard = NEXT_HEADERS_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    HEADERS_HANDLES.lock().unwrap().insert(id, headers);
    id
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

fn store_form_data(data: FormDataStore) -> usize {
    let mut id_guard = NEXT_FORM_DATA_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    FORM_DATA_HANDLES.lock().unwrap().insert(id, data);
    id
}

fn tagged_bool(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

fn is_missing_value(value: f64) -> bool {
    let bits = value.to_bits();
    value == 0.0 || bits == TAG_UNDEFINED || bits == TAG_NULL
}

fn bool_from_js(value: f64) -> bool {
    match value.to_bits() {
        TAG_TRUE => true,
        TAG_FALSE | TAG_NULL | TAG_UNDEFINED => false,
        _ => value != 0.0,
    }
}

fn default_abort_signal_value() -> f64 {
    unsafe extern "C" {
        fn js_abort_controller_new() -> *mut std::ffi::c_void;
        fn js_abort_controller_signal(controller: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }

    unsafe {
        let controller = js_abort_controller_new();
        let signal = js_abort_controller_signal(controller);
        f64::from_bits(JsValue::from_object_ptr(signal).bits())
    }
}

fn signal_or_default(signal: f64) -> f64 {
    if is_missing_value(signal) {
        default_abort_signal_value()
    } else {
        signal
    }
}

fn percent_decode_form_component(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push(((hi << 4) | lo) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form_data_from_urlencoded(body: &[u8]) -> FormDataStore {
    let text = String::from_utf8_lossy(body);
    let mut store = FormDataStore::default();
    for pair in text.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let name = percent_decode_form_component(parts.next().unwrap_or_default());
        let value = percent_decode_form_component(parts.next().unwrap_or_default());
        store.append(name, value);
    }
    store
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
                    let response_url = response.url().to_string();
                    let redirected = response_url != url;
                    let headers = headers_from_header_map(response.headers());
                    let body = response.bytes().await.unwrap_or_default();
                    Ok(FetchResponse {
                        status,
                        status_text,
                        headers,
                        body,
                        type_name: "basic".to_string(),
                        url: response_url,
                        redirected,
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
        promise.reject_string("Invalid URL");
        return raw;
    };
    let mut headers = HashMap::new();
    if let Some(auth) = read_str(auth_header_ptr) {
        if !auth.is_empty() {
            headers.insert("Authorization".to_string(), auth);
        }
    }
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
    map.get(&id).map(|r| r.status as f64).unwrap_or(0.0)
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

#[no_mangle]
pub extern "C" fn js_fetch_response_type(handle: f64) -> *mut StringHeader {
    let id = handle_id(handle);
    let g = FETCH_RESPONSES.lock().unwrap();
    match g.get(&id) {
        Some(r) => alloc_string(&r.type_name).as_raw(),
        None => alloc_string("").as_raw(),
    }
}

#[no_mangle]
pub extern "C" fn js_fetch_response_url(handle: f64) -> *mut StringHeader {
    let id = handle_id(handle);
    let g = FETCH_RESPONSES.lock().unwrap();
    match g.get(&id) {
        Some(r) => alloc_string(&r.url).as_raw(),
        None => alloc_string("").as_raw(),
    }
}

#[no_mangle]
pub extern "C" fn js_fetch_response_redirected(handle: f64) -> f64 {
    let id = handle_id(handle);
    let g = FETCH_RESPONSES.lock().unwrap();
    tagged_bool(g.get(&id).map(|r| r.redirected).unwrap_or(false))
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
            // `from_utf8_lossy` borrows when the body is already valid
            // UTF-8 (the common case), so the decode adds no allocation;
            // `alloc_string` then writes straight into the arena.
            promise.resolve(JsValue::from_string_ptr(
                alloc_string(&String::from_utf8_lossy(&b)).as_raw(),
            ));
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
            // perry-stdlib's existing copy. `from_utf8_lossy` borrows on
            // valid UTF-8, so the decode is allocation-free into the arena.
            promise.resolve(JsValue::from_string_ptr(
                alloc_string(&String::from_utf8_lossy(&b)).as_raw(),
            ));
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
    store_headers(HeadersStore::default()) as f64
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
        h.set(&key, &value);
        1.0
    } else {
        0.0
    }
}

/// # Safety
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_headers_append(
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
        h.append(&key, &value);
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
    match g.get(&id).and_then(|h| h.get(&key)) {
        Some(v) => alloc_string(&v).as_raw(),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn js_headers_get_set_cookie(handle: f64) -> f64 {
    let id = handle_id(handle);
    let values = HEADERS_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(HeadersStore::set_cookie_values)
        .unwrap_or_default();
    unsafe {
        let mut arr = perry_ffi::js_array_alloc(values.len() as u32);
        for v in values {
            arr = perry_ffi::js_array_push(arr, js_string_value(&v));
        }
        nanbox_array_ptr(arr)
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
    if g.get(&id).map(|h| h.has(&key)).unwrap_or(false) {
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
        if h.delete(&key) {
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
        .map(|h| h.entries.clone())
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
    let body_opt = read_str(body_ptr);
    let body_present = body_opt.is_some();
    let body = body_opt.unwrap_or_default().into_bytes();
    // NaN/0.0 are the codegen "no status field" sentinels → default 200.
    // Otherwise truncate toward zero + range-check 200..=599 (#2640).
    let status = if status.is_nan() || status == 0.0 {
        200
    } else {
        let truncated = status.trunc();
        if !(200.0..=599.0).contains(&truncated) {
            throw_range_error("init[\"status\"] must be in the range of 200 to 599, inclusive.");
        }
        truncated as u16
    };
    let status_text = match read_str(status_text_ptr) {
        Some(s) => {
            if !is_valid_status_text(&s) {
                throw_type_error("Invalid statusText");
            }
            s
        }
        None => String::new(),
    };
    if body_present && is_null_body_status(status) {
        throw_type_error(&format!(
            "Response constructor: Invalid response status code {status}"
        ));
    }
    let headers_id = handle_id(headers_handle);
    let headers = if headers_id != 0 {
        HEADERS_HANDLES
            .lock()
            .unwrap()
            .get(&headers_id)
            .cloned()
            .unwrap_or_default()
    } else {
        HeadersStore::default()
    };
    store_response(FetchResponse {
        status,
        status_text,
        headers,
        body: Bytes::from(body),
        type_name: "default".to_string(),
        url: String::new(),
        redirected: false,
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

/// Wrap arbitrary body bytes as the `JsValue` that `arrayBuffer()` /
/// `bytes()` resolve with: a runtime Buffer (`POINTER_TAG`), byte-exact.
///
/// The body must NOT go through a `String`: `new Uint8Array(value)` on
/// the JS side dispatches on the buffer tag, so a `STRING_TAG` value is
/// read as an empty buffer — and `from_utf8_unchecked` on a non-UTF-8
/// payload (a fetched PNG/protobuf) is both UB and lossy. Handing the
/// raw bytes to `alloc_buffer` is the only correct shape.
fn body_to_buffer_value(bytes: &[u8]) -> JsValue {
    JsValue::from_object_ptr(perry_ffi::alloc_buffer(bytes))
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
        Some(b) => promise.resolve(body_to_buffer_value(&b)),
        None => promise.reject_string("Invalid response handle"),
    }
    raw
}

/// # Safety
/// `handle` must come from a previous fetch.
#[no_mangle]
pub unsafe extern "C" fn js_response_bytes(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let id = handle_id(handle);
    let body = FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&id)
        .map(|r| r.body.clone());
    match body {
        Some(b) => promise.resolve(body_to_buffer_value(&b)),
        None => promise.reject_string("Invalid response handle"),
    }
    raw
}

/// # Safety
/// `handle` must come from a previous fetch.
#[no_mangle]
pub unsafe extern "C" fn js_response_form_data(handle: f64) -> *mut Promise {
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
            let form_id = store_form_data(form_data_from_urlencoded(&b));
            promise.resolve(JsValue::from_number(form_id as f64));
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
                .unwrap_or_else(|| "application/octet-stream".to_string());
            let blob_id = store_blob(BlobData {
                bytes: r.body.to_vec(),
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
pub unsafe extern "C" fn js_response_static_json(
    value: f64,
    init_status: f64,
    init_status_text_ptr: *const StringHeader,
    headers_handle: f64,
) -> f64 {
    let v = JsValue::from_bits(value.to_bits());
    let body = perry_ffi::json_stringify(v).unwrap_or_default();
    // #2638: honor `init.status` / `init.statusText` / `init.headers`.
    let status = if init_status.is_nan() || init_status == 0.0 {
        200
    } else {
        init_status as u16
    };
    // Node's `Response.json` leaves statusText "" when not provided — it does
    // not fall back to the status reason phrase.
    let status_text = read_str(init_status_text_ptr).unwrap_or_default();
    // Start from any user-provided headers, then add the default content-type
    // only if the init headers didn't already set one.
    let headers_id = handle_id(headers_handle);
    let mut headers = if headers_id != 0 {
        HEADERS_HANDLES
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
    store_response(FetchResponse {
        status,
        status_text,
        headers,
        body: Bytes::from(body),
        type_name: "default".to_string(),
        url: String::new(),
        redirected: false,
    }) as f64
}

/// `Response.redirect(url, status)` — static. `url_ptr` must be null or a
/// Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_response_static_redirect(
    url_ptr: *const StringHeader,
    status: f64,
) -> f64 {
    let url = read_str(url_ptr).unwrap_or_default();
    let status = redirect_status_from_value(status);
    if !is_redirect_status(status) {
        throw_range_error(&format!("Invalid status code {status}"));
    }
    let location = match parse_redirect_location(&url) {
        Ok(location) => location,
        Err(_) => throw_type_error(&format!("Failed to parse URL from {url}")),
    };
    let mut headers = HeadersStore::default();
    headers.set("location", &location);
    store_response(FetchResponse {
        status: status as u16,
        status_text: String::new(),
        headers,
        body: Bytes::new(),
        type_name: "default".to_string(),
        url: String::new(),
        redirected: false,
    }) as f64
}

#[no_mangle]
pub extern "C" fn js_response_static_error() -> f64 {
    store_response(FetchResponse {
        status: 0,
        status_text: String::new(),
        headers: HeadersStore::default(),
        body: Bytes::new(),
        type_name: "error".to_string(),
        url: String::new(),
        redirected: false,
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
        // Resolve a real Buffer object, not a string: the same non-UTF-8
        // corruption the Response `arrayBuffer` path had — `blob.arrayBuffer()`
        // (and `blob.bytes()`, which forwards here) must return the stored
        // bytes byte-exact.
        Some(b) => promise.resolve(body_to_buffer_value(&b)),
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
/// arg matches perry-stdlib's shape so the f64 arg lands in the right
/// register (declared in `crates/perry-codegen/src/runtime_decls.rs:1064`).
///
/// # Safety
/// All string pointers must be null or Perry-runtime `StringHeader`s;
/// `headers_handle` must be 0.0 / TAG_UNDEFINED or a valid handle id.
#[no_mangle]
pub unsafe extern "C" fn js_request_new(
    url_ptr: *const StringHeader,
    method_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    headers_handle: f64,
    referrer_ptr: *const StringHeader,
    referrer_policy_ptr: *const StringHeader,
    mode_ptr: *const StringHeader,
    credentials_ptr: *const StringHeader,
    cache_ptr: *const StringHeader,
    redirect_ptr: *const StringHeader,
    integrity_ptr: *const StringHeader,
    keepalive: f64,
    duplex_ptr: *const StringHeader,
    signal: f64,
) -> f64 {
    let url = read_str(url_ptr).unwrap_or_default();
    let raw_method = read_str(method_ptr).unwrap_or_else(|| "GET".to_string());
    // Forbidden methods rejected case-insensitively; message keeps the
    // caller's original casing (Node parity). #2643
    if is_forbidden_method(&raw_method.to_ascii_uppercase()) {
        throw_type_error(&format!("'{raw_method}' HTTP method is unsupported."));
    }
    let method = normalize_method(&raw_method);
    let body = read_str(body_ptr);
    if body.is_some() && (method == "GET" || method == "HEAD") {
        throw_type_error("Request with GET/HEAD method cannot have body.");
    }
    let headers_id = handle_id(headers_handle);
    let headers = if headers_id != 0 {
        HEADERS_HANDLES
            .lock()
            .unwrap()
            .get(&headers_id)
            .cloned()
            .unwrap_or_default()
    } else {
        HeadersStore::default()
    };
    store_request(RequestData {
        url,
        method,
        body,
        headers,
        destination: String::new(),
        referrer: read_str(referrer_ptr).unwrap_or_else(|| "about:client".to_string()),
        referrer_policy: read_str(referrer_policy_ptr).unwrap_or_default(),
        mode: read_str(mode_ptr).unwrap_or_else(|| "cors".to_string()),
        credentials: read_str(credentials_ptr).unwrap_or_else(|| "same-origin".to_string()),
        cache: read_str(cache_ptr).unwrap_or_else(|| "default".to_string()),
        redirect: read_str(redirect_ptr).unwrap_or_else(|| "follow".to_string()),
        integrity: read_str(integrity_ptr).unwrap_or_default(),
        keepalive: bool_from_js(keepalive),
        duplex: read_str(duplex_ptr).unwrap_or_else(|| "half".to_string()),
        signal: signal_or_default(signal),
    }) as f64
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

fn request_string_field(handle: f64, f: impl FnOnce(&RequestData) -> &str) -> *mut StringHeader {
    let id = handle_id(handle);
    let g = REQUEST_HANDLES.lock().unwrap();
    match g.get(&id) {
        Some(r) => {
            let s = f(r);
            alloc_string(s).as_raw()
        }
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
    let g = REQUEST_HANDLES.lock().unwrap();
    match g.get(&id).and_then(|r| r.body.as_ref()) {
        Some(s) => {
            let ptr = alloc_string(s).as_raw();
            f64::from_bits(STRING_TAG | (ptr as u64 & 0x0000_FFFF_FFFF_FFFF))
        }
        None => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Read a request's stored body (empty string for a bodiless request),
/// or `None` for an invalid handle. (#1688)
fn request_body_string(handle: f64) -> Option<String> {
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

/// request.arrayBuffer() -> Promise. Resolves with the body bytes as a string
/// (caller wraps in Uint8Array), matching `js_response_array_buffer`. (#1688)
///
/// # Safety
/// `handle` must come from a previous `js_request_new`.
#[no_mangle]
pub unsafe extern "C" fn js_request_array_buffer(handle: f64) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    match request_body_string(handle) {
        Some(s) => promise.resolve(JsValue::from_string_ptr(alloc_string(&s).as_raw())),
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
                bytes: r.body.unwrap_or_default().into_bytes(),
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
    match request_body_string(handle) {
        Some(s) => {
            let buf = perry_ffi::alloc_buffer(s.as_bytes());
            promise.resolve(JsValue::from_object_ptr(buf));
        }
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

/// # Safety
/// `name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_form_data_get_all(handle: f64, name_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let name = read_str(name_ptr).unwrap_or_default();
    let values = FORM_DATA_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|f| f.get_all(&name))
        .unwrap_or_default();
    let mut arr = perry_ffi::js_array_alloc(values.len() as u32);
    for value in values {
        arr = perry_ffi::js_array_push(arr, js_string_value(&value));
    }
    nanbox_array_ptr(arr)
}

#[no_mangle]
pub extern "C" fn js_form_data_entries(handle: f64) -> f64 {
    let id = handle_id(handle);
    let entries = FORM_DATA_HANDLES
        .lock()
        .unwrap()
        .get(&id)
        .map(|f| f.entries.clone())
        .unwrap_or_default();
    unsafe {
        let mut arr = perry_ffi::js_array_alloc(entries.len() as u32);
        for (name, value) in entries {
            let mut pair = perry_ffi::js_array_alloc(2);
            pair = perry_ffi::js_array_push(pair, js_string_value(&name));
            pair = perry_ffi::js_array_push(pair, js_string_value(&value));
            arr =
                perry_ffi::js_array_push(arr, JsValue::from_bits(nanbox_array_ptr(pair).to_bits()));
        }
        nanbox_array_ptr(arr)
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
mod tests;
