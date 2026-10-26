//! Scalar emitters for `JSON.stringify`: number formatting, string escaping,
//! and the BigInt serialization path (which always throws, per spec).
//!
//! Split out of `stringify.rs` to stay under the 2000-line CI cap. Pure code
//! move — no behaviour change.

use super::*;

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
    // A native id (Proxy, Web Fetch, stream) is a NaN pattern too, but an
    // object: the numeric fallbacks of every walker funnel it to the value
    // serializer, which owns Proxy semantics.
    if (value.to_bits() & 0xFFFF_0000_0000_0000) == crate::value::HANDLE_TAG {
        super::stringify::stringify_value_depth(value, TYPE_UNKNOWN, buf, 1);
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
        super::stringify_copy::push_str(buf, itoa_buf.format(n));
        return;
    }
    // #2089: a Date is now a NaN-boxed `DateCell` pointer, handled in
    // `stringify_value`/`stringify_value_depth` before this numeric funnel —
    // so no Date detection is needed here anymore.
    if value.is_nan() || value.is_infinite() {
        // JSON has no NaN/Infinity literal; the spec serializes them as null.
        buf.push_str("null");
    } else if value.abs() < crate::builtins::INT_EXACT_FASTPATH_LIMIT
        && (value as i64) as f64 == value
    {
        // Fast path for in-range integers (the overwhelming majority of JSON
        // numbers); identical to ECMAScript NumberToString below 2^53. Above it
        // the exact integer can carry more digits than the shortest round-trip
        // (`2**58`), so those use shortest-round-trip formatting below (#6127).
        // The integer test is the conversion round trip rather than `fract()`,
        // which is a libm `trunc` call on the baseline x86-64 target; both
        // spell either zero as "0".
        let mut itoa_buf = itoa::Buffer::new();
        super::stringify_copy::push_str(buf, itoa_buf.format(value as i64));
    } else if write_compact_decimal(buf, value) {
        // The guarded decimal spelling already round-trips to this number.
    } else {
        // ECMAScript shortest-round-trip digits and notation, directly into
        // stack storage. The general Rust formatter allocated a temporary
        // String for every non-integer element and could choose a different
        // final digit at a tie. Plain `ryu` also uses different exponent
        // thresholds; `ryu-js` implements Number::toString's spelling.
        let mut number = ryu_js::Buffer::new();
        super::stringify_copy::push_str(buf, number.format_finite(value));
    }
}

/// Fast fixed-point spelling when at most three fractional digits suffice.
/// Fail without changing the output if any guard does not hold.
#[inline]
pub(crate) fn write_compact_decimal(buf: &mut String, value: f64) -> bool {
    let abs = value.abs();
    if !(0.001..1e12).contains(&abs) {
        return false;
    }
    let scaled = abs * 1000.0;
    let digits = scaled as u64;
    let fraction = digits % 1000;
    if fraction == 0 || digits as f64 != scaled || digits as f64 / 1000.0 != abs {
        return false;
    }
    // digits < 10^15 is exact as f64. Below 10^12 the distance between
    // adjacent thousandths exceeds eight ULPs, so another three-or-fewer
    // fractional-digit spelling cannot round to this float. Removing trailing
    // zeros gives the shortest spelling; ECMAScript uses fixed notation here.
    if value < 0.0 {
        buf.push('-');
    }
    let mut integer = itoa::Buffer::new();
    super::stringify_copy::push_str(buf, integer.format(digits / 1000));
    buf.push('.');
    let bytes = [
        b'0' + (fraction / 100) as u8,
        b'0' + ((fraction / 10) % 10) as u8,
        b'0' + (fraction % 10) as u8,
    ];
    let len = if fraction % 10 != 0 {
        3
    } else if fraction % 100 != 0 {
        2
    } else {
        1
    };
    // All three bytes were constructed as ASCII digits.
    super::stringify_copy::push_str(buf, unsafe { std::str::from_utf8_unchecked(&bytes[..len]) });
    true
}

pub(crate) use crate::string::quote::escaping::{write_escaped_string, write_wtf8_key};

/// Quote a heap string, using parser provenance to skip a redundant escape
/// scan for payloads borrowed from an unescaped JSON token.
#[inline]
pub(crate) unsafe fn write_heap_string(buf: &mut String, ptr: *const StringHeader) -> bool {
    let Some(text) = str_from_header(ptr) else {
        return false;
    };
    if (*ptr).flags & crate::string::STRING_FLAG_JSON_ESCAPE_FREE != 0 {
        buf.reserve(text.len() + 2);
        buf.push('"');
        super::stringify_copy::push_str(buf, text);
        buf.push('"');
    } else {
        write_escaped_string(buf, text);
    }
    true
}

/// Quote an inline short-string byte payload. Returns false for malformed
/// UTF-8, preserving the existing serializer fallback. Plain ASCII is proven
/// together with escape absence so the common path avoids a second scan.
#[inline]
pub(crate) unsafe fn write_short_string(buf: &mut String, bytes: &[u8]) -> bool {
    if !super::simd::short_string_is_plain_ascii(bytes) {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return false;
        };
        write_escaped_string(buf, text);
        return true;
    }
    let text = std::str::from_utf8_unchecked(bytes);
    buf.reserve(bytes.len() + 2);
    buf.push('"');
    super::stringify_copy::push_str(buf, text);
    buf.push('"');
    true
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
    // `toJSON(key)` receives the property key of this BigInt value (#5909).
    let key_f64_arg = super::stringify_tojson_probe::current_to_json_key_arg();
    let result = crate::closure::native_call_value_this(
        f64::from_bits(method_bits),
        crate::closure::JsThis::from_f64(value_handle.get_nanbox_f64()),
        &key_f64_arg,
        1,
    );
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

#[cfg(test)]
#[path = "stringify_scalars_tests.rs"]
mod tests;
