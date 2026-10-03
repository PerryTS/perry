//! Small arrays must keep distinct identities and survive earlier growth/GC.
use super::*;

#[test]
fn small_arrays_reserve_four_slots_without_sharing_mutable_identity() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let empty = js_array_alloc(0);
    let other = js_array_alloc(0);
    assert_ne!(empty, other);
    unsafe {
        assert_eq!((*empty).capacity, 4);
        assert_eq!(array_physical_capacity(empty), 4);
        for index in 0..4 {
            assert_eq!(
                *array_elements_ptr(empty).add(index),
                crate::value::TAG_HOLE
            );
        }
        for requested in [1, 3, 4, 5, 16, 33] {
            let arr = js_array_alloc(requested);
            let pointers = js_array_alloc_pointer_elements(requested);
            assert_eq!((*pointers).capacity, requested.max(4));
            assert_eq!((*arr).capacity, requested.max(4));
            let holey = js_array_alloc_with_length(requested);
            assert_eq!((*holey).length, requested);
            assert_eq!((*holey).capacity, requested.max(4));
            for index in 0..requested {
                assert_eq!(
                    *array_elements_ptr(holey).add(index as usize),
                    crate::value::TAG_HOLE
                );
            }
        }
    }
    for i in 0..4 {
        assert_eq!(js_array_push_f64(empty, i as f64), empty);
    }
    assert_eq!(js_array_length(other), 0);
    let grown = js_array_push_f64(empty, 4.0);
    assert_ne!(grown, empty);
    assert_eq!(js_array_length(empty), 5);
    for i in 0..5 {
        assert_eq!(js_array_get_f64(empty, i), i as f64);
    }
}

#[test]
fn small_old_array_growth_keeps_young_elements_through_copying_minors() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _verify = crate::gc::knob_overrides::VerifyEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let initial = crate::arena::arena_alloc_gc_old_born_tenured(
        array_byte_size(4),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;
    unsafe {
        (*initial).length = 0;
        (*initial).capacity = 4;
        for i in 0..4 {
            // GC_STORE_AUDIT(INIT): unpublished old array starts with holes.
            array_elements_ptr(initial)
                .add(i)
                .write(crate::value::TAG_HOLE);
        }
        set_array_numeric_layout(initial, NumericArrayLayout::RawF64);
        crate::gc::layout_init_pointer_free(initial.cast());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let root = scope.root_raw_mut_ptr(initial);
    for index in 0..20 {
        let text = format!("young-{index}");
        let child = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let head = root.with_mut_ptr::<ArrayHeader, _>(|head| {
            js_array_push_f64(head, crate::value::js_nanbox_string(child as i64))
        });
        // The handle roots the current head; the initial alias must still forward.
        let current = scope.root_raw_mut_ptr(head);
        let before = crate::gc::copying_minor_cycles();
        let (_, head) = current.across_mut::<ArrayHeader, _>(crate::gc::gc_collect_minor);
        assert!(crate::gc::copying_minor_cycles() > before);
        assert_eq!(clean_arr_ptr_mut(initial), head);
        assert_ne!(
            crate::value::js_get_string_pointer_unified(js_array_get_f64(head, index)),
            child as i64,
            "the newly appended young child must actually move"
        );
        for i in 0..=index {
            let value = js_array_get_f64(head, i);
            let string = crate::value::js_get_string_pointer_unified(value)
                as *const crate::string::StringHeader;
            assert_eq!(
                unsafe {
                    std::slice::from_raw_parts(
                        crate::string::string_data(string),
                        (*string).byte_len as usize,
                    )
                },
                format!("young-{i}").as_bytes()
            );
        }
    }
}
