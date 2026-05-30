//! Unit tests.

use std::ptr;

use super::*;

extern "C" fn test_map_to_string(
    _closure: *const crate::closure::ClosureHeader,
    _element: f64,
    _index: f64,
) -> f64 {
    let str_ptr = crate::string::js_string_from_bytes(b"mapped".as_ptr(), 6);
    f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK))
}

fn gc_collection_count_for_tests() -> u64 {
    let mut collections = 0;
    crate::gc::js_gc_stats(&mut collections, ptr::null_mut(), ptr::null_mut());
    collections
}

fn assert_numeric_raw_values(arr: *mut ArrayHeader, expected: &[f64]) {
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_length(arr), expected.len() as u32);
    for (index, value) in expected.iter().enumerate() {
        assert_eq!(js_array_numeric_get_f64_unboxed(arr, index as u32), *value);
    }
}

fn int32_jsvalue_bits(value: i32) -> u64 {
    crate::value::JSValue::int32(value).bits()
}

fn assert_canonical_raw_slot(arr: *mut ArrayHeader, index: u32, expected: f64) {
    let raw_bits = js_array_get_f64_unchecked(arr, index).to_bits();
    assert_eq!(raw_bits, expected.to_bits());
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, index), expected);
}

unsafe fn raw_slot_bits(arr: *mut ArrayHeader, index: usize) -> u64 {
    let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    *elements.add(index)
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
fn test_array_clone_prefers_buffer_registry_before_gc_header_probe() {
    let mut adjacent = None;
    for _ in 0..4 {
        let fake_prev = crate::buffer::buffer_alloc(8);
        let buf = crate::buffer::buffer_alloc(4);
        let expected_next = fake_prev as usize
            + ((std::mem::size_of::<crate::buffer::BufferHeader>() + 8 + 7) & !7);
        if buf as usize == expected_next {
            adjacent = Some((fake_prev, buf));
            break;
        }
    }
    let (fake_prev, buf) = adjacent.expect("expected adjacent small-buffer slab allocations");

    unsafe {
        *crate::buffer::buffer_data_mut(fake_prev) = crate::gc::GC_TYPE_STRING;
        (*buf).length = 4;
        std::ptr::copy_nonoverlapping(
            [1u8, 2, 3, 4].as_ptr(),
            crate::buffer::buffer_data_mut(buf),
            4,
        );
    }

    let cloned = js_array_clone(buf as *const ArrayHeader);
    assert_numeric_raw_values(cloned, &[1.0, 2.0, 3.0, 4.0]);
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
fn test_numeric_array_layout_metadata_preserves_and_downgrades_on_writes() {
    let mut arr = js_array_alloc(4);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    arr = js_array_push_f64(arr, 1.25);
    arr = js_array_push_f64(arr, f64::NAN);
    arr = js_array_push_f64(arr, -0.0);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 3),
        Some(0)
    );

    let str_ptr = crate::string::js_string_from_bytes(b"not-number".as_ptr(), 10);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    arr = js_array_push_f64(arr, str_value);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 4),
        Some(1)
    );

    js_array_set_f64(arr, 0, 99.0);
    assert_eq!(
        js_array_is_numeric_f64_layout(arr),
        0,
        "numeric writes do not silently re-specialize a downgraded mixed array"
    );
}

#[test]
fn test_numeric_array_layout_mark_rejects_holes_and_accepts_dense_numbers() {
    let arr = js_array_alloc_with_length(2);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        js_array_mark_numeric_f64_layout(arr),
        0,
        "hole-filled arrays cannot be treated as dense numeric payloads"
    );

    js_array_set_f64(arr, 0, 3.5);
    js_array_set_f64(arr, 1, -0.0);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 2),
        Some(0)
    );

    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    js_array_set_f64(arr, 1, undefined);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 2),
        Some(0),
        "undefined downgrades numeric metadata but remains pointer-free for GC"
    );
}

#[test]
fn test_numeric_array_mark_canonicalizes_int32_and_nan_inline() {
    let arr = js_array_alloc_with_length(3);
    let int32_value = f64::from_bits(crate::value::INT32_TAG | ((-17i32 as u32) as u64));
    let payload_nan = f64::from_bits(0x7FF8_0000_0000_1234);

    js_array_set_f64(arr, 0, int32_value);
    js_array_set_f64(arr, 1, payload_nan);
    js_array_set_f64(arr, 2, -0.0);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 0), -17.0);
    assert!(js_array_numeric_get_f64_unboxed(arr, 1).is_nan());
    assert_eq!(
        js_array_numeric_get_f64_unboxed(arr, 2).to_bits(),
        (-0.0f64).to_bits()
    );
    unsafe {
        assert_eq!(raw_slot_bits(arr, 0), (-17.0f64).to_bits());
        assert_eq!(raw_slot_bits(arr, 1), f64::NAN.to_bits());
        assert_eq!(raw_slot_bits(arr, 2), (-0.0f64).to_bits());
    }
}

#[test]
fn test_numeric_array_raw_f64_payload_tracks_sets_and_downgrades() {
    let mut arr = js_array_alloc(2);
    arr = js_array_push_f64(arr, 1.5);
    arr = js_array_push_f64(arr, 2.5);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 0), 1.5);
    assert_eq!(js_array_numeric_set_f64_unboxed(arr, 1, 7.25), 1);
    assert_eq!(js_array_get_f64(arr, 1), 7.25);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 1), 7.25);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let str_ptr = crate::string::js_string_from_bytes(b"boxed".as_ptr(), 5);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    js_array_set_f64(arr, 1, str_value);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        js_array_numeric_get_f64_unboxed(arr, 1).to_bits(),
        str_value.to_bits(),
        "unboxed helper falls back to boxed slots after downgrade"
    );
}

#[test]
fn test_numeric_array_sparse_extend_fills_holes_and_downgrades_raw_layout() {
    let mut arr = js_array_alloc(8);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let extended = js_array_set_f64_extend(arr, 5, 6.0);

    assert_eq!(extended, arr);
    assert_eq!(js_array_length(extended), 6);
    assert_eq!(js_array_is_numeric_f64_layout(extended), 0);
    assert_eq!(js_array_get_f64(extended, 0), 1.0);
    assert_eq!(js_array_get_f64(extended, 1), 2.0);
    assert_eq!(
        js_array_get_f64(extended, 3).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    unsafe {
        assert_eq!(raw_slot_bits(extended, 3), crate::value::TAG_HOLE);
    }
    assert_eq!(js_array_get_f64(extended, 5), 6.0);
}

#[test]
fn test_numeric_array_raw_f64_payload_push_helper_preserves_and_downgrades() {
    let mut arr = js_array_alloc(2);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    arr = js_array_numeric_push_f64_unboxed(arr, 1.0);
    arr = js_array_numeric_push_f64_unboxed(arr, 2.0);

    assert_eq!(js_array_length(arr), 2);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 0), 1.0);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 1), 2.0);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let grown = js_array_numeric_push_f64_unboxed(arr, 3.0);
    assert_eq!(js_array_length(grown), 3);
    assert_eq!(js_array_numeric_get_f64_unboxed(grown, 2), 3.0);
    assert_eq!(js_array_is_numeric_f64_layout(grown), 1);

    let str_ptr = crate::string::js_string_from_bytes(b"push-boxed".as_ptr(), 10);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    let mixed = js_array_numeric_push_f64_unboxed(grown, str_value);

    assert_eq!(js_array_get_f64(mixed, 3).to_bits(), str_value.to_bits());
    assert_eq!(js_array_is_numeric_f64_layout(mixed), 0);
}

#[test]
fn test_array_push_jsvalue_int32_canonicalizes_raw_f64_slot() {
    let int_bits = int32_jsvalue_bits(42);
    let arr = js_array_push_jsvalue(js_array_alloc(1), int_bits);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);
    assert_ne!(js_array_get_f64_unchecked(arr, 0).to_bits(), int_bits);
    assert_canonical_raw_slot(arr, 0, 42.0);
}

#[test]
fn test_array_set_jsvalue_extend_int32_canonicalizes_dense_append() {
    let mut arr = js_array_alloc(2);
    arr = js_array_push_f64(arr, 1.0);

    let int_bits = int32_jsvalue_bits(-7);
    arr = js_array_set_jsvalue_extend(arr, 1, int_bits);

    assert_numeric_raw_values(arr, &[1.0, -7.0]);
    assert_ne!(js_array_get_f64_unchecked(arr, 1).to_bits(), int_bits);
}

#[test]
fn test_numeric_raw_f64_helpers_canonicalize_int32_shaped_values() {
    let mut arr = js_array_alloc(2);
    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);

    let push_bits = int32_jsvalue_bits(9);
    arr = js_array_numeric_push_f64_unboxed(arr, f64::from_bits(push_bits));
    assert_eq!(js_array_length(arr), 1);
    assert_ne!(js_array_get_f64_unchecked(arr, 0).to_bits(), push_bits);
    assert_canonical_raw_slot(arr, 0, 9.0);

    let set_bits = int32_jsvalue_bits(-11);
    assert_eq!(
        js_array_numeric_set_f64_unboxed(arr, 0, f64::from_bits(set_bits)),
        1
    );
    assert_ne!(js_array_get_f64_unchecked(arr, 0).to_bits(), set_bits);
    assert_canonical_raw_slot(arr, 0, -11.0);
}

#[test]
fn test_array_from_jsvalue_int32_rebuild_canonicalizes_raw_slots() {
    let elements = [int32_jsvalue_bits(3), int32_jsvalue_bits(-4)];
    let arr = js_array_from_jsvalue(elements.as_ptr(), elements.len() as u32);

    assert_numeric_raw_values(arr, &[3.0, -4.0]);
    assert_ne!(js_array_get_f64_unchecked(arr, 0).to_bits(), elements[0]);
    assert_ne!(js_array_get_f64_unchecked(arr, 1).to_bits(), elements[1]);
}

#[test]
fn test_nonnumeric_append_downgrades_raw_f64_and_preserves_payload() {
    let bool_bits = crate::value::JSValue::bool(true).bits();
    let arr = js_array_push_jsvalue(js_array_alloc(1), bool_bits);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(js_array_get_jsvalue(arr, 0), bool_bits);
    assert_eq!(js_array_get_f64_unchecked(arr, 0).to_bits(), bool_bits);
}

#[test]
fn test_numeric_array_layout_transfers_across_growth_forwarding() {
    let mut arr = js_array_alloc(0);
    let original = arr;
    let capacity = unsafe { (*arr).capacity };

    for i in 0..capacity {
        arr = js_array_push_f64(arr, i as f64);
    }

    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let grown = js_array_push_f64(arr, capacity as f64);

    assert_ne!(grown, arr);
    assert_eq!(js_array_is_numeric_f64_layout(grown), 1);
    assert_eq!(
        js_array_is_numeric_f64_layout(original),
        1,
        "stale pointers should follow growth forwarding before checking metadata"
    );
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(grown as usize, (capacity + 1) as usize),
        Some(0)
    );
}

#[test]
fn test_numeric_array_raw_f64_payload_rebuilds_after_growth_forwarding() {
    let mut arr = js_array_alloc(0);
    let original = arr;
    let capacity = unsafe { (*arr).capacity };

    for i in 0..capacity {
        arr = js_array_push_f64(arr, i as f64);
    }

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(
        js_array_numeric_get_f64_unboxed(arr, capacity - 1),
        (capacity - 1) as f64
    );

    let grown = js_array_push_f64(arr, capacity as f64);

    assert_ne!(grown, arr);
    assert_eq!(
        js_array_numeric_get_f64_unboxed(grown, capacity),
        capacity as f64
    );
    assert_eq!(
        js_array_numeric_get_f64_unboxed(original, capacity),
        capacity as f64,
        "stale forwarded handles rebuild the moved raw payload before reading"
    );
}

#[test]
fn test_numeric_array_layout_query_recovers_dense_numeric_metadata() {
    let mut arr = js_array_alloc(0);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);

    js_array_clear_numeric_layout(arr);

    assert_eq!(
        js_array_is_numeric_f64_layout(arr),
        1,
        "numeric layout metadata can be rebuilt from dense numeric slots"
    );
}

#[test]
fn test_array_get_f64_large_dense_array_preserves_values() {
    let arr = js_array_alloc_with_length(100_001);
    js_array_set_f64(arr, 100_000, 42.0);

    assert_eq!(js_array_get_f64(arr, 100_000), 42.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 100_000), 42.0);
}

#[test]
fn test_numeric_array_layout_bulk_rebuild_preserves_and_downgrades() {
    let values = [1.0, 2.0, 3.0, 4.0];
    let src = js_array_from_f64(values.as_ptr(), values.len() as u32);

    assert_eq!(js_array_is_numeric_f64_layout(src), 1);

    let sliced = js_array_slice(src, 1, 3);
    assert_numeric_raw_values(sliced, &[2.0, 3.0]);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(sliced as usize, 2),
        Some(0)
    );

    let concatenated = js_array_concat(js_array_alloc(0), src);
    assert_numeric_raw_values(concatenated, &values);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(concatenated as usize, values.len()),
        Some(0)
    );

    let str_ptr = crate::string::js_string_from_bytes(b"bulk-mixed".as_ptr(), 10);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    js_array_fill(concatenated, str_value);

    assert_eq!(js_array_is_numeric_f64_layout(concatenated), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(concatenated as usize, values.len()),
        Some(values.len())
    );
}

#[test]
fn test_array_slice_value_index_coercion() {
    let values = [1.0, 2.0, 3.0, 4.0];
    let src = js_array_from_f64(values.as_ptr(), values.len() as u32);
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

    assert_numeric_raw_values(js_array_slice_values(src, f64::NAN, undefined), &values);
    assert_numeric_raw_values(js_array_slice_values(src, f64::INFINITY, undefined), &[]);
    assert_numeric_raw_values(
        js_array_slice_values(src, f64::NEG_INFINITY, undefined),
        &values,
    );
    assert_numeric_raw_values(
        js_array_slice_values(src, 1.0, f64::INFINITY),
        &[2.0, 3.0, 4.0],
    );
    assert_numeric_raw_values(js_array_slice_values(src, 1.0, f64::NAN), &[]);
    assert_numeric_raw_values(js_array_slice_values(src, 1.9, 3.8), &[2.0, 3.0]);
    assert_numeric_raw_values(js_array_slice_values(src, 1.0, undefined), &[2.0, 3.0, 4.0]);

    let str_ptr = crate::string::js_string_from_bytes(b"2".as_ptr(), 1);
    let string_two = crate::value::js_nanbox_string(str_ptr as i64);
    assert_numeric_raw_values(
        js_array_slice_values(src, string_two, undefined),
        &[3.0, 4.0],
    );
}

#[test]
fn test_numeric_array_layout_length_and_delete_transitions() {
    let mut arr = js_array_alloc(4);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);
    arr = js_array_push_f64(arr, 3.0);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    js_array_set_length(arr, 2.0);

    assert_eq!(js_array_length(arr), 2);
    assert_eq!(
        js_array_is_numeric_f64_layout(arr),
        1,
        "truncation should preserve dense numeric layout for the reachable prefix"
    );

    js_array_set_length(arr, 4.0);

    assert_eq!(js_array_length(arr), 4);
    assert_eq!(
        js_array_is_numeric_f64_layout(arr),
        0,
        "extension pads with undefined and must downgrade numeric layout"
    );
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 4),
        Some(0)
    );

    let mut dense = js_array_alloc(4);
    dense = js_array_push_f64(dense, 10.0);
    dense = js_array_push_f64(dense, 20.0);
    assert_eq!(js_array_is_numeric_f64_layout(dense), 1);

    assert_eq!(js_array_delete(dense, 0), 1);
    assert_eq!(
        js_array_is_numeric_f64_layout(dense),
        0,
        "delete creates an undefined slot and downgrades dense numeric layout"
    );
}

#[test]
fn test_numeric_array_layout_immutable_helpers_preserve_or_downgrade() {
    let values = [10.0, 2.0, 30.0];
    let src = js_array_from_f64(values.as_ptr(), values.len() as u32);
    assert_numeric_raw_values(src, &values);

    let reversed = js_array_to_reversed(src);
    assert_numeric_raw_values(reversed, &[30.0, 2.0, 10.0]);

    let sorted = js_array_to_sorted_default(src);
    assert_numeric_raw_values(sorted, &[10.0, 2.0, 30.0]);

    let numeric_replaced = js_array_with(src, 1.0, 99.0);
    assert_numeric_raw_values(numeric_replaced, &[10.0, 99.0, 30.0]);

    let insert = [7.0, 8.0];
    let spliced = js_array_to_spliced(src, 1.0, 1.0, insert.as_ptr(), insert.len() as u32);
    assert_numeric_raw_values(spliced, &[10.0, 7.0, 8.0, 30.0]);

    let str_ptr = crate::string::js_string_from_bytes(b"immutable-mixed".as_ptr(), 15);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    let mixed = js_array_with(src, 1.0, str_value);

    assert_eq!(js_array_is_numeric_f64_layout(mixed), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(mixed as usize, values.len()),
        Some(1)
    );
}

#[test]
fn test_numeric_array_layout_map_fast_path_downgrades_mapped_pointers() {
    let mut arr = js_array_alloc(4);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);
    arr = js_array_push_f64(arr, 3.0);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let callback = crate::closure::js_closure_alloc(test_map_to_string as *const u8, 0);
    let mapped = js_array_map(arr, callback);

    assert_eq!(js_array_length(mapped), 3);
    assert_eq!(
        js_array_is_numeric_f64_layout(mapped),
        0,
        "small map() results use a layout-only fast path and must still downgrade"
    );
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(mapped as usize, 3),
        Some(3)
    );
}

#[test]
fn test_numeric_array_layout_entries_outer_downgrades_inner_pairs_preserve() {
    let values = [4.0, 5.0];
    let src = js_array_from_f64(values.as_ptr(), values.len() as u32);
    let entries = js_array_entries(src);

    assert_eq!(
        js_array_is_numeric_f64_layout(entries),
        0,
        "entries() outer array stores pair pointers, not raw numeric slots"
    );
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(entries as usize, values.len()),
        Some(values.len())
    );

    let pair_box = js_array_get_f64(entries, 0);
    let pair = (pair_box.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader;
    assert_eq!(js_array_is_numeric_f64_layout(pair), 1);
    assert_eq!(js_array_numeric_get_f64_unboxed(pair, 0), 0.0);
    assert_eq!(js_array_numeric_get_f64_unboxed(pair, 1), 4.0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(pair as usize, 2),
        Some(0)
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
fn test_array_last_index_of() {
    let arr = js_array_alloc(8);
    for v in [1.0, 2.0, 3.0, 2.0, 1.0] {
        js_array_push_f64(arr, v);
    }
    // No fromIndex (has_from == 0) → search from the last element.
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 0.0, 0), 3);
    assert_eq!(js_array_last_index_of_jsvalue(arr, 1.0, 0.0, 0), 4);
    assert_eq!(js_array_last_index_of_jsvalue(arr, 9.0, 0.0, 0), -1);
    // Explicit fromIndex (has_from == 1), including the spec's clamping.
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 2.0, 1), 1);
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, -2.0, 1), 3); // 5 + (-2) = 3
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, -10.0, 1), -1); // < -length
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 100.0, 1), 3); // clamp to len-1
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 0.0, 1), -1); // only index 0
                                                                      // Empty array.
    let empty = js_array_alloc(1);
    assert_eq!(js_array_last_index_of_jsvalue(empty, 1.0, 0.0, 0), -1);
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

#[test]
fn join_routes_objects_and_nested_arrays_through_tostring() {
    // #800/#2135: a POINTER_TAG element that is an object/array (not a string)
    // must go through the spec ToString — a nested array joins recursively, a
    // plain object becomes "[object Object]" — instead of being mis-read as a
    // StringHeader (which produced corrupted/empty output).
    unsafe {
        let inner = js_array_push_f64(js_array_push_f64(js_array_alloc(2), 1.0), 2.0);
        let inner_v = f64::from_bits(crate::value::JSValue::pointer(inner as *const u8).bits());
        let obj = crate::object::js_object_alloc(0, 0);
        let obj_v = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
        let mut arr = js_array_alloc(2);
        arr = js_array_push_f64(arr, inner_v);
        arr = js_array_push_f64(arr, obj_v);
        let sep = crate::string::js_string_from_bytes(b";".as_ptr(), 1);
        let out = js_array_join(arr, sep);
        let len = (*out).byte_len as usize;
        let data = (out as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let s = std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap();
        assert_eq!(s, "1,2;[object Object]");
    }
}
