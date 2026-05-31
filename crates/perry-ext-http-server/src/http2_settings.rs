//! `node:http2` settings pack/unpack helpers (#3168).
//!
//! `getDefaultSettings()`, `getPackedSettings(settings)`, and
//! `getUnpackedSettings(buf)` are pure functions — no networking — but they
//! live here (the crate that owns the HTTP/2 surface) rather than in
//! `perry-runtime`, because `getPackedSettings` returns a `Buffer` and
//! `getUnpackedSettings` reads one. Buffer allocation/recognition must go
//! through the `perry-ffi` extern shims (`alloc_buffer`, `value_byte_slice`)
//! so the registry lookups resolve through the same runtime copy that the
//! program-side dispatch uses; a `perry-runtime`-internal implementation
//! hit the staticlib thread-local divergence (returned Buffers were not
//! recognized, input Buffers failed the registry check).
//!
//! HTTP/2 SETTINGS wire format: each setting is a 6-byte record of a
//! 2-byte big-endian identifier followed by a 4-byte big-endian value.
//! Packing emits records in a fixed identifier order; unpacking walks the
//! buffer in record order. Node-compatible validation throws
//! `ERR_HTTP2_INVALID_SETTING_VALUE`, `ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH`,
//! and `ERR_INVALID_ARG_TYPE`.

use perry_ffi::{
    alloc_buffer, alloc_string, json_stringify, throw_with_code, value_byte_slice, BufferHeader,
    ErrorKind, JsValue, StringHeader,
};
use serde_json::Value;

// SETTINGS identifiers (RFC 7540 §6.5.2 + RFC 8441).
const ID_HEADER_TABLE_SIZE: u16 = 0x1;
const ID_ENABLE_PUSH: u16 = 0x2;
const ID_MAX_CONCURRENT_STREAMS: u16 = 0x3;
const ID_INITIAL_WINDOW_SIZE: u16 = 0x4;
const ID_MAX_FRAME_SIZE: u16 = 0x5;
const ID_MAX_HEADER_LIST_SIZE: u16 = 0x6;
const ID_ENABLE_CONNECT_PROTOCOL: u16 = 0x8;

const U32_MAX: u64 = 0xFFFF_FFFF;
// maxFrameSize is constrained to [2^14, 2^24 - 1].
const FRAME_MIN: u64 = 16_384;
const FRAME_MAX: u64 = 16_777_215;

/// `http2.getDefaultSettings()` — Node's default SETTINGS object. The JSON
/// is hand-written so the key order survives the round trip through
/// `js_json_parse_or_null` and matches Node's `Object.keys()` ordering.
#[no_mangle]
pub extern "C" fn js_node_http2_get_default_settings() -> *mut StringHeader {
    const DEFAULTS: &str = concat!(
        "{",
        "\"headerTableSize\":4096,",
        "\"enablePush\":true,",
        "\"initialWindowSize\":65535,",
        "\"maxFrameSize\":16384,",
        "\"maxConcurrentStreams\":4294967295,",
        "\"maxHeaderSize\":65535,",
        "\"maxHeaderListSize\":65535,",
        "\"enableConnectProtocol\":false",
        "}"
    );
    alloc_string(DEFAULTS).as_raw()
}

/// `http2.getPackedSettings(settings)` — pack a SETTINGS object into a
/// Buffer. Validates each setting in identifier order; an out-of-range
/// numeric value throws a `RangeError` and a non-boolean push/connect flag
/// throws a `TypeError`, both with code `ERR_HTTP2_INVALID_SETTING_VALUE`.
#[no_mangle]
pub extern "C" fn js_node_http2_get_packed_settings(settings_bits: i64) -> *mut BufferHeader {
    let map = settings_to_map(JsValue::from_bits(settings_bits as u64));
    let mut out: Vec<u8> = Vec::new();

    if let Some(v) = map.get("headerTableSize") {
        push_record(
            &mut out,
            ID_HEADER_TABLE_SIZE,
            require_uint32("headerTableSize", v),
        );
    }
    if let Some(v) = map.get("enablePush") {
        push_record(
            &mut out,
            ID_ENABLE_PUSH,
            require_bool("enablePush", v) as u32,
        );
    }
    if let Some(v) = map.get("maxConcurrentStreams") {
        push_record(
            &mut out,
            ID_MAX_CONCURRENT_STREAMS,
            require_uint32("maxConcurrentStreams", v),
        );
    }
    if let Some(v) = map.get("initialWindowSize") {
        push_record(
            &mut out,
            ID_INITIAL_WINDOW_SIZE,
            require_uint32("initialWindowSize", v),
        );
    }
    if let Some(v) = map.get("maxFrameSize") {
        push_record(
            &mut out,
            ID_MAX_FRAME_SIZE,
            require_frame_size("maxFrameSize", v),
        );
    }
    // maxHeaderSize and maxHeaderListSize share identifier 6; when both are
    // present Node lets maxHeaderSize win.
    let max_header = map.get("maxHeaderSize");
    let max_header_list = map.get("maxHeaderListSize");
    if let Some(v) = max_header.or(max_header_list) {
        let name = if max_header.is_some() {
            "maxHeaderSize"
        } else {
            "maxHeaderListSize"
        };
        push_record(&mut out, ID_MAX_HEADER_LIST_SIZE, require_uint32(name, v));
    }
    if let Some(v) = map.get("enableConnectProtocol") {
        push_record(
            &mut out,
            ID_ENABLE_CONNECT_PROTOCOL,
            require_bool("enableConnectProtocol", v) as u32,
        );
    }

    alloc_buffer(&out)
}

/// `http2.getUnpackedSettings(buf)` — decode a packed SETTINGS Buffer into
/// an object. Throws `ERR_INVALID_ARG_TYPE` for non-Buffer/TypedArray
/// input and `ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH` when the byte
/// length is not a multiple of six.
#[no_mangle]
pub extern "C" fn js_node_http2_get_unpacked_settings(buf_bits: i64) -> *mut StringHeader {
    let value = JsValue::from_bits(buf_bits as u64);
    let bytes = match value_byte_slice(value) {
        Some(b) => b,
        None => throw_not_buffer(value),
    };
    if bytes.len() % 6 != 0 {
        throw_with_code(
            "Packed settings length must be a multiple of six",
            "ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH",
            ErrorKind::RangeError,
        );
    }

    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 6 <= bytes.len() {
        let id = u16::from_be_bytes([bytes[i], bytes[i + 1]]);
        let val = u32::from_be_bytes([bytes[i + 2], bytes[i + 3], bytes[i + 4], bytes[i + 5]]);
        match id {
            ID_HEADER_TABLE_SIZE => parts.push(format!("\"headerTableSize\":{val}")),
            ID_ENABLE_PUSH => parts.push(format!("\"enablePush\":{}", val != 0)),
            ID_MAX_CONCURRENT_STREAMS => parts.push(format!("\"maxConcurrentStreams\":{val}")),
            ID_INITIAL_WINDOW_SIZE => parts.push(format!("\"initialWindowSize\":{val}")),
            ID_MAX_FRAME_SIZE => parts.push(format!("\"maxFrameSize\":{val}")),
            ID_MAX_HEADER_LIST_SIZE => {
                // Node populates both aliases from identifier 6.
                parts.push(format!("\"maxHeaderSize\":{val}"));
                parts.push(format!("\"maxHeaderListSize\":{val}"));
            }
            ID_ENABLE_CONNECT_PROTOCOL => {
                parts.push(format!("\"enableConnectProtocol\":{}", val != 0))
            }
            _ => {} // Unknown settings are ignored, per the HTTP/2 spec.
        }
        i += 6;
    }
    let json = format!("{{{}}}", parts.join(","));
    alloc_string(&json).as_raw()
}

// ── helpers ──────────────────────────────────────────────────────────

fn push_record(out: &mut Vec<u8>, id: u16, value: u32) {
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&value.to_be_bytes());
}

/// JSON-stringify the settings object and reparse it into a plain map.
/// `undefined`/`null`/non-object inputs (including the `0` padding the
/// codegen passes for a missing argument) yield an empty map, matching
/// Node's default of `{}`.
fn settings_to_map(value: JsValue) -> serde_json::Map<String, Value> {
    if value.is_undefined() || value.is_null() {
        return serde_json::Map::new();
    }
    match json_stringify(value).and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

/// Accept an integer in `[min, max]`, returning it as `u32`.
fn int_in_range(v: &Value, min: u64, max: u64) -> Option<u32> {
    let n = v.as_f64()?;
    if !n.is_finite() || n.fract() != 0.0 || n < min as f64 || n > max as f64 {
        return None;
    }
    Some(n as u32)
}

fn require_uint32(name: &str, v: &Value) -> u32 {
    match int_in_range(v, 0, U32_MAX) {
        Some(u) => u,
        None => throw_invalid_setting(name, v, ErrorKind::RangeError),
    }
}

fn require_frame_size(name: &str, v: &Value) -> u32 {
    match int_in_range(v, FRAME_MIN, FRAME_MAX) {
        Some(u) => u,
        None => throw_invalid_setting(name, v, ErrorKind::RangeError),
    }
}

fn require_bool(name: &str, v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        _ => throw_invalid_setting(name, v, ErrorKind::TypeError),
    }
}

fn throw_invalid_setting(name: &str, v: &Value, kind: ErrorKind) -> ! {
    let msg = format!(
        "Invalid value for setting \"{}\": {}",
        name,
        fmt_setting_value(v)
    );
    throw_with_code(&msg, "ERR_HTTP2_INVALID_SETTING_VALUE", kind);
}

/// Format a setting value the way Node's `String(value)` would for the
/// error message (numbers without JSON quoting; strings bare).
fn fmt_setting_value(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn throw_not_buffer(value: JsValue) -> ! {
    let msg = format!(
        "The \"buf\" argument must be an instance of Buffer or TypedArray. {}",
        describe_received(value)
    );
    throw_with_code(&msg, "ERR_INVALID_ARG_TYPE", ErrorKind::TypeError);
}

/// The "Received ..." clause Node appends to `ERR_INVALID_ARG_TYPE`.
fn describe_received(value: JsValue) -> String {
    if value.is_null() {
        "Received null".to_string()
    } else if value.is_undefined() {
        "Received undefined".to_string()
    } else if value.is_bool() {
        format!("Received type boolean ({})", value.to_bool())
    } else if value.is_int32() || value.is_number() {
        format!("Received type number ({})", fmt_number(value.to_number()))
    } else if value.is_string() {
        format!("Received type string ('{}')", read_js_string(value))
    } else {
        "Received an instance of Object".to_string()
    }
}

fn fmt_number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn read_js_string(value: JsValue) -> String {
    let ptr = value.as_string_ptr();
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: `ptr` is a STRING_TAG-tagged StringHeader; bytes follow the
    // header and are bounded by `byte_len`.
    unsafe {
        let header = &*ptr;
        let len = header.byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}
