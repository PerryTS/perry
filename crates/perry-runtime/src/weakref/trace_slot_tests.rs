//! Weak-slot admission must use the authoritative shape bound, including
//! allocated-but-non-live inline storage and both finalization weak fields.

use super::*;

#[test]
fn weak_trace_slot_respects_shape_bounds_and_brands() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        for class_id in [
            0,
            CLASS_ID_FINALIZATION_REGISTRY,
            CLASS_ID_WEAKREF,
            CLASS_ID_FINALIZATION_RECORD,
        ] {
            for live in [0, 1, 2, 4] {
                let obj = crate::object::js_object_alloc(class_id, live);
                let header = (obj as *mut u8)
                    .sub(crate::gc::GC_HEADER_SIZE)
                    .cast::<crate::gc::GcHeader>();
                assert_eq!((*header).obj_type, crate::gc::GC_TYPE_OBJECT);
                assert_eq!((*obj).class_id, class_id);
                assert_eq!(crate::object::object_live_slot_count(obj), live);
                let capacity = (live as usize).max(crate::object::INLINE_SLOT_FLOOR);
                // Include the one-past-capacity address; never dereference slots.
                for field in 0..=capacity {
                    let expected = (field as u32) < live
                        && (class_id == CLASS_ID_WEAKREF && field == 0
                            || class_id == CLASS_ID_FINALIZATION_RECORD && field < 2);
                    assert_eq!(
                        is_weak_target_trace_slot(header, object_field_slot(obj, field)),
                        expected,
                        "class {class_id:#x}, live {live}, field {field}"
                    );
                }
                assert!(!is_weak_target_trace_slot(header, std::ptr::null_mut()));
                let mut unrelated = 0u64;
                assert!(!is_weak_target_trace_slot(header, &mut unrelated));
            }
        }
    }
}

#[test]
fn weak_trace_slot_rejects_null_nonobject_and_absent_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        assert!(!is_weak_target_trace_slot(
            std::ptr::null_mut(),
            std::ptr::null_mut()
        ));

        let array = js_array_alloc(0);
        let array_header = (array as *mut u8)
            .sub(crate::gc::GC_HEADER_SIZE)
            .cast::<crate::gc::GcHeader>();
        assert_ne!((*array_header).obj_type, crate::gc::GC_TYPE_OBJECT);
        assert!(!is_weak_target_trace_slot(
            array_header,
            std::ptr::null_mut()
        ));

        for class_id in [CLASS_ID_WEAKREF, CLASS_ID_FINALIZATION_RECORD] {
            let obj = crate::object::js_object_alloc(class_id, 2);
            let header = (obj as *mut u8)
                .sub(crate::gc::GC_HEADER_SIZE)
                .cast::<crate::gc::GcHeader>();
            let stamp = (*obj).parent_class_id;
            assert_eq!(crate::object::object_live_slot_count(obj), 2);
            assert!(is_weak_target_trace_slot(header, object_field_slot(obj, 0)));
            if class_id == CLASS_ID_FINALIZATION_RECORD {
                assert!(is_weak_target_trace_slot(header, object_field_slot(obj, 1)));
            }
            // Synthetic absent-descriptor fixture: physical storage stays live.
            (*obj).parent_class_id = 0;
            assert_eq!(crate::object::object_live_slot_count(obj), 0);
            assert!(!is_weak_target_trace_slot(
                header,
                object_field_slot(obj, 0)
            ));
            assert!(!is_weak_target_trace_slot(
                header,
                object_field_slot(obj, 1)
            ));
            (*obj).parent_class_id = stamp;
            assert!(is_weak_target_trace_slot(header, object_field_slot(obj, 0)));
            if class_id == CLASS_ID_FINALIZATION_RECORD {
                assert!(is_weak_target_trace_slot(header, object_field_slot(obj, 1)));
            }
        }
    }
}

#[test]
fn owned_weak_storage_slots_exclude_free_list_and_bucket_words() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let map = js_weakmap_new();
        let map = scope.root_nanbox_f64(f64::from_bits(JSValue::pointer(map.cast()).bits()));
        let key = crate::object::js_object_alloc(0, 0);
        js_weakmap_set(
            map.get_nanbox_f64(),
            f64::from_bits(JSValue::pointer(key.cast()).bits()),
            1.0,
        );
        let table = storage::owned_storage(
            js_nanbox_get_pointer(map.get_nanbox_f64()) as *mut ObjectHeader
        );
        let header = header_from_user_addr(table as usize);
        let entry = (*table).entries();
        assert!(is_weak_target_trace_slot(header, &mut (*entry).key));
        assert!(is_weak_target_trace_slot(header, &mut (*entry).value));
        assert!(!is_weak_target_trace_slot(header, std::ptr::null_mut()));
        assert!(!is_weak_target_trace_slot(
            header,
            entry.add((*table).capacity as usize).cast()
        ));
        (*table).remove(0);
        assert!(!is_weak_target_trace_slot(header, &mut (*entry).key));
        assert!(!is_weak_target_trace_slot(header, &mut (*entry).value));
    }
}
