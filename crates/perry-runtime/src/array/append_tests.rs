//! The append grow entry shares push's GC-safe capacity replacement.
use super::*;

#[test]
fn append_growth_keeps_aliases_and_mixed_slots() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let original = js_array_alloc(4);
        let mut live = original;
        for i in 0..4 {
            live = js_array_push_f64(live, i as f64);
        }
        let value = crate::object::js_object_alloc(0, 0);
        let boxed = crate::value::js_nanbox_pointer(value as i64);
        let mut home = crate::value::js_nanbox_pointer(live as i64);
        home = js_array_append_set(home, 4.0, boxed, 1, home);
        live = (home.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader;
        assert_ne!(live, original, "the fixture must exercise growth");
        assert_eq!(clean_arr_ptr_mut(original), live, "aliases follow growth");
        assert_eq!((*live).length, 5);
        for i in 0..4 {
            assert_eq!(js_array_get_f64(original, i), i as f64);
        }
        assert_eq!(js_array_get_f64(original, 4).to_bits(), boxed.to_bits());
        assert_eq!(
            array_object_flags_resolved(live) & crate::gc::GC_ARRAY_RAW_F64_LAYOUT,
            0
        );
        // A shape-free mixed layout still traces the newly appended child.
        let mut visited = Vec::new();
        let exact =
            crate::gc::layout_visit_pointer_slots_for_user(live as usize, 5, |i| visited.push(i));
        // UNKNOWN uses the collector's tag-scan fallback rather than a mask.
        if exact {
            assert!(visited.contains(&4));
        } else {
            assert_eq!(
                array_object_flags_resolved(live) & crate::gc::GC_LAYOUT_STATE_MASK,
                0
            );
        }
    }
}

#[test]
fn append_growth_demotes_stored_heap_string() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let mut arr = js_array_alloc(4);
        for i in 0..4 {
            arr = js_array_push_f64(arr, i as f64);
        }
        let text = crate::string::js_string_from_bytes(b"a long append value".as_ptr(), 19);
        (*text).refcount = 1;
        let boxed = crate::value::js_nanbox_string(text as i64);
        let mut home = crate::value::js_nanbox_pointer(arr as i64);
        home = js_array_append_set(home, 4.0, boxed, 1, home);
        arr = (home.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader;
        assert_eq!(js_array_get_f64(arr, 4).to_bits(), boxed.to_bits());
        assert_eq!((*arr).length, 5);
        assert_eq!(
            (*text).refcount,
            0,
            "the alias must demote unique ownership"
        );
    }
}

#[test]
fn append_shared_continuation_updates_layout_before_publishing_length() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let arr = js_array_alloc(4);
        let object = crate::object::js_object_alloc(0, 0);
        let pointer = crate::value::js_nanbox_pointer(object as i64);
        let mut home = crate::value::js_nanbox_pointer(arr as i64);
        home = js_array_append_set(home, 0.0, pointer, 1, home);
        assert_eq!((*arr).length, 1);
        assert_ne!(
            array_object_flags_resolved(arr) & crate::gc::GC_LAYOUT_ALL_POINTERS,
            0
        );
        home = js_array_append_set(home, 1.0, 2.5, 1, home);
        assert_eq!((*arr).length, 2);
        assert_eq!(
            array_object_flags_resolved(arr) & crate::gc::GC_LAYOUT_ALL_POINTERS,
            0
        );
        assert_eq!(js_array_get_f64(arr, 0).to_bits(), pointer.to_bits());
        assert_eq!(js_array_get_f64(arr, 1), 2.5);
        home = js_array_append_set(home, 2.0, pointer, 1, home);
        assert_eq!((*arr).length, 3);
        assert_eq!(js_array_get_f64(arr, 2).to_bits(), pointer.to_bits());
    }
}

#[test]
fn append_heals_forwarding_without_replacing_a_rebound_home() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let arr = js_array_alloc(4);
        for i in 0..4 {
            js_array_push_f64(arr, i as f64);
        }
        let original = crate::value::js_nanbox_pointer(arr as i64);
        let replacement = crate::value::js_nanbox_pointer(js_array_alloc(4) as i64);
        let mut home = replacement;
        home = js_array_append_set(original, 4.0, 4.0, 1, home);
        assert_eq!(home.to_bits(), replacement.to_bits());
        let grown = clean_arr_ptr_mut(arr);
        assert_ne!(grown, arr, "fixture must exercise forwarding");
        assert_eq!((*grown).length, 5);
        home = original;
        home = js_array_append_set(original, 5.0, 5.0, 1, home);
        assert_eq!(
            home.to_bits(),
            crate::value::js_nanbox_pointer(grown as i64).to_bits()
        );
        assert_eq!((*grown).length, 6);
        assert_eq!(js_array_get_f64(arr, 5), 5.0);
    }
}

/// `js_array_fill_values` leaves a fresh array exactly as the same values
/// pushed one by one would: elements, length and the header's layout flags.
#[test]
fn fill_values_matches_one_push_per_value() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let object = crate::object::js_object_alloc(0, 0);
        let boxed = crate::value::js_nanbox_pointer(object as i64);
        for values in [
            vec![1.0, 2.5, -3.0],
            vec![1.0, boxed, 2.5, f64::from_bits(crate::value::TAG_UNDEFINED)],
            vec![boxed],
        ] {
            let n = values.len() as u32;
            let mut pushed = js_array_alloc(n);
            for v in &values {
                pushed = js_array_push_f64(pushed, *v);
            }
            let fresh = js_array_alloc(n);
            let filled = super::push_pop::js_array_fill_values(fresh, values.as_ptr(), n);
            assert_eq!(filled, fresh, "a fill never reallocates");
            assert_eq!((*filled).length, (*pushed).length);
            for i in 0..n {
                assert_eq!(
                    js_array_get_f64(filled, i).to_bits(),
                    js_array_get_f64(pushed, i).to_bits()
                );
            }
            assert_eq!(
                array_object_flags_resolved(filled),
                array_object_flags_resolved(pushed)
            );
        }
    }
}
