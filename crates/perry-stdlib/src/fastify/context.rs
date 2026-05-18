//! Request/Reply context objects
//!
//! Provides a unified context for both Fastify and Hono style request handling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};

use crate::common::{get_handle, get_handle_mut, Handle};

// Declare perry-runtime's JSON parser (defined in perry-runtime with #[no_mangle])
extern "C" {
    fn js_json_parse(text_ptr: *const StringHeader) -> u64; // returns NaN-boxed JSValue bits
}

/// Context ID counter
static CONTEXT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Helper to extract string from StringHeader pointer
pub(crate) unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Helper to extract string from raw i64 pointer (NaN-boxed or raw)
pub(crate) unsafe fn string_from_nanboxed(value: i64) -> Option<String> {
    let ptr = perry_runtime::js_get_string_pointer_unified(f64::from_bits(value as u64));
    if ptr == 0 {
        return None;
    }
    string_from_header(ptr as *const StringHeader)
}

/// Unified context for both Fastify and Hono styles
pub struct FastifyContext {
    /// Unique context ID
    pub id: u64,
    /// Request ID (from underlying server)
    pub request_id: u64,
    /// HTTP method
    pub method: String,
    /// Request URL path
    pub url: String,
    /// Query string (without leading ?)
    pub query_string: String,
    /// Extracted route parameters
    pub params: HashMap<String, String>,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// Request body (raw bytes)
    pub body: Option<Vec<u8>>,

    // Reply state
    /// Response status code
    pub status_code: u16,
    /// Response headers
    pub response_headers: Vec<(String, String)>,
    /// Whether response has been sent
    pub sent: bool,
    /// Response body (if built incrementally)
    pub response_body: Option<Vec<u8>>,
    /// User data attached by auth middleware (NaN-boxed JSValue bits)
    pub user_data: u64,
}

impl FastifyContext {
    /// Create a new context
    pub fn new(
        request_id: u64,
        method: String,
        url: String,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
        params: HashMap<String, String>,
    ) -> Self {
        // Parse query string from URL
        let (path, query_string) = match url.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (url.clone(), String::new()),
        };

        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        Self {
            id: CONTEXT_ID_COUNTER.fetch_add(1, Ordering::SeqCst),
            request_id,
            method,
            url: path,
            query_string,
            params,
            headers,
            body,
            status_code: 200,
            response_headers: Vec::new(),
            sent: false,
            response_body: None,
            user_data: TAG_UNDEFINED,
        }
    }

    /// Get a request header value
    pub fn get_header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    /// Get a route parameter value
    pub fn get_param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    /// Get a query parameter value
    pub fn get_query_param(&self, name: &str) -> Option<String> {
        for pair in self.query_string.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                if key == name {
                    return Some(urlencoding_decode(value));
                }
            }
        }
        None
    }

    /// Get all query parameters as a map
    pub fn get_query_params(&self) -> HashMap<String, String> {
        let mut params = HashMap::new();
        for pair in self.query_string.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                params.insert(key.to_string(), urlencoding_decode(value));
            }
        }
        params
    }

    /// Get body as string
    pub fn body_string(&self) -> Option<String> {
        self.body
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).to_string())
    }

    /// Set response status code
    pub fn set_status(&mut self, code: u16) {
        self.status_code = code;
    }

    /// Add a response header
    pub fn add_header(&mut self, name: &str, value: &str) {
        self.response_headers
            .push((name.to_string(), value.to_string()));
    }
}

/// Simple URL decoding
fn urlencoding_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }

    result
}

// ============================================================================
// Request Methods FFI
// ============================================================================

/// Get request method
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_method(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        return js_string_from_bytes(ctx.method.as_ptr(), ctx.method.len() as u32);
    }
    std::ptr::null_mut()
}

/// Get request URL path (includes query string from ctx.url)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_url(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        return js_string_from_bytes(ctx.url.as_ptr(), ctx.url.len() as u32);
    }
    std::ptr::null_mut()
}

/// Get all route params as JSON object
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_params(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Ok(json) = serde_json::to_string(&ctx.params) {
            return js_string_from_bytes(json.as_ptr(), json.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get all route params as a JavaScript object (NaN-boxed pointer)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_params_object(ctx_handle: Handle) -> f64 {
    use perry_runtime::{
        js_array_alloc, js_array_push_f64, js_nanbox_string, js_object_alloc,
        js_object_set_field_f64, js_object_set_keys,
    };

    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        let field_count = ctx.params.len() as u32;
        let obj = js_object_alloc(0, field_count);
        if obj.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let keys_arr = js_array_alloc(field_count);
        if keys_arr.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        for (i, (key, value)) in ctx.params.iter().enumerate() {
            let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
            let value_ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
            let key_nanboxed = js_nanbox_string(key_ptr as i64);
            js_array_push_f64(keys_arr, key_nanboxed);
            let value_nanboxed = js_nanbox_string(value_ptr as i64);
            js_object_set_field_f64(obj, i as u32, value_nanboxed);
        }
        js_object_set_keys(obj, keys_arr);
        let ptr = obj as u64;
        return f64::from_bits(0x7FFD_0000_0000_0000 | (ptr & 0x0000_FFFF_FFFF_FFFF));
    }
    f64::from_bits(0x7FFC_0000_0000_0001)
}

/// Get a single route param (Hono style)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_param(ctx_handle: Handle, name: i64) -> *mut StringHeader {
    let name = match string_from_nanboxed(name) {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };

    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Some(value) = ctx.params.get(&name) {
            return js_string_from_bytes(value.as_ptr(), value.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get all query params as JSON string (for backwards compatibility)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_query(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        let params = ctx.get_query_params();
        if let Ok(json) = serde_json::to_string(&params) {
            return js_string_from_bytes(json.as_ptr(), json.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get all query params as a JavaScript object (NaN-boxed pointer)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_query_object(ctx_handle: Handle) -> f64 {
    use perry_runtime::{
        js_array_alloc, js_array_push_f64, js_nanbox_string, js_object_alloc,
        js_object_set_field_f64, js_object_set_keys,
    };

    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        let params = ctx.get_query_params();
        let field_count = params.len() as u32;

        // Allocate object with enough fields
        let obj = js_object_alloc(0, field_count);
        if obj.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
        }

        // Allocate keys array
        let keys_arr = js_array_alloc(field_count);
        if keys_arr.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }

        // Set each field and add key to keys array
        for (i, (key, value)) in params.iter().enumerate() {
            // Create key string
            let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
            // Create value string
            let value_ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);

            // Add key to keys array (NaN-boxed)
            let key_nanboxed = js_nanbox_string(key_ptr as i64);
            js_array_push_f64(keys_arr, key_nanboxed);

            // Set field on object by index (NaN-boxed string value)
            let value_nanboxed = js_nanbox_string(value_ptr as i64);
            js_object_set_field_f64(obj, i as u32, value_nanboxed);
        }

        // Set keys array on object
        js_object_set_keys(obj, keys_arr);

        // Return NaN-boxed pointer
        let ptr = obj as u64;
        return f64::from_bits(0x7FFD_0000_0000_0000 | (ptr & 0x0000_FFFF_FFFF_FFFF));
    }

    f64::from_bits(0x7FFC_0000_0000_0001) // undefined
}

/// Get raw request body as string
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_body(ctx_handle: Handle) -> *mut StringHeader {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Some(body) = ctx.body_string() {
            return js_string_from_bytes(body.as_ptr(), body.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get parsed JSON body (returns NaN-boxed object or undefined)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_json(ctx_handle: Handle) -> f64 {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Some(body) = ctx.body_string() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
                return json_value_to_jsvalue(&value);
            }
        }
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Get all headers as JSON object
/// Get all request headers as a JS object (so request.headers.authorization works)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_headers(ctx_handle: Handle) -> i64 {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Ok(json) = serde_json::to_string(&ctx.headers) {
            // Create a Perry string for the JSON
            let json_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
            if !json_ptr.is_null() {
                // Parse the JSON string into a JS object using Perry's JSON parser
                let jsval_bits = js_json_parse(json_ptr as *const StringHeader);
                return jsval_bits as i64;
            }
        }
    }
    // Return undefined
    0x7FFC_0000_0000_0001u64 as i64
}

/// Get a single header value
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_header(ctx_handle: Handle, name: i64) -> *mut StringHeader {
    let name = match string_from_nanboxed(name) {
        Some(n) => n.to_lowercase(),
        None => return std::ptr::null_mut(),
    };

    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        if let Some(value) = ctx.headers.get(&name) {
            return js_string_from_bytes(value.as_ptr(), value.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get user data attached by auth middleware
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_get_user_data(ctx_handle: Handle) -> f64 {
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        return f64::from_bits(ctx.user_data);
    }
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    f64::from_bits(TAG_UNDEFINED)
}

/// Set user data from auth middleware
#[no_mangle]
pub unsafe extern "C" fn js_fastify_req_set_user_data(ctx_handle: Handle, data: f64) {
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.user_data = data.to_bits();
    }
}

// ============================================================================
// Reply Methods FFI (Fastify style)
// ============================================================================

/// Set response status code (chainable, returns handle)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_reply_status(ctx_handle: Handle, code: f64) -> Handle {
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.status_code = code as u16;
    }
    ctx_handle
}

/// Set a response header (chainable, returns handle)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_reply_header(
    ctx_handle: Handle,
    name: i64,
    value: i64,
) -> Handle {
    let name = match string_from_nanboxed(name) {
        Some(n) => n,
        None => return ctx_handle,
    };
    let value = match string_from_nanboxed(value) {
        Some(v) => v,
        None => return ctx_handle,
    };

    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.response_headers.push((name, value));
    }
    ctx_handle
}

/// `reply.type(value)` — Fastify alias for `reply.header("content-type", value)`.
/// Chainable: returns the reply handle so `.type(...).send(...)` works.
///
/// Previously missing from the stdlib copy of fastify (#1048): user code using
/// the bundled-fastify path failed at link time with
/// `Undefined symbols: _js_fastify_reply_type` because only perry-ext-fastify
/// shipped this symbol.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_reply_type(ctx_handle: Handle, value: i64) -> Handle {
    let value = match string_from_nanboxed(value) {
        Some(v) => v,
        None => return ctx_handle,
    };

    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.response_headers
            .push(("content-type".to_string(), value));
    }
    ctx_handle
}

/// Send response (Fastify style)
/// Returns true if sent, false otherwise
#[no_mangle]
pub unsafe extern "C" fn js_fastify_reply_send(ctx_handle: Handle, data: f64) -> bool {
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        if ctx.sent {
            return false;
        }

        // Convert data to response body
        let body = jsvalue_to_response_body(data);
        ctx.response_body = Some(body);
        ctx.sent = true;
        return true;
    }
    false
}

// ============================================================================
// Context Methods FFI (Hono style)
// ============================================================================

/// Send JSON response (Hono style)
/// Returns a response marker value
#[no_mangle]
pub unsafe extern "C" fn js_fastify_ctx_json(ctx_handle: Handle, data: f64, status: f64) -> f64 {
    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        if status > 0.0 {
            ctx.status_code = status as u16;
        }

        // Add content-type header
        ctx.response_headers.push((
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        ));

        // Convert data to JSON string
        let body = jsvalue_to_json_string(data);
        ctx.response_body = Some(body.into_bytes());
        ctx.sent = true;
    }

    // Return undefined (response is implicit)
    f64::from_bits(JSValue::undefined().bits())
}

/// Send text response (Hono style)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_ctx_text(ctx_handle: Handle, text: i64, status: f64) -> f64 {
    let text = string_from_nanboxed(text).unwrap_or_default();

    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        if status > 0.0 {
            ctx.status_code = status as u16;
        }

        ctx.response_headers.push((
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ));
        ctx.response_body = Some(text.into_bytes());
        ctx.sent = true;
    }

    f64::from_bits(JSValue::undefined().bits())
}

/// Send HTML response (Hono style)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_ctx_html(ctx_handle: Handle, html: i64, status: f64) -> f64 {
    let html = string_from_nanboxed(html).unwrap_or_default();

    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        if status > 0.0 {
            ctx.status_code = status as u16;
        }

        ctx.response_headers.push((
            "content-type".to_string(),
            "text/html; charset=utf-8".to_string(),
        ));
        ctx.response_body = Some(html.into_bytes());
        ctx.sent = true;
    }

    f64::from_bits(JSValue::undefined().bits())
}

/// Send redirect response (Hono style)
#[no_mangle]
pub unsafe extern "C" fn js_fastify_ctx_redirect(ctx_handle: Handle, url: i64, status: f64) -> f64 {
    let url = string_from_nanboxed(url).unwrap_or_default();

    if let Some(ctx) = get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.status_code = if status > 0.0 { status as u16 } else { 302 };
        ctx.response_headers.push(("location".to_string(), url));
        ctx.response_body = Some(Vec::new());
        ctx.sent = true;
    }

    f64::from_bits(JSValue::undefined().bits())
}

// ============================================================================
// Helper functions
// ============================================================================

/// Convert a JSValue to a response body (bytes)
unsafe fn jsvalue_to_response_body(value: f64) -> Vec<u8> {
    let jsv = JSValue::from_bits(value.to_bits());

    // Check if it's a string
    if jsv.is_string() {
        if let Some(s) = extract_jsvalue_string(value) {
            return s.into_bytes();
        }
    }

    // Otherwise serialize as JSON
    jsvalue_to_json_string(value).into_bytes()
}

/// Convert a JSValue to a JSON string
unsafe fn jsvalue_to_json_string(value: f64) -> String {
    let jsv = JSValue::from_bits(value.to_bits());

    if jsv.is_undefined() {
        return "null".to_string();
    }
    if jsv.is_null() {
        return "null".to_string();
    }
    if jsv.is_bool() {
        return if jsv.as_bool() {
            "true".to_string()
        } else {
            "false".to_string()
        };
    }
    if jsv.is_number() {
        return format!("{}", value);
    }
    if jsv.is_string() {
        if let Some(s) = extract_jsvalue_string(value) {
            // Escape string for JSON
            return serde_json::to_string(&s).unwrap_or_else(|_| format!("\"{}\"", s));
        }
    }

    // For objects/arrays, use JSON.stringify
    if jsv.is_pointer() {
        extern "C" {
            fn js_json_stringify(value: f64, type_hint: u32) -> *mut perry_runtime::StringHeader;
        }
        let str_ptr = js_json_stringify(value, 0);
        if !str_ptr.is_null() {
            if let Some(s) = string_from_header(str_ptr) {
                return s;
            }
        }
    }

    // Fallback: use runtime's toString
    let str_ptr = perry_runtime::js_jsvalue_to_string(value);
    if !str_ptr.is_null() {
        if let Some(s) = string_from_header(str_ptr) {
            return s;
        }
    }

    "null".to_string()
}

/// Extract string from JSValue
unsafe fn extract_jsvalue_string(value: f64) -> Option<String> {
    let ptr = perry_runtime::js_get_string_pointer_unified(value);
    if ptr == 0 {
        return None;
    }
    string_from_header(ptr as *const StringHeader)
}

/// Convert serde_json::Value to JSValue (as f64)
unsafe fn json_value_to_jsvalue(value: &serde_json::Value) -> f64 {
    match value {
        serde_json::Value::Null => f64::from_bits(JSValue::null().bits()),
        serde_json::Value::Bool(b) => f64::from_bits(JSValue::bool(*b).bits()),
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
        serde_json::Value::String(s) => {
            let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
            f64::from_bits(JSValue::string_ptr(ptr).bits())
        }
        serde_json::Value::Array(arr) => {
            let js_arr = perry_runtime::js_array_alloc(arr.len() as u32);
            for item in arr {
                let js_item = json_value_to_jsvalue(item);
                perry_runtime::js_array_push_f64(js_arr, js_item);
            }
            f64::from_bits(JSValue::pointer(js_arr as *const u8).bits())
        }
        serde_json::Value::Object(obj) => {
            let field_count = obj.len();
            let js_obj = perry_runtime::js_object_alloc(0, field_count as u32);
            for (key, val) in obj {
                let js_key = js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let js_val = json_value_to_jsvalue(val);
                perry_runtime::js_object_set_field_by_name(js_obj, js_key, js_val);
            }
            f64::from_bits(JSValue::pointer(js_obj as *const u8).bits())
        }
    }
}
