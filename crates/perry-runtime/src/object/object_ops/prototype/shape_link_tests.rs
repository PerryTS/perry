use super::*;

#[test]
fn declaration_collision_is_projected_when_the_shape_is_minted() {
    use crate::object::class_registry::{ClassVTable, VTableMethodEntry, CLASS_VTABLE_REGISTRY};
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let id = 0x4321_0011;
    unsafe { crate::object::js_register_anon_shape_class_id(id) };
    let before = crate::object::js_object_alloc(id, 0);
    assert_eq!(
        unsafe { crate::object::shapes::object_shape_identity(before) },
        crate::object::shapes::PROTO_ID_DEFAULT
    );
    unsafe { crate::object::js_register_class_name(id, b"ShapeCollision".as_ptr(), 14) };
    let mut table = CLASS_VTABLE_REGISTRY.write().unwrap();
    table
        .get_or_insert_with(crate::fast_hash::new_ptr_hash_map)
        .insert(
            id,
            ClassVTable {
                methods: std::collections::HashMap::from([(
                    "recur".into(),
                    VTableMethodEntry {
                        func_ptr: 0x1000,
                        param_count: 1,
                        has_synthetic_arguments: false,
                        has_rest: false,
                        entry: 0,
                    },
                )]),
                ..ClassVTable::default()
            },
        );
    drop(table);
    crate::object::class_registry::refresh_anon_declaration_role(id);
    let after = crate::object::js_object_alloc(id, 0);
    assert_eq!(
        unsafe { crate::object::shapes::object_shape_identity(after) },
        crate::object::shapes::PROTO_ID_CLASS | u64::from(id)
    );
    assert_eq!(
        unsafe { shape_link_prototype(after, false) },
        None,
        "class resolution retains its generation guard"
    );
    assert_eq!(
        unsafe { crate::object::shapes::object_shape_identity(before) },
        crate::object::shapes::PROTO_ID_DEFAULT,
        "the earlier anonymous object retains its birth shape"
    );
}

#[test]
fn ordinary_shape_links_are_complete_and_exotic_links_decline() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let realm = crate::object::builtin_prototype_value("Object");
    let plain = crate::object::js_object_alloc(0, 0);
    assert_eq!(
        unsafe { shape_link_prototype(plain, false) }
            .unwrap()
            .to_bits(),
        realm.to_bits()
    );
    let holder = crate::object::js_object_alloc(0, 0);
    let child = crate::object::js_object_create(crate::value::js_nanbox_pointer(holder as i64));
    let child_ptr = crate::value::js_nanbox_get_pointer(child) as *mut ObjectHeader;
    assert_eq!(
        unsafe { shape_link_prototype(child_ptr, false) }
            .unwrap()
            .to_bits(),
        crate::value::js_nanbox_pointer(holder as i64).to_bits()
    );
    assert_eq!(
        unsafe { shape_link_prototype(child_ptr, true) },
        None,
        "a serial must publish prototype exposure"
    );
    crate::object::prototype_chain::object_set_user_prototype(
        child_ptr as usize,
        crate::value::TAG_NULL,
    );
    assert_eq!(
        unsafe { shape_link_prototype(child_ptr, false) }
            .unwrap()
            .to_bits(),
        crate::value::TAG_NULL
    );
    crate::object::prototype_chain::object_set_user_prototype(child_ptr as usize, realm.to_bits());
    assert_eq!(
        unsafe { shape_link_prototype(child_ptr, false) }
            .unwrap()
            .to_bits(),
        realm.to_bits()
    );
    // Sabotage the receiver's live owner word: no old link may be served.
    let stamp = unsafe { (*child_ptr).parent_class_id };
    unsafe {
        (*child_ptr).parent_class_id = 0;
    }
    assert_eq!(unsafe { shape_link_prototype(child_ptr, false) }, None);
    unsafe {
        (*child_ptr).parent_class_id = stamp;
    }
    // TypedArray prototype semantics outrank its physical object layout.
    let header = unsafe { crate::gc::header_from_trusted_user_ptr(child_ptr.cast()).cast_mut() };
    let flags = unsafe { (*header)._reserved };
    unsafe {
        (*header)._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO;
    }
    assert_eq!(unsafe { shape_link_prototype(child_ptr, false) }, None);
    unsafe {
        (*header)._reserved = flags;
    }
}
