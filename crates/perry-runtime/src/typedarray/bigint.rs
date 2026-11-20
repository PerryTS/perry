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

use super::{store_at, typed_array_alloc, TypedArrayHeader, KIND_BIGINT64, KIND_BIGUINT64};

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
        let len = crate::typedarray::element_length(src);
        let out = typed_array_alloc(dst_kind, len);
        for i in 0..len as usize {
            let v = crate::buffer::js_buffer_get(src, i as i32) as f64;
            store_at(out, i, v);
        }
        out
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
        // ToNumber(BigInt) throws; `js_number_coerce` would convert it.
        if crate::value::JSValue::from_bits(raw.to_bits()).is_bigint() {
            crate::byte_cell_support::throw_bigint_number_mix();
        }
        // Typed-array element conversion is the full observable ToNumber,
        // not the primitive-only numeric unboxer. In particular an object
        // source element must run its @@toPrimitive/valueOf hooks.
        crate::builtins::js_number_coerce(raw)
    }
}

/// `ToBigInt(value)` for a `BigInt64`/`BigUint64` element store, returning the
/// value re-boxed as a NaN-boxed BigInt. Per the ECMAScript `ToBigInt`
/// abstract operation, a Number (including a NaN-boxed int32), `undefined`,
/// `null`, and Symbols are NOT convertible and throw a `TypeError`; only
/// BigInt, Boolean, String and object (via ToPrimitive) inputs coerce.
///
/// The typed-array arm is the group's own; every other value is converted by
/// `bigint_coerce`, which the BigInt statics call without this group.
pub(crate) fn to_bigint_for_store(value: f64) -> f64 {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_pointer()
        && !jsval.is_bigint()
        && unsafe { crate::symbol::js_is_symbol(value) } == 0
    {
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
    }
    crate::bigint_coerce::to_bigint_not_typed_array(value)
}

pub(crate) use crate::byte_cell_support::{
    bigint_slot_bits, is_bigint_kind, throw_bigint_number_mix,
};
