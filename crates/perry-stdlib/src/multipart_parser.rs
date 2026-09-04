//! Binary-safe multipart/form-data parsing shared by Web Fetch and the HTTP
//! server framework.

/// A single part from a multipart/form-data body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

fn parameter_segments(value: &str) -> Vec<&str> {
    let bytes = value.as_bytes();
    let mut segments = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;

    for (index, byte) in bytes.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' if quoted => escaped = true,
            b'"' => quoted = !quoted,
            b';' if !quoted => {
                segments.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    segments.push(&value[start..]);
    segments
}

fn parameter_value(value: &str) -> Option<String> {
    let value = value.trim();
    if !value.starts_with('"') {
        return Some(value.to_string());
    }
    if value.len() < 2 || !value.ends_with('"') {
        return None;
    }

    let mut result = String::with_capacity(value.len() - 2);
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            result.push(chars.next()?);
        } else {
            result.push(ch);
        }
    }
    Some(result)
}

/// Extract and validate the boundary parameter from a multipart Content-Type.
fn extract_boundary(content_type: &str) -> Option<String> {
    let segments = parameter_segments(content_type);
    if !segments
        .first()?
        .trim()
        .eq_ignore_ascii_case("multipart/form-data")
    {
        return None;
    }

    let boundary = segments.iter().skip(1).find_map(|segment| {
        let (name, value) = segment.trim().split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| parameter_value(value))?
    })?;

    // RFC 2046 limits boundaries to 70 characters and excludes control
    // characters. Rejecting CR/LF is also what prevents header injection from
    // manufacturing an apparent delimiter line.
    if boundary.is_empty()
        || boundary.len() > 70
        || !boundary.is_ascii()
        || boundary
            .bytes()
            .any(|byte| !(0x20..=0x7e).contains(&byte) || byte == b'"')
        || boundary.chars().last().is_some_and(char::is_whitespace)
    {
        return None;
    }
    Some(boundary)
}

fn parse_content_disposition(header: &str) -> Option<(String, Option<String>)> {
    let segments = parameter_segments(header);
    if !segments.first()?.trim().eq_ignore_ascii_case("form-data") {
        return None;
    }

    let mut name = None;
    let mut filename = None;
    for segment in segments.iter().skip(1) {
        let Some((key, value)) = segment.trim().split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("name") {
            name = parameter_value(value);
        } else if key.trim().eq_ignore_ascii_case("filename") {
            // `Some("")` is significant: an explicitly empty filename is a
            // File entry, while an absent filename is a string entry.
            filename = Some(parameter_value(value)?);
        }
    }
    Some((name?, filename))
}

fn find_header_end(data: &[u8]) -> Option<(usize, usize)> {
    let crlf = data
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, index + 4));
    let lf = data
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, index + 2));
    match (crlf, lf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(found), None) | (None, Some(found)) => Some(found),
        (None, None) => None,
    }
}

fn parse_part(segment: &[u8]) -> Result<MultipartPart, String> {
    let (header_end, body_start) = find_header_end(segment)
        .ok_or_else(|| "Multipart part is missing its header terminator".to_string())?;
    let headers = String::from_utf8_lossy(&segment[..header_end]);
    let mut disposition = None;
    let mut content_type = None;

    for line in headers.lines() {
        let line = line.trim_end_matches('\r');
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "Malformed multipart header".to_string())?;
        if name.trim().eq_ignore_ascii_case("content-disposition") {
            disposition = parse_content_disposition(value.trim());
        } else if name.trim().eq_ignore_ascii_case("content-type") {
            content_type = Some(value.trim().to_string());
        }
    }

    let (name, filename) = disposition
        .ok_or_else(|| "Multipart part is missing a valid Content-Disposition".to_string())?;
    Ok(MultipartPart {
        name,
        filename,
        content_type,
        data: segment[body_start..].to_vec(),
    })
}

#[derive(Clone, Copy)]
struct DelimiterMatch {
    data_end: usize,
    next: usize,
    closing: bool,
}

fn find_delimiter(body: &[u8], delimiter: &[u8], from: usize) -> Option<DelimiterMatch> {
    if delimiter.is_empty() || delimiter.len() > body.len() {
        return None;
    }

    for position in from..=body.len() - delimiter.len() {
        if &body[position..position + delimiter.len()] != delimiter {
            continue;
        }

        let separator_len = if position == 0 {
            0
        } else if position >= 2 && &body[position - 2..position] == b"\r\n" {
            2
        } else if body[position - 1] == b'\n' {
            1
        } else {
            continue;
        };

        let mut cursor = position + delimiter.len();
        let closing = body.get(cursor..cursor + 2) == Some(b"--");
        if closing {
            cursor += 2;
        }
        while matches!(body.get(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }

        let next = if cursor == body.len() {
            cursor
        } else if body.get(cursor..cursor + 2) == Some(b"\r\n") {
            cursor + 2
        } else if body.get(cursor) == Some(&b'\n') {
            cursor + 1
        } else {
            // A boundary-shaped byte sequence inside a file is payload unless
            // it occupies an entire delimiter line.
            continue;
        };

        return Some(DelimiterMatch {
            data_end: position.saturating_sub(separator_len),
            next,
            closing,
        });
    }
    None
}

/// Parse a multipart/form-data body into binary-safe parts.
pub fn parse_multipart(body: &[u8], content_type: &str) -> Result<Vec<MultipartPart>, String> {
    let boundary = extract_boundary(content_type)
        .ok_or_else(|| "Invalid or missing multipart boundary".to_string())?;
    let mut delimiter = Vec::with_capacity(boundary.len() + 2);
    delimiter.extend_from_slice(b"--");
    delimiter.extend_from_slice(boundary.as_bytes());

    let first = find_delimiter(body, &delimiter, 0)
        .ok_or_else(|| "Multipart body does not contain its boundary".to_string())?;
    if first.closing {
        return Ok(Vec::new());
    }

    let mut parts = Vec::new();
    let mut part_start = first.next;
    loop {
        let next = find_delimiter(body, &delimiter, part_start)
            .ok_or_else(|| "Multipart body is missing its closing boundary".to_string())?;
        if next.data_end < part_start {
            return Err("Malformed multipart delimiter ordering".to_string());
        }
        parts.push(parse_part(&body[part_start..next.data_end])?);
        if next.closing {
            return Ok(parts);
        }
        part_start = next.next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_quoted_case_insensitive_boundary() {
        assert_eq!(
            extract_boundary("Multipart/Form-Data; charset=utf-8; Boundary=\"abc-123\""),
            Some("abc-123".into())
        );
        assert_eq!(extract_boundary("application/json; boundary=x"), None);
        assert_eq!(extract_boundary("multipart/form-data; boundary=\"\""), None);
    }

    #[test]
    fn parses_text_and_binary_file_parts() {
        let file = [
            0, 1, 2, 255, b'-', b'-', b'b', b'o', b'u', b'n', b'd', b'a', b'r', b'y',
        ];
        let mut body = b"--boundary\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n--boundary\r\ncontent-disposition: form-data; name=\"file\"; filename=\"a\\\"b.bin\"\r\nCONTENT-TYPE: application/octet-stream\r\n\r\n".to_vec();
        body.extend_from_slice(&file);
        body.extend_from_slice(b"\r\n--boundary--\r\n");

        let parts = parse_multipart(&body, "multipart/form-data; boundary=boundary").unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "field");
        assert_eq!(parts[0].data, b"value");
        assert_eq!(parts[1].name, "file");
        assert_eq!(parts[1].filename.as_deref(), Some("a\"b.bin"));
        assert_eq!(
            parts[1].content_type.as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(parts[1].data, file);
    }

    #[test]
    fn does_not_split_on_boundary_shaped_file_bytes() {
        let body = b"--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"x\"\r\n\r\nprefix--boundary-inside\r\n--boundary--\r\n";
        let parts = parse_multipart(body, "multipart/form-data; boundary=boundary").unwrap();
        assert_eq!(parts[0].data, b"prefix--boundary-inside");
    }

    #[test]
    fn supports_lf_delimiters_and_empty_forms() {
        let parts = parse_multipart(
            b"--boundary\nContent-Disposition: form-data; name=\"field\"\n\nvalue\n--boundary--\n",
            "multipart/form-data; boundary=boundary",
        )
        .unwrap();
        assert_eq!(parts[0].data, b"value");
        assert!(parse_multipart(
            b"--boundary--\r\n",
            "multipart/form-data; boundary=boundary"
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn rejects_malformed_bodies() {
        assert!(parse_multipart(b"body", "application/json").is_err());
        assert!(parse_multipart(
            b"--boundary\r\nContent-Disposition: form-data; name=\"x\"\r\n\r\nvalue",
            "multipart/form-data; boundary=boundary"
        )
        .is_err());
        assert!(parse_multipart(
            b"--boundary\r\nContent-Type: text/plain\r\n\r\nvalue\r\n--boundary--\r\n",
            "multipart/form-data; boundary=boundary"
        )
        .is_err());
    }
}
