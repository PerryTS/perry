//! `ToBigInt(value)` (ECMAScript 7.1.13) for every caller, whichever runtime
//! families the program links.
//!
//! BigInt passes through; Boolean gives 0n / 1n; a String goes through
//! StringToBigInt (a malformed numeric string throws SyntaxError); an object
//! is reduced with `ToPrimitive(value, number)` and the primitive converted;
//! a Number, `undefined`, `null` and a Symbol throw node's TypeError
//! ("Cannot convert <x> to a BigInt").
//!
//! Only a typed array as the value needs the typed-array group (a patched own
//! `valueOf` / `toString` expando runs first). A typed array exists only in a
//! program whose group is installed, so that one arm goes through the group's
//! slot and every other value is converted here. Before, the BigInt statics
//! (`BigInt.asIntN` / `asUintN`) converted every value through that slot,
//! whose empty arm answers `undefined`: `BigInt.asIntN(8, 255n)` was `0n` in a
//! program without the group.

use crate::value::JSValue;

/// `ToBigInt(value)`, returning a NaN-boxed BigInt (or throwing).
pub(crate) fn to_bigint(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() && !jsval.is_bigint() {
        let addr = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::byte_cell_support::lookup_typed_array_kind(addr).is_some() {
            // The value is a byte cell, so the group that converts it is installed.
            return crate::typedarray_hook_slots::bigint::to_bigint_for_store(value);
        }
    }
    to_bigint_not_typed_array(value)
}

/// `ToBigInt(value)` for a value that is no typed array (the typed-array
/// group's own conversion ends here once its typed-array arm declined).
pub(crate) fn to_bigint_not_typed_array(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_bigint() {
        return value;
    }
    if jsval.is_bool() {
        let n = if jsval.as_bool() { 1 } else { 0 };
        return crate::value::js_nanbox_bigint(crate::bigint_hooks::js_bigint_from_i64(n) as i64);
    }
    if jsval.is_any_string() {
        let bi = crate::bigint_hooks::js_bigint_from_f64(value);
        return crate::value::js_nanbox_bigint(bi as i64);
    }
    // ToBigInt(object): `ToPrimitive(value, number)` (its `Symbol.toPrimitive` /
    // `valueOf` / `toString`) then `ToBigInt` on the primitive result. A
    // Symbol is rejected by the label path below; every other object (e.g.
    // `{ valueOf() { return 7n } }`) runs its coercion hook.
    if jsval.is_pointer() && unsafe { crate::symbol::js_is_symbol(value) } == 0 {
        match unsafe { crate::value::to_primitive_number(value) } {
            crate::value::OrdinaryToPrimitiveOutcome::Primitive(p)
                if p.to_bits() != value.to_bits() =>
            {
                return to_bigint(p);
            }
            crate::value::OrdinaryToPrimitiveOutcome::TypeError => {
                crate::byte_cell_support::throw_type_error(
                    b"Cannot convert object to primitive value",
                );
            }
            // `DefaultString` (no usable `valueOf` / `toString`) and an
            // unchanged `Primitive` fall through to the TypeError below.
            _ => {}
        }
    }
    let label = unconvertible_label(value);
    crate::byte_cell_support::throw_type_error(
        format!("Cannot convert {label} to a BigInt").as_bytes(),
    );
}

/// Label of a value `ToBigInt` rejects, as in node's
/// `Cannot convert <x> to a BigInt` TypeError text.
fn unconvertible_label(value: f64) -> String {
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
