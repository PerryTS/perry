use super::*;

#[test]
fn shift_queue_drain_keeps_survivors_at_their_original_addresses() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = js_array_alloc(10_000);
    for i in 0..10_000 {
        assert_eq!(js_array_push_f64(arr, i as f64), arr);
    }
    let original = unsafe { array_elements_ptr(arr) };
    for i in 0..10_000 {
        assert_eq!(js_array_shift_f64(arr), i as f64);
        unsafe {
            assert_eq!(*original.add(i), crate::value::TAG_HOLE);
            if i < 9_999 {
                assert_eq!(array_elements_ptr(arr), original.add(i + 1));
                assert_eq!(js_array_get_f64(arr, 0), (i + 1) as f64);
            }
        }
    }
    assert_eq!(js_array_length(arr), 0);
    assert_eq!(
        js_array_shift_f64(arr).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    unsafe {
        assert_eq!(array_elements_ptr(arr), original);
        assert_eq!((*arr).capacity, 10_000);
    }
    assert_eq!(js_array_push_f64(arr, 42.0), arr);
    assert_eq!(js_array_pop_f64(arr), 42.0);
}

#[test]
fn shift_queue_aliases_growth_holes_and_refill() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let alias = js_array_alloc(16);
    let mut arr = alias;
    for cycle in 0..5 {
        for i in 0..16 {
            arr = js_array_push_f64(arr, (cycle * 16 + i) as f64);
        }
        for i in 0..8 {
            assert_eq!(js_array_shift_f64(alias), (cycle * 16 + i) as f64);
        }
        for i in 16..80 {
            arr = js_array_push_f64(arr, (cycle * 16 + i) as f64);
        }
        assert_eq!(clean_arr_ptr_mut(alias), arr);
        for i in 8..80 {
            assert_eq!(js_array_get_f64(alias, i - 8), (cycle * 16 + i) as f64);
        }
        while js_array_length(alias) > 0 {
            js_array_shift_f64(alias);
        }
    }
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);
    arr = js_array_push_f64(arr, 3.0);
    assert_eq!(js_array_delete(arr, 1), 1);
    assert_eq!(js_array_shift_f64(arr), 1.0);
    assert_eq!(
        js_array_shift_f64(arr).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(js_array_shift_f64(arr), 3.0);
}

#[test]
fn shift_queue_shared_mutators_use_the_logical_start() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let mut arr = js_array_alloc(16);
    for i in 0..6 {
        arr = js_array_push_f64(arr, i as f64);
    }
    assert_eq!(js_array_shift_f64(arr), 0.0);
    assert_eq!(js_array_shift_f64(arr), 1.0);
    arr = js_array_unshift_f64(arr, 9.0);
    assert_eq!(js_array_shift_f64(arr), 9.0);
    assert_eq!(js_array_pop_f64(arr), 5.0);
    assert_eq!(js_array_get_f64(arr, 0), 2.0);
    js_array_set_length(arr, 1.0);
    assert_eq!(js_array_get_f64(arr, 0), 2.0);
    js_array_set_length(arr, 3.0);
    assert_eq!(
        js_array_get_f64(arr, 1).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(
        js_array_get_f64(arr, 2).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}
