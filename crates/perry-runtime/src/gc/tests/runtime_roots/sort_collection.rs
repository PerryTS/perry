use super::*;
use crate::closure::ClosureHeader;

extern "C" fn captured_direction(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    a: f64,
    b: f64,
) -> f64 {
    // Model generated capture loads: no forwarding lookup can repair a stale
    // closure argument. The getter changes only the relocated capture.
    let direction = unsafe {
        *((closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64)
    };
    direction * (a - b)
}

extern "C" fn collecting_getter(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let getter = scope.root_raw_const_ptr(closure);
    gc_collect_minor();
    getter.with_const_ptr(|current| {
        let comparator = crate::closure::js_closure_get_capture_f64(current, 0);
        crate::closure::js_closure_set_capture_f64(
            (comparator.to_bits() & crate::value::POINTER_MASK) as *mut ClosureHeader,
            0,
            1.0,
        );
    });
    3.0
}

extern "C" fn accept_sorted_value(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _value: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn collecting_index_setter(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _value: f64,
) -> f64 {
    gc_collect_minor();
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn indexed_accessor_set_returns_the_relocated_receiver() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();

    for strict in [false, true] {
        let scope = RuntimeHandleScope::new();
        let receiver = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(3));
        let setter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(collecting_index_setter, 1),
            0,
        ));
        let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let set_key = crate::js_string_from_bytes(b"set".as_ptr(), 3);
        descriptor.with_mut_ptr(|desc| {
            setter.with_const_ptr(|function: *const ClosureHeader| {
                crate::object::js_object_set_field_by_name(
                    desc,
                    set_key,
                    f64::from_bits(ptr_bits(function as usize)),
                );
            });
        });
        let index_key = crate::js_string_from_bytes(b"0".as_ptr(), 1);
        descriptor.with_const_ptr(|desc: *const u8| {
            receiver.with_const_ptr(|arr: *const crate::array::ArrayHeader| {
                crate::object::js_object_define_property(
                    f64::from_bits(ptr_bits(arr as usize)),
                    f64::from_bits(string_bits(index_key as usize)),
                    f64::from_bits(ptr_bits(desc as usize)),
                );
            });
        });
        let before = receiver.with_const_ptr(|arr: *const crate::array::ArrayHeader| arr as usize);
        assert!(matches!(
            crate::arena::classify_heap_generation(before),
            crate::arena::HeapGeneration::Nursery
        ));
        let (returned, current) = receiver.across_mut::<crate::array::ArrayHeader, _>(|| {
            receiver.with_mut_ptr(|arr| {
                if strict {
                    crate::array::js_array_set_f64_extend_strict(arr, 0, 42.0)
                } else {
                    crate::array::js_array_set_f64_extend(arr, 0, 42.0)
                }
            })
        });
        assert_ne!(
            current as usize, before,
            "setter must actually relocate the receiver"
        );
        // Compare addresses only: even a bad return must never be dereferenced.
        assert_eq!(
            returned, current,
            "strict={strict}: return must follow the root"
        );
    }
}

#[test]
fn sort_collection_getters_relocate_comparator_and_receiver() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();

    for real_array in [true, false] {
        let scope = RuntimeHandleScope::new();
        let comparator = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(captured_direction, 2),
            1,
        ));
        comparator.with_mut_ptr(|ptr| crate::closure::js_closure_set_capture_f64(ptr, 0, -1.0));
        let recv = if real_array {
            crate::array::js_array_alloc_with_length(3) as *mut u8
        } else {
            crate::object::js_object_alloc(0, 0) as *mut u8
        };
        let receiver = scope.root_nanbox_u64(ptr_bits(recv as usize));
        for (index, value) in [3.0, 1.0, 2.0].into_iter().enumerate() {
            crate::object::js_object_set_index_polymorphic(
                receiver.get_nanbox_u64() as i64,
                index as f64,
                value,
            );
        }
        let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(collecting_getter, 0),
            1,
        ));
        getter.with_mut_ptr(|getter| {
            comparator.with_const_ptr(|cmp: *const ClosureHeader| {
                crate::closure::js_closure_set_capture_f64(
                    getter,
                    0,
                    f64::from_bits(ptr_bits(cmp as usize)),
                );
            });
        });
        let setter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(accept_sorted_value, 1),
            0,
        ));
        let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for (name, function) in [(b"get", &getter), (b"set", &setter)] {
            let key = crate::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            descriptor.with_mut_ptr(|desc| {
                function.with_const_ptr(|func: *const ClosureHeader| {
                    crate::object::js_object_set_field_by_name(
                        desc,
                        key,
                        f64::from_bits(ptr_bits(func as usize)),
                    );
                });
            });
        }
        let name: &[u8] = if real_array { b"0" } else { b"length" };
        let key = crate::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        descriptor.with_const_ptr(|desc: *const u8| {
            crate::object::js_object_define_property(
                receiver.get_nanbox_f64(),
                f64::from_bits(string_bits(key as usize)),
                f64::from_bits(ptr_bits(desc as usize)),
            );
        });
        let before_cmp = comparator.with_const_ptr(|ptr: *const ClosureHeader| ptr as usize);
        let before_recv = receiver.get_nanbox_u64();
        let returned = comparator.with_const_ptr(|cmp| {
            crate::array::js_array_sort_with_comparator(
                (receiver.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut _,
                cmp,
            )
        });
        assert_ne!(
            comparator.with_const_ptr(|p: *const ClosureHeader| p as usize),
            before_cmp
        );
        assert_ne!(receiver.get_nanbox_u64(), before_recv);
        assert_eq!(ptr_bits(returned as usize), receiver.get_nanbox_u64());
        for (index, expected) in [(1, 2.0), (2, 3.0)] {
            let actual = crate::object::js_object_get_index_polymorphic(
                receiver.get_nanbox_u64() as i64,
                index as f64,
            );
            assert_eq!(
                actual, expected,
                "receiver kind: real_array={real_array}, index={index}"
            );
        }
    }
}

extern "C" fn collecting_compare(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    a: f64,
    b: f64,
) -> f64 {
    gc_collect_minor();
    a - b
}

#[test]
fn to_sorted_returns_the_live_copy_after_a_collecting_comparator() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _protection =
        crate::arena::ProtectionModeGuard::set(crate::arena::FromSpaceProtection::PoisonOnly);
    register_runtime_handle_root_scanner_for_tests();

    let scope = RuntimeHandleScope::new();
    let source = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(3));
    for (index, value) in [3.0, 1.0, 2.0].into_iter().enumerate() {
        source.with_mut_ptr(|array| crate::array::js_array_set_f64(array, index as u32, value));
    }
    let comparator = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(collecting_compare, 2),
        0,
    ));
    let before_source =
        source.with_const_ptr(|array: *const crate::array::ArrayHeader| array as usize);
    let returned = source.with_const_ptr(|array| {
        comparator.with_const_ptr(|comparator| {
            crate::array::js_array_to_sorted_with_comparator(array, comparator)
        })
    });
    source.with_const_ptr(|array: *const crate::array::ArrayHeader| {
        assert_ne!(
            array as usize, before_source,
            "the callback must actually move the nursery"
        );
        assert_ne!(
            returned.cast_const(),
            array,
            "toSorted must return a distinct copy"
        );
    });
    // Validate the returned address before reading its payload. A forwarding
    // stub can make generic reads appear correct while leaking a stale return.
    let header = unsafe {
        crate::value::addr_class::try_read_tracked_gc_header_of_type(
            returned as usize,
            GC_TYPE_ARRAY,
        )
    }
    .expect("toSorted must return a live tracked Array");
    assert_eq!(unsafe { header.as_ref().gc_flags } & GC_FLAG_FORWARDED, 0);
    for (index, expected) in [1.0, 2.0, 3.0].into_iter().enumerate() {
        assert_eq!(
            crate::array::js_array_get_f64(returned, index as u32),
            expected
        );
    }
    for (index, expected) in [3.0, 1.0, 2.0].into_iter().enumerate() {
        source.with_const_ptr(|array| {
            assert_eq!(
                crate::array::js_array_get_f64(array, index as u32),
                expected
            );
        });
    }
}
