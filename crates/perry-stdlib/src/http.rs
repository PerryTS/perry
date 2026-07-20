//! HTTP/HTTPS client module (Node.js http/https compatible)
//!
//! Native implementation of Node.js http.request(), http.get(), https.request(), https.get()
//! using reqwest. Provides callback-based API matching the Node.js pattern used by SDKs
//! like twitter-api-v2, rss-parser, web-push, etc.
//!
//! Both http and https share this implementation — reqwest handles TLS based on URL scheme.

use perry_runtime::{
    js_array_get_jsvalue, js_array_length, js_closure_call0, js_closure_call1,
    js_object_get_field_by_name, js_object_keys, js_string_from_bytes, ArrayHeader, ClosureHeader,
    JSValue, StringHeader,
};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::common::async_bridge::spawn;
use crate::common::{for_each_handle_mut_of, get_handle_mut, register_handle, Handle};

mod client_request_surface;
pub(crate) use client_request_surface::{
    dispatch_client_request_method, dispatch_client_request_property,
};
mod agent_dispatch;
pub(crate) use agent_dispatch::{
    dispatch_agent_method, dispatch_agent_property, dispatch_agent_property_set,
};
#[cfg(feature = "external-http-client-pump")]
mod external_client_request;

extern "C" {
    fn js_value_is_closure(value_bits: i64) -> i32;
    fn js_class_method_bind(
        instance: f64,
        method_name_ptr: *const u8,
        method_name_len: usize,
    ) -> f64;
}

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

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

    // #2154: stored `agent.createConnection` / `.createSocket` closure
    // pointers. Skip the 0-slot to avoid emitting an invalid root for
    // agents that haven't had an override assigned.
    for_each_handle_mut_of::<AgentHandle, _>(|agent| {
        if agent.create_connection != 0 {
            visitor.visit_i64_slot(&mut agent.create_connection);
        }
        if agent.create_socket != 0 {
            visitor.visit_i64_slot(&mut agent.create_socket);
        }
    });

    client_request_surface::scan_roots(visitor);
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
    /// `options.agent` handle (#2154). When non-zero, dispatch reads the
    /// Agent's `keepAlive` / `maxFreeSockets` / `keepAliveMsecs` and
    /// folds them into the per-request reqwest::ClientBuilder config so
    /// pool-related Agent options are honored instead of ignored.
    agent_handle: Handle,
}

/// Agent handle — Node's `http.Agent` / `https.Agent`. Perry's
/// `http.request` honors the Agent for its connection-pool config
/// (#2154); the rest of the fields are still pure metadata mirrored
/// from Node's defaults so `getName(options)` and the property
/// accessors agree byte-for-byte with Node's `lib/_http_agent.js`.
///
/// Trackers: #2129 (initial constructor + getName), #2154 (validation
/// + per-agent client + socket-counter accessors + setters).
pub struct AgentHandle {
    /// `https.Agent` defaults to `"https:"`, `http.Agent` to `"http:"`.
    /// `null` is a legitimate value (some tests set it explicitly).
    pub protocol: Option<String>,
    pub keep_alive: bool,
    pub keep_alive_msecs: f64,
    pub max_sockets: f64,
    pub max_total_sockets: f64,
    pub max_free_sockets: f64,
    pub scheduling: String,
    pub timeout_ms: Option<f64>,
    /// `agent.destroy()` flips this so the `destroyed` accessor mirrors
    /// Node's getter (#2154).
    pub destroyed: bool,
    /// User-supplied `createConnection` override closure pointer (#2154).
    /// Storage + GC-rooting only today — full happy-path invocation
    /// needs net.Socket-shaped JS objects and is tracked separately.
    pub create_connection: i64,
    pub create_socket: i64,
}

impl Default for AgentHandle {
    fn default() -> Self {
        AgentHandle {
            protocol: Some("http:".to_string()),
            keep_alive: false,
            keep_alive_msecs: 1000.0,
            max_sockets: f64::INFINITY,
            max_total_sockets: f64::INFINITY,
            max_free_sockets: 256.0,
            scheduling: "lifo".to_string(),
            timeout_ms: None,
            destroyed: false,
            create_connection: 0,
            create_socket: 0,
        }
    }
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
    /// Encoding requested through `res.setEncoding(enc)`.
    pub encoding: Option<String>,
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

/// Helper to fetch a raw NaN-boxed field value from a JS object by name.
/// Returns None when the receiver is not a pointer-like value; returns
/// `Some(JSValue::undefined())` when the field is absent (matches the
/// underlying `js_object_get_field_by_name` behavior for `obj.missing`).
unsafe fn get_object_field_raw(obj_f64: f64, field_name: &str) -> Option<JSValue> {
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
    Some(js_object_get_field_by_name(obj_ptr, key_str))
}

/// Returns true iff the JS value is "truthy" enough that Node's
/// `name += options.field` branch fires (i.e. `if (options.field)`):
/// not undefined, not null, not the empty string, not 0, not false.
fn jsvalue_is_truthy(v: JSValue) -> bool {
    if v.is_undefined() || v.is_null() {
        return false;
    }
    if v.is_bool() {
        return v.as_bool();
    }
    if v.is_int32() {
        return v.as_int32() != 0;
    }
    if v.is_number() {
        let n = v.as_number();
        return n != 0.0 && !n.is_nan();
    }
    if v.is_string() || v.is_short_string() {
        let s_ptr = perry_runtime::value::js_get_string_pointer_unified(f64::from_bits(v.bits()));
        if s_ptr == 0 {
            return false;
        }
        let header = s_ptr as *const StringHeader;
        unsafe { (*header).byte_len > 0 }
    } else {
        // Other pointer values (objects, arrays, buffers) are always truthy
        // in JS.
        true
    }
}

/// Coerce a JS value to its string representation, matching how
/// `name += options.field` does ToString in JS. Strings/numbers/bools
/// flow through directly; arrays comma-join; buffers stringify their
/// content; objects fall back to "[object Object]".
unsafe fn jsvalue_to_string(v: JSValue) -> String {
    let header = perry_runtime::value::js_jsvalue_to_string(f64::from_bits(v.bits()));
    string_from_header(header).unwrap_or_default()
}

/// `JSON.stringify(v)` as a Rust `String`. Used by https.Agent.getName
/// for the `sigalgs` field, which Node serializes as JSON.
unsafe fn jsvalue_to_json_string(v: JSValue) -> String {
    let header = perry_runtime::json::js_json_stringify(f64::from_bits(v.bits()), 0);
    string_from_header(header).unwrap_or_default()
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
// Agent extraction (used by http.request / https.request / http.get)
// ========================================================================

/// Extract `options.agent` from a NaN-boxed options object. Returns 0
/// when the field is missing, not a pointer, or doesn't resolve to an
/// AgentHandle. #2154.
unsafe fn extract_agent_handle(options_f64: f64) -> Handle {
    let obj_bits = options_f64.to_bits();
    let upper = obj_bits >> 48;
    let obj_ptr = if upper >= 0x7FF8 {
        (obj_bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ObjectHeader
    } else if upper == 0 && obj_bits >= 0x10000 {
        obj_bits as *const perry_runtime::ObjectHeader
    } else {
        return 0;
    };
    if obj_ptr.is_null() {
        return 0;
    }
    let key = js_string_from_bytes("agent".as_ptr(), 5);
    let val = js_object_get_field_by_name(obj_ptr, key);
    if !val.is_pointer() {
        return 0;
    }
    let candidate = (val.bits() & 0x0000_FFFF_FFFF_FFFF) as Handle;
    if get_handle_mut::<AgentHandle>(candidate).is_some() {
        candidate
    } else {
        0
    }
}

fn normalize_url(raw: String, default_protocol: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw
    } else if raw.is_empty() {
        String::new()
    } else {
        format!("{}://{}", default_protocol, raw)
    }
}

unsafe fn url_from_js_value(value: f64, default_protocol: &str) -> String {
    if is_string_value(value) {
        return normalize_url(
            extract_string_value(value).unwrap_or_default(),
            default_protocol,
        );
    }
    if let Some(href) = get_object_string_field(value, "href") {
        return normalize_url(href, default_protocol);
    }
    build_url_from_options(value, default_protocol)
}

#[derive(Default)]
struct RequestOverload {
    primary: Option<f64>,
    options: Option<f64>,
    callback: i64,
}

unsafe fn parse_request_overload(args_array: i64) -> RequestOverload {
    let mut out = RequestOverload::default();
    let arr_ptr = args_array as *const ArrayHeader;
    if arr_ptr.is_null() || (args_array as u64) >> 48 != 0 {
        return out;
    }
    let len = (*arr_ptr).length as usize;
    let elements = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    for i in 0..len {
        let bits = *elements.add(i);
        if js_value_is_closure(bits as i64) != 0 {
            out.callback = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
            continue;
        }
        let value = f64::from_bits(bits);
        if out.primary.is_none() {
            out.primary = Some(value);
        } else if out.options.is_none() {
            out.options = Some(value);
        }
    }
    out
}

unsafe fn request_parts_from_options(
    primary: f64,
    options: f64,
    default_protocol: &str,
    _auto_end: bool,
) -> (String, String, HashMap<String, String>, Option<u64>, Handle) {
    let method = get_object_string_field(options, "method")
        .unwrap_or_else(|| "GET".to_string())
        .to_uppercase();
    let url = url_from_js_value(primary, default_protocol);
    let mut headers = HashMap::new();
    if let Some(headers_val) = get_object_field_raw(options, "headers") {
        if !headers_val.is_undefined() && !headers_val.is_null() {
            headers = extract_headers_from_object(f64::from_bits(headers_val.bits()));
        }
    }
    let timeout_ms = get_object_number_field(options, "timeout").map(|n| n as u64);
    let agent_handle = extract_agent_handle(options);
    (method, url, headers, timeout_ms, agent_handle)
}

unsafe fn build_request_from_overload(
    overload: RequestOverload,
    default_protocol: &str,
    force_get: bool,
) -> Handle {
    ensure_gc_scanner_registered();
    let undefined = f64::from_bits(JSValue::undefined().bits());
    let primary = overload.primary.unwrap_or(undefined);
    let options = overload.options.unwrap_or(primary);
    let (method, url, headers, timeout_ms, agent_handle) =
        request_parts_from_options(primary, options, default_protocol, force_get);
    let handle = register_handle(ClientRequestHandle {
        method,
        url,
        headers,
        body: Vec::new(),
        response_callback: overload.callback,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
        agent_handle,
    });
    if force_get {
        js_http_client_request_end(handle, undefined);
    }
    handle
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
    let agent_handle = extract_agent_handle(options_f64);

    register_handle(ClientRequestHandle {
        method,
        url,
        headers,
        body: Vec::new(),
        response_callback: callback_i64,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
        agent_handle,
    })
}

/// `new http.ClientRequest(options)` (#4904). Perry's client model defers
/// the actual send to `.end()`, so constructing is exactly `http.request`
/// without a response callback. Node coerces a falsy `options.method` to
/// `GET` — mirror that here (`http.request` keeps whatever string it got).
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_standalone_new(options_f64: f64) -> Handle {
    let handle = js_http_request(options_f64, 0);
    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        if req.method.is_empty() {
            req.method = "GET".to_string();
        }
    }
    handle
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
    let agent_handle = extract_agent_handle(options_f64);

    register_handle(ClientRequestHandle {
        method,
        url,
        headers,
        body: Vec::new(),
        response_callback: callback_i64,
        listeners: HashMap::new(),
        timeout_ms,
        ended: false,
        agent_handle,
    })
}

#[no_mangle]
pub unsafe extern "C" fn js_https_request_variadic(args_array: i64) -> Handle {
    build_request_from_overload(parse_request_overload(args_array), "https", false)
}

/// http.get(url_or_options, callback) -> ClientRequest handle
/// Convenience method: sets method to GET and auto-calls end()
///
/// First arg can be a string URL or an options object
#[no_mangle]
pub unsafe extern "C" fn js_http_get(url_or_options_f64: f64, callback_i64: i64) -> Handle {
    ensure_gc_scanner_registered();
    let (url, headers, timeout_ms, agent_handle) = if is_string_value(url_or_options_f64) {
        let url = extract_string_value(url_or_options_f64).unwrap_or_default();
        (url, HashMap::new(), None, 0)
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
        let agent_handle = extract_agent_handle(url_or_options_f64);

        (url, headers, timeout_ms, agent_handle)
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
        agent_handle,
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
    let (url, headers, timeout_ms, agent_handle) = if is_string_value(url_or_options_f64) {
        let url = extract_string_value(url_or_options_f64).unwrap_or_default();
        // If URL doesn't start with https://, prepend it
        let url = if url.starts_with("http://") || url.starts_with("https://") {
            url
        } else {
            format!("https://{}", url)
        };
        (url, HashMap::new(), None, 0)
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
        let agent_handle = extract_agent_handle(url_or_options_f64);

        (url, headers, timeout_ms, agent_handle)
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
        agent_handle,
    });

    // GET auto-calls end()
    js_http_client_request_end(handle, f64::from_bits(JSValue::undefined().bits()));

    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_https_get_variadic(args_array: i64) -> Handle {
    build_request_from_overload(parse_request_overload(args_array), "https", true)
}

/// ClientRequest.write(body) — append data to request body
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_write(handle: Handle, body_f64: f64) -> Handle {
    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        if let Some(body_str) = extract_string_value(body_f64) {
            req.body.extend_from_slice(body_str.as_bytes());
        }
        return handle;
    }

    #[cfg(feature = "external-http-client-pump")]
    {
        let _ = unsafe { external_client_request::dispatch_method(handle, "write", &[body_f64]) };
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
    let (method, url, headers, body, timeout_ms, agent_pool) = {
        let req = match get_handle_mut::<ClientRequestHandle>(handle) {
            Some(r) => r,
            None => {
                #[cfg(feature = "external-http-client-pump")]
                {
                    let _ = unsafe {
                        external_client_request::dispatch_method(handle, "end", &[body_f64])
                    };
                }
                return handle;
            }
        };
        if req.ended {
            return handle; // Already sent
        }
        req.ended = true;
        // #2154: pull the Agent's pool config out NOW (still on the main
        // thread; tokio worker can't safely touch the handle registry).
        // `(keep_alive, max_free_sockets, keep_alive_msecs)` — None when
        // the caller didn't pass `options.agent`, in which case we
        // build a vanilla reqwest::Client below.
        let agent_pool = if req.agent_handle != 0 {
            get_handle_mut::<AgentHandle>(req.agent_handle)
                .map(|a| (a.keep_alive, a.max_free_sockets, a.keep_alive_msecs))
        } else {
            None
        };
        (
            req.method.clone(),
            req.url.clone(),
            req.headers.clone(),
            req.body.clone(),
            req.timeout_ms,
            agent_pool,
        )
    };

    // Spawn async HTTP request
    let req_handle = handle;
    spawn(async move {
        let mut builder = reqwest::Client::builder();
        // Node's http client never follows redirects; disable reqwest's default (rationale: `apply_node_proxy_policy` in `perry-ext-http`).
        builder = builder.redirect(reqwest::redirect::Policy::none());
        builder = if let Some(timeout) = timeout_ms {
            builder.timeout(std::time::Duration::from_millis(timeout))
        } else {
            builder.timeout(std::time::Duration::from_secs(30))
        };
        // #2154: honor Agent pool config when one is supplied. Without
        // an Agent we keep the prior vanilla builder (no idle pool
        // override) — Perry's stdlib http path historically created a
        // fresh Client per request and we don't want to silently
        // change that for code that doesn't opt in via options.agent.
        if let Some((keep_alive, max_free_sockets, keep_alive_msecs)) = agent_pool {
            let pool_max_idle = if keep_alive {
                if !max_free_sockets.is_finite() || max_free_sockets > usize::MAX as f64 {
                    256
                } else {
                    max_free_sockets.max(1.0) as usize
                }
            } else {
                0
            };
            let idle_timeout = if keep_alive {
                let ms = if keep_alive_msecs.is_finite() && keep_alive_msecs > 0.0 {
                    keep_alive_msecs
                } else {
                    1000.0
                };
                std::time::Duration::from_millis(ms as u64)
            } else {
                std::time::Duration::from_millis(0)
            };
            builder = builder
                .pool_max_idle_per_host(pool_max_idle)
                .pool_idle_timeout(idle_timeout);
        }
        let client = match builder.build() {
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

    if client_request_surface::is_client_request_handle(handle) {
        client_request_surface::set_header(handle, &name, value);
        return handle;
    }

    #[cfg(feature = "external-http-client-pump")]
    {
        let name_value = f64::from_bits(0x7FFF_0000_0000_0000u64 | (name_ptr as u64 & PTR_MASK));
        let value_value = f64::from_bits(0x7FFF_0000_0000_0000u64 | (value_ptr as u64 & PTR_MASK));
        let _ = unsafe {
            external_client_request::dispatch_method(
                handle,
                "setHeader",
                &[name_value, value_value],
            )
        };
    }

    handle
}

/// ClientRequest.setTimeout(ms) — set request timeout
#[no_mangle]
pub unsafe extern "C" fn js_http_set_timeout(handle: Handle, ms: f64) -> Handle {
    if let Some(req) = get_handle_mut::<ClientRequestHandle>(handle) {
        req.timeout_ms = Some(ms as u64);
        return handle;
    }

    #[cfg(feature = "external-http-client-pump")]
    {
        let _ = unsafe { external_client_request::dispatch_method(handle, "setTimeout", &[ms]) };
    }
    handle
}

/// IncomingMessage.setEncoding(encoding) — store the requested text encoding
/// for response data events and return the receiver for chaining.
#[no_mangle]
pub unsafe extern "C" fn js_http_incoming_message_set_encoding(
    handle: Handle,
    encoding_ptr: *const StringHeader,
) -> Handle {
    let encoding = string_from_header(encoding_ptr).unwrap_or_else(|| "utf8".to_string());
    let mut matched = false;
    if let Some(res) = get_handle_mut::<IncomingMessageHandle>(handle) {
        res.encoding = Some(encoding);
        matched = true;
    }
    if matched {
        return handle;
    }

    #[cfg(feature = "external-http-client-pump")]
    {
        extern "C" {
            fn js_ext_http_client_incoming_message_is_handle(handle: i64) -> i32;
            fn js_ext_http_client_incoming_message_set_encoding(
                handle: i64,
                encoding_ptr: *const StringHeader,
            ) -> i64;
        }
        if js_ext_http_client_incoming_message_is_handle(handle) != 0 {
            js_ext_http_client_incoming_message_set_encoding(handle, encoding_ptr);
            return handle;
        }
    }

    #[cfg(feature = "external-http-server-pump")]
    {
        extern "C" {
            fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
            fn js_node_http_im_set_encoding(handle: i64, encoding_ptr: *const StringHeader) -> i64;
        }
        if js_ext_http_incoming_message_is_handle(handle) != 0 {
            js_node_http_im_set_encoding(handle, encoding_ptr);
        }
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_http_client_request_method(handle: Handle) -> *mut StringHeader {
    let method = match get_handle_mut::<ClientRequestHandle>(handle) {
        Some(req) => req.method.clone(),
        None => {
            #[cfg(feature = "external-http-client-pump")]
            if let Some(ptr) = unsafe { external_client_request::string_property(handle, "method") }
            {
                return ptr;
            }
            String::new()
        }
    };
    unsafe { js_string_from_bytes(method.as_ptr(), method.len() as u32) }
}

#[no_mangle]
pub extern "C" fn js_http_client_request_protocol(handle: Handle) -> *mut StringHeader {
    let protocol = match get_handle_mut::<ClientRequestHandle>(handle) {
        Some(req) => reqwest::Url::parse(&req.url)
            .map(|u| format!("{}:", u.scheme()))
            .unwrap_or_default(),
        None => {
            #[cfg(feature = "external-http-client-pump")]
            if let Some(ptr) =
                unsafe { external_client_request::string_property(handle, "protocol") }
            {
                return ptr;
            }
            String::new()
        }
    };
    unsafe { js_string_from_bytes(protocol.as_ptr(), protocol.len() as u32) }
}

#[no_mangle]
pub extern "C" fn js_http_client_request_host(handle: Handle) -> *mut StringHeader {
    let host = match get_handle_mut::<ClientRequestHandle>(handle) {
        Some(req) => reqwest::Url::parse(&req.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_default(),
        None => {
            #[cfg(feature = "external-http-client-pump")]
            if let Some(ptr) = unsafe { external_client_request::string_property(handle, "host") } {
                return ptr;
            }
            String::new()
        }
    };
    unsafe { js_string_from_bytes(host.as_ptr(), host.len() as u32) }
}

#[no_mangle]
pub extern "C" fn js_http_client_request_path(handle: Handle) -> *mut StringHeader {
    let path = match get_handle_mut::<ClientRequestHandle>(handle) {
        Some(req) => reqwest::Url::parse(&req.url)
            .map(|u| {
                let mut path = u.path().to_string();
                if path.is_empty() {
                    path.push('/');
                }
                if let Some(q) = u.query() {
                    path.push('?');
                    path.push_str(q);
                }
                path
            })
            .unwrap_or_default(),
        None => {
            #[cfg(feature = "external-http-client-pump")]
            if let Some(ptr) = unsafe { external_client_request::string_property(handle, "path") } {
                return ptr;
            }
            String::new()
        }
    };
    unsafe { js_string_from_bytes(path.as_ptr(), path.len() as u32) }
}

#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_listener_count(
    handle: Handle,
    event_ptr: *const StringHeader,
) -> f64 {
    let event = match string_from_header(event_ptr) {
        Some(e) => e,
        None => return 0.0,
    };
    match get_handle_mut::<ClientRequestHandle>(handle) {
        Some(req) => {
            let explicit = req.listeners.get(&event).map(|v| v.len()).unwrap_or(0);
            let implicit_response = if event == "response" && req.response_callback != 0 {
                1
            } else {
                0
            };
            (explicit + implicit_response) as f64
        }
        None => {
            #[cfg(feature = "external-http-client-pump")]
            {
                let event_value =
                    f64::from_bits(0x7FFF_0000_0000_0000u64 | (event_ptr as u64 & PTR_MASK));
                if let Some(value) = unsafe {
                    external_client_request::dispatch_method(
                        handle,
                        "listenerCount",
                        &[event_value],
                    )
                } {
                    return value;
                }
            }
            0.0
        }
    }
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

    #[cfg(feature = "external-http-server-pump")]
    {
        extern "C" {
            fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
            fn js_ext_http_incoming_message_dispatch_property(
                handle: i64,
                property_ptr: *const u8,
                property_len: usize,
            ) -> f64;
        }
        if js_ext_http_incoming_message_is_handle(handle) != 0 {
            return js_ext_http_incoming_message_dispatch_property(handle, b"headers".as_ptr(), 7);
        }
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
                    encoding: None,
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

// ========================================================================
// http.Agent / https.Agent (#2129)
// ========================================================================

/// `new http.Agent(options?)` — register a fresh AgentHandle. `options` is
/// either undefined or a NaN-boxed object whose recognized fields override
/// the defaults; unknown fields are ignored (Node behavior).
///
/// Mirrors Node's argument validation for the small set of options whose
/// rejection is observable (`maxTotalSockets` and `maxSockets`: number,
/// finite, > 0). Other options are no-op overrides today because Perry
/// does not pool sockets.
#[no_mangle]
pub unsafe extern "C" fn js_http_agent_new(options_f64: f64) -> Handle {
    js_http_agent_new_with_protocol(options_f64, b"http:".as_ptr(), 5)
}

#[no_mangle]
pub unsafe extern "C" fn js_https_agent_new(options_f64: f64) -> Handle {
    js_http_agent_new_with_protocol(options_f64, b"https:".as_ptr(), 6)
}

/// #2154: throw `RangeError [ERR_OUT_OF_RANGE]` with Node's exact
/// message shape — `The value of "<name>" is out of range. It must be
/// <bound>. Received <received>`. The `assert.throws(..., { code: ... })`
/// path in test-http-agent-maxtotalsockets.js (and adjacent tests)
/// reads the `code` property so we need both the RangeError class and
/// the side-table code registration.
fn throw_agent_out_of_range(name: &str, bound: &str, received: f64) -> ! {
    let received_str = if received.is_nan() {
        "NaN".to_string()
    } else if received.is_infinite() {
        if received.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else if received.fract() == 0.0 && received.abs() < 1e21 {
        format!("{}", received as i64)
    } else {
        format!("{}", received)
    };
    let message = format!(
        "The value of \"{}\" is out of range. It must be {}. Received {}",
        name, bound, received_str
    );
    let msg_ptr = unsafe { js_string_from_bytes(message.as_ptr(), message.len() as u32) };
    perry_runtime::node_submodules::register_error_code_pub(msg_ptr, "ERR_OUT_OF_RANGE");
    let err = perry_runtime::error::js_rangeerror_new(msg_ptr);
    perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
}

fn validate_agent_positive(name: &str, v: f64) {
    // `+Infinity` is the Node default for maxSockets/maxTotalSockets, so
    // accept it explicitly even though `v > 0.0` would also pass — keep
    // the symmetry clear with the ext-http mirror.
    if v.is_infinite() && v.is_sign_positive() {
        return;
    }
    if v.is_nan() || v <= 0.0 {
        throw_agent_out_of_range(name, "> 0", v);
    }
}

unsafe fn js_http_agent_new_with_protocol(
    options_f64: f64,
    default_protocol_ptr: *const u8,
    default_protocol_len: usize,
) -> Handle {
    let default_protocol = std::str::from_utf8(std::slice::from_raw_parts(
        default_protocol_ptr,
        default_protocol_len,
    ))
    .unwrap_or("http:")
    .to_string();

    let mut agent = AgentHandle {
        protocol: Some(default_protocol),
        ..AgentHandle::default()
    };

    let opts_bits = options_f64.to_bits();
    let opts_undef =
        opts_bits == JSValue::undefined().bits() || opts_bits == JSValue::null().bits();

    if !opts_undef {
        if let Some(v) = get_object_number_field(options_f64, "keepAliveMsecs") {
            if v.is_nan() || v < 0.0 {
                throw_agent_out_of_range("keepAliveMsecs", ">= 0", v);
            }
            agent.keep_alive_msecs = v;
        }
        if let Some(v) = get_object_number_field(options_f64, "maxSockets") {
            validate_agent_positive("maxSockets", v);
            agent.max_sockets = v;
        }
        if let Some(v) = get_object_number_field(options_f64, "maxFreeSockets") {
            validate_agent_positive("maxFreeSockets", v);
            agent.max_free_sockets = v;
        }
        if let Some(v) = get_object_number_field(options_f64, "maxTotalSockets") {
            validate_agent_positive("maxTotalSockets", v);
            agent.max_total_sockets = v;
        }
        if let Some(v) = get_object_number_field(options_f64, "timeout") {
            agent.timeout_ms = Some(v);
        }
        if let Some(s) = get_object_string_field(options_f64, "scheduling") {
            agent.scheduling = s;
        }
        // `keepAlive` is a boolean; reuse the object header reader.
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
            let key = js_string_from_bytes("keepAlive".as_ptr(), 9);
            let val = js_object_get_field_by_name(obj_ptr, key);
            if val.is_bool() {
                agent.keep_alive = val.as_bool();
            }
            // #2154: storage for createConnection / createSocket
            // overrides. GC-rooted via `scan_http_roots_mut` below.
            for (slot_field, slot) in [
                ("createConnection", &mut agent.create_connection),
                ("createSocket", &mut agent.create_socket),
            ] {
                let key = js_string_from_bytes(slot_field.as_ptr(), slot_field.len() as u32);
                let val = js_object_get_field_by_name(obj_ptr, key);
                if val.is_pointer() {
                    *slot = (val.bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
                }
            }
        }
    }

    register_handle(agent)
}

/// `agent.getName([options])` — Node's canonical key under which sockets are
/// pooled. The base shape is `${host}:${port}:${localAddress}` with optional
/// `:${family}` and `:${socketPath}` appended. For https.Agent instances
/// 20 extra fields are appended (ca, cert, ciphers, key, …) per Node's
/// `lib/https.js`. Tests assert exact strings; see
/// `test/parallel/test-http-agent-getname.js` and
/// `test/parallel/test-https-agent-getname.js`.
#[no_mangle]
pub unsafe extern "C" fn js_http_agent_get_name(
    handle: Handle,
    options_f64: f64,
) -> *mut StringHeader {
    let is_https = get_handle_mut::<AgentHandle>(handle)
        .and_then(|a| a.protocol.as_deref().map(|p| p == "https:"))
        .unwrap_or(false);

    let mut name = build_http_agent_name(options_f64);
    if is_https {
        append_https_agent_name_fields(&mut name, options_f64);
    }
    js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// Compute the http.Agent.getName portion of the pool key.
unsafe fn build_http_agent_name(options_f64: f64) -> String {
    let opts_bits = options_f64.to_bits();
    let opts_undef =
        opts_bits == JSValue::undefined().bits() || opts_bits == JSValue::null().bits();

    if opts_undef {
        return "localhost::".to_string();
    }

    let host =
        get_object_string_field(options_f64, "host").unwrap_or_else(|| "localhost".to_string());
    let port = get_object_string_field(options_f64, "port").unwrap_or_default();
    let local_address = get_object_string_field(options_f64, "localAddress").unwrap_or_default();

    let mut name = format!("{}:{}:{}", host, port, local_address);

    // Per Node's lib/_http_agent.js: family is appended FIRST (when 4 or 6),
    // then socketPath. Both are independent — Node appends each separately
    // if present.
    if let Some(family) = get_object_number_field(options_f64, "family") {
        let f = family as i64;
        if f == 4 || f == 6 {
            name.push(':');
            name.push_str(&f.to_string());
        }
    }
    if let Some(socket_path) = get_object_string_field(options_f64, "socketPath") {
        name.push(':');
        name.push_str(&socket_path);
    }

    name
}

/// Append the 20 https.Agent.getName extension fields onto an already-built
/// http.Agent.getName prefix. Mirrors `Agent.prototype.getName` in Node's
/// `lib/https.js` (v22.x): every field gets its own `:` separator regardless
/// of whether the value is present, so an Agent with no options produces 20
/// trailing colons.
unsafe fn append_https_agent_name_fields(name: &mut String, options_f64: f64) {
    let opts_bits = options_f64.to_bits();
    let opts_undef =
        opts_bits == JSValue::undefined().bits() || opts_bits == JSValue::null().bits();

    if opts_undef {
        // 20 empty fields → 20 trailing colons (1 separator per field).
        for _ in 0..20 {
            name.push(':');
        }
        return;
    }

    // Most fields use the `if (options.field) name += options.field;` shape
    // — truthy → append ToString-coerced value. A small group
    // (rejectUnauthorized, honorCipherOrder, secureOptions) checks
    // `!== undefined` instead, so `false` and `0` are appended.
    let host_value = get_object_field_raw(options_f64, "host");

    let push_truthy_string = |name: &mut String, field: &str| {
        name.push(':');
        if let Some(v) = get_object_field_raw(options_f64, field) {
            if jsvalue_is_truthy(v) {
                name.push_str(&jsvalue_to_string(v));
            }
        }
    };
    let push_defined = |name: &mut String, field: &str| {
        name.push(':');
        if let Some(v) = get_object_field_raw(options_f64, field) {
            if !v.is_undefined() {
                name.push_str(&jsvalue_to_string(v));
            }
        }
    };

    push_truthy_string(name, "ca");
    push_truthy_string(name, "cert");
    push_truthy_string(name, "clientCertEngine");
    push_truthy_string(name, "ciphers");
    push_truthy_string(name, "key");
    push_truthy_string(name, "pfx");
    push_defined(name, "rejectUnauthorized");

    // servername appears only when defined AND distinct from host.
    name.push(':');
    if let Some(sn) = get_object_field_raw(options_f64, "servername") {
        if jsvalue_is_truthy(sn) {
            let same_as_host = match host_value {
                Some(h) if jsvalue_is_truthy(h) => jsvalue_to_string(h) == jsvalue_to_string(sn),
                _ => false,
            };
            if !same_as_host {
                name.push_str(&jsvalue_to_string(sn));
            }
        }
    }

    push_truthy_string(name, "minVersion");
    push_truthy_string(name, "maxVersion");
    push_truthy_string(name, "secureProtocol");
    push_truthy_string(name, "crl");
    push_defined(name, "honorCipherOrder");
    push_truthy_string(name, "ecdhCurve");
    push_truthy_string(name, "dhparam");
    push_defined(name, "secureOptions");
    push_truthy_string(name, "sessionIdContext");

    // sigalgs is JSON-stringified (Node: `name += JSONStringify(options.sigalgs)`).
    name.push(':');
    if let Some(v) = get_object_field_raw(options_f64, "sigalgs") {
        if jsvalue_is_truthy(v) {
            name.push_str(&jsvalue_to_json_string(v));
        }
    }

    push_truthy_string(name, "privateKeyIdentifier");
    push_truthy_string(name, "privateKeyEngine");
}

/// `agent.keepSocketAlive(socket)` / `agent.reuseSocket(socket, req)` —
/// this Agent flavor exposes no per-socket hooks to act on, so these return
/// the receiver for chainability but otherwise do nothing. Warn once instead
/// of silently succeeding (#4917). (Default builds route http through
/// perry-ext-http, where reqwest owns the keep-alive pool.)
#[no_mangle]
pub extern "C" fn js_http_agent_noop_self(handle: Handle) -> Handle {
    perry_runtime::stub_diag::perry_stub_warn(
        "http.Agent keepSocketAlive/reuseSocket",
        "this http Agent has no per-socket hooks; the call is a no-op",
        Some("#4917"),
    );
    handle
}

/// Property getters — `agent.maxSockets`, `agent.keepAlive`, etc. Return
/// the per-instance value where one was set; fall back to Node defaults
/// when the handle is missing (synthetic agent reads).
#[no_mangle]
pub extern "C" fn js_http_agent_max_sockets(handle: Handle) -> f64 {
    get_handle_mut::<AgentHandle>(handle)
        .map(|a| a.max_sockets)
        .unwrap_or(f64::INFINITY)
}

#[no_mangle]
pub extern "C" fn js_http_agent_max_free_sockets(handle: Handle) -> f64 {
    get_handle_mut::<AgentHandle>(handle)
        .map(|a| a.max_free_sockets)
        .unwrap_or(256.0)
}

#[no_mangle]
pub extern "C" fn js_http_agent_max_total_sockets(handle: Handle) -> f64 {
    get_handle_mut::<AgentHandle>(handle)
        .map(|a| a.max_total_sockets)
        .unwrap_or(f64::INFINITY)
}

#[no_mangle]
pub extern "C" fn js_http_agent_keep_alive_msecs(handle: Handle) -> f64 {
    get_handle_mut::<AgentHandle>(handle)
        .map(|a| a.keep_alive_msecs)
        .unwrap_or(1000.0)
}

#[no_mangle]
pub extern "C" fn js_http_agent_keep_alive(handle: Handle) -> f64 {
    let keep_alive = get_handle_mut::<AgentHandle>(handle)
        .map(|a| a.keep_alive)
        .unwrap_or(false);
    f64::from_bits(JSValue::bool(keep_alive).bits())
}

#[no_mangle]
pub extern "C" fn js_http_agent_protocol(handle: Handle) -> *mut StringHeader {
    let s = get_handle_mut::<AgentHandle>(handle)
        .and_then(|a| a.protocol.clone())
        .unwrap_or_else(|| "http:".to_string());
    unsafe { js_string_from_bytes(s.as_ptr(), s.len() as u32) }
}

#[no_mangle]
pub unsafe extern "C" fn js_http_agent_set_protocol(
    handle: Handle,
    value_ptr: *const StringHeader,
) {
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        if value_ptr.is_null() {
            agent.protocol = None;
        } else if let Some(s) = string_from_header(value_ptr) {
            agent.protocol = Some(s);
        }
    }
}

// #2154: validating setters for the tunable Agent properties. Node lets
// user code do `agent.maxSockets = 4` and rejects invalid writes with the
// same RangeError the constructor throws.

#[no_mangle]
pub extern "C" fn js_http_agent_set_max_sockets(handle: Handle, value: f64) {
    validate_agent_positive("maxSockets", value);
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.max_sockets = value;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_max_free_sockets(handle: Handle, value: f64) {
    validate_agent_positive("maxFreeSockets", value);
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.max_free_sockets = value;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_max_total_sockets(handle: Handle, value: f64) {
    validate_agent_positive("maxTotalSockets", value);
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.max_total_sockets = value;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_keep_alive_msecs(handle: Handle, value: f64) {
    if value.is_nan() || value < 0.0 {
        throw_agent_out_of_range("keepAliveMsecs", ">= 0", value);
    }
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.keep_alive_msecs = value;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_keep_alive(handle: Handle, value: f64) {
    let on = value != 0.0 && !value.is_nan();
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.keep_alive = on;
    }
}

/// `agent.destroyed`. Always 0/1 (matches the runtime's number ABI on
/// the `__get_<prop>` path).
#[no_mangle]
pub extern "C" fn js_http_agent_destroyed(handle: Handle) -> f64 {
    let destroyed = get_handle_mut::<AgentHandle>(handle)
        .map(|a| a.destroyed)
        .unwrap_or(false);
    f64::from_bits(JSValue::bool(destroyed).bits())
}

#[no_mangle]
pub extern "C" fn js_http_agent_default_port(handle: Handle) -> f64 {
    match get_handle_mut::<AgentHandle>(handle)
        .and_then(|a| a.protocol.clone())
        .unwrap_or_else(|| "http:".to_string())
        .as_str()
    {
        "https:" => 443.0,
        "http:" => 80.0,
        _ => 0.0,
    }
}

/// `agent.destroy()` — flag the agent as destroyed (so the `destroyed`
/// getter returns true) and return the handle for chainability.
#[no_mangle]
pub extern "C" fn js_http_agent_destroy(handle: Handle) -> Handle {
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.destroyed = true;
    }
    handle
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_create_connection(handle: Handle, closure_ptr: i64) {
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.create_connection = closure_ptr;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_create_socket(handle: Handle, closure_ptr: i64) {
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.create_socket = closure_ptr;
    }
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
            agent_handle: 0,
        });

        let mut msg_listeners = HashMap::new();
        msg_listeners.insert("data".to_string(), vec![0x3456_7890]);
        let msg_handle = register_handle(IncomingMessageHandle {
            status_code: 200,
            status_message: "OK".to_string(),
            headers: HashMap::new(),
            body: Vec::new(),
            listeners: msg_listeners,
            encoding: None,
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
