use super::*;
use crate::gc::{CopyingNurseryTestGuard, GcTriggerThresholdTestGuard, RuntimeHandleScope};

#[test]
fn homogeneous_message_owns_one_layout_and_keeps_cycles_through_moving_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let keys_scope = RuntimeHandleScope::new();
    let keys = unsafe {
        crate::object::static_shapes::canonical_keys_for_names(&[b"id", b"name", b"self"])
    };
    let keys = keys_scope.root_raw_mut_ptr(keys.arr());
    for frozen in [false, true] {
        let scope = RuntimeHandleScope::new();
        unsafe {
            let name = scope.root_nanbox_u64(
                crate::value::JSValue::string_ptr(crate::string::js_string_from_bytes(
                    b"repeated string beyond short-string length".as_ptr(),
                    b"repeated string beyond short-string length".len() as u32,
                ))
                .bits(),
            );
            const COUNT: usize = 4096;
            let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc((COUNT + 1) as u32));
            let mut first = TAG_UNDEFINED;
            for i in 0..COUNT {
                let obj = crate::object::alloc_plain::alloc_plain_record_with_keys(
                    3,
                    crate::object::ObjectKeys::new(keys.get_raw_mut_ptr(), 3),
                );
                let bits = crate::value::JSValue::pointer(obj.cast()).bits();
                store_thread_object_field(obj, 0, (i as f64).to_bits());
                store_thread_object_field(obj, 1, name.get_nanbox_u64());
                store_thread_object_field(obj, 2, bits);
                if frozen {
                    crate::object::js_object_freeze(f64::from_bits(bits));
                }
                store_thread_array_slot(array.get_raw_mut_ptr(), i, bits);
                if i == 0 {
                    first = bits;
                }
            }
            store_thread_array_slot(array.get_raw_mut_ptr(), COUNT, first);
            let message = serialize_message(
                crate::value::JSValue::pointer(array.get_raw_const_ptr::<u8>()).bits(),
                &[],
                None,
            )
            .unwrap();
            let SerializedValue::Message { data, .. } = &message else {
                panic!("message codec not used")
            };
            assert_eq!(
                data.shapes.len(),
                1,
                "keys belong to the layout, not each record"
            );
            assert_eq!(data.strings.len(), 1, "shared strings are message values");
            assert!(std::mem::size_of::<CloneNode>() <= 24);
            assert!(std::mem::size_of::<CloneSlot>() == 8);
            let cloned = scope.root_nanbox_u64(deserialize_nanbox_on_current_thread(&message));
            assert_ne!(
                cloned.get_nanbox_u64(),
                crate::value::JSValue::pointer(array.get_raw_const_ptr::<u8>()).bits()
            );
            let before = crate::gc::moved_objects_total();
            for _ in 0..2 {
                let _ = crate::gc::gc_collect_minor();
            }
            assert!(
                crate::gc::moved_objects_total() > before,
                "the witness must move live cells"
            );
            let arr = (cloned.get_nanbox_u64() & POINTER_MASK) as *const crate::array::ArrayHeader;
            let first = crate::array::js_array_get_f64(arr, 0).to_bits();
            assert_eq!(
                first,
                crate::array::js_array_get_f64(arr, COUNT as u32).to_bits()
            );
            for i in 0..COUNT {
                let bits = crate::array::js_array_get_f64(arr, i as u32).to_bits();
                let obj = (bits & POINTER_MASK) as *const crate::object::ObjectHeader;
                assert_eq!(
                    crate::object::js_object_get_field(obj, 0).bits(),
                    (i as f64).to_bits()
                );
                assert_eq!(crate::object::js_object_get_field(obj, 2).bits(), bits);
            }
            let obj = (first & POINTER_MASK) as *mut crate::object::ObjectHeader;
            crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(99.0));
            assert_eq!(
                crate::object::js_object_get_field(obj, 0).bits(),
                99.0f64.to_bits()
            );
        }
    }
}

#[test]
fn message_parsed_arrays_keep_aliases_through_moving_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    unsafe {
        let json = b"[{\"id\":1},[2,3],\"a long shared string\"]";
        let text = crate::string::js_string_from_bytes(json.as_ptr(), json.len() as u32);
        let lazy = scope.root_nanbox_u64(crate::json::js_json_parse(text).bits());
        let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc(2));
        for i in 0..2 {
            store_thread_array_slot(array.get_raw_mut_ptr(), i, lazy.get_nanbox_u64());
        }
        let message = serialize_message(
            crate::value::JSValue::pointer(array.get_raw_const_ptr::<u8>()).bits(),
            &[],
            None,
        )
        .unwrap();
        let result = scope.root_nanbox_u64(deserialize_nanbox_on_current_thread(&message));
        for _ in 0..2 {
            let _ = crate::gc::gc_collect_minor();
        }
        let result = (result.get_nanbox_u64() & POINTER_MASK) as *const crate::array::ArrayHeader;
        let a = crate::array::js_array_get_f64(result, 0).to_bits();
        assert_eq!(a, crate::array::js_array_get_f64(result, 1).to_bits());
        let a = (a & POINTER_MASK) as *const crate::array::ArrayHeader;
        assert_eq!((*a).length, 3);
        let obj = (crate::array::js_array_get_f64(a, 0).to_bits() & POINTER_MASK)
            as *const crate::object::ObjectHeader;
        assert_eq!(crate::object::js_object_get_field(obj, 0).as_number(), 1.0);
        let nums = (crate::array::js_array_get_f64(a, 1).to_bits() & POINTER_MASK)
            as *const crate::array::ArrayHeader;
        assert_eq!(
            crate::value::JSValue::from_bits(crate::array::js_array_get_f64(nums, 1).to_bits())
                .as_number(),
            3.0
        );
    }
}
#[test]
fn frozen_array_records_keep_creation_order() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    unsafe {
        let keys = crate::object::static_shapes::canonical_keys_for_names(&[b"value"]);
        let obj = scope.root_raw_mut_ptr(crate::object::alloc_plain::alloc_plain_record_with_keys(
            1, keys,
        ));
        store_thread_object_field(obj.get_raw_mut_ptr(), 0, 7.0f64.to_bits());
        let bits = crate::value::JSValue::pointer(obj.get_raw_const_ptr::<u8>()).bits();
        crate::object::js_object_freeze(f64::from_bits(bits));
        let arr = scope.root_raw_mut_ptr(crate::array::js_array_alloc(3));
        store_thread_array_slot(arr.get_raw_mut_ptr(), 0, bits);
        store_thread_array_slot(arr.get_raw_mut_ptr(), 1, TAG_UNDEFINED);
        store_thread_array_slot(arr.get_raw_mut_ptr(), 2, 9.0f64.to_bits());
        let bits = crate::value::JSValue::pointer(arr.get_raw_const_ptr::<u8>()).bits();
        crate::object::js_object_freeze(f64::from_bits(bits));
        let message = serialize_message(bits, &[], None).unwrap();
        let copied = deserialize_nanbox_on_current_thread(&message);
        let arr = (copied & POINTER_MASK) as *const crate::array::ArrayHeader;
        assert_eq!(crate::array::js_array_length(arr), 3);
        let obj = (crate::array::js_array_get_f64(arr, 0).to_bits() & POINTER_MASK)
            as *const crate::object::ObjectHeader;
        assert_eq!(
            crate::object::js_object_get_field(obj, 0).bits(),
            7.0f64.to_bits()
        );
        assert_eq!(
            crate::array::js_array_get_f64(arr, 1).to_bits(),
            TAG_UNDEFINED
        );
        assert_eq!(crate::array::js_array_get_f64(arr, 2), 9.0);
    }
}
