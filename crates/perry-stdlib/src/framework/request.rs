//! HTTP Request handling
//!
//! Provides access to request properties like method, path, headers, body.

use perry_runtime::{js_string_from_bytes, StringHeader};
use std::collections::HashMap;

use super::server::RequestHandle;
use crate::common::{get_handle, Handle};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Get request ID (for debugging/logging)
#[no_mangle]
pub unsafe extern "C" fn js_http_request_id(req_handle: Handle) -> f64 {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return req.id as f64;
    }
    0.0
}

/// Get a query parameter by name
#[no_mangle]
pub unsafe extern "C" fn js_http_request_query_param(
    req_handle: Handle,
    name_ptr: *const StringHeader,
) -> *mut StringHeader {
    let name = match string_from_header(name_ptr) {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        // Parse query string
        for pair in req.query.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                if key == name {
                    // URL decode the value
                    let decoded = urlencoding_decode(value);
                    return js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
                }
            }
        }
    }
    std::ptr::null_mut()
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

/// Get all query parameters as JSON string
#[no_mangle]
pub unsafe extern "C" fn js_http_request_query_all(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        let mut params = HashMap::new();

        for pair in req.query.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                params.insert(key.to_string(), urlencoding_decode(value));
            }
        }

        if let Ok(json) = serde_json::to_string(&params) {
            return js_string_from_bytes(json.as_ptr(), json.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get all headers as JSON string
#[no_mangle]
pub unsafe extern "C" fn js_http_request_headers_all(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        if let Ok(json) = serde_json::to_string(&req.headers) {
            return js_string_from_bytes(json.as_ptr(), json.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Check if request has a specific header
#[no_mangle]
pub unsafe extern "C" fn js_http_request_has_header(
    req_handle: Handle,
    name_ptr: *const StringHeader,
) -> bool {
    let name = match string_from_header(name_ptr) {
        Some(n) => n.to_lowercase(),
        None => return false,
    };

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return req.headers.contains_key(&name);
    }
    false
}

/// Get content type header
#[no_mangle]
pub unsafe extern "C" fn js_http_request_content_type(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        if let Some(value) = req.headers.get("content-type") {
            return js_string_from_bytes(value.as_ptr(), value.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Check if request method matches
#[no_mangle]
pub unsafe extern "C" fn js_http_request_is_method(
    req_handle: Handle,
    method_ptr: *const StringHeader,
) -> bool {
    let method = match string_from_header(method_ptr) {
        Some(m) => m.to_uppercase(),
        None => return false,
    };

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return req.method == method;
    }
    false
}

/// Get body length
#[no_mangle]
pub unsafe extern "C" fn js_http_request_body_length(req_handle: Handle) -> f64 {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        if let Some(ref body) = req.body {
            return body.len() as f64;
        }
    }
    0.0
}
