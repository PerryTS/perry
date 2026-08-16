use super::super::*;

#[test]
fn gc_recovers_keys_and_live_slots_from_shape_id_after_header_sabotage() {
    let _lock = global_side_table_test_lock();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
        let key_a = crate::string::js_string_from_bytes(b"shape_gc_a".as_ptr(), 10);
        let key_a_handle = scope.root_string_ptr(key_a);
        let key_b = crate::string::js_string_from_bytes(b"shape_gc_b".as_ptr(), 10);
        let key_b_handle = scope.root_string_ptr(key_b);
        obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
            key_a_handle.with_mut_ptr::<crate::StringHeader, _>(|key_a| {
                crate::object::js_object_set_field_by_name(
                    obj,
                    key_a,
                    crate::value::js_nanbox_pointer(key_a as i64),
                );
            });
        });
        obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
            key_b_handle.with_mut_ptr::<crate::StringHeader, _>(|key_b| {
                crate::object::js_object_set_field_by_name(
                    obj,
                    key_b,
                    crate::value::js_nanbox_pointer(key_b as i64),
                );
            });
        });

        obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
            let descriptor = crate::object::shapes::object_shape_descriptor(obj)
                .expect("published object must have an authoritative descriptor");
            assert_eq!(descriptor.logical_key_count, 2);
            assert_eq!(descriptor.live_inline_slot_count, 2);

            // `keys_array` is the last ABI mirror (#8047 removes it; #8113
            // already removed `field_count`). Corrupt it to prove the GC walk
            // derives its strong keys edge — and, since the payload range now
            // has NO header mirror at all, its exact slot count — from ShapeId.
            // GC_STORE_AUDIT(POINTER_FREE): test sabotage removes the compatibility edge by storing null.
            (*obj).keys_array = std::ptr::null_mut();

            let slots = super::support::test_heap_child_slots_for_user(obj as *mut u8);
            assert_eq!((*obj).keys_array as u64, descriptor.keys);

            let fields =
                (obj as *mut u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *mut u64;
            let child_slots: Vec<*mut u64> = slots
                .into_iter()
                .filter_map(|slot| match slot {
                    HeapChildSlot::Child(ptr, _) => Some(ptr),
                    HeapChildSlot::PointerFreeRange(_) => None,
                })
                .collect();
            assert!(child_slots.contains(&fields));
            assert!(child_slots.contains(&fields.add(1)));
        });
    }
}
