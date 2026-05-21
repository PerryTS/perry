//! Unit tests.

use std::ptr;

use super::*;

fn gc_collection_count_for_tests() -> u64 {
    let mut collections = 0;
    crate::gc::js_gc_stats(&mut collections, ptr::null_mut(), ptr::null_mut());
    collections
}

#[test]
fn test_array_alloc_and_access() {
    let arr = js_array_alloc(5);

    // Initially empty
    assert_eq!(js_array_length(arr), 0);

    // Push some values
    js_array_push_f64(arr, 1.0);
    js_array_push_f64(arr, 2.0);
    js_array_push_f64(arr, 3.0);

    assert_eq!(js_array_length(arr), 3);
    assert_eq!(js_array_get_f64(arr, 0), 1.0);
    assert_eq!(js_array_get_f64(arr, 1), 2.0);
    assert_eq!(js_array_get_f64(arr, 2), 3.0);

    // Out of bounds returns TAG_UNDEFINED (JS spec: arr[OOB] === undefined)
    assert_eq!(js_array_get_f64(arr, 5).to_bits(), 0x7FFC_0000_0000_0001u64);
}

#[test]
fn test_array_from_f64() {
    let values = [10.0, 20.0, 30.0, 40.0, 50.0];
    let arr = js_array_from_f64(values.as_ptr(), 5);

    assert_eq!(js_array_length(arr), 5);
    assert_eq!(js_array_get_f64(arr, 0), 10.0);
    assert_eq!(js_array_get_f64(arr, 2), 30.0);
    assert_eq!(js_array_get_f64(arr, 4), 50.0);
}

#[test]
fn test_array_set() {
    let arr = js_array_alloc(3);
    js_array_push_f64(arr, 1.0);
    js_array_push_f64(arr, 2.0);
    js_array_push_f64(arr, 3.0);

    js_array_set_f64(arr, 1, 99.0);
    assert_eq!(js_array_get_f64(arr, 1), 99.0);
}

#[test]
fn test_array_get_unchecked_basic() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 10.0);
    js_array_push_f64(arr, 20.0);
    js_array_push_f64(arr, 30.0);

    assert_eq!(js_array_get_f64_unchecked(arr, 0), 10.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 1), 20.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 2), 30.0);
}

#[test]
fn test_array_get_unchecked_out_of_bounds() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 1.0);

    // Out of bounds should return TAG_UNDEFINED (JS spec)
    assert_eq!(
        js_array_get_f64_unchecked(arr, 1).to_bits(),
        0x7FFC_0000_0000_0001u64
    );
    assert_eq!(
        js_array_get_f64_unchecked(arr, 100).to_bits(),
        0x7FFC_0000_0000_0001u64
    );
}

#[test]
fn test_array_get_f64_vs_unchecked_parity() {
    let arr = js_array_alloc(8);
    let values = [1.0, 2.5, -3.0, 0.0, 100.0, f64::INFINITY, f64::NEG_INFINITY];
    for &v in &values {
        js_array_push_f64(arr, v);
    }

    // Both functions should return identical results for plain arrays
    for i in 0..values.len() as u32 {
        let checked = js_array_get_f64(arr, i);
        let unchecked = js_array_get_f64_unchecked(arr, i);
        assert_eq!(
            checked.to_bits(),
            unchecked.to_bits(),
            "parity mismatch at index {}: checked={}, unchecked={}",
            i,
            checked,
            unchecked
        );
    }

    // Out of bounds parity — both return TAG_UNDEFINED
    let oob_checked = js_array_get_f64(arr, 100);
    let oob_unchecked = js_array_get_f64_unchecked(arr, 100);
    assert_eq!(oob_checked.to_bits(), 0x7FFC_0000_0000_0001u64);
    assert_eq!(oob_unchecked.to_bits(), 0x7FFC_0000_0000_0001u64);
}

#[test]
fn test_array_grow_capacity() {
    let mut arr = js_array_alloc(2);

    // Push well beyond initial capacity (push returns new ptr on grow)
    for i in 0..50 {
        arr = js_array_push_f64(arr, i as f64);
    }

    assert_eq!(js_array_length(arr), 50);

    // Verify all values preserved after growth
    for i in 0..50 {
        assert_eq!(
            js_array_get_f64(arr, i),
            i as f64,
            "value at index {} should be {}",
            i,
            i
        );
    }
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 50),
        Some(0),
        "numeric grow path should preserve pointer-free array layout"
    );
}

#[test]
fn test_array_push_f64_no_grow_fast_path() {
    let arr = js_array_alloc(4);
    let value = 42.5;
    let initial_capacity = unsafe { (*arr).capacity };

    let before = gc_collection_count_for_tests();
    let pushed = js_array_push_f64(arr, value);
    let after = gc_collection_count_for_tests();

    assert_eq!(pushed, arr);
    assert_eq!(after, before, "no-grow push must not trigger GC");
    assert_eq!(js_array_length(pushed), 1);
    assert_eq!(js_array_get_f64(pushed, 0), value);
    unsafe {
        assert_eq!((*pushed).capacity, initial_capacity);
    }

    let str_ptr = crate::string::js_string_from_bytes(b"fast-path".as_ptr(), 9);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));

    let before = gc_collection_count_for_tests();
    let pushed_again = js_array_push_f64(pushed, str_value);
    let after = gc_collection_count_for_tests();

    assert_eq!(pushed_again, pushed);
    assert_eq!(after, before, "tagged no-grow push must not trigger GC");
    assert_eq!(js_array_length(pushed_again), 2);
    assert_eq!(
        js_array_get_f64(pushed_again, 1).to_bits(),
        str_value.to_bits()
    );
}

#[test]
fn test_array_push_f64_grow_path_preserves_value_and_forwarding() {
    let mut arr = js_array_alloc(0);
    let initial = arr;
    let capacity = unsafe { (*arr).capacity };

    for i in 0..capacity {
        let pushed = js_array_push_f64(arr, i as f64);
        assert_eq!(pushed, arr);
        arr = pushed;
    }

    let str_ptr = crate::string::js_string_from_bytes(b"grow-path".as_ptr(), 9);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));

    let grown = js_array_push_f64(arr, str_value);

    assert_ne!(grown, arr, "push at capacity should grow the array");
    assert_eq!(js_array_length(grown), capacity + 1);
    assert_eq!(
        js_array_get_f64(grown, capacity).to_bits(),
        str_value.to_bits()
    );
    assert_eq!(
        js_array_length(initial),
        capacity + 1,
        "stale pre-grow pointer should follow the forwarding chain"
    );
    assert_eq!(
        js_array_get_f64(initial, capacity).to_bits(),
        str_value.to_bits()
    );
}

#[test]
fn test_array_set_unchecked_basic() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 1.0);
    js_array_push_f64(arr, 2.0);
    js_array_push_f64(arr, 3.0);

    js_array_set_f64_unchecked(arr, 1, 99.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 1), 99.0);
    // Other elements unchanged
    assert_eq!(js_array_get_f64_unchecked(arr, 0), 1.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 2), 3.0);
}

#[test]
fn test_array_pop_and_push() {
    let arr = js_array_alloc(4);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);

    let popped = js_array_pop_f64(arr);
    assert_eq!(popped, 3.0);
    assert_eq!(js_array_length(arr), 2);

    let arr = js_array_push_f64(arr, 4.0);
    assert_eq!(js_array_length(arr), 3);
    assert_eq!(js_array_get_f64(arr, 2), 4.0);
}

#[test]
fn test_array_indexOf() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 10.0);
    js_array_push_f64(arr, 20.0);
    js_array_push_f64(arr, 30.0);

    assert_eq!(js_array_indexOf_f64(arr, 10.0), 0);
    assert_eq!(js_array_indexOf_f64(arr, 20.0), 1);
    assert_eq!(js_array_indexOf_f64(arr, 30.0), 2);
    assert_eq!(js_array_indexOf_f64(arr, 99.0), -1);
}

#[test]
fn test_array_includes() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 1.0);
    js_array_push_f64(arr, 2.0);

    assert_eq!(js_array_includes_f64(arr, 1.0), 1);
    assert_eq!(js_array_includes_f64(arr, 2.0), 1);
    assert_eq!(js_array_includes_f64(arr, 3.0), 0);
}

#[test]
fn test_array_from_f64_and_length() {
    let values = [5.0, 10.0, 15.0];
    let arr = js_array_from_f64(values.as_ptr(), 3);

    assert_eq!(js_array_length(arr), 3);
    for i in 0..3 {
        assert_eq!(js_array_get_f64(arr, i), values[i as usize]);
    }
}

#[test]
fn test_array_null_safety() {
    // Null array pointer should not crash
    assert!(js_array_get_f64(std::ptr::null(), 0).is_nan());
    assert!(js_array_get_f64_unchecked(std::ptr::null(), 0).is_nan());
    assert_eq!(js_array_length(std::ptr::null()), 0);
}

#[test]
fn test_array_splice_delete_middle() {
    // [1,2,3,4,5].splice(1, 2) -> deleted=[2,3], arr=[1,4,5]
    let arr = js_array_alloc(8);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);
    let arr = js_array_push_f64(arr, 4.0);
    let arr = js_array_push_f64(arr, 5.0);
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 1, 2, std::ptr::null(), 0, &mut out_arr);

    assert_eq!(js_array_length(out_arr), 3);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 4.0);
    assert_eq!(js_array_get_f64(out_arr, 2), 5.0);

    assert_eq!(js_array_length(deleted), 2);
    assert_eq!(js_array_get_f64(deleted, 0), 2.0);
    assert_eq!(js_array_get_f64(deleted, 1), 3.0);
}

#[test]
fn test_array_splice_insert() {
    // [1,2,5].splice(2, 0, 3, 4) -> deleted=[], arr=[1,2,3,4,5]
    let arr = js_array_alloc(8);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 5.0);
    let items = [3.0_f64, 4.0];
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 2, 0, items.as_ptr(), 2, &mut out_arr);

    assert_eq!(js_array_length(deleted), 0);
    assert_eq!(js_array_length(out_arr), 5);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
    assert_eq!(js_array_get_f64(out_arr, 2), 3.0);
    assert_eq!(js_array_get_f64(out_arr, 3), 4.0);
    assert_eq!(js_array_get_f64(out_arr, 4), 5.0);
}

#[test]
fn test_array_splice_replace() {
    // [1,2,3].splice(1, 1, 99) -> deleted=[2], arr=[1,99,3]
    let arr = js_array_alloc(4);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);
    let items = [99.0_f64];
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 1, 1, items.as_ptr(), 1, &mut out_arr);

    assert_eq!(js_array_length(deleted), 1);
    assert_eq!(js_array_get_f64(deleted, 0), 2.0);
    assert_eq!(js_array_length(out_arr), 3);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 99.0);
    assert_eq!(js_array_get_f64(out_arr, 2), 3.0);
}

#[test]
fn test_array_splice_delete_to_end() {
    // [1,2,3,4].splice(2) -> deleted=[3,4], arr=[1,2]
    let arr = js_array_alloc(8);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);
    let arr = js_array_push_f64(arr, 4.0);
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 2, i32::MAX, std::ptr::null(), 0, &mut out_arr);

    assert_eq!(js_array_length(out_arr), 2);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
    assert_eq!(js_array_length(deleted), 2);
    assert_eq!(js_array_get_f64(deleted, 0), 3.0);
    assert_eq!(js_array_get_f64(deleted, 1), 4.0);
}

#[test]
fn test_array_splice_negative_start() {
    // [1,2,3,4].splice(-2, 1) -> deleted=[3], arr=[1,2,4]
    let arr = js_array_alloc(8);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);
    let arr = js_array_push_f64(arr, 4.0);
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, -2, 1, std::ptr::null(), 0, &mut out_arr);

    assert_eq!(js_array_length(deleted), 1);
    assert_eq!(js_array_get_f64(deleted, 0), 3.0);
    assert_eq!(js_array_length(out_arr), 3);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
    assert_eq!(js_array_get_f64(out_arr, 2), 4.0);
}

#[test]
fn test_array_splice_grow_realloc() {
    // Start with capacity 4, splice in 10 items to force reallocation
    let arr = js_array_alloc(4);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let items = [
        10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0_f64,
    ];
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 1, 0, items.as_ptr(), 10, &mut out_arr);

    assert_eq!(js_array_length(deleted), 0);
    assert_eq!(js_array_length(out_arr), 12);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    for i in 0..10 {
        assert_eq!(
            js_array_get_f64(out_arr, (i + 1) as u32),
            items[i],
            "mismatch at index {}",
            i + 1
        );
    }
    assert_eq!(js_array_get_f64(out_arr, 11), 2.0);
}
