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
    alloc_buffer, alloc_string, json_stringify, object_field_by_name, throw_with_code,
    value_byte_slice, BufferHeader, ErrorKind, JsValue, StringHeader, TransientRootScope,
    TransientRootedNanbox,
};
use serde_json::Value;
use std::collections::BTreeMap;

// SETTINGS identifiers (RFC 7540 §6.5.2 + RFC 8441).
const ID_HEADER_TABLE_SIZE: u16 = 0x1;
const ID_ENABLE_PUSH: u16 = 0x2;
const ID_MAX_CONCURRENT_STREAMS: u16 = 0x3;
const ID_INITIAL_WINDOW_SIZE: u16 = 0x4;
const ID_MAX_FRAME_SIZE: u16 = 0x5;
const ID_MAX_HEADER_LIST_SIZE: u16 = 0x6;
const ID_ENABLE_CONNECT_PROTOCOL: u16 = 0x8;

extern "C" {
    fn js_jsvalue_to_string(value: f64) -> *mut StringHeader;
}

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
    // Rooted: the JSON round trip allocates, and nonfinite values are read
    // back from the object afterwards.
    let roots = TransientRootScope::enter();
    let settings = roots.root_nanbox(f64::from_bits(settings_bits as u64));
    let map = settings_to_map(JsValue::from_bits(settings.get().to_bits()));
    let mut out: Vec<u8> = Vec::new();

    if let Some(v) = map.get("headerTableSize") {
        push_record(
            &mut out,
            ID_HEADER_TABLE_SIZE,
            require_uint32(
                "headerTableSize",
                setting_number(&settings, "headerTableSize", v),
            ),
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
            require_uint32(
                "maxConcurrentStreams",
                setting_number(&settings, "maxConcurrentStreams", v),
            ),
        );
    }
    if let Some(v) = map.get("initialWindowSize") {
        // HTTP/2 flow-control windows are limited to 2^31 - 1, not u32::MAX.
        let v = setting_number(&settings, "initialWindowSize", v);
        let size = match int_in_range(&v, 0, i32::MAX as u64) {
            Some(size) => size,
            None => throw_invalid_number("initialWindowSize", &v),
        };
        push_record(&mut out, ID_INITIAL_WINDOW_SIZE, size);
    }
    if let Some(v) = map.get("maxFrameSize") {
        push_record(
            &mut out,
            ID_MAX_FRAME_SIZE,
            require_frame_size("maxFrameSize", setting_number(&settings, "maxFrameSize", v)),
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
        push_record(
            &mut out,
            ID_MAX_HEADER_LIST_SIZE,
            require_uint32(name, setting_number(&settings, name, v)),
        );
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
    let len = perry_ffi::bytes::no_gc(|scope| value_byte_slice(value, scope).map(<[u8]>::len));
    let Some(len) = len else {
        throw_not_buffer(value)
    };
    if len % 6 != 0 {
        throw_with_code(
            "Packed settings length must be a multiple of six",
            "ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH",
            ErrorKind::RangeError,
        );
    }
    let json = perry_ffi::bytes::no_gc(|scope| {
        let bytes = value_byte_slice(value, scope).expect("validated byte value");
        let mut parts: Vec<String> = Vec::new();
        let mut custom_settings = BTreeMap::new();
        let mut custom_position = None;
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
                _ => {
                    // Node exposes unknown IDs, with the container inserted at
                    // the first unknown record and duplicate IDs last-wins.
                    if custom_position.is_none() {
                        custom_position = Some(parts.len());
                        parts.push(String::new());
                    }
                    custom_settings.insert(id, val);
                }
            }
            i += 6;
        }
        if let Some(position) = custom_position {
            // All u16 IDs are JS array-index keys, so enumerate them numerically.
            let custom_parts: Vec<String> = custom_settings
                .iter()
                .map(|(id, val)| format!("\"{id}\":{val}"))
                .collect();
            parts[position] = format!("\"customSettings\":{{{}}}", custom_parts.join(","));
        }
        format!("{{{}}}", parts.join(","))
    });
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

/// A numeric setting value. JSON erases NaN/±Infinity to `null`, so those
/// are carried as the raw number read back from the settings object.
enum SettingNumber<'a> {
    Json(&'a Value),
    NonFinite(f64),
}

fn setting_number<'a>(
    settings: &TransientRootedNanbox,
    name: &str,
    v: &'a Value,
) -> SettingNumber<'a> {
    if v.is_null() {
        let raw = object_field_by_name(JsValue::from_bits(settings.get().to_bits()), name);
        let n = f64::from_bits(raw.bits());
        // NaN-boxed tags are NaNs too; only the untagged bands are JS NaN.
        let is_js_nan = n.is_nan() && (raw.bits() >> 48) & 0x7FFF < 0x7FFA;
        if !raw.is_int32() && (n.is_infinite() || is_js_nan) {
            return SettingNumber::NonFinite(n);
        }
    }
    SettingNumber::Json(v)
}

/// Accept a number whose raw value lies in `[min, max]`, truncated to `u32`
/// like Node's uint32 packing (`1.5` packs as `1`, `-0.5` is out of range).
/// NaN fails neither comparison, so it packs as `0` like in Node.
fn int_in_range(v: &SettingNumber, min: u64, max: u64) -> Option<u32> {
    let n = match v {
        SettingNumber::Json(v) => v.as_f64()?,
        SettingNumber::NonFinite(n) => *n,
    };
    if n < min as f64 || n > max as f64 {
        return None;
    }
    Some(n as u32)
}

fn require_uint32(name: &str, v: SettingNumber) -> u32 {
    match int_in_range(&v, 0, U32_MAX) {
        Some(u) => u,
        None => throw_invalid_number(name, &v),
    }
}

fn require_frame_size(name: &str, v: SettingNumber) -> u32 {
    match int_in_range(&v, FRAME_MIN, FRAME_MAX) {
        Some(u) => u,
        None => throw_invalid_number(name, &v),
    }
}

fn throw_invalid_number(name: &str, v: &SettingNumber) -> ! {
    match v {
        SettingNumber::Json(v) => throw_invalid_setting(name, v, ErrorKind::RangeError),
        SettingNumber::NonFinite(n) => {
            // SAFETY: a plain number needs no rooting; the result is a live string.
            let text = unsafe { js_jsvalue_to_string(*n) };
            let msg = format!(
                "Invalid value for setting \"{}\": {}",
                name,
                read_js_string(JsValue::from_string_ptr(text))
            );
            throw_with_code(
                &msg,
                "ERR_HTTP2_INVALID_SETTING_VALUE",
                ErrorKind::RangeError,
            );
        }
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
    } else if value.is_short_string() {
        format!(
            "Received type string ('{}')",
            value.to_owned_string().unwrap_or_default()
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn unpack(records: &[(u16, u32)]) -> String {
        let mut bytes = Vec::new();
        for &(id, value) in records {
            push_record(&mut bytes, id, value);
        }
        let buffer = alloc_buffer(&bytes);
        let result =
            js_node_http2_get_unpacked_settings(JsValue::from_object_ptr(buffer).bits() as i64);
        read_js_string(JsValue::from_string_ptr(result))
    }

    fn pack(name: &str, value: impl std::fmt::Display) -> Result<Vec<u8>, f64> {
        let json = alloc_string(&format!("{{\"{name}\":{value}}}"));
        // SAFETY: the allocated JSON string is live for the parse call.
        let settings = unsafe { perry_runtime::json::js_json_parse(json.as_raw().cast()) };
        pack_object(JsValue::from_bits(settings.bits()))
    }

    fn pack_object(settings: JsValue) -> Result<Vec<u8>, f64> {
        perry_runtime::exception::catch_js_throw(|| {
            let buffer = js_node_http2_get_packed_settings(settings.bits() as i64);
            perry_ffi::bytes::no_gc(|scope| {
                value_byte_slice(JsValue::from_object_ptr(buffer), scope)
                    .expect("packed settings must be a Buffer")
                    .to_vec()
            })
        })
    }

    fn error_field(error: f64, name: &str) -> String {
        let key = alloc_string(name);
        let value = perry_runtime::object::js_object_get_field_by_name(
            error.to_bits() as *const perry_runtime::ObjectHeader,
            key.as_raw().cast(),
        );
        read_js_string(JsValue::from_bits(value.bits()))
    }

    #[test]
    fn packed_initial_window_size_boundaries() {
        for value in [0, i32::MAX as u32] {
            let mut expected = Vec::new();
            push_record(&mut expected, ID_INITIAL_WINDOW_SIZE, value);
            assert_eq!(pack("initialWindowSize", value).unwrap(), expected);
        }
        for (name, id) in [
            ("headerTableSize", ID_HEADER_TABLE_SIZE),
            ("maxConcurrentStreams", ID_MAX_CONCURRENT_STREAMS),
            ("maxHeaderListSize", ID_MAX_HEADER_LIST_SIZE),
            ("maxHeaderSize", ID_MAX_HEADER_LIST_SIZE),
        ] {
            let mut expected = Vec::new();
            push_record(&mut expected, id, u32::MAX);
            assert_eq!(pack(name, u32::MAX).unwrap(), expected, "{name}");
        }
        for value in [1_u32 << 31, u32::MAX] {
            let error = pack("initialWindowSize", value)
                .expect_err("initialWindowSize must reject values at or above 2^31");
            assert_eq!(error_field(error, "name"), "RangeError");
            assert_eq!(
                error_field(error, "code"),
                "ERR_HTTP2_INVALID_SETTING_VALUE"
            );
            assert_eq!(
                error_field(error, "message"),
                format!("Invalid value for setting \"initialWindowSize\": {value}")
            );
        }
    }

    #[test]
    fn packed_fractional_values_range_check_raw_then_truncate() {
        for (name, id, value, packed) in [
            ("headerTableSize", ID_HEADER_TABLE_SIZE, 1.5, 1),
            ("maxConcurrentStreams", ID_MAX_CONCURRENT_STREAMS, 7.25, 7),
            (
                "initialWindowSize",
                ID_INITIAL_WINDOW_SIZE,
                2147483646.9,
                i32::MAX as u32 - 1,
            ),
            ("maxFrameSize", ID_MAX_FRAME_SIZE, 16384.5, 16384),
            ("maxHeaderListSize", ID_MAX_HEADER_LIST_SIZE, 0.9, 0),
        ] {
            let mut expected = Vec::new();
            push_record(&mut expected, id, packed);
            assert_eq!(pack(name, value).unwrap(), expected, "{name} {value}");
        }
        for (name, value) in [
            ("headerTableSize", -0.5),
            ("headerTableSize", 4294967295.5),
            ("initialWindowSize", 2147483647.5),
            ("maxFrameSize", 16383.5),
            ("maxFrameSize", 16777215.5),
        ] {
            let error = pack(name, value).expect_err("raw value is out of range");
            assert_eq!(error_field(error, "name"), "RangeError");
            assert_eq!(
                error_field(error, "message"),
                format!("Invalid value for setting \"{name}\": {value}")
            );
        }
    }

    #[test]
    fn packed_nonfinite_values_pack_nan_as_zero_and_reject_infinity() {
        // JSON.parse cannot produce NaN/Infinity, so build the object directly.
        for (name, id) in [
            ("headerTableSize", ID_HEADER_TABLE_SIZE),
            ("maxConcurrentStreams", ID_MAX_CONCURRENT_STREAMS),
            ("initialWindowSize", ID_INITIAL_WINDOW_SIZE),
            ("maxHeaderListSize", ID_MAX_HEADER_LIST_SIZE),
        ] {
            let settings =
                perry_ffi::alloc_null_proto_object(&[(name, JsValue::from_number(f64::NAN))]);
            let mut expected = Vec::new();
            push_record(&mut expected, id, 0);
            assert_eq!(pack_object(settings).unwrap(), expected, "{name} NaN");
            for (value, text) in [
                (f64::INFINITY, "Infinity"),
                (f64::NEG_INFINITY, "-Infinity"),
            ] {
                let settings =
                    perry_ffi::alloc_null_proto_object(&[(name, JsValue::from_number(value))]);
                let error = pack_object(settings).expect_err("infinite settings are out of range");
                assert_eq!(error_field(error, "name"), "RangeError");
                assert_eq!(
                    error_field(error, "code"),
                    "ERR_HTTP2_INVALID_SETTING_VALUE"
                );
                assert_eq!(
                    error_field(error, "message"),
                    format!("Invalid value for setting \"{name}\": {text}")
                );
            }
        }
    }

    #[test]
    fn unpacked_custom_settings_preserve_wire_position_and_numeric_order() {
        assert_eq!(
            unpack(&[
                (1, 42),
                (9999, 301),
                (8, 1),
                (65535, u32::MAX),
                (9, 10),
                (7, 1),
                (0, 2),
                (9999, 3),
                (65535, 0),
                (6, u32::MAX)
            ]),
            concat!(
                "{\"headerTableSize\":42,",
                "\"customSettings\":{\"0\":2,\"7\":1,\"9\":10,\"9999\":3,\"65535\":0},",
                "\"enableConnectProtocol\":true,",
                "\"maxHeaderSize\":4294967295,\"maxHeaderListSize\":4294967295}"
            )
        );
    }

    #[test]
    fn unpacked_custom_settings_unknown_only_and_first_record() {
        assert_eq!(
            unpack(&[(9999, 301)]),
            "{\"customSettings\":{\"9999\":301}}"
        );
        assert_eq!(
            unpack(&[(65535, u32::MAX), (1, 42), (7, 0)]),
            "{\"customSettings\":{\"7\":0,\"65535\":4294967295},\"headerTableSize\":42}"
        );
    }

    #[test]
    fn unpacked_custom_settings_omitted_for_known_records() {
        assert_eq!(unpack(&[]), "{}");
        assert_eq!(
            unpack(&[
                (1, 4096),
                (2, 2),
                (3, u32::MAX),
                (4, 65535),
                (5, 1),
                (6, 42),
                (8, 2)
            ]),
            concat!(
                "{\"headerTableSize\":4096,\"enablePush\":true,",
                "\"maxConcurrentStreams\":4294967295,\"initialWindowSize\":65535,",
                "\"maxFrameSize\":1,\"maxHeaderSize\":42,\"maxHeaderListSize\":42,",
                "\"enableConnectProtocol\":true}"
            )
        );
    }
}
