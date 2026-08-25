//! `[...arr]` dense fast-path unit tests (#7533).
//!
//! These assert THE SUBJECT, not just the answer. `dense_spread_copy` produces
//! the same array the iterator drain would, so a test that only compared
//! elements would still pass if the fast path silently stopped applying — the
//! slow path is a correct fallback, which is exactly what makes a perf gate here
//! able to be green while measuring nothing (CLAUDE.md, "four ways a gate can be
//! unable to fail", case 4). So every case pins `dense_spread_source`'s verdict
//! as well: `is_some()` where the copy must be taken, `is_none()` where the
//! protocol must still run.

use super::*;
use crate::value::{JSValue, TAG_HOLE, TAG_NULL, TAG_UNDEFINED};

fn boxed(arr: *mut ArrayHeader) -> f64 {
    crate::value::js_nanbox_pointer(arr as i64)
}

fn dense(values: &[f64]) -> *mut ArrayHeader {
    let arr = js_array_alloc(values.len() as u32);
    let mut cur = arr;
    for v in values {
        cur = js_array_push_f64(cur, *v);
    }
    cur
}

unsafe fn slot_bits(arr: *const ArrayHeader, index: usize) -> u64 {
    let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    std::ptr::read(elements.add(index))
}

fn iterator_symbol_value() -> f64 {
    let sym = crate::symbol::well_known_symbol("iterator");
    assert!(!sym.is_null(), "well-known @@iterator must exist");
    f64::from_bits(JSValue::pointer(sym as *const u8).bits())
}

#[test]
fn plain_dense_array_takes_the_fast_path_and_copies_every_element() {
    let src = dense(&[1.0, 2.0, 3.0]);
    let value = boxed(src);
    assert!(
        dense_spread_source(value).is_some(),
        "an ordinary dense array is exactly what the fast path exists for"
    );
    let copy = dense_spread_copy(value);
    assert_ne!(
        copy as usize, src as usize,
        "spread must produce a new array"
    );
    unsafe {
        assert_eq!((*copy).length, 3);
        for i in 0..3 {
            assert_eq!(slot_bits(copy, i), slot_bits(src, i));
        }
    }
}

#[test]
fn the_copy_is_independent_of_its_source() {
    let src = dense(&[1.0, 2.0, 3.0]);
    let copy = dense_spread_copy(boxed(src));
    let _ = js_array_push_f64(copy, 99.0);
    unsafe {
        assert_eq!(
            (*src).length,
            3,
            "appending to the copy must not grow the source"
        );
        assert_eq!((*copy).length, 4);
    }
}

#[test]
fn an_empty_array_copies_to_an_empty_array() {
    let src = js_array_alloc(0);
    assert!(dense_spread_source(boxed(src)).is_some());
    let copy = dense_spread_copy(boxed(src));
    unsafe { assert_eq!((*copy).length, 0) };
}

#[test]
fn holes_become_undefined_not_hole_slots() {
    // The drain reads `arr[i]`, which is `undefined` for a hole; a raw memcpy
    // would hand the caller a TAG_HOLE slot, and `1 in [...[1, , 3]]` would flip
    // from true to false. This is the one place copy and drain disagree.
    let src = js_array_alloc(3);
    let mut cur = js_array_push_f64(src, 1.0);
    cur = js_array_push_hole(cur);
    cur = js_array_push_f64(cur, 3.0);
    unsafe {
        assert_eq!(
            slot_bits(cur, 1),
            TAG_HOLE,
            "probe must really plant a hole"
        )
    };

    assert!(dense_spread_source(boxed(cur)).is_some());
    let copy = dense_spread_copy(boxed(cur));
    unsafe {
        assert_eq!((*copy).length, 3);
        assert_eq!(slot_bits(copy, 0), 1.0f64.to_bits());
        assert_eq!(slot_bits(copy, 1), TAG_UNDEFINED);
        assert_eq!(slot_bits(copy, 2), 3.0f64.to_bits());
    }
}

#[test]
fn an_own_symbol_iterator_sends_the_spread_back_to_the_protocol() {
    let src = dense(&[1.0, 2.0, 3.0]);
    let value = boxed(src);
    assert!(
        dense_spread_source(value).is_some(),
        "must be eligible BEFORE the shadowing install, or this test proves nothing"
    );
    let sym = iterator_symbol_value();
    unsafe {
        crate::symbol::js_object_set_symbol_property(value, sym, 1.0);
    }
    assert!(
        dense_spread_source(value).is_none(),
        "an own [Symbol.iterator] shadows the builtin walk — the copy is not equivalent"
    );
}

#[test]
fn existence_probe_does_not_invoke_an_own_symbol_accessor() {
    // `own_symbol_property` answers by READING, which calls a user getter. The
    // guard must not: the slow path it falls back to reads the property again,
    // and a getter called twice is observable.
    let src = dense(&[1.0]);
    let value = boxed(src);
    let sym = iterator_symbol_value();
    unsafe {
        assert!(
            !crate::symbol::has_own_symbol_property(value, sym),
            "no own @@iterator yet"
        );
        crate::symbol::js_object_set_symbol_property(value, sym, 7.0);
        assert!(
            crate::symbol::has_own_symbol_property(value, sym),
            "existence must be visible without invoking anything"
        );
    }
}

#[test]
fn a_non_array_receiver_is_never_eligible() {
    // Every one of these has its own `obj_type`, and each has a spread meaning
    // the element copy would get wrong.
    let obj = crate::object::js_object_alloc(0, 2);
    assert!(dense_spread_source(crate::value::js_nanbox_pointer(obj as i64)).is_none());

    let s = crate::string::js_string_from_bytes(b"abc".as_ptr(), 3);
    assert!(dense_spread_source(f64::from_bits(JSValue::string_ptr(s).bits())).is_none());

    assert!(dense_spread_source(f64::from_bits(TAG_UNDEFINED)).is_none());
    assert!(dense_spread_source(f64::from_bits(crate::value::TAG_NULL)).is_none());
    assert!(dense_spread_source(42.0).is_none());
}

#[test]
fn a_handle_band_id_is_rejected_without_dereferencing_it() {
    // Web-Fetch / stream / timer ids are NaN-boxed POINTER values in the handle
    // band. Dereferencing `id - 8` as a GcHeader is a SIGSEGV (#7526), so the
    // rejection has to come from the band test, not from reading the header.
    use crate::value::addr_class::{
        COMMON_HANDLE_BAND_END, FETCH_HANDLE_BAND_START, HANDLE_BAND_MAX, ZLIB_HANDLE_BAND_START,
    };
    let ids = [
        1usize,
        COMMON_HANDLE_BAND_END,
        FETCH_HANDLE_BAND_START,
        ZLIB_HANDLE_BAND_START,
        HANDLE_BAND_MAX - 1,
    ];
    for id in ids {
        assert!(
            dense_spread_source(crate::value::js_nanbox_pointer(id as i64)).is_none(),
            "handle id {id:#x} must be rejected by band"
        );
    }
}

#[test]
fn a_grown_arrays_forwarding_header_is_followed_not_copied() {
    // `js_array_grow` leaves a GC_FLAG_FORWARDED header at the OLD address, and
    // its first eight bytes — where length/capacity used to live — now hold the
    // forwarding pointer. Reading `length` off the stale header yields a garbage
    // element count, and the memcpy derived from it took EXC_BAD_ACCESS. The JS
    // shapes were `[...sparse]` after `sparse.length = 5` and `[...beyond]` after
    // `beyond[9] = 9`; both grow past capacity while the caller still names the
    // pre-grow address.
    let src = dense(&[1.0, 2.0]);
    let stale = boxed(src);
    js_array_set_length(src, 40.0);

    let cleaned = clean_arr_ptr(src as *const ArrayHeader);
    assert_ne!(
        cleaned as usize, src as usize,
        "probe must really force a grow-and-forward, or it proves nothing"
    );

    // Everything below is driven from the STALE value the caller would hold.
    assert_eq!(
        dense_spread_source(stale).map(|p| p as usize),
        Some(cleaned as usize),
        "the guard must report the forwarded array, not the retired header"
    );
    let copy = dense_spread_copy(stale);
    unsafe {
        assert_eq!((*copy).length, 40);
        assert_eq!(slot_bits(copy, 0), 1.0f64.to_bits());
        assert_eq!(slot_bits(copy, 1), 2.0f64.to_bits());
        for i in 2..40 {
            assert_eq!(
                slot_bits(copy, i),
                TAG_UNDEFINED,
                "slot {i} past the original length reads undefined, like arr[i]"
            );
        }
    }
}

#[test]
fn a_sparse_array_whose_length_outruns_its_storage_is_not_eligible() {
    // Live indices past the dense backing store live in the named-property map,
    // which the element copy never reads — `array_iteration_is_exotic`'s third
    // arm. Without this gate `[...sparse]` would drop them.
    let src = dense(&[1.0, 2.0]);
    assert!(dense_spread_source(boxed(src)).is_some());
    unsafe { (*src).length = (*src).capacity + 1 };
    assert!(dense_spread_source(boxed(src)).is_none());
}

#[test]
fn short_packed_call_guard_copies_empty_and_one_element_arrays() {
    let mut out = [f64::NAN; 4];
    let empty = js_array_alloc(0);
    assert_eq!(
        unsafe { js_short_packed_spread_values(boxed(empty), out.as_mut_ptr()) },
        0
    );

    let one = dense(&[37.0]);
    assert_eq!(
        unsafe { js_short_packed_spread_values(boxed(one), out.as_mut_ptr()) },
        1
    );
    assert_eq!(out[0], 37.0);
}

#[test]
fn short_packed_call_guard_preserves_nullish_call_spread_as_empty() {
    let mut out = [f64::NAN; 4];
    assert_eq!(
        unsafe { js_short_packed_spread_values(f64::from_bits(TAG_UNDEFINED), out.as_mut_ptr()) },
        0
    );
    assert_eq!(
        unsafe { js_short_packed_spread_values(f64::from_bits(TAG_NULL), out.as_mut_ptr()) },
        0
    );
}

#[test]
fn short_packed_call_guard_rejects_holes_and_oversized_arrays() {
    let mut out = [0.0; 4];
    let holey = js_array_push_hole(js_array_push_f64(js_array_alloc(2), 1.0));
    assert_eq!(
        unsafe { js_short_packed_spread_values(boxed(holey), out.as_mut_ptr()) },
        -1,
        "a hole must take the iterator/apply fallback"
    );

    let oversized = dense(&[1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!(
        unsafe { js_short_packed_spread_values(boxed(oversized), out.as_mut_ptr()) },
        -1,
        "only arities 0 through 4 are specialized"
    );
}
