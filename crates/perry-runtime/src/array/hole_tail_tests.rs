//! The `[length, capacity)` hole invariant and the array allocation ceiling.
//!
//! Emitted element reads bound an index by `capacity` and read a slot past
//! `length` as a hole, so every length decrease must leave `TAG_HOLE` in the
//! vacated slots (`array_truncate_length`). These tests read the raw slots and
//! run the collector's verifier (`gc::verify_array_hole_tails`), which is what
//! turns red when a shrinking path skips the write.

use super::*;

fn boxed_string(text: &str) -> f64 {
    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    crate::value::js_nanbox_string(s as i64)
}

unsafe fn raw_slot(arr: *mut ArrayHeader, index: usize) -> u64 {
    *array_elements_ptr(clean_arr_ptr_mut(arr)).add(index)
}

fn assert_tail_is_holes(arr: *mut ArrayHeader, what: &str) {
    unsafe {
        let arr = clean_arr_ptr_mut(arr);
        for i in (*arr).length..(*arr).capacity {
            assert_eq!(
                raw_slot(arr, i as usize),
                crate::value::TAG_HOLE,
                "{what}: slot {i} past length {} is not a hole",
                (*arr).length
            );
        }
    }
    assert_eq!(
        crate::gc::verify_array_hole_tails(),
        None,
        "{what}: the collector's verifier found a non-hole slot past length"
    );
}

fn three_strings() -> *mut ArrayHeader {
    let mut arr = js_array_alloc(4);
    for text in ["alpha", "beta", "gamma"] {
        arr = js_array_push_f64(arr, boxed_string(text));
    }
    arr
}

#[test]
fn pop_leaves_a_hole_in_the_vacated_slot() {
    let arr = three_strings();
    let popped = js_array_pop_f64(arr);
    assert_ne!(popped.to_bits(), crate::value::TAG_HOLE);
    assert_eq!(js_array_length(arr), 2);
    assert_tail_is_holes(arr, "pop");
}

/// A pop, then `length++`, then a read of the re-exposed index: `undefined`,
/// never the popped value.
#[test]
fn pop_then_grow_reads_hole() {
    let arr = three_strings();
    js_array_pop_f64(arr);
    js_array_set_length(arr, 3.0);
    assert_eq!(
        js_array_get_f64(arr, 2).to_bits(),
        crate::value::TAG_UNDEFINED,
        "the popped element must not come back"
    );
}

#[test]
fn length_assignment_leaves_holes() {
    let arr = three_strings();
    js_array_set_length(arr, 1.0);
    assert_tail_is_holes(arr, "length = 1");
    let arr = three_strings();
    js_array_set_length(arr, 0.0);
    assert_tail_is_holes(arr, "length = 0");
}

#[test]
fn splice_that_shrinks_leaves_holes() {
    let arr = three_strings();
    // Returns the removed elements; the receiver comes back through out_arr.
    let mut receiver: *mut ArrayHeader = std::ptr::null_mut();
    let removed = js_array_splice(arr, 0, 2, std::ptr::null(), 0, &mut receiver);
    assert_eq!(js_array_length(removed), 2);
    let arr = receiver;
    assert_eq!(js_array_length(arr), 1);
    assert_tail_is_holes(arr, "splice(0, 2)");
}

#[test]
fn shift_leaves_holes() {
    let arr = three_strings();
    js_array_shift_f64(arr);
    assert_tail_is_holes(arr, "shift");
}

/// `GcHeader.size` is a `u32`: a backing store whose allocation does not fit
/// raises `RangeError: Invalid array length` before anything is reserved. Raise
/// the ceiling and this request reaches `gc_header_size_word`'s panic instead.
#[test]
fn capacity_past_the_size_ceiling_is_a_range_error() {
    const {
        assert!(
            super::alloc::ARRAY_MAX_CAPACITY as u64 * 8 + 16 <= u32::MAX as u64
                && (super::alloc::ARRAY_MAX_CAPACITY as u64 + 1) * 8 + 16 > u32::MAX as u64
        )
    };
    let outcome = crate::exception::catch_js_throw(|| {
        js_array_alloc(super::alloc::ARRAY_MAX_CAPACITY + 1) as usize
    });
    let thrown = outcome.expect_err("an allocation past the ceiling must throw");
    let err = (thrown.to_bits() & crate::value::POINTER_MASK) as *mut crate::error::ErrorHeader;
    let msg = crate::error::js_error_get_message(err);
    let text = unsafe { crate::object::has_own_helpers::str_from_string_header(msg) }
        .expect("the message is UTF-8")
        .to_string();
    assert_eq!(text, "Invalid array length");
}
