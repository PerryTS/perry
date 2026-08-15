//! A Symbol key on a typed-array receiver must do OrdinarySet — it must not be
//! swallowed by the numeric-index arms of the computed-store path.
//!
//! ECMA-262 §10.4.5.5: an Integer-Indexed exotic object routes a key that is
//! NOT a CanonicalNumericIndexString to OrdinarySet. A Symbol is definitionally
//! not one. `typed_array_set_numeric_index` could not tell the two apart — a
//! Symbol arrives as a NaN-boxed pointer, which AS AN f64 is a NaN, so it took
//! the "canonical-invalid index" arm, coerced for side effects, and returned
//! `true` meaning "write handled". The store was dropped in silence.
//!
//! # Why this test is shaped as a contract assertion
//!
//! The obvious end-to-end shape — allocate a typed array, call
//! `js_object_set_index_polymorphic` with a symbol key, read it back — PASSES
//! WITHOUT THE FIX and is therefore worthless. Measured: with the routing
//! removed, the direct call still stored the property, while the same source
//! compiled and run diverged from node. The direct call reaches a different
//! sub-arm than the compiled path does, so it cannot witness the bug.
//!
//! The end-to-end coverage therefore lives in
//! `test-files/test_gap_typed_array_symbol_key.ts`, which is byte-compared
//! against node and does fail without the fix. What is left here is the piece a
//! unit test CAN witness: that the numeric-index arm no longer claims a key it
//! cannot classify.

use crate::typedarray::{typed_array_alloc, KIND_UINT8};

/// The numeric-index arm must decline a Symbol key rather than report it
/// handled. Pre-fix this returned `true` and the write vanished.
#[test]
fn the_numeric_index_arm_does_not_claim_a_symbol_key() {
    let _serialized = crate::array::test_serialize();
    let ta = typed_array_alloc(KIND_UINT8, 2);
    crate::typedarray::js_typed_array_set(ta, 0, 1.0);
    crate::typedarray::js_typed_array_set(ta, 1, 2.0);

    let sym = unsafe { crate::symbol::js_symbol_new_empty() };
    assert_ne!(
        unsafe { crate::symbol::js_is_symbol(sym) },
        0,
        "precondition: the key under test must actually be a Symbol"
    );

    let claimed =
        unsafe { crate::typedarray_props::typed_array_set_numeric_index(ta as usize, sym, 5.0) };
    assert!(
        !claimed,
        "a Symbol is not a CanonicalNumericIndexString, so the numeric-index \
         arm must decline it and let the caller route it to OrdinarySet; \
         pre-fix it read the NaN-boxed symbol as a non-finite f64, classified \
         it a canonical-invalid index, and reported the write handled"
    );
}

/// The control that keeps the guard honest: a real out-of-bounds numeric index
/// must STILL be claimed and dropped per spec. Without this, making the
/// function decline everything would satisfy the test above.
#[test]
fn the_numeric_index_arm_still_claims_an_out_of_bounds_numeric_key() {
    let _serialized = crate::array::test_serialize();
    let ta = typed_array_alloc(KIND_UINT8, 2);

    let claimed =
        unsafe { crate::typedarray_props::typed_array_set_numeric_index(ta as usize, 99.0, 5.0) };
    assert!(
        claimed,
        "an out-of-bounds CanonicalNumericIndexString is still the numeric \
         arm's to handle — it is dropped per spec, not routed to OrdinarySet"
    );
}
