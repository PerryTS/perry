//! Callable `String.raw(callSite, ...substitutions)` (#2789).
//!
//! The tagged-template form ``String.raw`...` `` is lowered to direct string
//! concatenation in the HIR (see `lower_expr.rs`). The *callable* form —
//! `String.raw({ raw: [...] }, ...subs)` — is what libraries reach for when
//! they synthesize a call site by hand, so it needs the full ECMAScript
//! `String.raw` algorithm (§22.1.2.4) at runtime.

use super::*;

/// `String.raw(callSite, substitutions)` where `substitutions` is a
/// NaN-boxed array of the interpolated values (possibly empty/undefined).
///
/// Algorithm (ECMA-262 §22.1.2.4):
/// 1. `cooked = ToObject(callSite)` — nullish callSite throws `TypeError`.
/// 2. `raw = ToObject(cooked.raw)` — nullish `raw` throws `TypeError`.
/// 3. `literalCount = ToLength(raw.length)`; return `""` when 0.
/// 4. Walk segments, appending `ToString(raw[i])`, and between segments
///    `ToString(substitutions[i])` for as many substitutions as exist.
#[no_mangle]
pub extern "C" fn js_string_raw(call_site: f64, substitutions: f64) -> *mut StringHeader {
    let call_jsval = crate::value::JSValue::from_bits(call_site.to_bits());
    if call_jsval.is_undefined() || call_jsval.is_null() {
        throw_raw_type_error();
    }

    // cooked.raw
    let raw = get_named_prop_f64(call_site, b"raw");
    let raw_jsval = crate::value::JSValue::from_bits(raw.to_bits());
    if raw_jsval.is_undefined() || raw_jsval.is_null() {
        throw_raw_type_error();
    }

    let literal_count = to_length_u64(get_named_prop_f64(raw, b"length"));
    if literal_count == 0 {
        return js_string_from_bytes("".as_ptr(), 0);
    }

    // Use the polymorphic numeric-index getter so both real arrays and
    // array-like plain objects (`{ 0: "a", 1: "b", length: 2 }`) read
    // correctly — a plain object's "0"/"1" are string-named fields, which
    // `js_object_get_index_polymorphic` handles by stringifying the index.
    let raw_handle = raw.to_bits() as i64;
    let subs_handle = substitutions.to_bits() as i64;

    let mut result = String::new();
    let mut i: u64 = 0;
    loop {
        // ToString(raw[i]) — a Symbol segment throws a TypeError (§7.1.17).
        let seg = crate::object::js_object_get_index_polymorphic(raw_handle, i as f64);
        crate::builtins::reject_symbol_to_string(seg);
        let seg_ptr = crate::value::js_jsvalue_to_string(seg);
        if is_valid_string_ptr(seg_ptr) {
            result.push_str(string_as_str(seg_ptr));
        }
        if i + 1 == literal_count {
            break;
        }
        // ToString(substitutions[i]) interleaved between segments. Missing
        // substitutions are simply absent (ToString skipped) — matching the
        // spec's `if nextIndex < numberOfSubstitutions` guard.
        let sub = crate::object::js_object_get_index_polymorphic(subs_handle, i as f64);
        let sub_jsval = crate::value::JSValue::from_bits(sub.to_bits());
        if !sub_jsval.is_undefined() {
            // ToString(substitution) — a Symbol throws a TypeError (§7.1.17).
            crate::builtins::reject_symbol_to_string(sub);
            let sub_ptr = crate::value::js_jsvalue_to_string(sub);
            if is_valid_string_ptr(sub_ptr) {
                result.push_str(string_as_str(sub_ptr));
            }
        }
        i += 1;
    }

    let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
    std::hint::black_box(&result);
    ret
}

/// Read a named property as an `f64` JSValue from a NaN-boxed receiver,
/// covering both real arrays/objects and array-like plain objects. Returns
/// NaN-boxed `undefined` when the receiver isn't an object or the property
/// is absent.
fn get_named_prop_f64(value: f64, name: &[u8]) -> f64 {
    let ptr = crate::value::js_nanbox_get_pointer(value) as *const crate::object::ObjectHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(ptr, key)
}

/// ToLength of a value's `length`: NaN/negative → 0, +Infinity clamps to
/// `2^53 - 1`, fractional truncates. Returned as `u64` for the segment loop.
fn to_length_u64(value: f64) -> u64 {
    // `length` may itself need coercion (array-like with a string length).
    let n = crate::builtins::js_number_coerce(value);
    if n.is_nan() || n <= 0.0 {
        0
    } else if n.is_infinite() || n >= (1u64 << 53) as f64 {
        (1u64 << 53) - 1
    } else {
        n.trunc() as u64
    }
}

fn throw_raw_type_error() -> ! {
    let message = "Cannot convert undefined or null to object";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Keepalive anchor — `js_string_raw` is emitted only by generated code
/// (the `String.raw(...)` call lowering), so the auto-optimize whole-program
/// LLVM rebuild would otherwise dead-strip this `#[no_mangle]` symbol and
/// break linking (see PR #3320 / the `#[used]` keepalive pattern).
#[used]
static KEEP_STRING_RAW: extern "C" fn(f64, f64) -> *mut StringHeader = js_string_raw;
