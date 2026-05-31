//! Web Fetch constructor validation helpers shared by the Response and
//! Request constructors (`js_response_new` / `js_request_new`). Mirrors the
//! WHATWG fetch spec rules Node enforces; refs #2640 (Response status /
//! statusText validation + empty-string default) and #2643 (Request method
//! normalization + forbidden methods + GET/HEAD body rejection).

/// Web Fetch reason-phrase validation (HTTP token rules). A valid
/// `statusText` byte is HTAB (0x09), SP (0x20), VCHAR (0x21..=0x7E),
/// or obs-text (0x80..=0xFF). Anything else (e.g. a newline) is invalid.
pub(crate) fn is_valid_status_text(s: &str) -> bool {
    s.bytes()
        .all(|b| b == 0x09 || b == 0x20 || (0x21..=0x7E).contains(&b) || b >= 0x80)
}

/// Web Fetch null-body status codes — a Response with one of these may
/// not carry a body.
pub(crate) fn is_null_body_status(status: u16) -> bool {
    matches!(status, 101 | 103 | 204 | 205 | 304)
}

/// Web Fetch forbidden request methods — rejected by the Request ctor.
pub(crate) fn is_forbidden_method(method_upper: &str) -> bool {
    matches!(method_upper, "CONNECT" | "TRACE" | "TRACK")
}

/// Methods that Node/WHATWG normalize to canonical uppercase when given
/// case-insensitively. PATCH and any extension method keep their original
/// casing (Node parity: `patch` stays `patch`).
pub(crate) fn normalize_method(raw: &str) -> String {
    let upper = raw.to_ascii_uppercase();
    match upper.as_str() {
        "DELETE" | "GET" | "HEAD" | "OPTIONS" | "POST" | "PUT" => upper,
        _ => raw.to_string(),
    }
}

pub(crate) fn canonical_reason(status: u16) -> &'static str {
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
