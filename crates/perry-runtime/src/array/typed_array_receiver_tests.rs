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
use crate::array::{
    js_array_alloc, js_array_copy_within, js_array_fill, js_array_fill_range, js_array_push_f64,
    js_array_reverse,
};
use crate::typedarray::{js_typed_array_get, js_typed_array_set, TypedArrayHeader};

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
    assert!(
        !out.is_null(),
        "copyWithin must return its receiver, not null"
    );
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

// --------------------------------------------------------------------------
// #8096: the same defect in the IMMUTABLE `Array.prototype` methods and in
// `sort`. #8090 fixed the four in-place mutators above and named these six as
// still-broken; they had the identical post-clean-delegation shape.
//
// Two things make these harder to get right than `fill`/`reverse`:
//
// * a broken `toReversed` / `toSorted` / `with` returns `js_array_alloc(0)` —
//   an EMPTY PLAIN ARRAY, not the unmutated receiver — so a test that only
//   checked "the receiver did not change" would pass while the RESULT was
//   wrong. Every test below reads the RESULT.
// * `%TypedArray%.prototype.sort` / `toSorted` with no comparator sort
//   NUMERICALLY (§23.2.3.29 / §23.2.3.32, CompareTypedArrayElements), where
//   `Array.prototype` sorts by ToString. `[10, 9, 2, 1]` is the discriminating
//   input: numeric order is `1, 2, 9, 10`, string order is `1, 10, 2, 9`, and
//   a no-op leaves `10, 9, 2, 1`. All three are distinguishable, so none of
//   these can pass by accident.
// --------------------------------------------------------------------------

use crate::array::{
    js_array_sort_default, js_array_sort_with_comparator, js_array_to_reversed,
    js_array_to_sorted_default, js_array_to_sorted_with_comparator, js_array_with,
};

/// Read a helper's RESULT (which may be a fresh typed array) back through the
/// per-kind accessor. The broken helpers returned `js_array_alloc(0)`, so a
/// length-0 plain array reads as an empty vec here — never as the expected
/// elements.
fn typed_read_back(ta: *mut ArrayHeader, len: usize) -> Vec<f64> {
    read_back(ta as *mut TypedArrayHeader, len)
}

/// A two-argument comparator closure, built the way `array/tests.rs` builds
/// its `map` callback: a bare `extern "C"` function behind a capture-less
/// `ClosureHeader`, which is what `resolve_call2_direct` expects.
extern "C" fn descending_cmp(
    _closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
) -> f64 {
    b - a
}

fn descending_comparator() -> *const crate::closure::ClosureHeader {
    crate::closure::js_closure_alloc(descending_cmp as *const u8, 0)
}

#[test]
fn js_array_sort_default_sorts_a_typed_array_numerically() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(INT32, &[10.0, 9.0, 2.0, 1.0]);
    let out = js_array_sort_default(as_array(ta));
    assert!(!out.is_null(), "sort must return its receiver, not null");
    assert_eq!(
        typed_read_back(out, 4),
        vec![1.0, 2.0, 9.0, 10.0],
        "typed sort is NUMERIC (§23.2.3.29). `1,10,2,9` would mean the plain \
         Array ToString order ran; `10,9,2,1` would mean the delegation is \
         still unreachable and the sort was a no-op"
    );
    // Sorted in place: the receiver itself, not a copy.
    assert_eq!(typed_read_back(as_array(ta), 4), vec![1.0, 2.0, 9.0, 10.0]);
}

#[test]
fn js_array_sort_with_comparator_sorts_a_typed_array() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(INT32, &[1.0, 10.0, 2.0, 9.0]);
    // Descending. The input is deliberately NOT already descending — an
    // already-sorted input would let a no-op pass.
    let cmp = descending_comparator();
    let out = js_array_sort_with_comparator(as_array(ta), cmp);
    assert!(!out.is_null());
    assert_eq!(typed_read_back(out, 4), vec![10.0, 9.0, 2.0, 1.0]);
}

#[test]
fn js_array_to_reversed_reverses_a_typed_array_into_a_typed_array() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_to_reversed(as_array(ta));
    assert!(!out.is_null());
    assert_eq!(
        typed_read_back(out, 4),
        vec![4.0, 3.0, 2.0, 1.0],
        "an empty plain array (`js_array_alloc(0)`) is what the broken path \
         returned — reading element-typed values here is the proof it did not"
    );
    // The result is a %TypedArray%, not a plain Array: `dr.constructor.name`
    // was "Array" before the fix.
    assert!(
        crate::typedarray::lookup_typed_array_kind(out as usize).is_some(),
        "toReversed on a typed array must produce a typed array"
    );
    // Immutable: the source is untouched.
    assert_eq!(typed_read_back(as_array(ta), 4), vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn js_array_to_sorted_default_sorts_a_typed_array_numerically() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(INT32, &[10.0, 9.0, 2.0, 1.0]);
    let out = js_array_to_sorted_default(as_array(ta));
    assert!(!out.is_null());
    assert_eq!(typed_read_back(out, 4), vec![1.0, 2.0, 9.0, 10.0]);
    assert!(crate::typedarray::lookup_typed_array_kind(out as usize).is_some());
    assert_eq!(
        typed_read_back(as_array(ta), 4),
        vec![10.0, 9.0, 2.0, 1.0],
        "toSorted is immutable — the receiver must not be sorted in place"
    );
}

#[test]
fn js_array_to_sorted_with_comparator_sorts_a_typed_array() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(INT32, &[1.0, 10.0, 2.0, 9.0]);
    let cmp = descending_comparator();
    let out = js_array_to_sorted_with_comparator(as_array(ta), cmp);
    assert!(!out.is_null());
    assert_eq!(typed_read_back(out, 4), vec![10.0, 9.0, 2.0, 1.0]);
    assert!(crate::typedarray::lookup_typed_array_kind(out as usize).is_some());
    assert_eq!(typed_read_back(as_array(ta), 4), vec![1.0, 10.0, 2.0, 9.0]);
}

#[test]
fn js_array_with_replaces_one_typed_element_and_honours_the_lane_width() {
    let _serialized = crate::array::test_serialize();
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_with(as_array(ta), 1.0, 70000.0);
    assert!(!out.is_null());
    // 70000 & 0xFFFF == 4464: the replacement went through the per-kind
    // store, not a raw f64 slot write.
    assert_eq!(typed_read_back(out, 4), vec![1.0, 4464.0, 3.0, 4.0]);
    assert!(crate::typedarray::lookup_typed_array_kind(out as usize).is_some());
    assert_eq!(typed_read_back(as_array(ta), 4), vec![1.0, 2.0, 3.0, 4.0]);

    // Negative index counts from the end.
    let ta = typed(UINT16, &[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_with(as_array(ta), -1.0, 9.0);
    assert_eq!(typed_read_back(out, 4), vec![1.0, 2.0, 3.0, 9.0]);
}

#[test]
fn plain_array_immutable_methods_and_sort_are_unchanged_by_the_typed_pre_check() {
    let _serialized = crate::array::test_serialize();

    // Plain `Array.prototype.sort` keeps its ToString ordering — the typed
    // pre-check must not have hijacked it into the numeric comparator.
    let arr = plain(&[10.0, 9.0, 2.0, 1.0]);
    js_array_sort_default(arr);
    assert_eq!(plain_read(arr, 4), vec![1.0, 10.0, 2.0, 9.0]);

    let arr = plain(&[10.0, 9.0, 2.0, 1.0]);
    let out = js_array_to_sorted_default(arr);
    assert_eq!(plain_read(out, 4), vec![1.0, 10.0, 2.0, 9.0]);
    assert!(
        crate::typedarray::lookup_typed_array_kind(out as usize).is_none(),
        "toSorted on a plain Array must not produce a typed array"
    );
    assert_eq!(plain_read(arr, 4), vec![10.0, 9.0, 2.0, 1.0]);

    let arr = plain(&[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_to_reversed(arr);
    assert_eq!(plain_read(out, 4), vec![4.0, 3.0, 2.0, 1.0]);
    assert_eq!(plain_read(arr, 4), vec![1.0, 2.0, 3.0, 4.0]);

    let arr = plain(&[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_with(arr, 1.0, 70000.0);
    assert_eq!(
        plain_read(out, 4),
        vec![1.0, 70000.0, 3.0, 4.0],
        "a plain Array slot is a boxed f64 — no 16-bit truncation"
    );
    assert_eq!(plain_read(arr, 4), vec![1.0, 2.0, 3.0, 4.0]);
}

// --------------------------------------------------------------------------
// #8096, the Buffer-backed `Uint8Array` half.
//
// `new Uint8Array([…])` does NOT produce a registry `TypedArrayHeader` in
// perry — `buffer::js_uint8array_new` returns a `BufferHeader`, registered as
// a buffer and marked `mark_as_uint8array`. So `typed_array_receiver` answers
// `None` for the most common typed array in the language, while
// `clean_arr_ptr` still rejects it (a tracked non-`GC_TYPE_ARRAY` allocation),
// and the helper answered an EMPTY plain array.
//
// Measured against node v26.5.1 before this arm existed:
//
//   ann u8 toReversed: 4 9 2 10 1 [object Uint8Array]   <- node
//   ann u8 toReversed: 0 undefined … [object Array]     <- perry
//
// `sort` / `with` / `reverse` / `fill` on this shape were already right — they
// resolve through the dynamic method dispatcher rather than these helpers —
// and `copyWithin` got its own Buffer arm in #8090. Only `toReversed` and
// `toSorted` had no Buffer arm anywhere, on either dispatch path.
// --------------------------------------------------------------------------

/// The real constructor path: a plain array through `js_uint8array_new`,
/// exactly what `new Uint8Array([…])` lowers to.
fn uint8_buffer(values: &[f64]) -> *mut ArrayHeader {
    let arr = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    let boxed = crate::value::js_nanbox_pointer(arr as i64);
    crate::buffer::js_uint8array_new(boxed) as *mut ArrayHeader
}

#[test]
fn a_new_uint8array_is_a_buffer_not_a_registry_typed_array() {
    let _serialized = crate::array::test_serialize();
    let buf = uint8_buffer(&[1.0, 2.0, 3.0, 4.0]);
    let addr = buf as usize;

    // This is the precondition the Buffer arm exists for. If it ever flips —
    // `new Uint8Array` starting to produce a registry typed array — the arm
    // becomes redundant and `typed_array_receiver` covers this shape, so this
    // failing is a signal to re-read the constructor, not to delete the test.
    assert!(
        crate::buffer::is_registered_buffer(addr),
        "new Uint8Array([…]) must be a registered buffer"
    );
    assert!(
        crate::typedarray::lookup_typed_array_kind(addr).is_none(),
        "…and NOT in the typed-array registry, which is why \
         typed_array_receiver cannot answer for it"
    );
    assert!(
        crate::array::header::typed_array_receiver(buf).is_none(),
        "typed_array_receiver is registry-backed, so it must answer None here"
    );
    // …and the shared funnel still rejects it, so a post-clean branch would be
    // just as unreachable as it is for a real GC_TYPE_TYPED_ARRAY.
    assert!(
        crate::array::header::clean_arr_ptr_mut(buf).is_null(),
        "clean_arr_ptr must keep rejecting a BufferHeader receiver"
    );
}

#[test]
fn js_array_to_reversed_reverses_a_buffer_backed_uint8array() {
    let _serialized = crate::array::test_serialize();
    let buf = uint8_buffer(&[1.0, 2.0, 3.0, 4.0]);
    let out = js_array_to_reversed(buf);
    assert_eq!(
        typed_read_back(out, 4),
        vec![4.0, 3.0, 2.0, 1.0],
        "the broken path returned js_array_alloc(0) — an EMPTY plain array"
    );
    assert_eq!(
        crate::typedarray::lookup_typed_array_kind(out as usize),
        Some(crate::typedarray::KIND_UINT8),
        "node answers a Uint8Array here, not an Array"
    );
    // Immutable: the source buffer is untouched.
    assert_eq!(crate::buffer::js_buffer_get(buf as *const _, 0), 1);
    assert_eq!(crate::buffer::js_buffer_get(buf as *const _, 3), 4);
}

#[test]
fn js_array_to_sorted_sorts_a_buffer_backed_uint8array_numerically() {
    let _serialized = crate::array::test_serialize();
    // 10 before 9 before 2 before 1: numeric order is 1,2,9,10; the plain
    // Array ToString order would be 1,10,2,9; a no-op would be 10,9,2,1.
    let buf = uint8_buffer(&[10.0, 9.0, 2.0, 1.0]);
    let out = js_array_to_sorted_default(buf);
    assert_eq!(typed_read_back(out, 4), vec![1.0, 2.0, 9.0, 10.0]);
    assert_eq!(
        crate::typedarray::lookup_typed_array_kind(out as usize),
        Some(crate::typedarray::KIND_UINT8)
    );
    assert_eq!(crate::buffer::js_buffer_get(buf as *const _, 0), 10);

    let buf = uint8_buffer(&[1.0, 10.0, 2.0, 9.0]);
    let out = js_array_to_sorted_with_comparator(buf, descending_comparator());
    assert_eq!(typed_read_back(out, 4), vec![10.0, 9.0, 2.0, 1.0]);
}

#[test]
fn js_array_with_replaces_one_byte_of_a_buffer_backed_uint8array() {
    let _serialized = crate::array::test_serialize();
    let buf = uint8_buffer(&[1.0, 2.0, 3.0, 4.0]);
    // 300 must wrap to 44: the replacement went through the 1-byte lane.
    let out = js_array_with(buf, 1.0, 300.0);
    assert_eq!(typed_read_back(out, 4), vec![1.0, 44.0, 3.0, 4.0]);
    assert_eq!(
        crate::typedarray::lookup_typed_array_kind(out as usize),
        Some(crate::typedarray::KIND_UINT8)
    );
    assert_eq!(crate::buffer::js_buffer_get(buf as *const _, 1), 2);
}

#[test]
fn an_array_buffer_receiver_is_not_treated_as_a_uint8array() {
    let _serialized = crate::array::test_serialize();
    // `ArrayBuffer` / `SharedArrayBuffer` / `DataView` have no
    // %TypedArray%.prototype — node throws `TypeError: … is not a function`
    // rather than answering elements, so the Buffer arm must decline them and
    // leave the pre-existing behaviour alone.
    let ab = crate::buffer::buffer_alloc(4);
    crate::buffer::mark_as_array_buffer(ab as usize);
    assert!(
        crate::array::buffer_receiver_as_uint8_typed_array(ab as *mut ArrayHeader).is_none(),
        "an ArrayBuffer receiver must not be served as a Uint8Array"
    );

    let dv = crate::buffer::buffer_alloc(4);
    crate::buffer::mark_as_data_view(dv as usize);
    assert!(
        crate::array::buffer_receiver_as_uint8_typed_array(dv as *mut ArrayHeader).is_none(),
        "a DataView receiver must not be served as a Uint8Array"
    );
}

// ---------------------------------------------------------------------------
// #8140: the `.values()` / `.keys()` / `.entries()` iterator entry points must
// resolve a Buffer-backed `Uint8Array` receiver BEFORE the array-only funnel.
//
// The precondition is already pinned by
// `a_new_uint8array_is_a_buffer_not_a_registry_typed_array` above: perry's
// `new Uint8Array([…])` is a `BufferHeader`, absent from the typed-array
// registry (so the #3148 `lookup_typed_array_kind` delegations never answer for
// it) and nulled by `clean_arr_ptr` (so a post-clean branch is unreachable).
// `buffer_alloc` stamps a real `GC_TYPE_BUFFER` GcHeader through
// `arena_alloc_gc_old`, which is why #8041's "reject every TRACKED non-array"
// catches it and its predecessor "reject GC_TYPE_OBJECT / GC_TYPE_CLOSURE"
// did not.
//
// `array_iter_obj_raw` opens with that funnel, so EVERY branch below it — the
// fs-dir arm, the Map/Set arm, and the iterator construction itself — was
// unreachable for a Buffer receiver, and all three methods yielded an EMPTY
// iterator. Measured against node v26.5.1 on `new Uint8Array([3,1,2])`:
//
//   method      node                    perry pre-fix
//   .values()   [3,1,2]                 []
//   .keys()     [0,1,2]                 []
//   .entries()  [[0,3],[1,1],[2,2]]     []
//
// `keys` is the sharpest evidence this is a REGRESSION and not a standing gap:
// it only ever reads `length`, so it answered correctly before #8041.
//
// Reachable from BOTH a property-read receiver (`holder.u.values()`, which
// codegen fuses to `js_array_values_iter_obj`) and a fully dynamic one
// (`opaque(u).values()`), so it is not a narrow static-typing corner.
// ---------------------------------------------------------------------------

/// Drive an iterator object returned by `js_array_*_iter_obj` to exhaustion,
/// rendering each yielded value so the expectations below read as the node
/// output they were taken from.
fn drain_iter(iter: i64) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let obj = iter as *mut crate::object::ObjectHeader;
        assert!(!obj.is_null(), "iter_obj must return an iterator object");
        for _ in 0..64 {
            let result = crate::array::dispatch_array_iterator_method(obj, "next");
            let robj =
                crate::value::js_nanbox_get_pointer(result) as *mut crate::object::ObjectHeader;
            assert!(!robj.is_null(), "next() must return a result object");
            let value = crate::object::js_object_get_field(robj, 0);
            let done = crate::object::js_object_get_field(robj, 1);
            if done.bits() == crate::value::TAG_TRUE {
                break;
            }
            out.push(render(f64::from_bits(value.bits())));
        }
    }
    out
}

/// `3` for a number, `[0,3]` for a 2-element pair array — enough to tell the
/// three iterator kinds apart, and to tell a correct element from a raw byte
/// reinterpreted as an f64 (which renders as `1.5e-323`, never as `3`).
fn render(v: f64) -> String {
    let bits = v.to_bits();
    if (bits >> 48) == 0x7FFD {
        let inner = crate::value::js_nanbox_get_pointer(v) as *const ArrayHeader;
        let cleaned = crate::array::header::clean_arr_ptr(inner);
        if !cleaned.is_null() {
            let n = unsafe { (*cleaned).length } as usize;
            let parts: Vec<String> = (0..n)
                .map(|i| render(crate::array::js_array_get_f64(cleaned, i as u32)))
                .collect();
            return format!("[{}]", parts.join(","));
        }
    }
    let n = f64::from_bits(bits);
    if n.is_finite() && n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n:?}")
    }
}

#[test]
fn js_array_values_iter_obj_yields_a_buffer_receivers_bytes() {
    let _serialized = crate::array::test_serialize();
    let buf = uint8_buffer(&[3.0, 1.0, 2.0]);
    assert_eq!(
        drain_iter(crate::array::js_array_values_iter_obj(buf)),
        vec!["3", "1", "2"],
        "node yields [3,1,2]; an EMPTY list is #8140 — the funnel nulled the \
         receiver before the Buffer question was ever asked"
    );
}

#[test]
fn js_array_keys_iter_obj_yields_a_buffer_receivers_indices() {
    let _serialized = crate::array::test_serialize();
    let buf = uint8_buffer(&[3.0, 1.0, 2.0]);
    assert_eq!(
        drain_iter(crate::array::js_array_keys_iter_obj(buf)),
        vec!["0", "1", "2"],
        "node yields [0,1,2]. `keys` reads only `length`, so it was CORRECT \
         before #8041 — this case is the proof that #8140 is a regression"
    );
}

#[test]
fn js_array_entries_iter_obj_yields_a_buffer_receivers_pairs() {
    let _serialized = crate::array::test_serialize();
    let buf = uint8_buffer(&[3.0, 1.0, 2.0]);
    assert_eq!(
        drain_iter(crate::array::js_array_entries_iter_obj(buf)),
        vec!["[0,3]", "[1,1]", "[2,2]"],
        "node yields [[0,3],[1,1],[2,2]]"
    );
}

#[test]
fn a_typed_array_receiver_still_yields_element_typed_values() {
    let _serialized = crate::array::test_serialize();
    // The pre-existing #3148 arm must survive the reordering. `70000 & 0xFFFF`
    // is 4464, so a raw-f64 reinterpretation cannot produce this answer.
    let ta = typed(UINT16, &[70000.0, 2.0]);
    assert_eq!(
        drain_iter(crate::array::js_array_values_iter_obj(as_array(ta))),
        vec!["4464", "2"],
        "the typed-array materialization must still run element-typed reads"
    );
}

#[test]
fn a_plain_array_iterator_never_probes_the_typed_array_registry() {
    let _serialized = crate::array::test_serialize();
    let mut arr = js_array_alloc(3);
    for v in [7.0, 8.0, 9.0] {
        arr = js_array_push_f64(arr, v);
    }

    // Prime anything built lazily on first touch, then measure ONLY the
    // receiver-resolution call. `drain_iter`/`render` read elements through
    // `js_array_get_f64`, which runs its own #8109 typed-array probe once per
    // element — measuring across the drain counts those and says nothing about
    // the subject. (Measured: it reports exactly +1 per element, so a naive
    // window would have "failed" here for the wrong reason.)
    let _ = drain_iter(crate::array::js_array_values_iter_obj(arr));
    let before = crate::typedarray::test_typed_array_registry_probe_count();
    let iter = crate::array::js_array_values_iter_obj(arr);
    let after = crate::typedarray::test_typed_array_registry_probe_count();

    assert_eq!(
        drain_iter(iter),
        vec!["7", "8", "9"],
        "the control receiver must keep iterating its own elements"
    );
    assert_eq!(
        after, before,
        "a GC_TYPE_ARRAY receiver must never reach lookup_typed_array_kind — \
         delete the receiver-tag gate at the top of `typed_array_iter_arr` and \
         this is what fails, even though the ANSWER above stays correct"
    );
}

#[test]
fn an_array_buffer_or_data_view_receiver_gets_no_element_iterator() {
    let _serialized = crate::array::test_serialize();
    // `ArrayBuffer` / `SharedArrayBuffer` / `DataView` have no
    // %TypedArray%.prototype, so node throws `… is not a function` rather than
    // answering elements. The Buffer arm must decline them and leave the
    // pre-existing behaviour untouched, exactly as
    // `buffer_receiver_as_uint8_typed_array` does.
    let ab = crate::buffer::buffer_alloc(4);
    unsafe { (*ab).length = 4 };
    crate::buffer::mark_as_array_buffer(ab as usize);
    assert!(
        drain_iter(crate::array::js_array_values_iter_obj(
            ab as *mut ArrayHeader
        ))
        .is_empty(),
        "an ArrayBuffer receiver must not be served a Uint8Array iterator"
    );

    let dv = crate::buffer::buffer_alloc(4);
    unsafe { (*dv).length = 4 };
    crate::buffer::mark_as_data_view(dv as usize);
    assert!(
        drain_iter(crate::array::js_array_values_iter_obj(
            dv as *mut ArrayHeader
        ))
        .is_empty(),
        "a DataView receiver must not be served a Uint8Array iterator"
    );
}
