//! Shared NaN-boxing constants, runtime extern declarations, and
//! port/host extraction helpers.

use perry_ffi::{ArrayHeader, BufferHeader, JsValue, StringHeader};

pub const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
pub const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
pub const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;

// Runtime symbols not yet wrapped by perry-ffi — declared locally.
extern "C" {
    pub fn js_promise_run_microtasks() -> i32;
    /// #6710 — clear every per-handle JS-property side table (string expando +
    /// symbol props/attrs/accessors) for a recycled handle id, so a reused
    /// `IncomingMessage`/`ServerResponse` id does not carry the previous
    /// request's own props (Next.js `isRSCRequest` / `NextInternalRequestMeta`).
    /// Must be called on the main thread. Defined in
    /// `crates/perry-runtime/src/symbol/properties.rs::js_handle_clear_side_tables`.
    pub fn js_handle_clear_side_tables(handle: i64);
    pub fn js_promise_state(ptr: *mut Promise) -> i32;
    pub fn js_promise_reason(ptr: *mut Promise) -> f64;
    pub fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
    /// Issue #1124 — returns 1 if `ptr` is a registered Buffer / Uint8Array
    /// in the runtime's BUFFER_REGISTRY (which is the only safe way to tell
    /// a `BufferHeader` apart from a `StringHeader` after both have been
    /// NaN-boxed with POINTER_TAG and stripped to a raw pointer). Defined
    /// in `crates/perry-runtime/src/buffer.rs::js_buffer_is_buffer`.
    pub fn js_buffer_is_buffer(ptr: i64) -> i32;
    /// Issue #2041 — returns 1 when the NaN-boxed value bits are a
    /// closure/function (POINTER_TAG + CLOSURE_MAGIC), 0 otherwise. Lets
    /// `parse_listen_args` pick the `server.listen()` completion callback out
    /// of the argument array by type rather than position, and distinguish it
    /// from an options-object argument. Defined in
    /// `crates/perry-runtime/src/closure/dynamic_props.rs::js_value_is_closure`.
    pub fn js_value_is_closure(value_bits: i64) -> i32;
    /// #4965 — normalize a `res.setHeaders(x)` argument into a JSON
    /// `[name, value]` entries array (value is a string, or an array of
    /// strings for multi-valued headers like `Set-Cookie`). Returns null when
    /// `x` is neither a `Headers` nor a `Map` (→ `ERR_INVALID_ARG_TYPE`).
    /// Classifies by address band so a `Headers` registry *handle* is never
    /// dereferenced as a heap object. Defined in
    /// `crates/perry-runtime/src/object/global_fetch.rs`.
    pub fn js_node_setheaders_entries_json(value: f64) -> *mut StringHeader;
    /// #1781 — materialize ANY string representation (heap
    /// `STRING_TAG` *or* inline `SHORT_STRING_TAG` SSO) to a real
    /// `*const StringHeader`, returned as `i64`. SSO bytes are copied
    /// onto the heap; a heap string returns its existing pointer.
    /// Returns 0 for non-strings. Defined in
    /// `crates/perry-runtime/src/value/nanbox.rs::js_get_string_pointer_unified`.
    pub fn js_get_string_pointer_unified(value: f64) -> i64;
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

/// Parsed shape of Node's variadic `server.listen(...)` overloads, shared by
/// the http / https / http2 bind paths. Issue #2041.
pub struct ListenArgs {
    /// First positional argument — a bare port number, an options object
    /// (`{ port, host, backlog }`), or `undefined`. Fed to `extract_port`
    /// and `extract_host`.
    pub opts: f64,
    /// A standalone host string argument (the `listen(port, host, ...)`
    /// overload). Takes precedence over a `host` field inside an options
    /// object when both are present.
    pub host: Option<String>,
    /// Raw `*const ClosureHeader` pointer for the completion callback, or `0`
    /// when no function argument was supplied.
    pub callback: i64,
}

/// Normalize the JS-side `server.listen(...)` argument array into the
/// `(opts, host, callback)` triple the bind path consumes.
///
/// `args_array` is a raw `*const ArrayHeader` (NOT NaN-boxed) holding every
/// user-supplied `listen()` argument — codegen packs them via the `NA_VARARGS`
/// arg kind. Resolution mirrors Node's type-directed overload handling: the
/// first arg is the port / options / path; a later string arg is the host; the
/// single function arg is the completion callback regardless of its position
/// (`listen(port, cb)` vs `listen(port, host, cb)`); a numeric backlog arg is
/// accepted and ignored (Perry exposes no backlog knob). The degenerate
/// `listen(cb)` form (first and only arg is a function) is handled too.
///
/// # Safety
/// `args_array` must be `0`/null or a valid Perry-runtime `ArrayHeader`.
pub unsafe fn parse_listen_args(args_array: i64) -> ListenArgs {
    let mut out = ListenArgs {
        opts: f64::from_bits(TAG_UNDEFINED),
        host: None,
        callback: 0,
    };
    let arr_ptr = args_array as *const ArrayHeader;
    if arr_ptr.is_null() {
        return out;
    }
    // Codegen passes a clean raw pointer; reject a stray NaN-boxed value
    // rather than dereferencing tag bits as an address.
    if (args_array as u64) >> 48 != 0 {
        return out;
    }
    let len = (*arr_ptr).length as usize;
    let elements = (arr_ptr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    for i in 0..len {
        let bits = *elements.add(i);
        let v = JsValue::from_bits(bits);
        // The completion callback is the (single) function argument — match it
        // by value type, not position, so it's picked up wherever it floats.
        let is_callback = js_value_is_closure(bits as i64) != 0;
        if is_callback {
            out.callback = (bits & PTR_MASK) as i64;
            continue;
        }
        if i == 0 {
            // port / options / path.
            out.opts = f64::from_bits(bits);
            continue;
        }
        if v.is_string() {
            if let Some(s) = jsvalue_to_owned_string(f64::from_bits(bits)) {
                out.host = Some(s);
            }
        }
        // A numeric backlog (or anything else) at i > 0 is ignored.
    }
    out
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
    // JS string — heap STRING_TAG *or* inline SSO SHORT_STRING_TAG.
    // #1781: the strict `is_string()` matched STRING_TAG only, so a
    // short response body (`res.end("hi")`) fell through every branch
    // to `None` and was silently dropped. Gate on `is_any_string()`
    // and materialize via `js_get_string_pointer_unified`, which copies
    // SSO bytes onto the heap (and returns the existing pointer for a
    // heap string), so the `StringHeader` read works for both reprs.
    if v.is_any_string() {
        let ptr = unsafe { js_get_string_pointer_unified(value) } as *mut StringHeader;
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

    // Build a JS array from a slice of JsValues, the shape codegen's
    // `NA_VARARGS` arg kind hands `js_node_http_server_listen` /
    // `parse_listen_args`.
    unsafe fn make_args(vals: &[JsValue]) -> i64 {
        let mut arr = perry_ffi::js_array_alloc(vals.len() as u32);
        for v in vals {
            arr = perry_ffi::js_array_push(arr, *v);
        }
        arr as i64
    }

    #[test]
    fn listen_null_array_is_all_defaults() {
        // No args → no opts/host/callback; the bind path falls back to its
        // module default port + 0.0.0.0.
        let parsed = unsafe { parse_listen_args(0) };
        assert!(JsValue::from_bits(parsed.opts.to_bits()).is_undefined());
        assert!(parsed.host.is_none());
        assert_eq!(parsed.callback, 0);
    }

    #[test]
    fn listen_port_then_host_string_overload() {
        // `listen(port, host)` — the standalone host string (#2041) must land
        // in `host`, not be mistaken for a callback, and `opts` keeps the port.
        let args = unsafe {
            make_args(&[
                JsValue::from_number(44500.0),
                JsValue::from_string_ptr(perry_ffi::alloc_string("127.0.0.1").as_raw()),
            ])
        };
        let parsed = unsafe { parse_listen_args(args) };
        assert_eq!(unsafe { extract_port(parsed.opts, 3000) }, 44500);
        assert_eq!(parsed.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(
            parsed.callback, 0,
            "a host string must not be read as a callback"
        );
    }

    /// Encode `bytes` (len ≤ 5) as an inline SSO `SHORT_STRING_TAG`
    /// NaN-box, mirroring the runtime's `JSValue::try_short_string`:
    /// tag 0x7FF9, length in bits 40..=47, data little-endian in bits
    /// 0..=39. This is the exact representation codegen hands the
    /// `res.write` / `res.end` shim for a short string literal — the
    /// repr `jsvalue_to_body_bytes` must not drop.
    fn sso(bytes: &[u8]) -> f64 {
        assert!(bytes.len() <= 5);
        let mut payload: u64 = 0;
        for (i, &b) in bytes.iter().enumerate() {
            payload |= (b as u64) << (i * 8);
        }
        let bits = 0x7FF9_0000_0000_0000_u64 | ((bytes.len() as u64) << 40) | payload;
        f64::from_bits(bits)
    }

    /// A short response body (`res.end("hi")`) arrives as an inline SSO
    /// value, not a `STRING_TAG` heap pointer. Gating on the strict
    /// `is_string()` (STRING_TAG only) makes the SSO value match no branch,
    /// so `jsvalue_to_body_bytes` silently drops the body (returns `None`)
    /// and the wire response has an empty body. It must convert to its
    /// UTF-8 bytes, including the empty string and the 5-byte SSO boundary.
    #[test]
    fn body_bytes_converts_sso_short_string() {
        assert_eq!(
            jsvalue_to_body_bytes(sso(b"hi")).as_deref(),
            Some(&b"hi"[..]),
            "SSO short body must convert to its bytes, not be dropped"
        );
        assert_eq!(
            jsvalue_to_body_bytes(sso(b"")).as_deref(),
            Some(&b""[..]),
            "empty SSO body should yield empty bytes, not None"
        );
        assert_eq!(
            jsvalue_to_body_bytes(sso(b"hello")).as_deref(),
            Some(&b"hello"[..]),
            "the 5-byte SSO boundary must convert"
        );
    }

    /// A heap `STRING_TAG` body (length > 5) still converts — widening the
    /// string branch to match SSO must not regress the long-string path
    /// that already worked.
    #[test]
    fn body_bytes_converts_heap_string() {
        let v = JsValue::from_string_ptr(perry_ffi::alloc_string("longer-than-sso").as_raw());
        assert_eq!(
            jsvalue_to_body_bytes(f64::from_bits(v.bits())).as_deref(),
            Some(&b"longer-than-sso"[..])
        );
    }

    #[test]
    fn listen_non_closure_pointer_is_not_mistaken_for_callback() {
        // The first arg of `listen(options)` / `listen(path-ish)` is a heap
        // pointer (POINTER_TAG, like a closure) but NOT a closure. It must stay
        // in `opts` and must not be captured as the callback — guards the
        // `js_value_is_closure` check that tells `listen(options)` apart from
        // `listen(cb)`. An array stands in for any non-closure heap pointer.
        let inner = unsafe { perry_ffi::js_array_alloc(0) };
        let args = unsafe { make_args(&[JsValue::from_object_ptr(inner)]) };
        let parsed = unsafe { parse_listen_args(args) };
        assert_eq!(
            parsed.callback, 0,
            "a non-closure pointer must not be read as a callback"
        );
        assert!(parsed.host.is_none());
        assert!(
            JsValue::from_bits(parsed.opts.to_bits()).is_pointer(),
            "the pointer arg should be retained as opts, not dropped"
        );
    }
}
