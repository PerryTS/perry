//! HTTP Response handling
//!
//! Provides helpers for building and sending HTTP responses.

use perry_runtime::{js_string_from_bytes, StringHeader};
use std::collections::HashMap;

use super::server::{HttpResponse, RequestHandle, PENDING_RESPONSES};
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

/// Send a text response
#[no_mangle]
pub unsafe extern "C" fn js_http_respond_text(
    req_handle: Handle,
    status: f64,
    body_ptr: *const StringHeader,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let body = string_from_header(body_ptr).unwrap_or_default();

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        );

        let response = HttpResponse {
            status: status as u16,
            headers,
            body: body.into_bytes(),
        };

        if let Some((_, tx)) = PENDING_RESPONSES.remove(&req.id) {
            let _: Result<(), HttpResponse> = tx.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Send a JSON response
#[no_mangle]
pub unsafe extern "C" fn js_http_respond_json(
    req_handle: Handle,
    status: f64,
    body_ptr: *const StringHeader,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let body = string_from_header(body_ptr).unwrap_or_else(|| "{}".to_string());

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        );

        let response = HttpResponse {
            status: status as u16,
            headers,
            body: body.into_bytes(),
        };

        if let Some((_, tx)) = PENDING_RESPONSES.remove(&req.id) {
            let _: Result<(), HttpResponse> = tx.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Send an HTML response
#[no_mangle]
pub unsafe extern "C" fn js_http_respond_html(
    req_handle: Handle,
    status: f64,
    body_ptr: *const StringHeader,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let body = string_from_header(body_ptr).unwrap_or_default();

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "text/html; charset=utf-8".to_string(),
        );

        let response = HttpResponse {
            status: status as u16,
            headers,
            body: body.into_bytes(),
        };

        if let Some((_, tx)) = PENDING_RESPONSES.remove(&req.id) {
            let _: Result<(), HttpResponse> = tx.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Send a response with custom headers (headers as JSON)
#[no_mangle]
pub unsafe extern "C" fn js_http_respond_with_headers(
    req_handle: Handle,
    status: f64,
    body_ptr: *const StringHeader,
    headers_json_ptr: *const StringHeader,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let body = string_from_header(body_ptr).unwrap_or_default();
    let headers_json = string_from_header(headers_json_ptr).unwrap_or_else(|| "{}".to_string());

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        // Parse headers JSON
        let headers: HashMap<String, String> =
            serde_json::from_str(&headers_json).unwrap_or_default();

        let response = HttpResponse {
            status: status as u16,
            headers,
            body: body.into_bytes(),
        };

        if let Some((_, tx)) = PENDING_RESPONSES.remove(&req.id) {
            let _: Result<(), HttpResponse> = tx.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Send a redirect response
#[no_mangle]
pub unsafe extern "C" fn js_http_respond_redirect(
    req_handle: Handle,
    url_ptr: *const StringHeader,
    permanent: f64,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => return f64::from_bits(TAG_FALSE),
    };

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        let mut headers = HashMap::new();
        headers.insert("location".to_string(), url);

        // Interpret NaN-boxed boolean: TAG_TRUE means permanent
        let is_permanent = permanent.to_bits() == TAG_TRUE;
        let status = if is_permanent { 301 } else { 302 };

        let response = HttpResponse {
            status,
            headers,
            body: Vec::new(),
        };

        if let Some((_, tx)) = PENDING_RESPONSES.remove(&req.id) {
            let _: Result<(), HttpResponse> = tx.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Send a "not found" response
#[no_mangle]
pub unsafe extern "C" fn js_http_respond_not_found(req_handle: Handle) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        );

        let response = HttpResponse {
            status: 404,
            headers,
            body: "Not Found".into(),
        };

        if let Some((_, tx)) = PENDING_RESPONSES.remove(&req.id) {
            let _: Result<(), HttpResponse> = tx.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Send an error response
#[no_mangle]
pub unsafe extern "C" fn js_http_respond_error(
    req_handle: Handle,
    status: f64,
    message_ptr: *const StringHeader,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let message =
        string_from_header(message_ptr).unwrap_or_else(|| "Internal Server Error".to_string());

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        let mut headers = HashMap::new();
        headers.insert(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        );

        let response = HttpResponse {
            status: status as u16,
            headers,
            body: message.into_bytes(),
        };

        if let Some((_, tx)) = PENDING_RESPONSES.remove(&req.id) {
            let _: Result<(), HttpResponse> = tx.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Set a response header (for building response incrementally)
/// Note: This requires a different approach - building response then sending
/// For now, use the respond_with_headers function for custom headers.
#[no_mangle]
pub unsafe extern "C" fn js_http_respond_status_text(status: f64) -> *mut StringHeader {
    let text = match status as u16 {
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
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Unknown",
    };

    js_string_from_bytes(text.as_ptr(), text.len() as u32)
}
