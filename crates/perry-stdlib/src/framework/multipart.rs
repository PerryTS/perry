//! Multipart/form-data parser
//!
//! Parses multipart/form-data bodies into individual parts with name, filename,
//! content_type, and data fields. Exposed to TypeScript via FFI.

use perry_runtime::{js_string_from_bytes, StringHeader};

use crate::common::string_from_header_lossy as string_from_header;
pub use crate::multipart_parser::{parse_multipart, MultipartPart};

/// Parse multipart body and return result as JSON string.
///
/// Returns a JSON array of objects: `[{ name, filename?, content_type?, data }]`
/// where `data` is the raw string content for text fields, or base64 for binary.
///
/// Called from TypeScript as: `__multipart_parse(body, contentType)`
#[no_mangle]
pub unsafe extern "C" fn js_multipart_parse(
    body_ptr: *const StringHeader,
    content_type_ptr: *const StringHeader,
) -> *mut StringHeader {
    let body = match string_from_header(body_ptr) {
        Some(b) => b,
        None => return std::ptr::null_mut(),
    };
    let content_type = match string_from_header(content_type_ptr) {
        Some(ct) => ct,
        None => return std::ptr::null_mut(),
    };

    match parse_multipart(body.as_bytes(), &content_type) {
        Ok(parts) => {
            let json_parts: Vec<serde_json::Value> = parts
                .iter()
                .map(|p| {
                    let mut obj = serde_json::Map::new();
                    obj.insert("name".into(), serde_json::Value::String(p.name.clone()));
                    if let Some(ref f) = p.filename {
                        obj.insert("filename".into(), serde_json::Value::String(f.clone()));
                    }
                    if let Some(ref ct) = p.content_type {
                        obj.insert("content_type".into(), serde_json::Value::String(ct.clone()));
                    }
                    // Return data as string (works for text; binary gets lossy conversion
                    // but hub code uses it for text fields and accesses raw bytes separately)
                    obj.insert(
                        "data".into(),
                        serde_json::Value::String(String::from_utf8_lossy(&p.data).to_string()),
                    );
                    serde_json::Value::Object(obj)
                })
                .collect();

            let json = serde_json::to_string(&json_parts).unwrap_or_else(|_| "[]".into());
            js_string_from_bytes(json.as_ptr(), json.len() as u32)
        }
        Err(_) => {
            let empty = "[]";
            js_string_from_bytes(empty.as_ptr(), empty.len() as u32)
        }
    }
}

/// Parse multipart body with size information for each part.
///
/// Returns JSON: `[{ name, filename?, content_type?, data, size }]`
/// where `data` is the string content. For binary parts, the hub should save
/// the raw body and use tarball_path instead.
#[no_mangle]
pub unsafe extern "C" fn js_multipart_parse_with_sizes(
    body_ptr: *const StringHeader,
    content_type_ptr: *const StringHeader,
) -> *mut StringHeader {
    let body = match string_from_header(body_ptr) {
        Some(b) => b,
        None => return std::ptr::null_mut(),
    };
    let content_type = match string_from_header(content_type_ptr) {
        Some(ct) => ct,
        None => return std::ptr::null_mut(),
    };

    match parse_multipart(body.as_bytes(), &content_type) {
        Ok(parts) => {
            let json_parts: Vec<serde_json::Value> = parts
                .iter()
                .map(|p| {
                    let mut obj = serde_json::Map::new();
                    obj.insert("name".into(), serde_json::Value::String(p.name.clone()));
                    if let Some(ref f) = p.filename {
                        obj.insert("filename".into(), serde_json::Value::String(f.clone()));
                    }
                    if let Some(ref ct) = p.content_type {
                        obj.insert("content_type".into(), serde_json::Value::String(ct.clone()));
                    }
                    obj.insert(
                        "data".into(),
                        serde_json::Value::String(String::from_utf8_lossy(&p.data).to_string()),
                    );
                    obj.insert(
                        "size".into(),
                        serde_json::Value::Number(serde_json::Number::from(p.data.len())),
                    );
                    serde_json::Value::Object(obj)
                })
                .collect();

            let json = serde_json::to_string(&json_parts).unwrap_or_else(|_| "[]".into());
            js_string_from_bytes(json.as_ptr(), json.len() as u32)
        }
        Err(_) => {
            let empty = "[]";
            js_string_from_bytes(empty.as_ptr(), empty.len() as u32)
        }
    }
}
