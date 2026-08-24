/// #8113 replaces #8067's "saved lineage beats an interim self-heal" test.
/// An unstamped receiver must miss without publishing a lineage-free shape.
#[test]
fn an_unstamped_receiver_misses_instead_of_being_self_healed() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        const CID: u32 = 0x8068;
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 1));
        let ((), obj) = obj_handle.across_mut::<crate::ObjectHeader, _>(|| {
            obj_handle.with_mut_ptr::<crate::ObjectHeader, _>(|obj| {
                super::js_object_mark_class(obj as i64)
            })
        });
        let predecessor =
            crate::object::shapes::object_shape_descriptor(obj).expect("marked class descriptor");
        assert_eq!(
            predecessor.object_kind,
            crate::object::shapes::ShapeObjectKind::Class
        );
        assert_eq!(predecessor.live_inline_slot_count, 1);

        assert!(crate::object::shapes::clear_object_shape_stamp(obj));
        let (interim, obj) = obj_handle.across_mut::<crate::ObjectHeader, _>(|| {
            crate::typed_feedback::test_object_shape_token(obj as usize)
        });
        assert_eq!(
            interim, 0,
            "an unstamped receiver must miss instead of publishing a zero-slot shape"
        );
        assert!(crate::object::shapes::object_shape_descriptor(obj).is_none());

        crate::object::shapes::synchronize_object_shape_descriptor_from(
            obj,
            Some(predecessor),
            predecessor.live_inline_slot_count,
        );
        let restored =
            crate::object::shapes::object_shape_descriptor(obj).expect("restored descriptor");
        assert_eq!(
            restored.object_kind,
            crate::object::shapes::ShapeObjectKind::Class
        );
        assert_eq!(restored.live_inline_slot_count, 1);
    }
}
