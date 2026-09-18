//! The failure half of the receiver check codegen emits in front of every
//! static-type `Map` / `Set` fast path (#10446).
//!
//! Codegen lowers `s.add(v)`, `m.get(k)`, `this.labels.has(x)`, ... to a
//! direct `js_set_*` / `js_map_*` call when the receiver's DECLARED type is
//! `Set<T>` / `Map<K, V>`. Those helpers take the receiver as an unboxed 48-bit
//! payload, and a declared type is not a runtime fact: an uninitialized field
//! reads `undefined`, whose payload is the address `0x1`, and `js_set_add`
//! dereferenced it (SIGSEGV instead of a catchable TypeError — mongodb's
//! `MongoError.addErrorLabel` on an `errorLabelSet` that #10443 had left
//! undefined).
//!
//! The emitted guard is one compare of the box's top 16 bits against the
//! object tag. Every receiver that fails it and is not one of the untagged or
//! handle words the helpers have always accepted lands here. None of them
//! (`undefined`, `null`, booleans, numbers, strings, bigints) can have a
//! collection method, so this throws what the ordinary property lookup plus
//! call would have thrown.

use crate::value::JSValue;

/// Throw the `TypeError` for calling collection method `method` on the
/// non-object `receiver`.
///
/// `undefined` / `null` produce V8's `Cannot read properties of undefined
/// (reading 'add')`; any other primitive produces perry's generic
/// `(number).add is not a function`, the same text the untyped method
/// dispatch throws for that receiver.
///
/// `C-unwind` because generated code catches this through the same exception
/// path as `js_throw_type_error_property_access`, which it delegates to.
#[no_mangle]
pub extern "C-unwind" fn js_throw_collection_receiver_type_error(
    receiver: f64,
    method_ptr: *const u8,
    method_len: usize,
) -> ! {
    let value = JSValue::from_bits(receiver.to_bits());
    if value.is_undefined() || value.is_null() {
        crate::error::js_throw_type_error_property_access(
            u32::from(value.is_null()),
            method_ptr,
            method_len,
        );
    }
    let kind: &[u8] = if value.is_bool() {
        b"boolean"
    } else if value.is_any_string() {
        b"string"
    } else if value.is_bigint() {
        b"bigint"
    } else if value.is_int32() || value.is_number() {
        b"number"
    } else {
        b""
    };
    crate::error::js_throw_type_error_not_a_function(
        kind.as_ptr(),
        kind.len(),
        method_ptr,
        method_len,
    )
}
