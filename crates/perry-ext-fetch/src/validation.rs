//! Web Fetch constructor validation helpers shared by the Response and
//! Request constructors (`js_response_new` / `js_request_new`). Mirrors the
//! WHATWG fetch spec rules Node enforces; refs #2640 (Response status /
//! statusText validation + empty-string default) and #2643 (Request method
//! normalization + forbidden methods + GET/HEAD body rejection).

use perry_ffi::{alloc_string, JsValue, StringHeader};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

// Web Fetch constructor validation throws real TypeError / RangeError
// objects. perry-ffi doesn't re-export the runtime error/throw entry
// points, but with the `runtime-link` feature the `#[no_mangle]` symbols
// from perry-runtime are present in the final link, so we declare them
// here. (Mirrors perry-stdlib's `throw_fetch_type_error`, which reaches
// them through the `perry_runtime::` path.)
extern "C" {
    fn js_typeerror_new(message: *mut StringHeader) -> *mut u8;
    fn js_rangeerror_new(message: *mut StringHeader) -> *mut u8;
    fn js_throw(value: f64) -> !;
}

pub(crate) unsafe fn throw_type_error(msg: &str) -> ! {
    let m = alloc_string(msg).as_raw();
    let err = js_typeerror_new(m);
    js_throw(f64::from_bits(JsValue::from_object_ptr(err).bits()));
}

pub(crate) unsafe fn throw_range_error(msg: &str) -> ! {
    let m = alloc_string(msg).as_raw();
    let err = js_rangeerror_new(m);
    js_throw(f64::from_bits(JsValue::from_object_ptr(err).bits()));
}

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

pub(crate) fn redirect_status_from_value(status: f64) -> i32 {
    if status.to_bits() == TAG_UNDEFINED {
        return 302;
    }
    let number = JsValue::from_bits(status.to_bits()).to_number();
    if !number.is_finite() {
        return 0;
    }
    (number.trunc() % 65536.0) as i32
}

pub(crate) fn is_redirect_status(status: i32) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

pub(crate) fn parse_redirect_location(raw: &str) -> Result<String, ()> {
    reqwest::Url::parse(raw)
        .map(|parsed| parsed.to_string())
        .map_err(|_| ())
}
