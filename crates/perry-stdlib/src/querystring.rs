//! `node:querystring` — legacy URL-encoded form parser/serialiser.
//!
//! Node still ships this even though `URLSearchParams` superseded it
//! (deprecated since Node 11) — enough npm packages depend on it that
//! Perry can't punt. This module supplies:
//!
//!   * `js_querystring_escape(str)` — Node's percent-encoder. Encodes
//!     every byte outside `[A-Za-z0-9-_.!~*'()]`. Differs from
//!     `encodeURIComponent` only in that Node uses a small lookup
//!     table and accepts a custom `encodeURIComponent` override at
//!     the `parse`/`stringify` layer (we accept the override slot but
//!     don't expose it yet — adding the optional-callback arm in a
//!     follow-up if needed).
//!   * `js_querystring_unescape(str)` — Node's `decodeURIComponent`
//!     wrapped to swallow malformed `%XX` sequences (returns the raw
//!     `%` instead of throwing — matches Node 18+ behaviour).
//!   * `js_querystring_parse(str, sep, eq)` — splits on `sep`
//!     (default `&`), each pair on `eq` (default `=`), unescapes both
//!     sides, builds an object where repeated keys produce arrays.
//!   * `js_querystring_stringify(obj, sep, eq)` — opposite direction.
//!     Object array values produce repeated `key=v1&key=v2`.
//!
//! `decode` / `encode` are aliases for `parse` / `stringify`; the
//! identity-equality check (`decode === parse`) is satisfied by the
//! native dispatch table routing both names to the same `runtime`
//! symbol.

use crate::common::handle::Handle;
use perry_runtime::array::{js_array_alloc, js_array_length, js_array_push_f64};
use perry_runtime::{
    js_object_alloc, js_object_get_field_by_name, js_object_set_field_by_name,
    js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader, StringHeader,
};

// Suppress unused — `Handle` is re-exported for symmetry with other modules.
#[allow(dead_code)]
fn _unused(_: Handle) {}

/// Decode the bytes behind a NaN-boxed string-ish value into a Rust
/// `String`. Returns `None` if the value isn't a string-shaped pointer
/// (used to detect optional-undefined args).
unsafe fn nanboxed_to_string(value: f64) -> Option<String> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    // SHORT_STRING_TAG (0x7FF9) inline form. Encoding (per
    // `perry_runtime::value`): bits 40..47 carry the byte length (max 5),
    // bits 0..39 carry the UTF-8 payload little-endian. The original PR
    // checked 0x7FFA (BIGINT_TAG) with a 4-bit-at-offset-44 length field —
    // both wrong; safe in practice today because codegen passes string
    // literals as heap-allocated STRING_TAG (0x7FFF) pointers, so the bad
    // branch never fired. Fixed at merge time (PR #1151 review).
    if top16 == 0x7FF9 {
        let len = ((bits >> 40) & 0xFF) as usize;
        if len == 0 {
            return Some(String::new());
        }
        if len > 5 {
            return None;
        }
        let mut buf = [0u8; 5];
        for (i, b) in buf.iter_mut().enumerate().take(len) {
            *b = ((bits >> (i * 8)) & 0xFF) as u8;
        }
        return Some(String::from_utf8_lossy(&buf[..len]).into_owned());
    }
    // STRING_TAG / POINTER_TAG / raw heap pointer — all keep the address
    // in the low 48 bits, and the layout starts with `byte_len: u32`
    // followed by `byte_len` bytes of UTF-8. Reject anything below the
    // small-handle ceiling (0x10000) — matches the runtime's
    // pointer-vs-handle guards in object.rs / value.rs.
    let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if addr < 0x10000 {
        return None;
    }
    let hdr = addr as *const StringHeader;
    let len = (*hdr).byte_len as usize;
    let data = (hdr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Allocate a heap StringHeader from a Rust `&str`.
fn intern_string(s: &str) -> *mut StringHeader {
    unsafe { js_string_from_bytes(s.as_ptr(), s.len() as u32) }
}

/// NaN-box a `*mut StringHeader` with STRING_TAG so it returns through
/// the `f64` calling convention as a real JS string.
fn nanbox_string(ptr: *mut StringHeader) -> f64 {
    f64::from_bits(0x7FFF_0000_0000_0000u64 | ((ptr as u64) & 0x0000_FFFF_FFFF_FFFF))
}

/// Node's percent-encoder allowlist: ASCII alphanumerics plus
/// `- _ . ! ~ * ' ( )`. Everything else gets `%XX`-encoded byte by
/// byte over the UTF-8 representation.
fn is_querystring_unreserved(b: u8) -> bool {
    matches!(b,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
        | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
    )
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        if is_querystring_unreserved(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX_UPPER[(b >> 4) as usize] as char);
            out.push(HEX_UPPER[(b & 0xF) as usize] as char);
        }
    }
    out
}

fn percent_decode(input: &str, plus_as_space: bool) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            let hi = bytes[i + 1];
            let lo = bytes[i + 2];
            let (h, l) = (hex_nibble(hi), hex_nibble(lo));
            match (h, l) {
                (Some(h), Some(l)) => {
                    out.push((h << 4) | l);
                    i += 3;
                    continue;
                }
                _ => {
                    // Malformed `%XX`: emit the literal `%` and keep
                    // scanning. Matches Node 18+'s lenient mode.
                    out.push(b'%');
                    i += 1;
                    continue;
                }
            }
        }
        if plus_as_space && b == b'+' {
            out.push(b' ');
        } else {
            out.push(b);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `querystring.escape(str)` → string.
#[no_mangle]
pub unsafe extern "C" fn js_querystring_escape(str_arg: f64) -> f64 {
    let s = match nanboxed_to_string(str_arg) {
        Some(s) => s,
        None => return f64::from_bits(JSValue::undefined().bits()),
    };
    nanbox_string(intern_string(&percent_encode(&s)))
}

/// `querystring.unescape(str)` → string.
#[no_mangle]
pub unsafe extern "C" fn js_querystring_unescape(str_arg: f64) -> f64 {
    let s = match nanboxed_to_string(str_arg) {
        Some(s) => s,
        None => return f64::from_bits(JSValue::undefined().bits()),
    };
    nanbox_string(intern_string(&percent_decode(&s, false)))
}

/// `querystring.parse(str, sep?, eq?, options?)` → object. Repeated keys produce
/// array values. Empty input returns an empty object.
#[no_mangle]
pub unsafe extern "C" fn js_querystring_parse(
    str_arg: f64,
    sep_arg: f64,
    eq_arg: f64,
    options_arg: f64,
) -> *mut ObjectHeader {
    let input = nanboxed_to_string(str_arg).unwrap_or_default();
    let sep = resolve_separator_str(sep_arg, "&");
    let eq = resolve_separator_str(eq_arg, "=");
    let max_keys = resolve_max_keys(options_arg);

    let obj = js_object_alloc(0, 0);
    if input.is_empty() {
        return obj;
    }

    let pairs: Box<dyn Iterator<Item = &str>> = if sep.is_empty() {
        Box::new(std::iter::once(input.as_str()))
    } else {
        Box::new(input.split(sep.as_str()))
    };
    let mut parsed = 0usize;
    for pair in pairs {
        if let Some(limit) = max_keys {
            if parsed >= limit {
                break;
            }
        }
        if pair.is_empty() {
            continue;
        }
        let (key_raw, val_raw) = if eq.is_empty() {
            (pair, "")
        } else {
            match pair.find(eq.as_str()) {
                Some(p) => (&pair[..p], &pair[p + eq.len()..]),
                None => (pair, ""),
            }
        };
        let key = percent_decode(key_raw, true);
        let value = percent_decode(val_raw, true);
        push_parsed_pair(obj, &key, &value);
        parsed += 1;
    }
    obj
}

fn resolve_max_keys(options: f64) -> Option<usize> {
    let value = JSValue::from_bits(options.to_bits());
    if !value.is_pointer() {
        return Some(1000);
    }
    let obj = value.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return Some(1000);
    }
    let key = intern_string("maxKeys");
    let max_keys = unsafe { js_object_get_field_by_name(obj, key) };
    if max_keys.is_undefined() || max_keys.is_null() {
        return Some(1000);
    }
    let n = if max_keys.is_int32() {
        max_keys.as_int32() as f64
    } else if max_keys.is_number() {
        max_keys.as_number()
    } else {
        return Some(1000);
    };
    if n <= 0.0 || !n.is_finite() {
        None
    } else {
        Some(n.floor() as usize)
    }
}

/// Insert `(key, value)` into the parse result, promoting to an array
/// on repeated keys. Mirrors Node's behaviour:
///   - first occurrence stores the value as a plain string
///   - second occurrence promotes to a 2-element array
///   - subsequent occurrences push onto the existing array
unsafe fn push_parsed_pair(obj: *mut ObjectHeader, key: &str, value: &str) {
    let key_hdr = intern_string(key);
    let existing_bits = js_object_get_field_by_name(obj, key_hdr).bits();

    let value_str = intern_string(value);
    let value_f64 = nanbox_string(value_str);

    if existing_bits == JSValue::undefined().bits() {
        js_object_set_field_by_name(obj, key_hdr, value_f64);
        return;
    }

    let top16 = existing_bits >> 48;
    if top16 == 0x7FFD {
        // POINTER_TAG — likely an array already.
        let addr = (existing_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if addr >= 0x1000 {
            let arr = addr as *mut ArrayHeader;
            js_array_push_f64(arr, value_f64);
            return;
        }
    }

    // Promote: existing string + new string → 2-element array.
    let arr = js_array_alloc(0);
    js_array_push_f64(arr, f64::from_bits(existing_bits));
    js_array_push_f64(arr, value_f64);
    let arr_boxed = JSValue::pointer(arr as *const u8);
    js_object_set_field_by_name(obj, key_hdr, f64::from_bits(arr_boxed.bits()));
}

/// `querystring.stringify(obj, sep?, eq?)` → string.
#[no_mangle]
pub unsafe extern "C" fn js_querystring_stringify(obj_arg: f64, sep_arg: f64, eq_arg: f64) -> f64 {
    // Default separators are the bytes Node uses; we read into UTF-8
    // strings instead of bytes since the chars are always ASCII.
    let sep = resolve_separator_str(sep_arg, "&");
    let eq = resolve_separator_str(eq_arg, "=");

    let bits = obj_arg.to_bits();
    let top16 = bits >> 48;
    if top16 != 0x7FFD {
        // Not a POINTER_TAG — nothing to iterate. Node returns "" for
        // null / undefined / primitives.
        return nanbox_string(intern_string(""));
    }
    let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if addr < 0x1000 {
        return nanbox_string(intern_string(""));
    }
    let obj = addr as *mut ObjectHeader;

    let keys = (*obj).keys_array;
    if keys.is_null() {
        return nanbox_string(intern_string(""));
    }

    let mut out = String::new();
    let n = js_array_length(keys);
    for i in 0..n {
        let key_value = perry_runtime::array::js_array_get_f64(keys, i);
        let key = match nanboxed_to_string(key_value) {
            Some(s) => s,
            None => continue,
        };
        let key_hdr = intern_string(&key);
        let value_bits = js_object_get_field_by_name(obj, key_hdr).bits();
        append_stringify_value(&mut out, &key, value_bits, &sep, &eq);
    }
    nanbox_string(intern_string(&out))
}

fn resolve_separator_str(value: f64, default: &'static str) -> String {
    let bits = value.to_bits();
    if bits == JSValue::undefined().bits() || bits == JSValue::null().bits() {
        return default.to_string();
    }
    match unsafe { nanboxed_to_string(value) } {
        Some(s) if !s.is_empty() => s,
        _ => default.to_string(),
    }
}

/// Append `key=value` (or `key=v1&key=v2` for arrays) to `out`.
unsafe fn append_stringify_value(
    out: &mut String,
    key: &str,
    value_bits: u64,
    sep: &str,
    eq: &str,
) {
    let top16 = value_bits >> 48;
    let value_f64 = f64::from_bits(value_bits);

    // Array case — repeated `key=v` joins.
    if top16 == 0x7FFD {
        let addr = (value_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if addr >= 0x1000 {
            // Heuristic: detect array via the GC header obj_type. Falls
            // back to "treat as object → toString" if not an array.
            let gc_hdr = (addr as *const u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
                as *const perry_runtime::gc::GcHeader;
            let is_array = (*gc_hdr).obj_type == perry_runtime::gc::GC_TYPE_ARRAY;
            if is_array {
                let arr = addr as *mut ArrayHeader;
                let n = js_array_length(arr);
                for i in 0..n {
                    let elem = perry_runtime::array::js_array_get_f64(arr, i);
                    let elem_str = querystring_scalar_to_string(elem.to_bits());
                    if !out.is_empty() {
                        out.push_str(sep);
                    }
                    out.push_str(&percent_encode(key));
                    out.push_str(eq);
                    out.push_str(&percent_encode(&elem_str));
                }
                return;
            }
        }
    }

    let value_str = querystring_scalar_to_string(value_f64.to_bits());
    if !out.is_empty() {
        out.push_str(sep);
    }
    out.push_str(&percent_encode(key));
    out.push_str(eq);
    out.push_str(&percent_encode(&value_str));
}

fn querystring_scalar_to_string(value_bits: u64) -> String {
    let value = JSValue::from_bits(value_bits);
    if value.is_undefined() || value.is_null() {
        return String::new();
    }
    if perry_runtime::date::is_registered_date_bits(value_bits) {
        return String::new();
    }
    if value.is_bool() {
        return value.as_bool().to_string();
    }
    if value.is_int32() {
        return value.as_int32().to_string();
    }
    if value.is_number() {
        let n = value.as_number();
        if !n.is_finite() {
            return String::new();
        }
        if n.fract() == 0.0 {
            return (n as i64).to_string();
        }
        return n.to_string();
    }
    if value.is_any_string() {
        return unsafe { nanboxed_to_string(f64::from_bits(value_bits)) }.unwrap_or_default();
    }
    String::new()
}
