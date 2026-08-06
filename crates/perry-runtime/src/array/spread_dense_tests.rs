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
use crate::value::{JSValue, TAG_HOLE, TAG_UNDEFINED};

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
    for id in [1i64, 0x1000, 0x40000, 0xE0000] {
        assert!(
            dense_spread_source(crate::value::js_nanbox_pointer(id)).is_none(),
            "handle id {id:#x} must be rejected by band"
        );
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
