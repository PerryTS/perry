//! HTTP/HTTPS client module (Node.js http/https compatible)
//!
//! Native implementation of Node.js http.request(), http.get(), https.request(), https.get()
//! using reqwest. Provides callback-based API matching the Node.js pattern used by SDKs
//! like twitter-api-v2, rss-parser, web-push, etc.
//!
//! Both http and https share this implementation — reqwest handles TLS based on URL scheme.

use perry_runtime::{
    js_array_get_jsvalue, js_array_length, js_closure_call0, js_closure_call1,
    js_object_get_field_by_name, js_object_keys, js_string_from_bytes, ClosureHeader, JSValue,
    StringHeader,
};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::common::async_bridge::spawn;
use crate::common::{for_each_handle_mut_of, get_handle_mut, register_handle, Handle};

/// Pending HTTP events to be processed on the main thread
static HTTP_PENDING_EVENTS: once_cell::sync::Lazy<Mutex<Vec<PendingHttpEvent>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(Vec::new()));

/// Push an HTTP event and wake the main thread (issue #84).
/// Every producer is inside an `async move { ... }` running on a tokio
/// worker — without the notify the event waits for the next event-loop
/// timeout to be picked up.
fn push_http_event(ev: PendingHttpEvent) {
    HTTP_PENDING_EVENTS.lock().unwrap().push(ev);
    perry_runtime::event_pump::js_notify_main_thread();
}

static HTTP_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the http GC root scanner exactly once. User closures passed
/// to `http.request(options, cb)` or `req.on('error', cb)` / `res.on(...)`
/// are stored inside ClientRequestHandle / IncomingMessageHandle values
/// in the handle registry and would otherwise not be marked by GC —
/// issue #35 pattern, same root cause as net.Socket listeners.
fn ensure_gc_scanner_registered() {
    HTTP_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:http",
            scan_http_roots_mut,
        );
    });
}

/// GC root scanner for HTTP callback closures. Walks every
/// ClientRequestHandle (response callback + 'error' listeners) and
/// IncomingMessageHandle ('data' / 'end' / 'error' listeners) in the
/// handle registry.
#[allow(dead_code)]
fn scan_http_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(mark);
    scan_http_roots_mut(&mut visitor);
}

fn scan_http_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    for_each_handle_mut_of::<ClientRequestHandle, _>(|req| {
        visitor.visit_i64_slot(&mut req.response_callback);
        for cb_vec in req.listeners.values_mut() {
            for cb in cb_vec.iter_mut() {
                visitor.visit_i64_slot(cb);
            }
        }
    });

    for_each_handle_mut_of::<IncomingMessageHandle, _>(|msg| {
        for cb_vec in msg.listeners.values_mut() {
            for cb in cb_vec.iter_mut() {
                visitor.visit_i64_slot(cb);
            }
        }
    });
}

/// Events that fire on the main thread via js_http_process_pending
enum PendingHttpEvent {
    /// Response received: (request_handle, status, status_message, headers, body)
    Response {
        request_handle: Handle,
        status: u16,
        status_message: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Error on request: (request_handle, error_message)
    Error {
        request_handle: Handle,
        error_message: String,
    },
}

/// ClientRequest handle — accumulates request options before sending
pub struct ClientRequestHandle {
    /// HTTP method
    method: String,
    /// Full URL to request
    url: String,
    /// Request headers
    headers: HashMap<String, String>,
    /// Request body (accumulated via write())
    body: Vec<u8>,
    /// Response callback closure pointer (receives IncomingMessage handle)
    response_callback: i64,
    /// Event listeners: 'error' callbacks
    listeners: HashMap<String, Vec<i64>>,
    /// Timeout in milliseconds
    timeout_ms: Option<u64>,
    /// Whether end() has been called (prevents double-send)
    ended: bool,
}

/// IncomingMessage handle — represents an HTTP response
pub struct IncomingMessageHandle {
    /// HTTP status code
    pub status_code: u16,
    /// HTTP status message
    pub status_message: String,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body
    pub body: Vec<u8>,
    /// Event listeners: 'data', 'end', 'error' callbacks
    pub listeners: HashMap<String, Vec<i64>>,
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Helper to extract a string field from a NaN-boxed JS object
unsafe fn get_object_string_field(obj_f64: f64, field_name: &str) -> Option<String> {
    let obj_bits = obj_f64.to_bits();
    let upper = obj_bits >> 48;
    // Must be a pointer-like value (POINTER_TAG 0x7FFD or raw pointer)
    let obj_ptr = if upper >= 0x7FF8 {
        (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ObjectHeader
    } else if upper == 0 && obj_bits >= 0x10000 {
        obj_bits as *const perry_runtime::ObjectHeader
    } else {
        return None;
    };
    if obj_ptr.is_null() {
        return None;
    }

    let key_str = js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let field_val = js_object_get_field_by_name(obj_ptr, key_str);

    if field_val.is_undefined() || field_val.is_null() {
        return None;
    }

    if field_val.is_string() {
        let str_ptr = field_val.as_string_ptr();
        if !str_ptr.is_null() {
            return string_from_header(str_ptr);
        }
    }

    // Try to extract from a number (port is often a number)
    if field_val.is_number() {
        return Some(format!("{}", field_val.as_number() as i64));
    }

    None
}

/// Helper to extract a number field from a NaN-boxed JS object
unsafe fn get_object_number_field(obj_f64: f64, field_name: &str) -> Option<f64> {
    let obj_bits = obj_f64.to_bits();
    let upper = obj_bits >> 48;
    let obj_ptr = if upper >= 0x7FF8 {
        (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ObjectHeader
    } else if upper == 0 && obj_bits >= 0x10000 {
        obj_bits as *const perry_runtime::ObjectHeader
    } else {
        return None;
    };
    if obj_ptr.is_null() {
        return None;
    }

    let key_str = js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let field_val = js_object_get_field_by_name(obj_ptr, key_str);

    if field_val.is_undefined() || field_val.is_null() {
        return None;
    }

    if field_val.is_number() {
        return Some(field_val.as_number());
    }

    None
}

/// Helper to extract headers from a NaN-boxed JS headers object
unsafe fn extract_headers_from_object(obj_f64: f64) -> HashMap<String, String> {
    let mut result = HashMap::new();

    let obj_bits = obj_f64.to_bits();
    let upper = obj_bits >> 48;
    let obj_ptr = if upper >= 0x7FF8 {
        (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *mut perry_runtime::ObjectHeader
    } else if upper == 0 && obj_bits >= 0x10000 {
        obj_bits as *mut perry_runtime::ObjectHeader
    } else {
        return result;
    };
    if obj_ptr.is_null() {
        return result;
    }

    // Get the keys array
    let keys_ptr = js_object_keys(obj_ptr);
    if keys_ptr.is_null() {
        return result;
    }
    let len = js_array_length(keys_ptr);

    for i in 0..len {
        let key_bits = js_array_get_jsvalue(keys_ptr, i);
        let key_val = JSValue::from_bits(key_bits);
        if key_val.is_string() {
            let key_str_ptr = key_val.as_string_ptr();
            if !key_str_ptr.is_null() {
                if let Some(key) = string_from_header(key_str_ptr) {
                    // Get value for this key
                    let val = js_object_get_field_by_name(
                        obj_ptr as *const perry_runtime::ObjectHeader,
                        key_str_ptr,
                    );
                    if val.is_string() {
                        let val_ptr = val.as_string_ptr();
                        if !val_ptr.is_null() {
                            if let Some(value) = string_from_header(val_ptr) {
                                result.insert(key, value);
                            }
                        }
                    }
                }
            }
        }
    }

    result
}

/// Build URL from Node.js http.request options object
/// Options can have: hostname, host, port, path, protocol
unsafe fn build_url_from_options(options_f64: f64, default_protocol: &str) -> String {
    let protocol = get_object_string_field(options_f64, "protocol")
        .unwrap_or_else(|| format!("{}:", default_protocol));
    let protocol = protocol.trim_end_matches(':');

    let hostname = get_object_string_field(options_f64, "hostname")
        .or_else(|| get_object_string_field(options_f64, "host"))
        .unwrap_or_else(|| "localhost".to_string());

    // Remove port from hostname if present (host can be "hostname:port")
    let hostname = hostname.split(':').next().unwrap_or("localhost");

    let port = get_object_string_field(options_f64, "port")
        .or_else(|| get_object_number_field(options_f64, "port").map(|n| format!("{}", n as u16)));

    let path = get_object_string_field(options_f64, "path").unwrap_or_else(|| "/".to_string());

    match port {
        Some(p) => format!("{}://{}:{}{}", protocol, hostname, p, path),
        None => format!("{}://{}{}", protocol, hostname, path),
    }
}

/// Check if a f64 value is a NaN-boxed string pointer
fn is_string_value(val: f64) -> bool {
    let bits = val.to_bits();
    let upper = bits >> 48;
    upper == 0x7FFF // STRING_TAG
}

/// Extract string from a NaN-boxed string value
unsafe fn extract_string_value(val: f64) -> Option<String> {
    let bits = val.to_bits();
    let upper = bits >> 48;
    let ptr = if upper == 0x7FFF {
        // STRING_TAG
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader
    } else if upper == 0x7FFD {
        // POINTER_TAG (sometimes strings use this)
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader
    } else if upper == 0 && bits >= 0x10000 {
        bits as *const StringHeader
    } else {
        return None;
    };
    if ptr.is_null() {
        return None;
    }
    string_from_header(ptr)
}

// ========================================================================
// FFI Functions
// ========================================================================

/// http.request(options, callback) -> ClientRequest handle
///
/// options: NaN-boxed JS object with hostname, port, path, method, headers
/// callback: closure pointer for response callback (receives IncomingMessage handle)
///
/// Returns a ClientRequest handle (i64)
#[no_mangle]
pub unsafe extern "C" fn js_http_request(options_f64: f64, callback_i64: i64) -> Handle {
    ensure_gc_scanner_registered();
    let method = get_object_string_field(options_f64, "method")
        .unwrap_or_else(|| "GET".to_string())
        .to_uppercase();

    let url = build_url_from_options(options_f64, "http");

    let mut headers = HashMap::new();

    // Extract headers sub-object
    let obj_bits = options_f64.to_bits();
    let upper = obj_bits >> 48;
    let obj_ptr = if upper >= 0x7FF8 {
        (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ObjectHeader
    } else if upper == 0 && obj_bits >= 0x10000 {
        obj_bits as *const perry_runtime::ObjectHeader
    } else {
        std::ptr::null()
    };

    if !obj_ptr.is_null() {
        let headers_key = js_string_from_bytes("headers".as_ptr(), 7);
        let headers_val = js_object_get_field_by_name(obj_ptr, headers_key);
        if !headers_val.is_undefined() && !headers_val.is_null() {
            let headers_f64 = f64::from_bits(headers_val.bits());
            headers = extract_headers_from_object(headers_f64);
        }
    }

    let timeout_ms = get_object_number_field(options_f64, "timeout").map(|n| n as u64);

    register_handle(ClientRequestHandle {
        method,
        url,
        headers,
        body: Vec::new(),
        response_callback: callback_i64,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
    })
}

/// https.request(options, callback) -> ClientRequest handle
/// Same as http.request but defaults to https protocol
#[no_mangle]
pub unsafe extern "C" fn js_https_request(options_f64: f64, callback_i64: i64) -> Handle {
    ensure_gc_scanner_registered();
    let method = get_object_string_field(options_f64, "method")
        .unwrap_or_else(|| "GET".to_string())
        .to_uppercase();

    let url = build_url_from_options(options_f64, "https");

    let mut headers = HashMap::new();

    let obj_bits = options_f64.to_bits();
    let upper = obj_bits >> 48;
    let obj_ptr = if upper >= 0x7FF8 {
        (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ObjectHeader
    } else if upper == 0 && obj_bits >= 0x10000 {
        obj_bits as *const perry_runtime::ObjectHeader
    } else {
        std::ptr::null()
    };

    if !obj_ptr.is_null() {
        let headers_key = js_string_from_bytes("headers".as_ptr(), 7);
        let headers_val = js_object_get_field_by_name(obj_ptr, headers_key);
        if !headers_val.is_undefined() && !headers_val.is_null() {
            let headers_f64 = f64::from_bits(headers_val.bits());
            headers = extract_headers_from_object(headers_f64);
        }
    }

    let timeout_ms = get_object_number_field(options_f64, "timeout").map(|n| n as u64);

    register_handle(ClientRequestHandle {
        method,
        url,
        headers,
        body: Vec::new(),
        response_callback: callback_i64,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
    })
}

/// http.get(url_or_options, callback) -> ClientRequest handle
/// Convenience method: sets method to GET and auto-calls end()
///
/// First arg can be a string URL or an options object
#[no_mangle]
pub unsafe extern "C" fn js_http_get(url_or_options_f64: f64, callback_i64: i64) -> Handle {
    ensure_gc_scanner_registered();
    let (url, headers, timeout_ms) = if is_string_value(url_or_options_f64) {
        let url = extract_string_value(url_or_options_f64).unwrap_or_default();
        (url, HashMap::new(), None)
    } else {
        // Options object
        let url = build_url_from_options(url_or_options_f64, "http");
        let mut headers = HashMap::new();

        let obj_bits = url_or_options_f64.to_bits();
        let upper = obj_bits >> 48;
        let obj_ptr = if upper >= 0x7FF8 {
            (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ObjectHeader
        } else if upper == 0 && obj_bits >= 0x10000 {
            obj_bits as *const perry_runtime::ObjectHeader
        } else {
            std::ptr::null()
        };

        if !obj_ptr.is_null() {
            let headers_key = js_string_from_bytes("headers".as_ptr(), 7);
            let headers_val = js_object_get_field_by_name(obj_ptr, headers_key);
            if !headers_val.is_undefined() && !headers_val.is_null() {
                let headers_f64 = f64::from_bits(headers_val.bits());
                headers = extract_headers_from_object(headers_f64);
            }
        }

        let timeout_ms = get_object_number_field(url_or_options_f64, "timeout").map(|n| n as u64);

        (url, headers, timeout_ms)
    };

    let handle = register_handle(ClientRequestHandle {
        method: "GET".to_string(),
        url,
        headers,
        body: Vec::new(),
        response_callback: callback_i64,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
    });

    // GET auto-calls end()
    js_http_client_request_end(handle, f64::from_bits(JSValue::undefined().bits()));

    handle
}

/// https.get(url_or_options, callback) -> ClientRequest handle
/// Same as http.get but defaults to https
#[no_mangle]
pub unsafe extern "C" fn js_https_get(url_or_options_f64: f64, callback_i64: i64) -> Handle {
    ensure_gc_scanner_registered();
    let (url, headers, timeout_ms) = if is_string_value(url_or_options_f64) {
        let url = extract_string_value(url_or_options_f64).unwrap_or_default();
        // If URL doesn't start with https://, prepend it
        let url = if url.starts_with("http://") || url.starts_with("https://") {
            url
        } else {
            format!("https://{}", url)
        };
        (url, HashMap::new(), None)
    } else {
        let url = build_url_from_options(url_or_options_f64, "https");
        let mut headers = HashMap::new();

        let obj_bits = url_or_options_f64.to_bits();
        let upper = obj_bits >> 48;
        let obj_ptr = if upper >= 0x7FF8 {
            (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ObjectHeader
        } else if upper == 0 && obj_bits >= 0x10000 {
            obj_bits as *const perry_runtime::ObjectHeader
        } else {
            std::ptr::null()
        };

        if !obj_ptr.is_null() {
            let headers_key = js_string_from_bytes("headers".as_ptr(), 7);
            let headers_val = js_object_get_field_by_name(obj_ptr, headers_key);
            if !headers_val.is_undefined() && !headers_val.is_null() {
                let headers_f64 = f64::from_bits(headers_val.bits());
                headers = extract_headers_from_object(headers_f64);
            }
        }

        let timeout_ms = get_object_number_field(url_or_options_f64, "timeout").map(|n| n as u64);

        (url, headers, timeout_ms)
    };

    let handle = register_handle(ClientRequestHandle {
        method: "GET".to_string(),
        url,
        headers,
        body: Vec::new(),
        response_callback: callback_i64,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
    });

    // GET auto-calls end()
    js_http_client_request_end(handle, f64::from_bits(JSValue::undefined().bits()));

    handle
}

/// ClientRequest.write(body) — append data to request body
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_write(handle: Handle, body_f64: f64) -> Handle {
    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        if let Some(body_str) = extract_string_value(body_f64) {
            req.body.extend_from_slice(body_str.as_bytes());
        }
    }
    handle
}

/// ClientRequest.end(body?) — finalize request and send it
/// Optional body parameter is appended before sending.
/// Spawns async reqwest request and queues response for main thread processing.
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_end(handle: Handle, body_f64: f64) -> Handle {
    // Append optional body
    if let Some(body_str) = extract_string_value(body_f64) {
        if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
            req.body.extend_from_slice(body_str.as_bytes());
        }
    }

    // Extract request data for async task
    let (method, url, headers, body, timeout_ms) = {
        let req = match get_handle_mut::<ClientRequestHandle>(handle) {
            Some(r) => r,
            None => return handle,
        };
        if req.ended {
            return handle; // Already sent
        }
        req.ended = true;
        (
            req.method.clone(),
            req.url.clone(),
            req.headers.clone(),
            req.body.clone(),
            req.timeout_ms,
        )
    };

    // Spawn async HTTP request
    let req_handle = handle;
    spawn(async move {
        let client = reqwest::Client::builder();
        let client = if let Some(timeout) = timeout_ms {
            client.timeout(std::time::Duration::from_millis(timeout))
        } else {
            client.timeout(std::time::Duration::from_secs(30))
        };
        let client = match client.build() {
            Ok(c) => c,
            Err(e) => {
                push_http_event(PendingHttpEvent::Error {
                    request_handle: req_handle,
                    error_message: format!("Failed to create HTTP client: {}", e),
                });
                return;
            }
        };

        let mut request = match method.as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            "HEAD" => client.head(&url),
            "OPTIONS" => client.request(reqwest::Method::OPTIONS, &url),
            _ => client.get(&url),
        };

        // Add headers
        for (key, value) in &headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Add body if non-empty
        if !body.is_empty() {
            request = request.body(body);
        }

        match request.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_message = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();

                let mut resp_headers = Vec::new();
                for (key, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        resp_headers.push((key.to_string(), v.to_string()));
                    }
                }

                let body = response.bytes().await.unwrap_or_default().to_vec();

                push_http_event(PendingHttpEvent::Response {
                    request_handle: req_handle,
                    status,
                    status_message,
                    headers: resp_headers,
                    body,
                });
            }
            Err(e) => {
                push_http_event(PendingHttpEvent::Error {
                    request_handle: req_handle,
                    error_message: format!("{}", e),
                });
            }
        }
    });

    handle
}

/// ClientRequest/IncomingMessage .on(event, callback) — register event listener
/// Works for both ClientRequest ('error') and IncomingMessage ('data', 'end', 'error')
#[no_mangle]
pub unsafe extern "C" fn js_http_on(
    handle: Handle,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> Handle {
    ensure_gc_scanner_registered();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => return handle,
    };

    if callback_ptr == 0 {
        return handle;
    }

    // Try ClientRequest first
    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        req.listeners
            .entry(event_name)
            .or_insert_with(Vec::new)
            .push(callback_ptr);
        return handle;
    }

    // Try IncomingMessage
    if let Some(res) = get_handle_mut::<IncomingMessageHandle>(handle) {
        res.listeners
            .entry(event_name)
            .or_insert_with(Vec::new)
            .push(callback_ptr);
        return handle;
    }

    handle
}

/// ClientRequest.setHeader(name, value) — set a request header
#[no_mangle]
pub unsafe extern "C" fn js_http_set_header(
    handle: Handle,
    name_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> Handle {
    let name = match string_from_header(name_ptr) {
        Some(n) => n,
        None => return handle,
    };
    let value = match string_from_header(value_ptr) {
        Some(v) => v,
        None => return handle,
    };

    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        req.headers.insert(name, value);
    }

    handle
}

/// ClientRequest.setTimeout(ms) — set request timeout
#[no_mangle]
pub unsafe extern "C" fn js_http_set_timeout(handle: Handle, ms: f64) -> Handle {
    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        req.timeout_ms = Some(ms as u64);
    }
    handle
}

/// IncomingMessage.statusCode — get response status code
#[no_mangle]
pub extern "C" fn js_http_status_code(handle: Handle) -> f64 {
    if let Some(res) = get_handle_mut::<IncomingMessageHandle>(handle) {
        return res.status_code as f64;
    }
    0.0
}

/// IncomingMessage.statusMessage — get response status message
#[no_mangle]
pub extern "C" fn js_http_status_message(handle: Handle) -> *mut StringHeader {
    if let Some(res) = get_handle_mut::<IncomingMessageHandle>(handle) {
        return js_string_from_bytes(res.status_message.as_ptr(), res.status_message.len() as u32);
    }
    js_string_from_bytes("".as_ptr(), 0)
}

/// IncomingMessage.headers — get response headers as a JS object
/// Returns a NaN-boxed object pointer (f64)
#[no_mangle]
pub unsafe extern "C" fn js_http_response_headers(handle: Handle) -> f64 {
    if let Some(res) = get_handle_mut::<IncomingMessageHandle>(handle) {
        // Build a JS object with the headers
        let obj = perry_runtime::js_object_alloc(0, res.headers.len() as u32);
        let keys_arr = perry_runtime::js_array_alloc(res.headers.len() as u32);

        for (idx, (key, val)) in res.headers.iter().enumerate() {
            let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
            perry_runtime::js_array_push(keys_arr, JSValue::string_ptr(key_ptr));
            let val_ptr = js_string_from_bytes(val.as_ptr(), val.len() as u32);
            perry_runtime::js_object_set_field(obj, idx as u32, JSValue::string_ptr(val_ptr));
        }
        perry_runtime::js_object_set_keys(obj, keys_arr);

        return f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits());
    }

    f64::from_bits(JSValue::undefined().bits())
}

/// Process pending HTTP events on the main thread.
/// Called from js_stdlib_process_pending().
/// Returns number of events processed.
///
/// #1114 followup: same per-tick scratch-Vec discipline as the fastify
/// (e538caa7), net, and ws pumps. Called every event-loop iteration +
/// every inline `await` poll iteration; the original
/// `Vec::drain(..).collect()` was a per-call heap alloc that contributed
/// to the GC `madvise` churn under sustained HTTP client traffic.
#[no_mangle]
pub unsafe extern "C" fn js_http_process_pending() -> i32 {
    thread_local! {
        static SCRATCH: std::cell::RefCell<Vec<PendingHttpEvent>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let mut events = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
    events.clear();
    {
        let mut guard = HTTP_PENDING_EVENTS.lock().unwrap();
        events.append(&mut *guard);
    }

    let count = events.len() as i32;

    for event in events.drain(..) {
        match event {
            PendingHttpEvent::Response {
                request_handle,
                status,
                status_message,
                headers,
                body,
            } => {
                // Get the response callback and error listeners from the ClientRequest
                let (response_callback, _error_listeners) = {
                    match get_handle_mut::<ClientRequestHandle>(request_handle) {
                        Some(req) => (
                            req.response_callback,
                            req.listeners.get("error").cloned().unwrap_or_default(),
                        ),
                        None => continue,
                    }
                };

                // Create IncomingMessage handle
                let mut headers_map = HashMap::new();
                for (k, v) in headers {
                    headers_map.insert(k, v);
                }

                let body_clone = body.clone();

                let incoming_handle = register_handle(IncomingMessageHandle {
                    status_code: status,
                    status_message,
                    headers: headers_map,
                    body,
                    listeners: HashMap::new(),
                });

                // Call the response callback with the IncomingMessage handle
                // The handle must be NaN-boxed with POINTER_TAG so the closure
                // parameter extraction (js_nanbox_get_pointer) can extract it
                if response_callback != 0 {
                    let closure_ptr = response_callback as *const ClosureHeader;
                    let handle_f64 = f64::from_bits(
                        0x7FFD_0000_0000_0000u64 | (incoming_handle as u64 & 0x0000_FFFF_FFFF_FFFF),
                    );
                    js_closure_call1(closure_ptr, handle_f64);
                }

                // After the response callback has returned, data/end listeners
                // should be registered on the IncomingMessage. Fire them now.

                // Fire 'data' event with the full body as a single chunk
                let data_listeners: Vec<i64> = {
                    match get_handle_mut::<IncomingMessageHandle>(incoming_handle) {
                        Some(res) => res.listeners.get("data").cloned().unwrap_or_default(),
                        None => Vec::new(),
                    }
                };

                if !data_listeners.is_empty() && !body_clone.is_empty() {
                    // Create a NaN-boxed string from the body
                    let body_str =
                        js_string_from_bytes(body_clone.as_ptr(), body_clone.len() as u32);
                    let body_f64 = f64::from_bits(
                        0x7FFF_0000_0000_0000u64 | (body_str as u64 & 0x0000_FFFF_FFFF_FFFF),
                    );

                    for cb in data_listeners {
                        if cb != 0 {
                            let closure = cb as *const ClosureHeader;
                            js_closure_call1(closure, body_f64);
                        }
                    }
                }

                // Fire 'end' event
                let end_listeners: Vec<i64> = {
                    match get_handle_mut::<IncomingMessageHandle>(incoming_handle) {
                        Some(res) => res.listeners.get("end").cloned().unwrap_or_default(),
                        None => Vec::new(),
                    }
                };

                for cb in end_listeners {
                    if cb != 0 {
                        let closure = cb as *const ClosureHeader;
                        js_closure_call0(closure);
                    }
                }
            }

            PendingHttpEvent::Error {
                request_handle,
                error_message,
            } => {
                // Get 'error' listeners from the ClientRequest
                let error_listeners: Vec<i64> = {
                    match get_handle_mut::<ClientRequestHandle>(request_handle) {
                        Some(req) => req.listeners.get("error").cloned().unwrap_or_default(),
                        None => Vec::new(),
                    }
                };

                if !error_listeners.is_empty() {
                    // Create error string as NaN-boxed value
                    let err_str =
                        js_string_from_bytes(error_message.as_ptr(), error_message.len() as u32);
                    let err_f64 = f64::from_bits(
                        0x7FFF_0000_0000_0000u64 | (err_str as u64 & 0x0000_FFFF_FFFF_FFFF),
                    );

                    for cb in error_listeners {
                        if cb != 0 {
                            let closure = cb as *const ClosureHeader;
                            js_closure_call1(closure, err_f64);
                        }
                    }
                }
            }
        }
    }

    // Restore the (capacity-retaining) buffer to the thread-local so the
    // next tick reuses it. A re-entrant pump call during dispatch may
    // have left a grown buffer in the slot — keep whichever is larger.
    SCRATCH.with(|s| {
        let mut slot = s.borrow_mut();
        if events.capacity() >= slot.capacity() {
            *slot = events;
        }
    });

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scanner_emits_request_and_response_listeners() {
        let mut req_listeners = HashMap::new();
        req_listeners.insert("error".to_string(), vec![0x1234_5678]);
        let req_handle = register_handle(ClientRequestHandle {
            method: "GET".to_string(),
            url: "http://example.test".to_string(),
            headers: HashMap::new(),
            body: Vec::new(),
            response_callback: 0x2345_6780,
            listeners: req_listeners,
            timeout_ms: None,
            ended: false,
        });

        let mut msg_listeners = HashMap::new();
        msg_listeners.insert("data".to_string(), vec![0x3456_7890]);
        let msg_handle = register_handle(IncomingMessageHandle {
            status_code: 200,
            status_message: "OK".to_string(),
            headers: HashMap::new(),
            body: Vec::new(),
            listeners: msg_listeners,
        });

        let mut emitted = Vec::new();
        scan_http_roots(&mut |value| emitted.push(value.to_bits()));

        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x1234_5678)));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x3456_7890)));
        crate::common::drop_handle(req_handle);
        crate::common::drop_handle(msg_handle);
    }
}
