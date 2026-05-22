//! Shared NaN-boxing constants, runtime extern declarations, and
//! port/host extraction helpers.

use perry_ffi::{BufferHeader, JsValue, StringHeader};

pub const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
pub const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;

// Runtime symbols not yet wrapped by perry-ffi — declared locally.
extern "C" {
    pub fn js_promise_run_microtasks() -> i32;
    pub fn js_promise_state(ptr: *mut Promise) -> i32;
    pub fn js_promise_reason(ptr: *mut Promise) -> f64;
    pub fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
    /// Issue #1124 — returns 1 if `ptr` is a registered Buffer / Uint8Array
    /// in the runtime's BUFFER_REGISTRY (which is the only safe way to tell
    /// a `BufferHeader` apart from a `StringHeader` after both have been
    /// NaN-boxed with POINTER_TAG and stripped to a raw pointer). Defined
    /// in `crates/perry-runtime/src/buffer.rs::js_buffer_is_buffer`.
    pub fn js_buffer_is_buffer(ptr: i64) -> i32;
}

/// Opaque marker for the runtime's Promise struct — pass pointers
/// only; never read fields.
#[repr(C)]
pub struct Promise {
    _opaque: [u8; 0],
}

/// Extract a port from `{ port }` object, bare number, or fall back.
/// `default_port` is used when neither shape yields a usable value.
///
/// Node treats `0` as a request for an OS-assigned ephemeral port — it's
/// *not* the "missing value" sentinel. Pre-fix #1121, `extract_port`
/// returned `default_port` for `listen(0)` so user code that asked for
/// ephemeral binding ended up clashing on the default 3000 / 8080.
pub unsafe fn extract_port(opts: f64, default_port: u16) -> u16 {
    let v = JsValue::from_bits(opts.to_bits());
    if v.is_pointer() {
        if let Some(json) = perry_ffi::json_stringify(v) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(p) = parsed.get("port").and_then(|p| {
                    p.as_u64()
                        .or_else(|| p.as_i64().map(|n| n.max(0) as u64))
                        .or_else(|| p.as_f64().map(|n| n.max(0.0) as u64))
                }) {
                    return p as u16;
                }
            }
        }
        return default_port;
    }
    if v.is_number() {
        let n = v.to_number();
        if n.is_finite() && n >= 0.0 && n <= u16::MAX as f64 {
            return n as u16;
        }
    }
    default_port
}

/// Extract a hostname from `{ host }` object literal, falling back
/// to "0.0.0.0". Standalone hostname-as-string is also accepted (for
/// the `listen(port, hostname, cb)` overload).
pub unsafe fn extract_host(opts: f64, default_host: &str) -> String {
    let v = JsValue::from_bits(opts.to_bits());
    if v.is_string() {
        if let Some(s) = jsvalue_to_owned_string(opts) {
            return s;
        }
    }
    if v.is_pointer() {
        if let Some(json) = perry_ffi::json_stringify(v) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
                // Node accepts both `host` and `hostname`.
                if let Some(h) = parsed
                    .get("hostname")
                    .or_else(|| parsed.get("host"))
                    .and_then(|h| h.as_str())
                {
                    return h.to_string();
                }
            }
        }
    }
    default_host.to_string()
}

/// Read a NaN-boxed JsValue as an owned String. Used for both
/// `IncomingMessage.on(eventName, cb)` event-name extraction and
/// for `ServerResponse.write/end(chunk)` body extraction.
pub fn jsvalue_to_owned_string(value: f64) -> Option<String> {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    if v.is_string() {
        let bits = value.to_bits();
        let ptr = (bits & PTR_MASK) as *mut StringHeader;
        if ptr.is_null() {
            return None;
        }
        return read_string_header(ptr);
    }
    if v.is_number() {
        return Some(v.to_number().to_string());
    }
    if v.is_bool() {
        return Some(if v.to_bool() { "true" } else { "false" }.to_string());
    }
    // Object / array — JSON-stringify so chained `res.end(obj)` writes
    // something rather than nothing.
    if v.is_pointer() {
        unsafe {
            let str_ptr = js_json_stringify(value, 0);
            if !str_ptr.is_null() {
                return read_string_header(str_ptr);
            }
        }
    }
    None
}

/// Read a NaN-boxed JsValue as raw bytes for response body output.
/// Distinguished from `jsvalue_to_owned_string` because Buffer / Uint8Array
/// chunks must preserve binary contents (no UTF-8 round-trip).
pub fn jsvalue_to_body_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    if v.is_string() {
        let bits = value.to_bits();
        let ptr = (bits & PTR_MASK) as *mut StringHeader;
        if ptr.is_null() {
            return None;
        }
        return read_string_header_bytes(ptr);
    }
    // Issue #1124 — Buffer / Uint8Array do NOT follow the
    // `StringHeader` layout: `BufferHeader` is `{ length: u32,
    // capacity: u32 }` (8 bytes, data immediately after), while
    // `StringHeader` is `{ utf16_len, byte_len, capacity, refcount,
    // flags }` (20 bytes, data after that). Reading a buffer through
    // the string-shaped header used to surface the buffer's
    // `capacity` slot as `byte_len` (often equal to the requested
    // size, so the length was preserved) but indexed the data from
    // `ptr + sizeof(StringHeader)` — past the actual bytes — so the
    // wire body was all zeros (#1124 repro). Probe the runtime's
    // `BUFFER_REGISTRY` first to pick the correct layout; fall back
    // to `StringHeader` only for non-buffer pointer-tagged values
    // (the existing string-body path still has to work via this
    // branch when the caller already pre-strung a value into the
    // string-tag slot, e.g. some chunked `res.write(stringValue)`
    // call sites).
    if v.is_pointer() {
        let bits = value.to_bits();
        let raw = (bits & PTR_MASK) as i64;
        // SAFETY: `js_buffer_is_buffer` is a C-exposed registry check
        // that handles null / sub-0x1000 garbage internally.
        let is_buffer = unsafe { js_buffer_is_buffer(raw) } != 0;
        if is_buffer {
            let buf = raw as *const BufferHeader;
            if !buf.is_null() {
                unsafe {
                    let len = (*buf).length as usize;
                    let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
                    let slice = std::slice::from_raw_parts(data, len);
                    return Some(slice.to_vec());
                }
            }
        }
        // Non-buffer pointer — try the string-shaped header (shared
        // layout for runtime strings the codegen NaN-boxed as
        // POINTER_TAG instead of STRING_TAG).
        let ptr = raw as *mut StringHeader;
        if !ptr.is_null() {
            if let Some(b) = read_string_header_bytes(ptr) {
                return Some(b);
            }
        }
        // Fallback: stringify (objects → JSON).
        if let Some(s) = jsvalue_to_owned_string(value) {
            return Some(s.into_bytes());
        }
    }
    if v.is_number() {
        return Some(v.to_number().to_string().into_bytes());
    }
    if v.is_bool() {
        return Some(
            if v.to_bool() { "true" } else { "false" }
                .to_string()
                .into_bytes(),
        );
    }
    None
}

/// Read a `StringHeader` as a Rust `String`, copying its bytes.
pub(crate) fn read_string_header(ptr: *mut StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data, len);
        Some(String::from_utf8_lossy(slice).into_owned())
    }
}

/// Read a `StringHeader` as raw bytes — used when the payload is
/// not necessarily UTF-8 (Buffer / Uint8Array round-trip).
pub(crate) fn read_string_header_bytes(ptr: *mut StringHeader) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data, len);
        Some(slice.to_vec())
    }
}

#[allow(dead_code)]
pub(crate) unsafe fn _force_promise_reason_link(p: *mut Promise) -> f64 {
    js_promise_reason(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_zero_is_kept_for_ephemeral_binding() {
        // Node semantic: `listen(0)` asks the OS to pick a free port. The
        // pre-#1121 check (`n > 0.0`) treated 0 as "missing" and fell back
        // to default_port — so user code that asked for ephemeral binding
        // collided on the default 3000 / 8080 instead.
        let port = unsafe { extract_port(0.0_f64, 8080) };
        assert_eq!(
            port, 0,
            "extract_port(0.0) should return 0, not the fallback"
        );
    }

    #[test]
    fn finite_positive_port_is_kept() {
        let port = unsafe { extract_port(4242.0_f64, 8080) };
        assert_eq!(port, 4242);
    }

    #[test]
    fn negative_or_nan_or_inf_falls_back_to_default() {
        assert_eq!(unsafe { extract_port(-1.0_f64, 8080) }, 8080);
        assert_eq!(unsafe { extract_port(f64::NAN, 8080) }, 8080);
        assert_eq!(unsafe { extract_port(f64::INFINITY, 8080) }, 8080);
    }

    #[test]
    fn out_of_range_port_falls_back_to_default() {
        // Anything above 65535 isn't a valid TCP port — fall back rather
        // than wrap on the `as u16` cast (which would silently bind on
        // a wrong, low-numbered port).
        let port = unsafe { extract_port(70000.0_f64, 8080) };
        assert_eq!(port, 8080);
    }
}
