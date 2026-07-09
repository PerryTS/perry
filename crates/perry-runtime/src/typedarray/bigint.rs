//! BigInt element coercion for `BigInt64Array` / `BigUint64Array` (#4356).
//!
//! These views store real BigInt values, so the element read/write path can't
//! funnel them through `f64` like the numeric kinds. The helpers here bridge
//! between a NaN-boxed JS value and the raw 64-bit slot bits:
//!
//!   - `bigint_slot_bits` — unbox a BigInt to the low limb to store.
//!   - `to_bigint_for_store` — `ToBigInt(value)` for an element write
//!     (a Number throws `TypeError`), returning a NaN-boxed BigInt.
//!   - `coerce_for_kind` — pick `ToBigInt` vs `ToNumber` by destination kind
//!     for the construction / `set()` paths.

use super::{
    jsvalue_to_f64, store_at, throw_type_error, typed_array_alloc, TypedArrayHeader, KIND_BIGINT64,
    KIND_BIGUINT64,
};

pub(crate) fn is_bigint_kind(kind: u8) -> bool {
    matches!(kind, KIND_BIGINT64 | KIND_BIGUINT64)
}

#[cold]
pub(crate) fn throw_bigint_number_mix() -> ! {
    throw_type_error(b"Cannot mix BigInt and other types, use explicit conversions")
}

#[inline]
pub(super) fn validate_copy_kinds(dst_kind: u8, src_kind: u8) {
    if is_bigint_kind(dst_kind) != is_bigint_kind(src_kind) {
        throw_bigint_number_mix();
    }
}

pub(super) fn copy_from_uint8_buffer(
    dst_kind: u8,
    src: *const crate::buffer::BufferHeader,
) -> *mut TypedArrayHeader {
    if is_bigint_kind(dst_kind) {
        throw_bigint_number_mix();
    }
    if src.is_null() {
        return typed_array_alloc(dst_kind, 0);
    }
    unsafe {
        let len = (*src).length;
        let out = typed_array_alloc(dst_kind, len);
        for i in 0..len as usize {
            let v = crate::buffer::js_buffer_get(src, i as i32) as f64;
            store_at(out, i, v);
        }
        out
    }
}

/// Extract the low 64 bits to write into a `BigInt64`/`BigUint64` slot. The
/// element-set path hands us an already-`ToBigInt`-coerced, NaN-boxed BigInt
/// (so reads round-trip the raw bits); internal `load_at`→`store_at` copies do
/// the same. A bare finite Number falls back to a truncating `as i64` cast so
/// any legacy numeric caller keeps its old behavior rather than reading garbage.
pub(super) fn bigint_slot_bits(value: f64) -> u64 {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader;
        let cleaned = crate::bigint::clean_bigint_ptr(ptr);
        if cleaned.is_null() {
            return 0;
        }
        unsafe { (*cleaned).limbs[0] }
    } else if value.is_finite() {
        value as i64 as u64
    } else {
        0
    }
}

/// Coerce a raw (NaN-boxed) source value to the representation `store_at`
/// expects for `dst_kind`: a NaN-boxed BigInt for the bigint kinds (via
/// `ToBigInt`), or a plain numeric f64 (via `ToNumber`) for every other kind.
/// Used by the array/array-like construction and `set()` paths so a bigint view
/// keeps real BigInt elements instead of `jsvalue_to_f64`-mangling them to NaN.
pub(crate) fn coerce_for_kind(dst_kind: u8, raw: f64) -> f64 {
    if dst_kind == KIND_BIGINT64 || dst_kind == KIND_BIGUINT64 {
        to_bigint_for_store(raw)
    } else {
        jsvalue_to_f64(raw)
    }
}

/// `ToBigInt(value)` for a `BigInt64`/`BigUint64` element store, returning the
/// value re-boxed as a NaN-boxed BigInt. Per the ECMAScript `ToBigInt`
/// abstract operation, a Number (including a NaN-boxed int32), `undefined`,
/// `null`, and Symbols are NOT convertible and throw a `TypeError`; only
/// BigInt, Boolean, and String inputs coerce.
pub(crate) fn to_bigint_for_store(value: f64) -> f64 {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_bigint() {
        return value;
    }
    if jsval.is_bool() {
        let n = if jsval.as_bool() { 1 } else { 0 };
        return crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(n) as i64);
    }
    if jsval.is_any_string() {
        // StringToBigInt (a malformed numeric string throws SyntaxError).
        let bi = crate::bigint::js_bigint_from_f64(value);
        return crate::value::js_nanbox_bigint(bi as i64);
    }
    // ToBigInt(object): `ToPrimitive(value, number)` (its `Symbol.toPrimitive` /
    // `valueOf` / `toString`) then `ToBigInt` on the primitive result. A
    // Symbol pointer is rejected (handled by the label path below); every other
    // object — e.g. `bigIntView.fill({ valueOf() { return 7n } })` — must run its
    // coercion hook rather than throwing "Cannot convert NaN to a BigInt".
    if jsval.is_pointer() && unsafe { crate::symbol::js_is_symbol(value) } == 0 {
        // A typed-array source with a *patched own* `valueOf`/`toString`
        // expando runs it first (test262 ctors-bigint/object-arg/
        // throws-setting-obj-* — the patch may throw, observably).
        let addr = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
            if let Some(p) =
                unsafe { crate::typedarray_props::typed_array_own_to_primitive_number(addr, value) }
            {
                return to_bigint_for_store(p);
            }
        }
        match unsafe { crate::value::to_primitive_number(value) } {
            crate::value::OrdinaryToPrimitiveOutcome::Primitive(p)
                if p.to_bits() != value.to_bits() =>
            {
                return to_bigint_for_store(p);
            }
            crate::value::OrdinaryToPrimitiveOutcome::TypeError => {
                throw_type_error(b"Cannot convert object to primitive value");
            }
            // `Primitive` that didn't change (shouldn't happen for an object) and
            // `DefaultString` (no usable `valueOf`/`toString`) fall through to the
            // unconvertible-label `TypeError` below.
            _ => {}
        }
    }
    let label = bigint_unconvertible_label(value);
    throw_type_error(format!("Cannot convert {label} to a BigInt").as_bytes());
}

/// Human-readable label for a value that `ToBigInt` rejects, matching Node's
/// `Cannot convert <x> to a BigInt` TypeError text.
fn bigint_unconvertible_label(value: f64) -> String {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        "undefined".to_string()
    } else if jsval.is_null() {
        "null".to_string()
    } else if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        "a Symbol value".to_string()
    } else if jsval.is_int32() {
        jsval.as_int32().to_string()
    } else {
        format!("{value}")
    }
}
