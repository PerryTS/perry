use super::header::array_numeric_layout;
use super::header_gc_slots::{
    test_dense_move_layout_classified_slots, test_reset_dense_move_layout_classified_slots,
};
use super::*;

#[test]
fn repeated_dense_unshift_classifies_only_the_inserted_slots() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    const N: usize = 512;
    let arr = js_array_alloc(N as u32);

    test_reset_dense_move_layout_classified_slots();
    for value in 0..N {
        assert_eq!(js_array_unshift_f64(arr, value as f64), arr);
    }

    assert_eq!(test_dense_move_layout_classified_slots(), N);
    assert_eq!(js_array_length(arr), N as u32);
    for index in 0..N {
        assert_eq!(js_array_get_f64(arr, index as u32), (N - index - 1) as f64);
    }
}

#[test]
fn repeated_dense_splice_layout_work_is_linear_in_operations() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    const N: usize = 256;

    let inserted = js_array_alloc(N as u32);
    test_reset_dense_move_layout_classified_slots();
    for value in 0..N {
        let item = [value as f64];
        let mut out = inserted;
        let deleted = js_array_splice(inserted, (value / 2) as i32, 0, item.as_ptr(), 1, &mut out);
        assert_eq!(out, inserted);
        assert_eq!(js_array_length(deleted), 0);
    }
    assert_eq!(test_dense_move_layout_classified_slots(), N);

    let removed = js_array_alloc(N as u32);
    for value in 0..N {
        assert_eq!(js_array_push_f64(removed, value as f64), removed);
    }
    test_reset_dense_move_layout_classified_slots();
    for _ in 0..N {
        let mut out = removed;
        let deleted = js_array_splice(
            removed,
            (js_array_length(removed) / 2) as i32,
            1,
            std::ptr::null(),
            0,
            &mut out,
        );
        assert_eq!(out, removed);
        assert_eq!(js_array_length(deleted), 1);
    }
    assert_eq!(test_dense_move_layout_classified_slots(), N);
    assert_eq!(js_array_length(removed), 0);
}

#[test]
fn dense_moves_preserve_the_existing_numeric_layout_without_a_rebuild() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = js_array_alloc(16);
    for value in [1.0, 2.0, 3.0, 4.0] {
        assert_eq!(js_array_push_f64(arr, value), arr);
    }
    assert_eq!(
        unsafe { array_numeric_layout(arr) },
        Some(NumericArrayLayout::RawF64)
    );

    let arr = js_array_unshift_f64(arr, f64::from_bits(crate::value::JSValue::int32(0).bits()));
    assert_eq!(
        unsafe { array_numeric_layout(arr) },
        Some(NumericArrayLayout::RawF64)
    );
    assert_eq!(js_array_get_f64(arr, 0), 0.0);

    let item = [f64::from_bits(crate::value::JSValue::int32(9).bits())];
    let mut out = arr;
    js_array_splice(arr, 2, 1, item.as_ptr(), 1, &mut out);
    assert_eq!(
        unsafe { array_numeric_layout(out) },
        Some(NumericArrayLayout::RawF64)
    );
    assert_eq!(js_array_get_f64(out, 2), 9.0);
}
