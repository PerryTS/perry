//! Web Fetch body helpers and metadata FFIs.
//!
//! Split out of `fetch/mod.rs` to keep that file below the size gate. As a
//! child module, this can use the fetch registries and private helper types.

use super::*;

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

lazy_static::lazy_static! {
    static ref FORM_DATA_REGISTRY: Mutex<HashMap<usize, FormDataStore>> = Mutex::new(HashMap::new());
}

fn alloc_form_data(store: FormDataStore) -> usize {
    let mut id_guard = NEXT_FETCH_HANDLE_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    FORM_DATA_REGISTRY.lock().unwrap().insert(id, store);
    id
}

fn is_missing_value(value: f64) -> bool {
    let bits = value.to_bits();
    value == 0.0 || bits == TAG_UNDEFINED || bits == TAG_NULL
}

pub(super) fn bool_from_js(value: f64) -> bool {
    match value.to_bits() {
        TAG_TRUE => true,
        TAG_FALSE | TAG_NULL | TAG_UNDEFINED => false,
        _ => value != 0.0,
    }
}

fn default_abort_signal_value() -> f64 {
    let controller = perry_runtime::url::js_abort_controller_new();
    let signal = perry_runtime::url::js_abort_controller_signal(controller);
    f64::from_bits(JSValue::object_ptr(signal as *mut u8).bits())
}

pub(super) fn signal_or_default(signal: f64) -> f64 {
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

fn response_string_field(handle: f64, f: impl FnOnce(&FetchResponse) -> &str) -> *mut StringHeader {
    let id = handle_id(handle);
    let guard = FETCH_RESPONSES.lock().unwrap();
    match guard.get(&id) {
        Some(resp) => {
            let value = f(resp);
            js_string_from_bytes(value.as_ptr(), value.len() as u32)
        }
        None => js_string_from_bytes("".as_ptr(), 0),
    }
}

#[no_mangle]
pub extern "C" fn js_fetch_response_type(handle: f64) -> *mut StringHeader {
    response_string_field(handle, |resp| &resp.type_name)
}

#[no_mangle]
pub extern "C" fn js_fetch_response_url(handle: f64) -> *mut StringHeader {
    response_string_field(handle, |resp| &resp.url)
}

#[no_mangle]
pub extern "C" fn js_fetch_response_redirected(handle: f64) -> f64 {
    let id = handle_id(handle);
    let guard = FETCH_RESPONSES.lock().unwrap();
    tagged_bool(guard.get(&id).map(|resp| resp.redirected).unwrap_or(false))
}

#[no_mangle]
pub extern "C" fn js_response_static_error() -> f64 {
    let mut id_guard = NEXT_FETCH_HANDLE_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    FETCH_RESPONSES.lock().unwrap().insert(
        id,
        FetchResponse {
            status: 0,
            status_text: String::new(),
            headers: HeadersStore::default(),
            body: Vec::new(),
            body_present: false,
            body_used: false,
            type_name: "error".to_string(),
            url: String::new(),
            redirected: false,
            cached_headers_id: None,
            cached_body_stream_id: None,
        },
    );
    handle_to_f64(id)
}

unsafe fn resolve_bytes_promise(promise: *mut perry_runtime::Promise, body: Vec<u8>) {
    let buf = perry_runtime::buffer::buffer_alloc(body.len() as u32);
    (*buf).length = body.len() as u32;
    if !body.is_empty() {
        std::ptr::copy_nonoverlapping(
            body.as_ptr(),
            perry_runtime::buffer::buffer_data_mut(buf),
            body.len(),
        );
    }
    let value = JSValue::object_ptr(buf as *mut u8);
    perry_runtime::js_promise_resolve(promise, f64::from_bits(value.bits()));
}

#[no_mangle]
pub unsafe extern "C" fn js_response_bytes(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    match consume_response_body(handle) {
        Ok(body) => resolve_bytes_promise(promise, body),
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

#[no_mangle]
pub unsafe extern "C" fn js_response_form_data(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    match consume_response_body(handle) {
        Ok(body) => {
            let form_id = alloc_form_data(form_data_from_urlencoded(&body));
            perry_runtime::js_promise_resolve(promise, handle_to_f64(form_id));
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

fn request_string_field(handle: f64, f: impl FnOnce(&RequestRecord) -> &str) -> *mut StringHeader {
    let id = handle_id(handle);
    let guard = REQUEST_REGISTRY.lock().unwrap();
    match guard.get(&id) {
        Some(req) => {
            let value = f(req);
            js_string_from_bytes(value.as_ptr(), value.len() as u32)
        }
        None => js_string_from_bytes("".as_ptr(), 0),
    }
}

#[no_mangle]
pub extern "C" fn js_request_get_destination(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.destination)
}

#[no_mangle]
pub extern "C" fn js_request_get_referrer(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.referrer)
}

#[no_mangle]
pub extern "C" fn js_request_get_referrer_policy(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.referrer_policy)
}

#[no_mangle]
pub extern "C" fn js_request_get_mode(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.mode)
}

#[no_mangle]
pub extern "C" fn js_request_get_credentials(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.credentials)
}

#[no_mangle]
pub extern "C" fn js_request_get_cache(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.cache)
}

#[no_mangle]
pub extern "C" fn js_request_get_redirect(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.redirect)
}

#[no_mangle]
pub extern "C" fn js_request_get_integrity(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.integrity)
}

#[no_mangle]
pub extern "C" fn js_request_get_duplex(handle: f64) -> *mut StringHeader {
    request_string_field(handle, |req| &req.duplex)
}

#[no_mangle]
pub extern "C" fn js_request_get_keepalive(handle: f64) -> f64 {
    let id = handle_id(handle);
    let guard = REQUEST_REGISTRY.lock().unwrap();
    tagged_bool(guard.get(&id).map(|req| req.keepalive).unwrap_or(false))
}

#[no_mangle]
pub extern "C" fn js_request_get_signal(handle: f64) -> f64 {
    let id = handle_id(handle);
    let guard = REQUEST_REGISTRY.lock().unwrap();
    guard
        .get(&id)
        .map(|req| req.signal)
        .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub unsafe extern "C" fn js_request_blob(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let content_type = {
        let id = handle_id(handle);
        REQUEST_REGISTRY
            .lock()
            .unwrap()
            .get(&id)
            .and_then(|req| req.headers.get("content-type"))
            .unwrap_or_default()
    };
    match consume_request_body(handle) {
        Ok(body) => {
            let blob_id = alloc_blob(BlobData::blob(body, content_type));
            perry_runtime::js_promise_resolve(promise, handle_to_f64(blob_id));
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

#[no_mangle]
pub unsafe extern "C" fn js_request_bytes(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    match consume_request_body(handle) {
        Ok(body) => resolve_bytes_promise(promise, body),
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

#[no_mangle]
pub unsafe extern "C" fn js_request_form_data(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    match consume_request_body(handle) {
        Ok(body) => {
            let form_id = alloc_form_data(form_data_from_urlencoded(&body));
            perry_runtime::js_promise_resolve(promise, handle_to_f64(form_id));
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

#[no_mangle]
pub unsafe extern "C" fn js_form_data_get(handle: f64, name_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let Some(name) = string_from_header(name_ptr) else {
        return f64::from_bits(TAG_NULL);
    };
    let guard = FORM_DATA_REGISTRY.lock().unwrap();
    match guard.get(&id).and_then(|form| form.get(&name)) {
        Some(value) => {
            let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
        None => f64::from_bits(TAG_NULL),
    }
}

#[inline]
fn nanbox_array_pointer(arr: *mut perry_runtime::ArrayHeader) -> f64 {
    f64::from_bits(JSValue::object_ptr(arr as *mut u8).bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_form_data_get_all(handle: f64, name_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let name = string_from_header(name_ptr).unwrap_or_default();
    let values = FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|form| form.get_all(&name))
        .unwrap_or_default();
    let mut arr = perry_runtime::js_array_alloc(values.len() as u32);
    for value in values {
        let value_ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
        arr = perry_runtime::js_array_push_f64(
            arr,
            f64::from_bits(JSValue::string_ptr(value_ptr).bits()),
        );
    }
    nanbox_array_pointer(arr)
}

#[no_mangle]
pub extern "C" fn js_form_data_entries(handle: f64) -> f64 {
    let id = handle_id(handle);
    let entries = FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|form| form.entries.clone())
        .unwrap_or_default();
    let mut arr = perry_runtime::js_array_alloc(entries.len() as u32);
    for (name, value) in entries {
        let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value_ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
        let mut pair = perry_runtime::js_array_alloc(2);
        pair = perry_runtime::js_array_push_f64(
            pair,
            f64::from_bits(JSValue::string_ptr(name_ptr).bits()),
        );
        pair = perry_runtime::js_array_push_f64(
            pair,
            f64::from_bits(JSValue::string_ptr(value_ptr).bits()),
        );
        arr = perry_runtime::js_array_push_f64(arr, nanbox_array_pointer(pair));
    }
    nanbox_array_pointer(arr)
}
