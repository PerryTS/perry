//! Fresh Object.keys results from ordinary shape storage.
use super::*;

pub(super) fn try_keys(obj: *const ObjectHeader) -> Option<*mut ArrayHeader> {
    let addr = strip_nanbox_addr(obj);
    let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }?;
    unsafe {
        if !crate::gc::gc_type_is_object(header.as_ref().obj_type) {
            return None;
        }
        let obj = addr as *const ObjectHeader;
        let record = crate::object::shapes::object_shape_record(obj)?;
        // Dictionary, native namespace and Array-subclass shapes retain their
        // existing enumeration. Class storage retains its internal-key filter.
        // A layout that can have holes takes the walk that asks `key_view`.
        if !matches!(
            record.object_kind(),
            crate::object::shapes::ShapeObjectKind::Ordinary
                | crate::object::shapes::ShapeObjectKind::OrdinaryUnmarked
        ) || record.summary()
            & (crate::object::key_attrs::SUMMARY_NON_ENUMERABLE
                | crate::object::key_attrs::SUMMARY_PRIVATE
                | crate::object::key_attrs::SUMMARY_HOLES)
            != 0
        {
            return None;
        }
        // Closed literals may have anonymous class ids. Their DEFAULT shape
        // prototype proves that no class vtable contributes virtual own keys.
        // Native and synthetic classes can also use DEFAULT; retain their
        // existing dispatch, as for real class and Array-subclass instances.
        if (*obj).class_id != 0
            && ((*obj).class_id
                >= crate::object::class_registry::prototype_objects::SYNTHETIC_CLASS_ID_BASE
                || record.proto_id() != crate::object::shapes::PROTO_ID_DEFAULT)
        {
            return None;
        }
        // Some dictionary publishers retain an ordinary kind on their keyless
        // shape. The existing private-list edge still proves receiver ownership.
        if super::super::overflow::has_overflow(obj) {
            return None;
        }
        let keys = crate::object::object_keys_from_shape_record(obj, record);
        let order = ecma_own_key_order(keys);
        let count = keys.count();
        // Keep the shape's immutable key prefix across result allocation, which
        // may collect or reenter. Root its backing directly; neither the receiver
        // nor a borrowed shape record is needed afterwards. The prefix count and
        // class-key filter are scalar snapshots of the selected key list.
        let filter_internal = (*obj).class_id != 0;
        let scope = crate::gc::RuntimeHandleScope::new();
        let backing = scope.root_raw_const_ptr(keys.arr());
        let out = crate::array::js_array_alloc(count);
        backing.with_const_ptr::<ArrayHeader, _>(|keys| {
            // Reacquire the rooted backing after allocation, retaining the
            // generic grow/forward resolution of an ObjectKeys view.
            let keys = crate::object::ObjectKeys::new(keys.cast_mut(), count);
            let (source, available) = keys.dense_slots();
            let dest = crate::array::array_elements_ptr(out) as *mut f64;
            let mut written = 0;
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            for i in 0..count as usize {
                let slot = order.as_ref().map_or(i, |order| order[i] as usize);
                if slot >= available {
                    continue;
                }
                let value = *source.add(slot);
                // Symbols and tombstones are never string keys.
                if !JSValue::from_bits(value.to_bits()).is_any_string() {
                    continue;
                }
                if filter_internal
                    && crate::string::js_string_key_bytes(
                        JSValue::from_bits(value.to_bits()),
                        &mut scratch,
                    )
                    .is_some_and(is_internal_runtime_key_bytes)
                {
                    continue;
                }
                crate::string::js_string_addref_if_heap_string(value);
                // GC_STORE_AUDIT(BARRIERED): bulk initialization is followed by
                // the existing exact layout/barrier rebuild before publication.
                dest.add(written).write(value);
                written += 1;
            }
            (*out).length = written as u32;
            // This array is fresh and unpublished: initialize its exact slot
            // layout using the same helper as fresh canonical key lists. A bulk
            // mutation rebuild would revoke element-shape metadata it never had.
            // The helper retains numeric reclassification and old-parent barriers.
            crate::object::gc_slots::rebuild_array_layout_from_slots(out);
        });
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objops_keys_copy_the_shape_prefix_and_refuse_non_enumerable_or_dictionary() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        unsafe {
            let key =
                |name: &[u8]| crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let proof = crate::object::canonical_keys::SharedLayout::shape_cache_entry();
            let first = crate::object::canonical_keys::extend_key(
                &proof,
                crate::object::canonical_keys::CanonicalKeys::EMPTY,
                key(b"objops_long_first"),
            );
            let second = crate::object::canonical_keys::extend_key(
                &proof,
                first,
                key(b"objops_long_second"),
            );
            assert_eq!(
                first.as_ptr(),
                second.as_ptr(),
                "live shared-backing premise"
            );
            assert_eq!(crate::array::js_array_length(first.as_ptr()), 2);
            crate::object::declarations::test_declare_anonymous(0x12345);
            let obj = crate::object::js_object_alloc(0x12345, 4);
            crate::object::set_object_keys_with_live(obj, first.view(), 4);
            let result = try_keys(obj).expect("ordinary shape must take the bulk copy");
            assert_ne!(result, first.as_ptr());
            assert_eq!(crate::array::js_array_length(result), 1);
            assert_eq!(
                crate::array::js_array_get(result, 0).bits(),
                first.view().get(0).bits()
            );
            // Mutating a fresh result cannot mutate the shape's key storage.
            crate::array::js_array_push_f64(result, 42.0);
            assert_eq!(crate::object::object_keys(obj).count(), 1);
            crate::object::set_property_attrs(
                obj as usize,
                "objops_long_first".into(),
                crate::object::PropertyAttrs::new(true, false, true),
            );
            assert!(
                try_keys(obj).is_none(),
                "non-enumerable must retain filtering"
            );
            let native = crate::object::js_object_alloc(crate::wasi::CLASS_ID_WASI, 4);
            assert!(
                try_keys(native).is_none(),
                "native DEFAULT retains dispatch"
            );
            let class = crate::object::js_object_alloc(0x23456, 4);
            assert!(
                try_keys(class).is_none(),
                "class prototype retains dispatch"
            );
            let dict = crate::object::js_object_alloc(0, 4);
            crate::object::js_object_set_field_by_name(dict, key(b"dict_key"), 1.0);
            assert!(crate::object::overflow::establish_order(dict));
            assert!(
                try_keys(dict).is_none(),
                "receiver-owned keys keep their path"
            );
        }
    }
}
