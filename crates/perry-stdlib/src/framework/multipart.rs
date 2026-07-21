//! Multipart/form-data parser
//!
//! Parses multipart/form-data bodies into individual parts with name, filename,
//! content_type, and data fields. Exposed to TypeScript via FFI.

use perry_runtime::{js_string_from_bytes, StringHeader};

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

/// A single part from a multipart/form-data body
#[derive(Debug, Clone)]
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

/// Extract boundary string from Content-Type header
/// e.g. "multipart/form-data; boundary=----WebKitFormBoundary7MA4YWxkTrZu0gW"
fn extract_boundary(content_type: &str) -> Option<String> {
    for part in content_type.split(';') {
        let trimmed = part.trim();
        if let Some(val) = trimmed.strip_prefix("boundary=") {
            // Remove optional quotes
            let val = val.trim_matches('"');
            return Some(val.to_string());
        }
    }
    None
}

/// Parse Content-Disposition header to extract name and optional filename
/// e.g. `form-data; name="field1"` or `form-data; name="file"; filename="test.tar.gz"`
fn parse_content_disposition(header: &str) -> (Option<String>, Option<String>) {
    let mut name = None;
    let mut filename = None;

    for part in header.split(';') {
        let trimmed = part.trim();
        if let Some(val) = trimmed.strip_prefix("name=") {
            name = Some(val.trim_matches('"').to_string());
        } else if let Some(val) = trimmed.strip_prefix("filename=") {
            filename = Some(val.trim_matches('"').to_string());
        }
    }

    (name, filename)
}

/// Parse a multipart/form-data body into parts
pub fn parse_multipart(body: &[u8], content_type: &str) -> Result<Vec<MultipartPart>, String> {
    let boundary = extract_boundary(content_type)
        .ok_or_else(|| "No boundary found in Content-Type".to_string())?;

    let delimiter = format!("--{boundary}");
    let end_delimiter = format!("--{boundary}--");

    // Find delimiter positions in raw bytes
    let delimiter_bytes = delimiter.as_bytes();
    let _end_bytes = end_delimiter.as_bytes();

    let mut segments = Vec::new();
    let mut search_start = 0;

    loop {
        if let Some(pos) = find_bytes(body, delimiter_bytes, search_start) {
            if search_start > 0 {
                // Capture segment between previous delimiter end and this delimiter start
                segments.push(&body[search_start..pos]);
            }
            let after_delim = pos + delimiter_bytes.len();
            // Check if this is the end delimiter
            if after_delim + 2 <= body.len() && &body[after_delim..after_delim + 2] == b"--" {
                break;
            }
            // Skip past \r\n after delimiter
            search_start = if after_delim + 2 <= body.len()
                && &body[after_delim..after_delim + 2] == b"\r\n"
            {
                after_delim + 2
            } else if after_delim < body.len() && body[after_delim] == b'\n' {
                after_delim + 1
            } else {
                after_delim
            };
        } else {
            break;
        }
    }

    let mut result = Vec::new();

    for segment in segments {
        // Each segment: headers\r\n\r\nbody (with trailing \r\n)
        let header_end = find_header_end(segment);
        if header_end.is_none() {
            continue;
        }
        let (header_end_pos, body_start) = header_end.unwrap();

        let headers_bytes = &segment[..header_end_pos];
        let headers_str = String::from_utf8_lossy(headers_bytes);

        let mut part_data = &segment[body_start..];
        // Strip trailing \r\n
        if part_data.len() >= 2 && &part_data[part_data.len() - 2..] == b"\r\n" {
            part_data = &part_data[..part_data.len() - 2];
        } else if !part_data.is_empty() && part_data[part_data.len() - 1] == b'\n' {
            part_data = &part_data[..part_data.len() - 1];
        }

        let mut name = None;
        let mut filename = None;
        let mut part_content_type = None;

        for line in headers_str.split('\n') {
            let line = line.trim_end_matches('\r');
            if let Some(val) = line.strip_prefix("Content-Disposition:") {
                let (n, f) = parse_content_disposition(val.trim());
                name = n;
                filename = f;
            } else if let Some(val) = line.strip_prefix("content-disposition:") {
                let (n, f) = parse_content_disposition(val.trim());
                name = n;
                filename = f;
            } else if let Some(val) = line.strip_prefix("Content-Type:") {
                part_content_type = Some(val.trim().to_string());
            } else if let Some(val) = line.strip_prefix("content-type:") {
                part_content_type = Some(val.trim().to_string());
            }
        }

        if let Some(name) = name {
            result.push(MultipartPart {
                name,
                filename,
                content_type: part_content_type,
                data: part_data.to_vec(),
            });
        }
    }

    Ok(result)
}

/// Find a byte sequence within a larger byte slice, starting from offset
fn find_bytes(haystack: &[u8], needle: &[u8], offset: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let end = haystack.len() - needle.len() + 1;
    for i in offset..end {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

/// Find the end of headers (blank line: \r\n\r\n or \n\n)
/// Returns (header_end_pos, body_start_pos)
fn find_header_end(data: &[u8]) -> Option<(usize, usize)> {
    // Look for \r\n\r\n
    for i in 0..data.len().saturating_sub(3) {
        if &data[i..i + 4] == b"\r\n\r\n" {
            return Some((i, i + 4));
        }
    }
    // Fall back to \n\n
    for i in 0..data.len().saturating_sub(1) {
        if &data[i..i + 2] == b"\n\n" {
            return Some((i, i + 2));
        }
    }
    None
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_boundary() {
        assert_eq!(
            extract_boundary("multipart/form-data; boundary=----WebKitFormBoundary7MA4"),
            Some("----WebKitFormBoundary7MA4".into())
        );
        assert_eq!(
            extract_boundary("multipart/form-data; boundary=\"abc123\""),
            Some("abc123".into())
        );
        assert_eq!(extract_boundary("application/json"), None);
    }

    #[test]
    fn test_parse_content_disposition() {
        let (name, filename) = parse_content_disposition("form-data; name=\"field1\"");
        assert_eq!(name, Some("field1".into()));
        assert_eq!(filename, None);

        let (name, filename) =
            parse_content_disposition("form-data; name=\"file\"; filename=\"test.tar.gz\"");
        assert_eq!(name, Some("file".into()));
        assert_eq!(filename, Some("test.tar.gz".into()));
    }

    #[test]
    fn test_parse_multipart_text_fields() {
        let boundary = "----boundary123";
        let body = format!(
            "------boundary123\r\n\
             Content-Disposition: form-data; name=\"field1\"\r\n\
             \r\n\
             value1\r\n\
             ------boundary123\r\n\
             Content-Disposition: form-data; name=\"field2\"\r\n\
             \r\n\
             value2\r\n\
             ------boundary123--\r\n"
        );
        let content_type = format!("multipart/form-data; boundary={boundary}");

        let parts = parse_multipart(body.as_bytes(), &content_type).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "field1");
        assert_eq!(parts[0].data, b"value1");
        assert_eq!(parts[0].filename, None);
        assert_eq!(parts[1].name, "field2");
        assert_eq!(parts[1].data, b"value2");
    }

    #[test]
    fn test_parse_multipart_with_file() {
        let _boundary = "boundary456";
        let file_content = vec![0u8, 1, 2, 3, 4, 5, 255, 254, 253];
        let mut body = Vec::new();
        body.extend_from_slice(b"--boundary456\r\n");
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"text_field\"\r\n");
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(b"hello\r\n");
        body.extend_from_slice(b"--boundary456\r\n");
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"file\"; filename=\"test.bin\"\r\n",
        );
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(&file_content);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(b"--boundary456--\r\n");

        let content_type = "multipart/form-data; boundary=boundary456";
        let parts = parse_multipart(&body, content_type).unwrap();

        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "text_field");
        assert_eq!(parts[0].data, b"hello");
        assert_eq!(parts[0].filename, None);

        assert_eq!(parts[1].name, "file");
        assert_eq!(parts[1].filename, Some("test.bin".into()));
        assert_eq!(
            parts[1].content_type,
            Some("application/octet-stream".into())
        );
        assert_eq!(parts[1].data, file_content);
    }

    #[test]
    fn test_parse_multipart_empty() {
        let body = b"--boundary--\r\n";
        let content_type = "multipart/form-data; boundary=boundary";
        let parts = parse_multipart(body, content_type).unwrap();
        assert_eq!(parts.len(), 0);
    }

    #[test]
    fn test_no_boundary() {
        let result = parse_multipart(b"some body", "application/json");
        assert!(result.is_err());
    }
}
