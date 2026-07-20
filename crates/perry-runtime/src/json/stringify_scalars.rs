//! Scalar emitters for `JSON.stringify`: number formatting, string escaping,
//! and the BigInt serialization path (which always throws, per spec).
//!
//! Split out of `stringify.rs` to stay under the 2000-line CI cap. Pure code
//! move — no behaviour change.

use super::*;
use crate::JSValue;
use std::fmt::Write as FmtWrite;

#[inline]
pub(crate) unsafe fn write_number(buf: &mut String, value: f64) {
    // A BigInt's NaN-boxed bits ARE an IEEE NaN, so it would otherwise fall into
    // the `is_nan()` → "null" arm below. BigInt is unserializable (modulo a
    // `toJSON`), so funnel it through the throwing serializer. Centralizes the
    // BigInt rule for every object-field / array-element numeric fallback that
    // reaches here (test262 JSON/stringify/value-bigint `{x: 0n}`).
    if (value.to_bits() & 0xFFFF_0000_0000_0000) == BIGINT_TAG {
        serialize_bigint(value, buf);
        return;
    }
    // An int32 is NaN-boxed (INT32_TAG = 0x7FFE); its bits ARE an IEEE NaN, so it
    // would otherwise fall into the `is_nan()` → "null" arm below and silently
    // drop the value. Decode and emit the signed integer. Integer columns from
    // the sqlite binding (and other int32-tagged numbers) funnel here; numeric
    // literals are stored as plain f64 doubles by codegen and never take this
    // branch. Mirrors the BigInt funnel above.
    if (value.to_bits() & 0xFFFF_0000_0000_0000) == INT32_TAG {
        let n = (value.to_bits() & INT32_MASK) as u32 as i32;
        let mut itoa_buf = itoa::Buffer::new();
        buf.push_str(itoa_buf.format(n));
        return;
    }
    // #2089: a Date is now a NaN-boxed `DateCell` pointer, handled in
    // `stringify_value`/`stringify_value_depth` before this numeric funnel —
    // so no Date detection is needed here anymore.
    if value.is_nan() || value.is_infinite() {
        // JSON has no NaN/Infinity literal; the spec serializes them as null.
        buf.push_str("null");
    } else if value.fract() == 0.0 && value.abs() < crate::builtins::INT_EXACT_FASTPATH_LIMIT {
        // Fast path for in-range integers (the overwhelming majority of JSON
        // numbers); identical to ECMAScript NumberToString below 2^53. Above it
        // the exact integer can carry more digits than the shortest round-trip
        // (`2**58`), so those fall through to `js_format_f64` in the else (#6127).
        let mut itoa_buf = itoa::Buffer::new();
        buf.push_str(itoa_buf.format(value as i64));
    } else {
        // ECMAScript Number::toString (spec 6.1.6.1.20): fixed notation for an
        // exponent in -6..=20, else exponential with an `e+`/`e-` sign. `ryu`
        // emits shortest round-trip digits but its own notation (`1e20`,
        // `1e-6`, `1e21`), so JSON.stringify diverged from `String(n)` and
        // Node. Reuse the shared JS formatter so `JSON.stringify(1e20)` is
        // `100000000000000000000` (not `1e20`) and `1e21` is `1e+21`.
        buf.push_str(&crate::string::js_format_f64(value));
    }
}

#[inline]
pub(crate) unsafe fn write_escaped_string(buf: &mut String, s: &str) {
    let bytes = s.as_bytes();
    // Fast path: scan for any escape-triggering byte. JSON output is
    // overwhelmingly escape-free (ASCII identifiers, simple values), so
    // a straight-line SIMD-friendly scan + one `push_str` beats the
    // scalar per-byte escape loop. Needs_escape fires for `"`, `\`, or
    // any control byte (< 0x20).
    // Also trip the slow path for WTF-8 lone surrogate sequences
    // (issue #1182): a lead byte of 0xED followed by 0xA0..=0xBF means
    // we have a 3-byte encoding of U+D800..=U+DFFF and need to emit a
    // `\uXXXX` escape rather than the raw (invalid-UTF-8) bytes.
    let needs_escape = bytes
        .iter()
        .any(|&b| b < 0x20 || b == b'"' || b == b'\\' || b == 0xED);
    if !needs_escape {
        buf.reserve(bytes.len() + 2);
        buf.push('"');
        buf.push_str(s);
        buf.push('"');
        return;
    }

    buf.push('"');
    let mut start = 0;
    // Issue #548: `s` reaches us via `str_from_header`, which uses
    // `from_utf8_unchecked` — a misclassified pointer (e.g. an
    // ArrayHeader interpreted as a StringHeader through the GC-type
    // fallback heuristic) can produce a `&str` whose bytes are not
    // valid UTF-8. The original `&s[start..i]` slice operation
    // panics in `core::str::is_char_boundary` whenever `i` lands
    // mid-multibyte (or on a stray continuation byte). Switching to
    // byte-level `extend_from_slice` writes the raw bytes through and
    // never inspects char boundaries; the JSON output stays
    // byte-identical for valid UTF-8 inputs and degrades gracefully
    // (non-UTF-8 bytes pass through verbatim) instead of aborting the
    // whole process. The String we hand back is technically
    // ill-formed in the worst case, but every consumer in this
    // codebase treats stringify output as a byte stream — and an
    // ill-formed result is strictly preferable to a SIGABRT.
    let buf_vec = buf.as_mut_vec();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // WTF-8 surrogate handling (issue #1182). A 0xED 0xA0..=0xBF
        // 0x80..=0xBF triple encodes a code unit in U+D800..=U+DFFF.
        // Two cases mirror Node's JSON.stringify:
        //
        //   * A high surrogate (0xA0..=0xAF mid byte) immediately
        //     followed by a low surrogate (0xB0..=0xBF mid byte) is
        //     a *valid* UTF-16 pair — re-encode it as the 4-byte
        //     UTF-8 of the astral codepoint, no escape. This is the
        //     only way `'\uD83D' + '\uDC4D'` round-trips through
        //     `dec.end()` + concat as 👍 instead of two escapes.
        //
        //   * Any remaining (lone) surrogate triple emits the
        //     `\uXXXX` escape.
        if b == 0xED
            && i + 2 < bytes.len()
            && (0xA0..=0xBF).contains(&bytes[i + 1])
            && (0x80..=0xBF).contains(&bytes[i + 2])
        {
            let high_cu: u32 = (((b & 0x0F) as u32) << 12)
                | (((bytes[i + 1] & 0x3F) as u32) << 6)
                | ((bytes[i + 2] & 0x3F) as u32);
            // Valid pair? Need the next 3 bytes to be a low surrogate
            // (0xED 0xB0..=0xBF 0x80..=0xBF) directly adjacent.
            let pair_low = if (0xD800..=0xDBFF).contains(&high_cu)
                && i + 5 < bytes.len()
                && bytes[i + 3] == 0xED
                && (0xB0..=0xBF).contains(&bytes[i + 4])
                && (0x80..=0xBF).contains(&bytes[i + 5])
            {
                let low_cu: u32 = (((bytes[i + 3] & 0x0F) as u32) << 12)
                    | (((bytes[i + 4] & 0x3F) as u32) << 6)
                    | ((bytes[i + 5] & 0x3F) as u32);
                Some(low_cu)
            } else {
                None
            };
            if start < i {
                buf_vec.extend_from_slice(&bytes[start..i]);
            }
            if let Some(low_cu) = pair_low {
                let cp = 0x10000 + ((high_cu - 0xD800) << 10) + (low_cu - 0xDC00);
                if let Some(c) = char::from_u32(cp) {
                    let mut tmp = [0u8; 4];
                    let s = c.encode_utf8(&mut tmp);
                    buf_vec.extend_from_slice(s.as_bytes());
                    i += 6;
                    start = i;
                    continue;
                }
            }
            buf_vec.extend_from_slice(format!("\\u{:04x}", high_cu).as_bytes());
            i += 3;
            start = i;
            continue;
        }
        let escape = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0x08 => Some("\\b"),
            0x0c => Some("\\f"),
            0..=0x1f => {
                if start < i {
                    buf_vec.extend_from_slice(&bytes[start..i]);
                }
                buf_vec.extend_from_slice(format!("\\u{:04x}", b).as_bytes());
                start = i + 1;
                i += 1;
                continue;
            }
            _ => None,
        };
        if let Some(esc) = escape {
            if start < i {
                buf_vec.extend_from_slice(&bytes[start..i]);
            }
            buf_vec.extend_from_slice(esc.as_bytes());
            start = i + 1;
        }
        i += 1;
    }
    if start < bytes.len() {
        buf_vec.extend_from_slice(&bytes[start..]);
    }
    buf_vec.push(b'"');
}

/// ECMA-262 SerializeJSONProperty step 2 for a BigInt: `GetV(value, "toJSON")`
/// resolves through `BigInt.prototype`. If a callable `toJSON` is installed
/// (e.g. a userland `BigInt.prototype.toJSON`), invoke it with `this` bound to
/// the BigInt and return the (serializable) result; otherwise `None` so the
/// caller throws. Unlike objects, a primitive BigInt never reaches
/// `object_get_to_json` (that helper only walks `GC_TYPE_OBJECT` layouts), so
/// the toJSON application lives here.
pub(crate) unsafe fn bigint_apply_to_json(value: f64) -> Option<f64> {
    let proto = crate::object::builtin_prototype_value("BigInt");
    let proto_bits = proto.to_bits();
    if (proto_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return None;
    }
    let proto_ptr = (proto_bits & POINTER_MASK) as *const crate::ObjectHeader;
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let key = js_string_from_bytes(b"toJSON".as_ptr(), 6);
    let key_handle = scope.root_string_ptr(key);
    // `GetV(value, "toJSON")` (spec) resolves "toJSON" on `BigInt.prototype`
    // but the accessor's `this` must be the BigInt VALUE, not the prototype
    // object `js_object_get_field_by_name` derives its receiver from.
    // `accessor_receiver_override_begin` is the same one-shot override
    // `resolve_inherited_field` uses for inherited-getter receivers; stash
    // the real BigInt value so a `toJSON` installed via
    // `Object.defineProperty(BigInt.prototype, "toJSON", { get() {...} })`
    // observes the correct receiver (test262
    // JSON/stringify/value-bigint-tojson-receiver).
    let prev_override =
        crate::object::accessor_receiver_override_begin(value_handle.get_nanbox_f64());
    let method = crate::object::js_object_get_field_by_name(
        proto_ptr,
        key_handle.get_raw_const_ptr::<crate::string::StringHeader>(),
    );
    crate::object::accessor_receiver_override_end(prev_override);
    let method_bits = method.bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return None;
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return None;
    }
    let recv = value_handle.get_nanbox_f64();
    // `toJSON(key)` receives the property key of this BigInt value (#5909).
    let key_f64_arg = super::stringify_tojson_probe::current_to_json_key_arg();
    let prev_this = crate::object::js_implicit_this_set(recv);
    let result = crate::closure::js_native_call_value(f64::from_bits(method_bits), &key_f64_arg, 1);
    crate::object::js_implicit_this_set(prev_this);
    // The user callback may have installed/removed `Object.prototype.toJSON`.
    invalidate_object_proto_tojson_state();
    Some(result)
}

/// Serialize a BigInt: apply `BigInt.prototype.toJSON` if present, else throw a
/// TypeError. If `toJSON` returns another BigInt the value remains
/// unserializable and we throw.
pub(crate) unsafe fn serialize_bigint(value: f64, buf: &mut String) {
    if let Some(converted) = bigint_apply_to_json(value) {
        if (converted.to_bits() & 0xFFFF_0000_0000_0000) == BIGINT_TAG {
            throw_bigint_serialize();
        }
        stringify_value(converted, TYPE_UNKNOWN, buf);
        return;
    }
    throw_bigint_serialize();
}

/// ECMA-262 SerializeJSONProperty step for a BigInt value: throw a TypeError.
/// A `toJSON`/replacer that converts the BigInt runs earlier in the walk, so
/// any BigInt reaching a serializer is unconvertible.
pub(crate) fn throw_bigint_serialize() -> ! {
    let msg = "Do not know how to serialize a BigInt";
    let msg_ptr = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_ptr);
    crate::exception::js_throw(f64::from_bits(
        POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
    ))
}
