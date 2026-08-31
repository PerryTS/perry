//! #8067 shape-authority tests, split out of `parent_static.rs` to keep it
//! under the 2000-line cap. Body unchanged; only its home file moved.

fn key<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    name: &str,
) -> crate::gc::RuntimeHandle<'scope> {
    scope.root_string_ptr(crate::string::js_string_from_bytes(
        name.as_ptr(),
        name.len() as u32,
    ))
}

#[test]
fn mark_class_rejects_non_heap_addresses() {
    // Representative ids from the native-handle and proxy bands. The
    // extern entry point must validate before reading a preceding header.
    super::js_object_mark_class(0x40000);
    super::js_object_mark_class(1);
}

#[test]
fn class_kind_survives_static_field_installation_and_deletion() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        const CID: u32 = 0x8067;
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 8));
        let before = obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
            let before = crate::object::shapes::object_shape_id(obj);
            assert!(crate::object::object_is_regular(obj));
            before
        });

        let ((), obj) = obj_handle.across_mut::<crate::ObjectHeader, _>(|| {
            obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
                super::js_object_mark_class(obj as i64)
            })
        });
        let after = crate::object::shapes::object_shape_id(obj);
        assert_ne!(before, after, "becoming a class object is semantic");
        assert_eq!(
            crate::object::shapes::object_shape_descriptor(obj)
                .expect("class descriptor")
                .object_kind,
            crate::object::shapes::ShapeObjectKind::Class
        );

        // #8113 removed the `object_type` compatibility mirror this used to
        // sabotage. Classification is driven by the ShapeId descriptor
        // transition above and by nothing else, so assert that directly.
        assert!(super::is_class_object_ptr(obj.cast()));
        assert!(!crate::object::object_is_regular(obj));

        // Numeric layout installation historically set/cleared bits in
        // GcHeader::_reserved, where the old class marker collided with
        // GC_LAYOUT_ALL_POINTERS. Shape kind must be unaffected.
        let numeric_key = key(&scope, "numericStatic");
        let ((), obj) = obj_handle.across_mut::<crate::ObjectHeader, _>(|| {
            obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
                numeric_key.with_const_ptr::<crate::StringHeader, _>(|key| {
                    crate::object::js_object_set_field_by_name(obj, key, 42.0)
                })
            })
        });
        assert!(
            super::is_class_object_ptr(obj.cast()),
            "numeric static write changed class descriptor: {:?}",
            crate::object::shapes::object_shape_descriptor(obj)
        );
        assert!(!crate::object::object_is_regular(obj));

        // Repeat with a pointer-bearing static value, which drives the
        // opposite GC layout state and used to erase the aliased bit.
        let pointer_key = key(&scope, "pointerStatic");
        let payload = key(&scope, "rootedStaticValue");
        let ((), obj) = obj_handle.across_mut::<crate::ObjectHeader, _>(|| {
            obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
                pointer_key.with_const_ptr::<crate::StringHeader, _>(|key| {
                    payload.with_mut_ptr::<crate::StringHeader, _>(|payload| {
                        let value =
                            f64::from_bits(crate::value::JSValue::string_ptr(payload).bits());
                        crate::object::js_object_set_field_by_name(obj, key, value)
                    })
                })
            })
        });
        assert!(super::is_class_object_ptr(obj.cast()));
        assert!(!crate::object::object_is_regular(obj));
        assert_eq!(
            crate::object::shapes::object_shape_descriptor(obj)
                .expect("post-write class descriptor")
                .object_kind,
            crate::object::shapes::ShapeObjectKind::Class,
            "class kind must never share storage with GC layout flags"
        );

        // The pointer-bearing write above leaves a typed/side-mask layout.
        // Growing the keys array from that state invalidates typed
        // feedback. The invalidation asks for the receiver's shape, so a
        // keys transition must keep the old class stamp visible until the
        // invalidation finishes and must prefer its saved predecessor over
        // any defensive self-heal in the temporary cleared-stamp window.
        let after_pointer_key = key(&scope, "afterPointerStatic");
        let ((), obj) = obj_handle.across_mut::<crate::ObjectHeader, _>(|| {
            obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
                after_pointer_key.with_const_ptr::<crate::StringHeader, _>(|key| {
                    crate::object::js_object_set_field_by_name(obj, key, 7.0)
                })
            })
        });
        assert!(
            super::is_class_object_ptr(obj.cast()),
            "typed-layout invalidation erased class descriptor lineage: {:?}",
            crate::object::shapes::object_shape_descriptor(obj)
        );
        assert_eq!(
            crate::object::shapes::object_shape_descriptor(obj)
                .expect("post-invalidation class descriptor")
                .object_kind,
            crate::object::shapes::ShapeObjectKind::Class
        );

        // Deletion installs a cloned keys array, which clears the current
        // stamp. The replacement descriptor must inherit class kind from
        // the predecessor captured before that clear.
        let (deleted, obj) = obj_handle.across_mut::<crate::ObjectHeader, _>(|| {
            obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
                numeric_key.with_const_ptr::<crate::StringHeader, _>(|key| {
                    crate::object::js_object_delete_field(obj, key)
                })
            })
        });
        assert_eq!(deleted, 1);
        assert!(
            super::is_class_object_ptr(obj.cast()),
            "deleting a static field erased class descriptor lineage: {:?}",
            crate::object::shapes::object_shape_descriptor(obj)
        );
        assert_eq!(
            crate::object::shapes::object_shape_descriptor(obj)
                .expect("post-delete class descriptor")
                .object_kind,
            crate::object::shapes::ShapeObjectKind::Class
        );

        let class_value = crate::value::js_nanbox_pointer(obj as i64);
        let class_value_handle = scope.root_nanbox_f64(class_value);
        let typeof_ptr = crate::builtins::js_value_typeof(class_value);
        assert_eq!(crate::regex::string_as_str(typeof_ptr), "function");

        let instance = crate::object::js_new_function_construct(
            class_value_handle.get_nanbox_f64(),
            std::ptr::null(),
            0,
        );
        let instance_handle = scope.root_nanbox_f64(instance);
        let instance_value = crate::value::JSValue::from_bits(instance.to_bits());
        assert!(
            instance_value.is_pointer(),
            "construction must return an object"
        );
        let instance_ptr = instance_value.as_pointer::<crate::ObjectHeader>();
        assert_eq!((*instance_ptr).class_id, CID);
        assert_eq!(
            crate::object::js_instanceof_dynamic(
                instance_handle.get_nanbox_f64(),
                class_value_handle.get_nanbox_f64(),
            )
            .to_bits(),
            crate::value::TAG_TRUE,
            "instanceof must still recognize the class after static writes"
        );
    }
}
