//! Web Fetch body helpers and metadata FFIs.
//!
//! Split out of `fetch/mod.rs` to keep that file below the size gate. As a
//! child module, this can use the fetch registries and private helper types.

use super::*;

#[derive(Clone)]
enum FormDataValue {
    Text(String),
    File(usize),
}

#[derive(Clone, Default)]
struct FormDataStore {
    entries: Vec<(String, FormDataValue)>,
}

impl FormDataStore {
    fn append(&mut self, name: String, value: FormDataValue) {
        self.entries.push((name, value));
    }

    fn set(&mut self, name: String, value: FormDataValue) {
        let mut replaced_first = false;
        self.entries.retain_mut(|(k, v)| {
            if k != &name {
                return true;
            }
            if replaced_first {
                return false;
            }
            *v = value.clone();
            replaced_first = true;
            true
        });
        if !replaced_first {
            self.entries.push((name, value));
        }
    }

    fn delete(&mut self, name: &str) {
        self.entries.retain(|(k, _)| k != name);
    }

    fn get(&self, name: &str) -> Option<FormDataValue> {
        self.entries
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == name)
    }

    fn get_all(&self, name: &str) -> Vec<FormDataValue> {
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
    let id = alloc_fetch_handle_id();
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
        store.append(name, FormDataValue::Text(value));
    }
    store
}

const FORM_DATA_PARSE_MESSAGE: &str = "Failed to parse body as FormData.";
const FORM_DATA_CONTENT_TYPE_MESSAGE: &str =
    "Content-Type was not one of \"multipart/form-data\" or \"application/x-www-form-urlencoded\".";

fn file_last_modified_now() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as f64)
        .unwrap_or(0.0)
}

fn form_data_from_multipart(
    body: &[u8],
    content_type: &str,
) -> Result<FormDataStore, &'static str> {
    let parts = crate::multipart_parser::parse_multipart(body, content_type)
        .map_err(|_| FORM_DATA_PARSE_MESSAGE)?;
    let mut store = FormDataStore::default();
    for part in parts {
        let value = if let Some(filename) = part.filename {
            let content_type = crate::fetch_blob::normalize_blob_type(
                part.content_type.as_deref().unwrap_or("text/plain"),
            );
            let file_id = alloc_blob(BlobData {
                body: part.data,
                content_type,
                file_name: Some(filename),
                last_modified_ms: Some(file_last_modified_now()),
            });
            FormDataValue::File(file_id)
        } else {
            FormDataValue::Text(String::from_utf8_lossy(&part.data).into_owned())
        };
        store.append(part.name, value);
    }
    Ok(store)
}

fn form_data_from_body(body: &[u8], content_type: &str) -> Result<FormDataStore, &'static str> {
    let media_type = content_type.split(';').next().unwrap_or_default().trim();
    if media_type.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
        Ok(form_data_from_urlencoded(body))
    } else if media_type.eq_ignore_ascii_case("multipart/form-data") {
        form_data_from_multipart(body, content_type)
    } else {
        Err(FORM_DATA_CONTENT_TYPE_MESSAGE)
    }
}

unsafe fn form_data_value_string(value: f64) -> String {
    let ptr = perry_runtime::value::js_jsvalue_to_string(value);
    string_from_header(ptr as *const StringHeader).unwrap_or_default()
}

fn form_data_string_array(values: Vec<String>) -> f64 {
    // #8163: `arr` must survive the per-value string allocations below.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(values.len() as u32));
    for value in values {
        let value_ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
        let arr = perry_runtime::js_array_push_f64(
            arr_handle.get_raw_mut_ptr::<perry_runtime::ArrayHeader>(),
            f64::from_bits(JSValue::string_ptr(value_ptr).bits()),
        );
        arr_handle.set_raw_mut_ptr(arr);
    }
    nanbox_array_pointer(arr_handle.get_raw_mut_ptr())
}

fn form_data_value_to_js(value: &FormDataValue) -> f64 {
    match value {
        FormDataValue::Text(value) => {
            let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
        FormDataValue::File(id) => handle_to_f64(*id),
    }
}

fn form_data_value_array(values: Vec<FormDataValue>) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(values.len() as u32));
    for value in values {
        // A text value can move if growing the array collects; a File is a
        // small registry handle and the GC's handle-band filter leaves it as-is.
        let inner = perry_runtime::gc::RuntimeHandleScope::new();
        let value_handle = inner.root_nanbox_f64(form_data_value_to_js(&value));
        let arr = perry_runtime::js_array_push_f64(
            arr_handle.get_raw_mut_ptr::<perry_runtime::ArrayHeader>(),
            value_handle.get_nanbox_f64(),
        );
        arr_handle.set_raw_mut_ptr(arr);
    }
    nanbox_array_pointer(arr_handle.get_raw_mut_ptr())
}

fn response_content_type(handle: f64) -> String {
    let id = handle_id(handle);
    FETCH_RESPONSES
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|response| response_headers_snapshot(response).get("content-type"))
        .unwrap_or_default()
}

fn request_content_type(handle: f64) -> String {
    let id = handle_id(handle);
    REQUEST_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|request| request_headers_snapshot(request).get("content-type"))
        .unwrap_or_default()
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
    let id = alloc_fetch_handle_id();
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
            body_stream_id: None,
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
    let content_type = response_content_type(handle);
    match consume_response_body(handle) {
        Ok(body) => match form_data_from_body(&body, &content_type) {
            Ok(form) => {
                let form_id = alloc_form_data(form);
                perry_runtime::js_promise_resolve(promise, handle_to_f64(form_id));
            }
            Err(message) => reject_fetch_type_error(promise, message),
        },
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
    // #8163: snapshot under the guard, allocate after it is dropped — the Fetch
    // root scanner takes this same lock during a collection on this thread, so
    // allocating inside the guard's scope self-deadlocks whenever that
    // allocation triggers one. See `fetch::gc` and `js_request_get_url`.
    let value = REQUEST_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|req| f(req).to_owned())
        .unwrap_or_default();
    js_string_from_bytes(value.as_ptr(), value.len() as u32)
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
    let content_type = request_content_type(handle);
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
    let content_type = request_content_type(handle);
    match consume_request_body(handle) {
        Ok(body) => match form_data_from_body(&body, &content_type) {
            Ok(form) => {
                let form_id = alloc_form_data(form);
                perry_runtime::js_promise_resolve(promise, handle_to_f64(form_id));
            }
            Err(message) => reject_fetch_type_error(promise, message),
        },
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
pub extern "C" fn js_form_data_new() -> f64 {
    handle_to_f64(alloc_form_data(FormDataStore::default()))
}

#[no_mangle]
pub unsafe extern "C" fn js_form_data_append(handle: f64, name: f64, value: f64) -> f64 {
    let id = handle_id(handle);
    let name = form_data_value_string(name);
    let value = form_data_value_string(value);
    if let Some(form) = FORM_DATA_REGISTRY.lock().unwrap().get_mut(&id) {
        form.append(name, FormDataValue::Text(value));
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_form_data_set(handle: f64, name: f64, value: f64) -> f64 {
    let id = handle_id(handle);
    let name = form_data_value_string(name);
    let value = form_data_value_string(value);
    if let Some(form) = FORM_DATA_REGISTRY.lock().unwrap().get_mut(&id) {
        form.set(name, FormDataValue::Text(value));
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_form_data_delete(handle: f64, name_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let name = string_from_header(name_ptr).unwrap_or_default();
    if let Some(form) = FORM_DATA_REGISTRY.lock().unwrap().get_mut(&id) {
        form.delete(&name);
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_form_data_has(handle: f64, name_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let Some(name) = string_from_header(name_ptr) else {
        return f64::from_bits(TAG_FALSE);
    };
    let has = FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|form| form.has(&name))
        .unwrap_or(false);
    tagged_bool(has)
}

#[no_mangle]
pub unsafe extern "C" fn js_form_data_get(handle: f64, name_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let Some(name) = string_from_header(name_ptr) else {
        return f64::from_bits(TAG_NULL);
    };
    let value = FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|form| form.get(&name));
    match value {
        Some(value) => form_data_value_to_js(&value),
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
    form_data_value_array(values)
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
    // #8163: `arr`, `name_ptr` and `pair` are all raw heap addresses held across
    // later allocations in this same loop. See `js_headers_entries`.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(entries.len() as u32));
    for (name, value) in entries {
        let inner = perry_runtime::gc::RuntimeHandleScope::new();
        let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let name_handle = inner.root_nanbox_u64(JSValue::string_ptr(name_ptr).bits());
        let value_handle = inner.root_nanbox_f64(form_data_value_to_js(&value));
        let pair_handle = inner.root_raw_mut_ptr(perry_runtime::js_array_alloc(2));
        let pair = perry_runtime::js_array_push_f64(
            pair_handle.get_raw_mut_ptr::<perry_runtime::ArrayHeader>(),
            name_handle.get_nanbox_f64(),
        );
        pair_handle.set_raw_mut_ptr(pair);
        let pair = perry_runtime::js_array_push_f64(
            pair_handle.get_raw_mut_ptr::<perry_runtime::ArrayHeader>(),
            value_handle.get_nanbox_f64(),
        );
        pair_handle.set_raw_mut_ptr(pair);
        let arr = perry_runtime::js_array_push_f64(
            arr_handle.get_raw_mut_ptr::<perry_runtime::ArrayHeader>(),
            nanbox_array_pointer(pair_handle.get_raw_mut_ptr()),
        );
        arr_handle.set_raw_mut_ptr(arr);
    }
    nanbox_array_pointer(arr_handle.get_raw_mut_ptr())
}

#[no_mangle]
pub extern "C" fn js_form_data_keys(handle: f64) -> f64 {
    let id = handle_id(handle);
    let values = FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|form| form.entries.iter().map(|(k, _)| k.clone()).collect())
        .unwrap_or_default();
    form_data_string_array(values)
}

#[no_mangle]
pub extern "C" fn js_form_data_values(handle: f64) -> f64 {
    let id = handle_id(handle);
    let values = FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|form| form.entries.iter().map(|(_, v)| v.clone()).collect())
        .unwrap_or_default();
    form_data_value_array(values)
}

#[no_mangle]
pub extern "C" fn js_form_data_for_each(handle: f64, callback: f64) -> f64 {
    let id = handle_id(handle);
    let entries = FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(|form| form.entries.clone())
        .unwrap_or_default();
    let cb_ptr = (callback.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
    if cb_ptr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // #8163: the raw `ClosureHeader*` and the first of the two strings are held
    // across an allocation and across user JS. See `js_headers_for_each`.
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let cb_handle = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(cb_ptr));
    for (name, value) in entries {
        let inner = perry_runtime::gc::RuntimeHandleScope::new();
        let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let name_handle = inner.root_nanbox_u64(JSValue::string_ptr(name_ptr).bits());
        let value_handle = inner.root_nanbox_f64(form_data_value_to_js(&value));
        let closure = perry_runtime::js_nanbox_get_pointer(cb_handle.get_nanbox_f64())
            as *const perry_runtime::ClosureHeader;
        perry_runtime::js_closure_call3(
            closure,
            value_handle.get_nanbox_f64(),
            name_handle.get_nanbox_f64(),
            handle,
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[doc(hidden)]
pub fn form_data_contains_handle(handle: usize) -> bool {
    FORM_DATA_REGISTRY.lock().unwrap().contains_key(&handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_urlencoded_and_multipart_parsers_from_content_type() {
        let encoded = form_data_from_body(
            b"name=Perry+TS&name=second",
            "application/x-www-form-urlencoded; charset=UTF-8",
        )
        .unwrap();
        assert!(matches!(
            &encoded.entries[0],
            (name, FormDataValue::Text(value)) if name == "name" && value == "Perry TS"
        ));

        let body = b"--boundary\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"\"\r\n\r\n\0\xff\r\n--boundary--\r\n";
        let multipart = form_data_from_body(
            body,
            "multipart/form-data; charset=utf-8; boundary=boundary",
        )
        .unwrap();
        let file_id = match &multipart.entries[0] {
            (name, FormDataValue::File(id)) if name == "upload" => *id,
            _ => panic!("expected a File form entry"),
        };
        let file = BLOB_REGISTRY.lock().unwrap().remove(&file_id).unwrap();
        assert_eq!(file.body, b"\0\xff");
        assert_eq!(file.file_name.as_deref(), Some(""));
        assert_eq!(file.content_type, "text/plain");

        assert!(form_data_from_body(b"{}", "application/json").is_err());
    }
}
