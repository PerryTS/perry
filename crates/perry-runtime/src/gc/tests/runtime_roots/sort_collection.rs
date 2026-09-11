use super::*;
use crate::closure::ClosureHeader;

extern "C" fn captured_direction(closure: *const ClosureHeader, a: f64, b: f64) -> f64 {
    // Model generated capture loads: no forwarding lookup can repair a stale
    // closure argument. The getter changes only the relocated capture.
    let direction = unsafe {
        *((closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const f64)
    };
    direction * (a - b)
}

extern "C" fn collecting_getter(closure: *const ClosureHeader) -> f64 {
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

extern "C" fn accept_sorted_value(_closure: *const ClosureHeader, _value: f64) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
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
            captured_direction as *const u8,
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
            collecting_getter as *const u8,
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
            accept_sorted_value as *const u8,
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
