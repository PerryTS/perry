//! #2879 / #7574: a %TypedArray% receiver that reaches an `Array.prototype`
//! **in-place mutator** must land on the element-typed `js_typed_array_*`
//! implementation, not fall off the end of the plain-array helper.
//!
//! ## Why this file exists
//!
//! Codegen deliberately routes typed-array receivers through the generic
//! `js_array_*` helpers — `is_array_expr` (perry-codegen
//! `type_analysis/predicates.rs`) answers `true` for `Int32Array` &co. on the
//! #3148 contract that each helper re-dispatches on
//! `lookup_typed_array_kind`. Around forty helpers in this module implement
//! their half of that contract.
//!
//! Those delegations sat **after** the shared `clean_arr_ptr` funnel, which
//! since #7574 rejects every *tracked non-array* GC object, and since the
//! 2026-07-09 typed-array audit every typed array is a tracked
//! `GC_TYPE_TYPED_ARRAY` allocation. So the four in-place mutators returned at
//! `arr.is_null()` and their typed branch became unreachable code: `fill`,
//! `reverse` and `copyWithin` silently did nothing at all, with no error and
//! no diagnostic.
//!
//! `clean_arr_ptr`'s rejection is correct and stays — a `TypedArrayHeader`'s
//! raw storage must never be read as boxed f64 `ArrayHeader` slots. What was
//! wrong is the ORDER. `typed_array_receiver` now answers the "is this a
//! typed array?" question up front, from the raw (possibly NaN-boxed) argument.
//!
//! ## What each test asserts, and how it can fail
//!
//! * `clean_arr_ptr_still_rejects_a_typed_array_receiver` pins the
//!   *precondition*. If it ever goes green-by-accident (clean starts accepting
//!   typed arrays) the fix below is redundant and the type-confusion #7574
//!   closed is back — so this test failing is a signal to re-read the guard,
//!   not to delete the test.
//! * every mutator test asserts a **narrower-than-f64 element width** was
//!   used, by storing a value that only survives per-kind truncation
//!   (`70000 & 0xFFFF == 4464` for `Uint16Array`). A regression that
//!   memcpy'd raw f64 slots, or that no-op'd, fails that assertion — so the
//!   test cannot pass while merely "not throwing" (CLAUDE.md's fourth way a
//!   gate cannot fail).
//! * the plain-`Array` controls prove the typed pre-check did not hijack the
//!   ordinary path.

use super::*;
use crate::array::{js_array_alloc, js_array_copy_within, js_array_fill, js_array_fill_range,
    js_array_push_f64, js_array_reverse};
use crate::typedarray::{TypedArrayHeader, js_typed_array_get, js_typed_array_set};

/// `Uint16Array` (kind 4 per `elem_size_for_kind`) — 2-byte elements, so a
/// value above 0xFFFF proves the store went through the per-kind accessor.
const UINT16: u8 = crate::typedarray::KIND_UINT16;
/// `Int32Array` — 4-byte elements, and signed, so 0x8000_0000 reads back
/// negative only if the element width was honoured.
const INT32: u8 = crate::typedarray::KIND_INT32;

fn typed(kind: u8, values: &[f64]) -> *mut TypedArrayHeader {
    let ta = crate::typedarray::typed_array_alloc(kind, values.len() as u32);
    for (i, v) in values.iter().enumerate() {
        js_typed_array_set(ta, i as i32, *v);
    }
    ta
}

fn read_back(ta: *mut TypedArrayHeader, len: usize) -> Vec<f64> {
    (0..len).map(|i| js_typed_array_get(ta, i as i32)).collect()
}

/// A typed array handed to a plain-array helper, exactly as codegen emits it.
fn as_array(ta: *mut TypedArrayHeader) -> *mut ArrayHeader {
    ta as *mut ArrayHeader
}

#[test]
fn clean_arr_ptr_still_rejects_a_typed_array_receiver() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);

    // The registry probe the #3148 delegations key on works fine — so a dead
    // delegation is never "the typed array wasn't registered".
    assert!(
        crate::typedarray::lookup_typed_array_kind(ta as usize).is_some(),
        "a freshly allocated typed array must be registered, or the probe \
         below proves nothing about ordering"
    );

    // ...and yet the shared receiver funnel rejects it, because it is a
    // tracked GC_TYPE_TYPED_ARRAY object rather than a GC_TYPE_ARRAY one.
    // This is WHY every post-clean typed branch was unreachable.
    assert!(
        crate::array::header::clean_arr_ptr_mut(as_array(ta)).is_null(),
        "clean_arr_ptr must keep rejecting a TypedArrayHeader (#7574) — the \
         typed pre-check exists precisely because it does"
    );
}

#[test]
fn js_array_fill_fills_a_typed_array_receiver_element_typed() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_fill(as_array(ta), 70000.0);
    assert!(!out.is_null(), "fill must return its receiver, not null");
    // 70000 truncated to 16 bits == 4464: proof the per-kind store ran.
    assert_eq!(read_back(ta, 4), vec![4464.0, 4464.0, 4464.0, 4464.0]);
}

#[test]
fn js_array_fill_range_fills_only_the_requested_range() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);
    js_array_fill_range(as_array(ta), 9.0, 0.0, 2.0);
    assert_eq!(
        read_back(ta, 4),
        vec![9.0, 9.0, 3.0, 4.0],
        "a 3-arg fill must respect [start, end) — filling the whole array is \
         the failure mode the range plumbing exists to prevent"
    );

    // Negative indices count from the end, and +Infinity (codegen's absent-end
    // sentinel) clamps to length.
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);
    js_array_fill_range(as_array(ta), 7.0, -2.0, f64::INFINITY);
    assert_eq!(read_back(ta, 4), vec![1.0, 2.0, 7.0, 7.0]);
}

#[test]
fn js_array_reverse_reverses_a_typed_array_receiver() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(INT32, &[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_reverse(as_array(ta));
    assert!(!out.is_null(), "reverse must return its receiver, not null");
    assert_eq!(read_back(ta, 4), vec![4.0, 3.0, 2.0, 1.0]);

    // Odd length: the middle element stays put.
    let ta = typed(INT32, &[1.0, 2.0, 3.0, 4.0, 5.0]);
    js_array_reverse(as_array(ta));
    assert_eq!(read_back(ta, 5), vec![5.0, 4.0, 3.0, 2.0, 1.0]);
}

#[test]
fn js_array_copy_within_copies_typed_elements() {
    let _serialized = crate::array::test_serialize();
    // `c.copyWithin(0, 2)` — no end argument (has_end == 0 means "to length").
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_copy_within(as_array(ta), 0.0, 2.0, 0, 0.0);
    assert!(!out.is_null(), "copyWithin must return its receiver, not null");
    assert_eq!(read_back(ta, 4), vec![3.0, 4.0, 3.0, 4.0]);

    // Negative target/start, and an out-of-range end that clamps to length.
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);
    js_array_copy_within(as_array(ta), -2.0, -4.0, 1, 99.0);
    assert_eq!(read_back(ta, 4), vec![1.0, 2.0, 1.0, 2.0]);
}

#[test]
fn typed_mutators_honour_the_element_width_not_raw_f64_slots() {
    let _serialized = crate::array::test_serialize();
    // 0x8000_0000 into an Int32Array reads back as i32::MIN. A plain-array
    // f64-slot path would return 2147483648, and a no-op would return 0 —
    // both distinguishable, which is what makes this a live-subject check.
    let ta = typed(INT32, &[0.0, 0.0]);
    js_array_fill(as_array(ta), 2147483648.0);
    assert_eq!(read_back(ta, 2), vec![-2147483648.0, -2147483648.0]);
}

// --------------------------------------------------------------------------
// Controls: the ordinary plain-Array path must be untouched.
// --------------------------------------------------------------------------

fn plain(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(values.len() as u32);
    for v in values {
        arr = js_array_push_f64(arr, *v);
    }
    arr
}

fn plain_read(arr: *mut ArrayHeader, len: usize) -> Vec<f64> {
    (0..len)
        .map(|i| crate::array::js_array_get_element(arr as i64, i as i64))
        .collect()
}

#[test]
fn plain_array_mutators_are_unchanged_by_the_typed_pre_check() {
    let _serialized = crate::array::test_serialize();

    let arr = plain(&[1.0, 2.0, 3.0, 4.0]);
    js_array_fill(arr, 9.0);
    assert_eq!(plain_read(arr, 4), vec![9.0, 9.0, 9.0, 9.0]);

    let arr = plain(&[1.0, 2.0, 3.0, 4.0]);
    js_array_fill_range(arr, 9.0, 0.0, 2.0);
    assert_eq!(plain_read(arr, 4), vec![9.0, 9.0, 3.0, 4.0]);

    let arr = plain(&[1.0, 2.0, 3.0, 4.0, 5.0]);
    js_array_reverse(arr);
    assert_eq!(plain_read(arr, 5), vec![5.0, 4.0, 3.0, 2.0, 1.0]);

    let arr = plain(&[1.0, 2.0, 3.0, 4.0]);
    js_array_copy_within(arr, 0.0, 2.0, 0, 0.0);
    assert_eq!(plain_read(arr, 4), vec![3.0, 4.0, 3.0, 4.0]);
}
