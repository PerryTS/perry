use super::*;

fn subclass(scope: &crate::gc::RuntimeHandleScope, kind: i32) -> crate::gc::RuntimeHandle<'_> {
    let object = crate::object::js_object_alloc(0x1063_7001 + kind as u32, 0);
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(object as i64));
    crate::object::map_set_subclass::js_map_set_subclass_init(
        receiver.get_nanbox_f64(),
        kind,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    receiver
}

#[test]
fn reflective_map_mutators_keep_subclass_identity() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = subclass(&scope, 0);
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
        receiver.get_nanbox_f64(),
    ));
    assert_eq!(
        map_proto_set_thunk(
            std::ptr::null(),
            crate::closure::body_call::current_this(),
            1.0,
            10.0
        )
        .to_bits(),
        receiver.get_nanbox_f64().to_bits()
    );
    assert_eq!(
        map_proto_get_thunk(
            std::ptr::null(),
            crate::closure::body_call::current_this(),
            1.0
        ),
        10.0
    );
    assert_eq!(
        map_proto_size_getter_thunk(std::ptr::null(), crate::closure::body_call::current_this()),
        1.0
    );
    assert_eq!(
        map_proto_delete_thunk(
            std::ptr::null(),
            crate::closure::body_call::current_this(),
            1.0
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        map_proto_has_thunk(
            std::ptr::null(),
            crate::closure::body_call::current_this(),
            1.0
        )
        .to_bits(),
        crate::value::TAG_FALSE
    );
    map_proto_set_thunk(
        std::ptr::null(),
        crate::closure::body_call::current_this(),
        2.0,
        20.0,
    );
    map_proto_clear_thunk(
        std::ptr::null(),
        crate::closure::body_call::current_this(),
        0.0,
    );
    assert_eq!(
        map_proto_size_getter_thunk(std::ptr::null(), crate::closure::body_call::current_this()),
        0.0
    );
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
}

#[test]
fn reflective_set_mutators_keep_subclass_identity() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = subclass(&scope, 1);
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
        receiver.get_nanbox_f64(),
    ));
    assert_eq!(
        set_proto_add_thunk(
            std::ptr::null(),
            crate::closure::body_call::current_this(),
            1.0
        )
        .to_bits(),
        receiver.get_nanbox_f64().to_bits()
    );
    assert_eq!(
        set_proto_has_thunk(
            std::ptr::null(),
            crate::closure::body_call::current_this(),
            1.0
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        set_proto_size_getter_thunk(std::ptr::null(), crate::closure::body_call::current_this()),
        1.0
    );
    assert_eq!(
        set_proto_delete_thunk(
            std::ptr::null(),
            crate::closure::body_call::current_this(),
            1.0
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        set_proto_has_thunk(
            std::ptr::null(),
            crate::closure::body_call::current_this(),
            1.0
        )
        .to_bits(),
        crate::value::TAG_FALSE
    );
    set_proto_add_thunk(
        std::ptr::null(),
        crate::closure::body_call::current_this(),
        2.0,
    );
    set_proto_clear_thunk(
        std::ptr::null(),
        crate::closure::body_call::current_this(),
        0.0,
    );
    assert_eq!(
        set_proto_size_getter_thunk(std::ptr::null(), crate::closure::body_call::current_this()),
        0.0
    );
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
}

#[test]
fn collection_backing_is_not_inherited() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = subclass(&scope, 0);
    let child = crate::object::js_object_alloc(0, 0);
    let child = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(child as i64));
    crate::object::js_object_set_prototype_of(child.get_nanbox_f64(), receiver.get_nanbox_f64());
    assert!(
        crate::object::map_set_subclass::subclass_backing_of(receiver.get_nanbox_f64()).is_some()
    );
    assert!(crate::object::map_set_subclass::subclass_backing_of(child.get_nanbox_f64()).is_none());
}
