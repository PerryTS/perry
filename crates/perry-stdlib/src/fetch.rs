//! HTTP Fetch module (node-fetch compatible)
//!
//! Native implementation of the 'node-fetch' npm package using reqwest.
//! Provides fetch() function for making HTTP requests.

use perry_runtime::{
    js_array_alloc, js_array_push, js_object_alloc, js_object_alloc_with_shape,
    js_object_set_field, js_object_set_keys,
    js_string_from_bytes, JSValue, StringHeader,
};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::common::async_bridge::{queue_promise_resolution, spawn};

// Response handle storage
lazy_static::lazy_static! {
    static ref FETCH_RESPONSES: Mutex<HashMap<usize, FetchResponse>> = Mutex::new(HashMap::new());
    static ref NEXT_RESPONSE_ID: Mutex<usize> = Mutex::new(1);
    static ref STREAM_HANDLES: Mutex<HashMap<usize, StreamState>> = Mutex::new(HashMap::new());
    static ref NEXT_STREAM_ID: Mutex<usize> = Mutex::new(1);

    /// Shared HTTP client — reuses connection pool, DNS cache, and TLS session cache
    /// across all fetch() calls. Without this, each fetch allocates a fresh
    /// reqwest::Client (~250KB of state per request) and the memory never gets
    /// reused, causing unbounded RSS growth in long-running services.
    static ref HTTP_CLIENT: reqwest::Client = reqwest::Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(16)
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
}

struct StreamState {
    status: u8,           // 0=connecting, 1=streaming, 2=done, 3=error
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
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    // NaN-boxed TAG_UNDEFINED (0x7FFC_0000_0000_0001) unboxes to 0x1
    // after POINTER_MASK. Treat any pointer below page size as invalid.
    if ptr.is_null() || (ptr as usize) < 0x1000 {
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
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    spawn(async move {
        match HTTP_CLIENT.get(&url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response.status().canonical_reason().unwrap_or("").to_string();

                let mut headers = HashMap::new();
                for (key, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        headers.insert(key.to_string(), v.to_string());
                    }
                }

                let body = response.bytes().await.unwrap_or_default().to_vec();

                // Store response
                let mut id_guard = NEXT_RESPONSE_ID.lock().unwrap();
                let response_id = *id_guard;
                *id_guard += 1;
                drop(id_guard);

                FETCH_RESPONSES.lock().unwrap().insert(response_id, FetchResponse {
                    status,
                    status_text,
                    headers,
                    body,
                });

                // Return response handle
                let result_bits = (response_id as f64).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
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
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
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
                let status_text = response.status().canonical_reason().unwrap_or("").to_string();

                let mut headers = HashMap::new();
                for (key, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        headers.insert(key.to_string(), v.to_string());
                    }
                }

                let body = response.bytes().await.unwrap_or_default().to_vec();

                let mut id_guard = NEXT_RESPONSE_ID.lock().unwrap();
                let response_id = *id_guard;
                *id_guard += 1;
                drop(id_guard);

                FETCH_RESPONSES.lock().unwrap().insert(response_id, FetchResponse {
                    status,
                    status_text,
                    headers,
                    body,
                });

                let result_bits = (response_id as f64).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
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
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let auth_header = string_from_header(auth_header_ptr).unwrap_or_default();
    let body = string_from_header(body_ptr).unwrap_or_default();

    spawn(async move {
        let client = HTTP_CLIENT.clone();
        let mut request = client.post(&url)
            .header("Content-Type", "application/json");
        if !auth_header.is_empty() {
            request = request.header("Authorization", &auth_header);
        }
        request = request.body(body);
        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response.status().canonical_reason().unwrap_or("").to_string();

                let mut headers = HashMap::new();
                for (key, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        headers.insert(key.to_string(), v.to_string());
                    }
                }

                let body = response.bytes().await.unwrap_or_default().to_vec();

                let mut id_guard = NEXT_RESPONSE_ID.lock().unwrap();
                let response_id = *id_guard;
                *id_guard += 1;
                drop(id_guard);

                FETCH_RESPONSES.lock().unwrap().insert(response_id, FetchResponse {
                    status,
                    status_text,
                    headers,
                    body,
                });

                let result_bits = (response_id as f64).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
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
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let body = string_from_header(body_ptr).unwrap_or_default();
    let content_type = string_from_header(content_type_ptr).unwrap_or_else(|| "application/json".to_string());

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
                let status_text = response.status().canonical_reason().unwrap_or("").to_string();

                let mut headers = HashMap::new();
                for (key, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        headers.insert(key.to_string(), v.to_string());
                    }
                }

                let body = response.bytes().await.unwrap_or_default().to_vec();

                // Store response
                let mut id_guard = NEXT_RESPONSE_ID.lock().unwrap();
                let response_id = *id_guard;
                *id_guard += 1;
                drop(id_guard);

                FETCH_RESPONSES.lock().unwrap().insert(response_id, FetchResponse {
                    status,
                    status_text,
                    headers,
                    body,
                });

                // Return response handle
                let result_bits = (response_id as f64).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
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
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            let err_msg = "Invalid URL";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    let method = string_from_header(method_ptr).unwrap_or_else(|| "GET".to_string());
    let body = string_from_header(body_ptr);
    let headers_json = string_from_header(headers_json_ptr).unwrap_or_else(|| "{}".to_string());


    // Parse headers from JSON
    let custom_headers: HashMap<String, String> = serde_json::from_str(&headers_json).unwrap_or_default();

    spawn(async move {
        let client = HTTP_CLIENT.clone();
        let mut request = match method.to_uppercase().as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            "HEAD" => client.head(&url),
            _ => client.get(&url), // Default to GET
        };

        // Add custom headers
        for (key, value) in &custom_headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Add body if present
        if let Some(b) = body {
            request = request.body(b);
        }

        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response.status().canonical_reason().unwrap_or("").to_string();

                let mut headers = HashMap::new();
                for (key, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        headers.insert(key.to_string(), v.to_string());
                    }
                }

                let body = response.bytes().await.unwrap_or_default().to_vec();

                // Store response
                let mut id_guard = NEXT_RESPONSE_ID.lock().unwrap();
                let response_id = *id_guard;
                *id_guard += 1;
                drop(id_guard);

                FETCH_RESPONSES.lock().unwrap().insert(response_id, FetchResponse {
                    status,
                    status_text,
                    headers,
                    body,
                });

                // Return response handle
                let result_bits = (response_id as f64).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

    promise
}

/// Get response status code
/// response.status -> number
#[no_mangle]
pub extern "C" fn js_fetch_response_status(handle: i64) -> f64 {
    let response_id = handle as usize;
    let guard = FETCH_RESPONSES.lock().unwrap();
    match guard.get(&response_id) {
        Some(resp) => resp.status as f64,
        None => 0.0,
    }
}

/// Get response status text
/// response.statusText -> string
#[no_mangle]
pub extern "C" fn js_fetch_response_status_text(handle: i64) -> *mut StringHeader {
    let response_id = handle as usize;
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
pub extern "C" fn js_fetch_response_ok(handle: i64) -> f64 {
    let response_id = handle as usize;
    let guard = FETCH_RESPONSES.lock().unwrap();
    match guard.get(&response_id) {
        Some(resp) => if resp.status >= 200 && resp.status < 300 { 1.0 } else { 0.0 },
        None => 0.0,
    }
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
pub unsafe extern "C" fn js_fetch_response_text(handle: i64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let response_id = handle as usize;

    // Clone the body — JS spec says each body is readable once, but other
    // accessors (status, headers) may still be used afterwards. The Response
    // entry stays in FETCH_RESPONSES until cleanup; in practice the test suite
    // doesn't accumulate enough responses to matter.
    let body = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        match guard.get(&response_id) {
            Some(resp) => resp.body.clone(),
            None => {
                let err_msg = "Invalid response handle";
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_nan = f64::from_bits(JSValue::pointer(err_str as *const u8).bits());
                perry_runtime::js_promise_reject(promise, err_nan);
                return promise;
            }
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
pub unsafe extern "C" fn js_fetch_response_json(handle: i64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let response_id = handle as usize;

    // Take (not clone) the body — consumes the FETCH_RESPONSES entry.
    let body = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        match guard.get(&response_id) {
            Some(resp) => resp.body.clone(),
            None => {
                let err_msg = "Invalid response handle";
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_nan = f64::from_bits(JSValue::pointer(err_str as *const u8).bits());
                perry_runtime::js_promise_reject(promise, err_nan);
                return promise;
            }
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
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_nan = f64::from_bits(JSValue::pointer(err_str as *const u8).bits());
            perry_runtime::js_promise_reject(promise, err_nan);
        }
    }

    promise
}

/// Simple fetch that returns text directly (convenience function)
/// fetchText(url) -> Promise<string>
#[no_mangle]
pub unsafe extern "C" fn js_fetch_text(url_ptr: *const StringHeader) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            let err_msg = "Invalid URL";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    spawn(async move {
        match HTTP_CLIENT.get(&url).send().await {
            Ok(response) => {
                match response.text().await {
                    Ok(text) => {
                        let result_str = js_string_from_bytes(text.as_ptr(), text.len() as u32);
                        let result_bits = JSValue::pointer(result_str as *const u8).bits();
                        queue_promise_resolution(promise_ptr, true, result_bits);
                    }
                    Err(e) => {
                        let err_msg = format!("Read error: {}", e);
                        let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                        let err_bits = JSValue::pointer(err_str as *const u8).bits();
                        queue_promise_resolution(promise_ptr, false, err_bits);
                    }
                }
            }
            Err(e) => {
                let err_msg = format!("Fetch error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
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
    url_ptr: *const StringHeader, method_ptr: *const StringHeader,
    body_ptr: *const StringHeader, headers_json_ptr: *const StringHeader,
) -> f64 {
    let url = string_from_header(url_ptr).unwrap_or_default();
    let method = string_from_header(method_ptr).unwrap_or_else(|| "POST".to_string());
    let body = string_from_header(body_ptr);
    let headers_json = string_from_header(headers_json_ptr).unwrap_or_else(|| "{}".to_string());
    let custom_headers: HashMap<String, String> = serde_json::from_str(&headers_json).unwrap_or_default();
    let mut id_guard = NEXT_STREAM_ID.lock().unwrap();
    let stream_id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    STREAM_HANDLES.lock().unwrap().insert(stream_id, StreamState {
        status: 0, pending_lines: Vec::new(), partial: String::new(), http_status: 0, error: String::new(),
    });
    let sid = stream_id;
    spawn(async move {
        let client = HTTP_CLIENT.clone();
        let mut request = match method.to_uppercase().as_str() {
            "POST" => client.post(&url), "PUT" => client.put(&url),
            "PATCH" => client.patch(&url), _ => client.get(&url),
        };
        for (key, value) in &custom_headers { request = request.header(key.as_str(), value.as_str()); }
        if let Some(b) = body { request = request.body(b); }
        match request.send().await {
            Ok(mut response) => {
                let http_status = response.status().as_u16();
                { let mut g = STREAM_HANDLES.lock().unwrap(); if let Some(s) = g.get_mut(&sid) { s.http_status = http_status; s.status = 1; } }
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
                                        if !line.is_empty() { s.pending_lines.push(line); }
                                    } else { break; }
                                }
                            } else { break; }
                        }
                        Ok(None) => {
                            let mut g = STREAM_HANDLES.lock().unwrap();
                            if let Some(s) = g.get_mut(&sid) {
                                if !s.partial.is_empty() { let r = std::mem::take(&mut s.partial); s.pending_lines.push(r); }
                                s.status = 2;
                            }
                            break;
                        }
                        Err(e) => {
                            let mut g = STREAM_HANDLES.lock().unwrap();
                            if let Some(s) = g.get_mut(&sid) { s.error = format!("Stream error: {}", e); s.status = 3; }
                            break;
                        }
                    }
                }
            }
            Err(e) => { let mut g = STREAM_HANDLES.lock().unwrap(); if let Some(s) = g.get_mut(&sid) { s.error = format!("Connection error: {}", e); s.status = 3; } }
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
    if let Some(s) = g.get(&id) { s.status as f64 } else { 3.0 }
}

#[no_mangle]
pub extern "C" fn js_fetch_stream_close(handle: f64) -> f64 {
    let id = handle as usize;
    let mut g = STREAM_HANDLES.lock().unwrap();
    if g.remove(&id).is_some() { 1.0 } else { 0.0 }
}

// ========================================================================
// Web Fetch API: Headers, Request, Response constructors and methods
// ========================================================================

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
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
        for entry in self.entries.iter_mut() {
            if entry.0 == lk {
                entry.1 = value.to_string();
                return;
            }
        }
        self.entries.push((lk, value.to_string()));
    }
    fn get(&self, key: &str) -> Option<&str> {
        let lk = key.to_ascii_lowercase();
        self.entries.iter().find(|(k, _)| *k == lk).map(|(_, v)| v.as_str())
    }
    fn has(&self, key: &str) -> bool {
        let lk = key.to_ascii_lowercase();
        self.entries.iter().any(|(k, _)| *k == lk)
    }
    fn delete(&mut self, key: &str) {
        let lk = key.to_ascii_lowercase();
        self.entries.retain(|(k, _)| *k != lk);
    }
    fn from_hashmap(m: &HashMap<String, String>) -> Self {
        let mut s = Self::default();
        for (k, v) in m {
            s.set(k, v);
        }
        s
    }
}

#[derive(Clone)]
struct RequestRecord {
    url: String,
    method: String,
    body: Option<String>,
    #[allow(dead_code)]
    headers: HeadersStore,
}

lazy_static::lazy_static! {
    static ref HEADERS_REGISTRY: Mutex<HashMap<usize, HeadersStore>> = Mutex::new(HashMap::new());
    static ref NEXT_HEADERS_ID: Mutex<usize> = Mutex::new(1);
    static ref REQUEST_REGISTRY: Mutex<HashMap<usize, RequestRecord>> = Mutex::new(HashMap::new());
    static ref NEXT_REQUEST_ID: Mutex<usize> = Mutex::new(1);
}

fn alloc_headers(store: HeadersStore) -> usize {
    let mut id_guard = NEXT_HEADERS_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    HEADERS_REGISTRY.lock().unwrap().insert(id, store);
    id
}

fn alloc_response(status: u16, status_text: String, headers: HeadersStore, body: Vec<u8>) -> usize {
    let mut id_guard = NEXT_RESPONSE_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    let hdr_map: HashMap<String, String> = headers.entries.iter().cloned().collect();
    FETCH_RESPONSES.lock().unwrap().insert(id, FetchResponse {
        status,
        status_text,
        headers: hdr_map,
        body,
    });
    id
}

// ----------------- Headers FFI -----------------

/// new Headers() — returns numeric handle as f64
#[no_mangle]
pub extern "C" fn js_headers_new() -> f64 {
    alloc_headers(HeadersStore::default()) as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_set(handle: f64, key_ptr: *const StringHeader, value_ptr: *const StringHeader) -> f64 {
    let id = handle as usize;
    let key = string_from_header(key_ptr).unwrap_or_default();
    let value = string_from_header(value_ptr).unwrap_or_default();
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get_mut(&id) {
        store.set(&key, &value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_get(handle: f64, key_ptr: *const StringHeader) -> *mut StringHeader {
    let id = handle as usize;
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get(&id) {
        if let Some(v) = store.get(&key) {
            return js_string_from_bytes(v.as_ptr(), v.len() as u32);
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_has(handle: f64, key_ptr: *const StringHeader) -> f64 {
    let id = handle as usize;
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return f64::from_bits(TAG_FALSE),
    };
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get(&id) {
        if store.has(&key) {
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_delete(handle: f64, key_ptr: *const StringHeader) -> f64 {
    let id = handle as usize;
    let key = string_from_header(key_ptr).unwrap_or_default();
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get_mut(&id) {
        store.delete(&key);
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_headers_for_each(handle: f64, callback: f64) -> f64 {
    let id = handle as usize;
    let entries: Vec<(String, String)> = match HEADERS_REGISTRY.lock().unwrap().get(&id) {
        Some(s) => s.entries.clone(),
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    // Extract closure pointer from NaN-boxed callback
    let cb_bits = callback.to_bits();
    let cb_ptr = (cb_bits & 0x0000_FFFF_FFFF_FFFF) as i64;
    if cb_ptr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let closure = cb_ptr as *const perry_runtime::ClosureHeader;
    for (k, v) in entries {
        let v_ptr = unsafe { js_string_from_bytes(v.as_ptr(), v.len() as u32) };
        let k_ptr = unsafe { js_string_from_bytes(k.as_ptr(), k.len() as u32) };
        let v_nan = JSValue::string_ptr(v_ptr).bits();
        let k_nan = JSValue::string_ptr(k_ptr).bits();
        unsafe {
            perry_runtime::js_closure_call2(closure, f64::from_bits(v_nan), f64::from_bits(k_nan));
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

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
    let body_str = string_from_header(body_ptr).unwrap_or_default();
    let body = body_str.into_bytes();
    let status_u16 = if status.is_nan() || status == 0.0 { 200 } else { status as u16 };
    let status_text = string_from_header(status_text_ptr)
        .unwrap_or_else(|| canonical_reason(status_u16).to_string());
    let headers = if headers_handle > 0.0 {
        HEADERS_REGISTRY.lock().unwrap().get(&(headers_handle as usize)).cloned().unwrap_or_default()
    } else {
        HeadersStore::default()
    };
    alloc_response(status_u16, status_text, headers, body) as f64
}

fn canonical_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "",
    }
}

/// response.headers — returns a Headers handle (f64). Lazily allocates a Headers entry
/// from the response's stored header HashMap if one doesn't exist yet.
#[no_mangle]
pub extern "C" fn js_response_get_headers(handle: f64) -> f64 {
    let id = handle as usize;
    let store = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        match guard.get(&id) {
            Some(resp) => HeadersStore::from_hashmap(&resp.headers),
            None => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    alloc_headers(store) as f64
}

/// response.clone() — duplicates the response (deep copy of body + headers)
#[no_mangle]
pub extern "C" fn js_response_clone(handle: f64) -> f64 {
    let id = handle as usize;
    let cloned = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        match guard.get(&id) {
            Some(resp) => Some(FetchResponse {
                status: resp.status,
                status_text: resp.status_text.clone(),
                headers: resp.headers.clone(),
                body: resp.body.clone(),
            }),
            None => None,
        }
    };
    if let Some(new_resp) = cloned {
        let mut id_guard = NEXT_RESPONSE_ID.lock().unwrap();
        let new_id = *id_guard;
        *id_guard += 1;
        drop(id_guard);
        FETCH_RESPONSES.lock().unwrap().insert(new_id, new_resp);
        return new_id as f64;
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
    let id = handle as usize;
    let body: Vec<u8> = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        match guard.get(&id) {
            Some(resp) => resp.body.clone(),
            None => Vec::new(),
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

/// response.blob() — returns an object { size: N, type: "..." }
/// Resolved synchronously; see `js_fetch_response_text`.
///
/// TODO(#234): body bytes are dropped here. A real Blob requires implementing
/// `.arrayBuffer()` / `.text()` / `.bytes()` / `.slice()` instance methods plus
/// codegen dispatch routing (`crates/perry-codegen/src/lower_call.rs`).
/// Until that lands, users needing binary body bytes should call
/// `response.arrayBuffer()` directly instead of `response.blob()`.
#[no_mangle]
pub unsafe extern "C" fn js_response_blob(handle: f64) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let id = handle as usize;
    let (body_len, content_type) = {
        let guard = FETCH_RESPONSES.lock().unwrap();
        match guard.get(&id) {
            Some(resp) => {
                let ct = resp.headers.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                (resp.body.len(), ct)
            }
            None => (0, String::new()),
        }
    };
    let packed = b"size\0type\0".as_ptr();
    let obj = js_object_alloc_with_shape(0x7FFE_FE02, 2, packed, 10);
    perry_runtime::js_object_set_field(obj, 0, JSValue::number(body_len as f64));
    let type_str = js_string_from_bytes(content_type.as_ptr(), content_type.len() as u32);
    perry_runtime::js_object_set_field(obj, 1, JSValue::string_ptr(type_str));
    let val = JSValue::object_ptr(obj as *mut u8);
    perry_runtime::js_promise_resolve(promise, f64::from_bits(val.bits()));
    promise
}

/// Response.json(value) — static method. Allocates a Response with JSON-stringified body
/// and Content-Type: application/json. The value is passed as NaN-boxed JSValue bits (f64).
#[no_mangle]
pub unsafe extern "C" fn js_response_static_json(value: f64) -> f64 {
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
    let mut headers = HeadersStore::default();
    headers.set("content-type", "application/json");
    alloc_response(200, "OK".to_string(), headers, body_str.into_bytes()) as f64
}

/// Response.redirect(url, status) — static method. Allocates a redirect response.
#[no_mangle]
pub unsafe extern "C" fn js_response_static_redirect(url_ptr: *const StringHeader, status: f64) -> f64 {
    let url = string_from_header(url_ptr).unwrap_or_default();
    let status_u16 = if status == 0.0 || status.is_nan() { 302 } else { status as u16 };
    let mut headers = HeadersStore::default();
    headers.set("location", &url);
    alloc_response(status_u16, canonical_reason(status_u16).to_string(), headers, Vec::new()) as f64
}

// ----------------- Request FFI -----------------

/// new Request(url, methodOpt, bodyOpt, headersHandleOpt)
#[no_mangle]
pub unsafe extern "C" fn js_request_new(
    url_ptr: *const StringHeader,
    method_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    headers_handle: f64,
) -> f64 {
    let url = string_from_header(url_ptr).unwrap_or_default();
    let method = string_from_header(method_ptr).unwrap_or_else(|| "GET".to_string());
    let body = string_from_header(body_ptr);
    let headers = if headers_handle > 0.0 {
        HEADERS_REGISTRY.lock().unwrap().get(&(headers_handle as usize)).cloned().unwrap_or_default()
    } else {
        HeadersStore::default()
    };
    let mut id_guard = NEXT_REQUEST_ID.lock().unwrap();
    let id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    REQUEST_REGISTRY.lock().unwrap().insert(id, RequestRecord {
        url,
        method,
        body,
        headers,
    });
    id as f64
}

#[no_mangle]
pub extern "C" fn js_request_get_url(handle: f64) -> *mut StringHeader {
    let id = handle as usize;
    let guard = REQUEST_REGISTRY.lock().unwrap();
    match guard.get(&id) {
        Some(req) => js_string_from_bytes(req.url.as_ptr(), req.url.len() as u32),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn js_request_get_method(handle: f64) -> *mut StringHeader {
    let id = handle as usize;
    let guard = REQUEST_REGISTRY.lock().unwrap();
    match guard.get(&id) {
        Some(req) => js_string_from_bytes(req.method.as_ptr(), req.method.len() as u32),
        None => std::ptr::null_mut(),
    }
}

/// req.body — returns a string body or null. NaN-boxed return.
#[no_mangle]
pub extern "C" fn js_request_get_body(handle: f64) -> f64 {
    let id = handle as usize;
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
