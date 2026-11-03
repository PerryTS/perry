use super::*;
use host_resource::payload;
use serde_json::json;

fn world() -> Value {
    json!({"imports":[{"name":"test:host/items","resources":{"item":"t0","other":"t1"}}]})
}
fn text<'a>(scope: &'a RuntimeHandleScope, value: &str) -> crate::gc::RuntimeHandle<'a> {
    scope.root_nanbox_f64(crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32) as i64,
    ))
}
fn reference(generation: u32) -> Value {
    json!({"token":"1","type":"t0","generation":generation})
}
fn transfer(owner: f64) {
    payload::event(
        owner,
        &json!({"kind":"lower","created":[reference(1)],"transferred":[reference(1)]}),
    );
}
fn wire(generation: u32, owned: bool) -> Value {
    json!({"$hostResource":"1","type":"t0","generation":generation,"owned":owned,"new":false})
}
fn read(owner: f64, ty: f64, resource: f64) -> f64 {
    host_resource::js_component_host_resource_value(owner, ty, resource)
}
fn method(resource: f64, name: &[u8]) -> f64 {
    unsafe {
        crate::closure::js_native_call_value(
            field(resource, name),
            crate::closure::plain_call_receiver(),
            std::ptr::null(),
            0,
        )
    }
}

#[test]
fn host_payload_survives_moving_gc_and_returned_ownership_invalidates_old_aliases() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(world(), undefined()));
    let ty = text(&scope, "t0");
    let old = scope.root_nanbox_f64(undefined());
    let weak = scope.root_nanbox_f64(undefined());
    let before;
    {
        let temporary = RuntimeHandleScope::new();
        let value = text(&temporary, "payload kept only by the host table");
        // WeakRef requires an object, so store the text inside an object.
        let object = temporary.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        set_field(object.get_nanbox_f64(), b"value", value.get_nanbox_f64());
        before = object.get_nanbox_f64().to_bits();
        weak.set_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::weakref::js_weakref_new(object.get_nanbox_f64()) as i64,
        ));
        old.set_nanbox_f64(payload::js_component_host_resource_create(
            owner.get_nanbox_f64(),
            ty.get_nanbox_f64(),
            object.get_nanbox_f64(),
            undefined(),
        ));
    }
    let borrowed = scope.root_nanbox_f64(method(old.get_nanbox_f64(), b"borrow"));
    transfer(owner.get_nanbox_f64());
    crate::gc::gc_collect_minor();
    let moved = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
    assert!(!JSValue::from_bits(moved.to_bits()).is_undefined());
    assert_ne!(moved.to_bits(), before, "the payload must actually move");
    for resource in [&old, &borrowed] {
        assert!(crate::exception::catch_js_throw(|| read(
            owner.get_nanbox_f64(),
            ty.get_nanbox_f64(),
            resource.get_nanbox_f64()
        ))
        .is_err());
    }
    payload::event(
        owner.get_nanbox_f64(),
        &json!({"kind":"lift","resource":reference(2)}),
    );
    let returned = scope.root_nanbox_f64(host_resource::decode(
        owner.get_nanbox_f64(),
        &wire(2, true),
        undefined(),
    ));
    assert_eq!(
        string(field(
            read(
                owner.get_nanbox_f64(),
                ty.get_nanbox_f64(),
                returned.get_nanbox_f64()
            ),
            b"value"
        )),
        "payload kept only by the host table"
    );
    // Returning the same resource must not revive either old wrapper.
    for resource in [&old, &borrowed] {
        assert!(crate::exception::catch_js_throw(|| read(
            owner.get_nanbox_f64(),
            ty.get_nanbox_f64(),
            resource.get_nanbox_f64()
        ))
        .is_err());
    }
    payload::event(
        owner.get_nanbox_f64(),
        &json!({"kind":"drop","resource":reference(2)}),
    );
    crate::gc::gc_collect_minor();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits())
            .is_undefined()
    );
    context::finish_disposal(owner.get_nanbox_f64(), perry_abi::component_host::OK);
}

#[test]
fn host_resources_reject_foreign_types_instances_forgery_and_expired_borrows() {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(world(), undefined()));
    let foreign = scope.root_nanbox_f64(context::publish(world(), undefined()));
    let ty = text(&scope, "t0");
    let other = text(&scope, "t1");
    let owned = scope.root_nanbox_f64(payload::js_component_host_resource_create(
        owner.get_nanbox_f64(),
        ty.get_nanbox_f64(),
        42.0,
        undefined(),
    ));
    assert!(crate::exception::catch_js_throw(|| read(
        foreign.get_nanbox_f64(),
        ty.get_nanbox_f64(),
        owned.get_nanbox_f64()
    ))
    .is_err());
    assert!(crate::exception::catch_js_throw(|| read(
        owner.get_nanbox_f64(),
        other.get_nanbox_f64(),
        owned.get_nanbox_f64()
    ))
    .is_err());
    assert!(crate::exception::catch_js_throw(|| value::encode_for(
        owned.get_nanbox_f64(),
        foreign.get_nanbox_f64()
    ))
    .is_err());
    let forged = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(forged.get_nanbox_f64(), b"$hostResource", 1.0);
    assert!(crate::exception::catch_js_throw(|| value::encode_for(
        forged.get_nanbox_f64(),
        owner.get_nanbox_f64()
    ))
    .is_err());
    // A later invalid transfer must leave the first resource untouched.
    assert!(crate::exception::catch_js_throw(|| payload::event(owner.get_nanbox_f64(), &json!({"kind":"lower","created":[reference(1)],"transferred":[reference(1),{"token":"9","type":"t0","generation":1}]}))).is_err());
    assert_eq!(
        read(
            owner.get_nanbox_f64(),
            ty.get_nanbox_f64(),
            owned.get_nanbox_f64()
        ),
        42.0
    );
    assert_eq!(
        value::encode_for(owned.get_nanbox_f64(), owner.get_nanbox_f64())["new"],
        true
    );
    transfer(owner.get_nanbox_f64());
    let lifetime = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    crate::array::js_array_set_f64(array_pointer(lifetime.get_nanbox_f64()).unwrap(), 0, 1.0);
    let borrowed = scope.root_nanbox_f64(host_resource::decode(
        owner.get_nanbox_f64(),
        &wire(1, false),
        lifetime.get_nanbox_f64(),
    ));
    assert_eq!(
        read(
            owner.get_nanbox_f64(),
            ty.get_nanbox_f64(),
            borrowed.get_nanbox_f64()
        ),
        42.0
    );
    crate::array::js_array_set_f64(array_pointer(lifetime.get_nanbox_f64()).unwrap(), 0, 0.0);
    assert!(crate::exception::catch_js_throw(|| read(
        owner.get_nanbox_f64(),
        ty.get_nanbox_f64(),
        borrowed.get_nanbox_f64()
    ))
    .is_err());
    context::finish_disposal(owner.get_nanbox_f64(), perry_abi::component_host::OK);
    context::finish_disposal(foreign.get_nanbox_f64(), perry_abi::component_host::OK);
}

#[test]
fn imported_borrow_capabilities_move_with_their_private_wrapper_and_expire() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(world(), undefined()));
    let ty = text(&scope, "t0");
    let owned = scope.root_nanbox_f64(payload::js_component_host_resource_create(
        owner.get_nanbox_f64(),
        ty.get_nanbox_f64(),
        42.0,
        undefined(),
    ));
    // Locally created borrow views have no imported canonical lease.
    let view = scope.root_nanbox_f64(method(owned.get_nanbox_f64(), b"borrow"));
    assert!(
        value::encode_for(view.get_nanbox_f64(), owner.get_nanbox_f64())
            .get("$borrow")
            .is_none()
    );
    transfer(owner.get_nanbox_f64());
    let lifetime = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    crate::array::js_array_set_f64(array_pointer(lifetime.get_nanbox_f64()).unwrap(), 0, 1.0);
    let mut descriptor = wire(1, false);
    descriptor["$borrow"] = json!("opaque-native-borrow-with-a-long-store-nonce:123456789");
    let borrowed = scope.root_nanbox_f64(host_resource::decode(
        owner.get_nanbox_f64(),
        &descriptor,
        lifetime.get_nanbox_f64(),
    ));
    assert!(
        JSValue::from_bits(field(borrowed.get_nanbox_f64(), b"$borrow").to_bits()).is_undefined()
    );
    assert_eq!(
        value::encode_for(borrowed.get_nanbox_f64(), owner.get_nanbox_f64()),
        descriptor
    );
    let before = borrowed.get_nanbox_f64().to_bits();
    crate::gc::gc_collect_minor();
    assert_ne!(
        borrowed.get_nanbox_f64().to_bits(),
        before,
        "the wrapper must actually move"
    );
    assert_eq!(
        value::encode_for(borrowed.get_nanbox_f64(), owner.get_nanbox_f64()),
        descriptor
    );
    assert_eq!(
        read(
            owner.get_nanbox_f64(),
            ty.get_nanbox_f64(),
            borrowed.get_nanbox_f64()
        ),
        42.0
    );
    crate::array::js_array_set_f64(array_pointer(lifetime.get_nanbox_f64()).unwrap(), 0, 0.0);
    assert!(crate::exception::catch_js_throw(|| value::encode_for(
        borrowed.get_nanbox_f64(),
        owner.get_nanbox_f64(),
    ))
    .is_err());
    context::finish_disposal(owner.get_nanbox_f64(), perry_abi::component_host::OK);
}

extern "C-unwind" fn destroy(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    payload: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let counter = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let count = field(counter.get_nanbox_f64(), b"count");
    set_field(counter.get_nanbox_f64(), b"count", count + 1.0);
    crate::exception::js_throw(payload)
}

#[test]
fn disposing_an_owned_callback_argument_releases_payload_before_canonical_drain() {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(world(), undefined()));
    let ty = text(&scope, "t0");
    let counter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(counter.get_nanbox_f64(), b"count", 0.0);
    let destructor = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind destroy, 1),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        destructor.get_raw_mut_ptr(),
        0,
        counter.get_nanbox_f64(),
    );
    payload::js_component_host_resource_create(
        owner.get_nanbox_f64(),
        ty.get_nanbox_f64(),
        77.0,
        crate::value::js_nanbox_pointer(
            destructor.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    transfer(owner.get_nanbox_f64());
    payload::event(
        owner.get_nanbox_f64(),
        &json!({"kind":"lift","resource":reference(2)}),
    );
    let lifetime = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(1) as i64,
    ));
    crate::array::js_array_set_f64(array_pointer(lifetime.get_nanbox_f64()).unwrap(), 0, 1.0);
    let owned = scope.root_nanbox_f64(host_resource::decode(
        owner.get_nanbox_f64(),
        &wire(2, true),
        lifetime.get_nanbox_f64(),
    ));
    assert_eq!(
        crate::exception::catch_js_throw(|| method(owned.get_nanbox_f64(), b"dispose"))
            .unwrap_err(),
        77.0
    );
    method(owned.get_nanbox_f64(), b"dispose");
    assert_eq!(field(counter.get_nanbox_f64(), b"count"), 1.0);
    let pending = unsafe {
        std::mem::take(&mut (*context::get(owner.get_nanbox_f64())).pending_resource_drops)
    };
    assert_eq!(pending, [reference(2)]);
    payload::event(
        owner.get_nanbox_f64(),
        &json!({"kind":"drop","resource":pending[0]}),
    );
    context::finish_disposal(owner.get_nanbox_f64(), perry_abi::component_host::OK);
    assert_eq!(
        field(counter.get_nanbox_f64(), b"count"),
        1.0,
        "canonical retirement must not repeat the destructor"
    );
}

#[test]
fn throwing_host_destructors_run_once_and_cleanup_preserves_the_first_exception() {
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(world(), undefined()));
    let ty = text(&scope, "t0");
    let counter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(counter.get_nanbox_f64(), b"count", 0.0);
    let destructor = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind destroy, 1),
        1,
    ));
    crate::closure::js_closure_set_capture_f64(
        destructor.get_raw_mut_ptr(),
        0,
        counter.get_nanbox_f64(),
    );
    let resource = scope.root_nanbox_f64(payload::js_component_host_resource_create(
        owner.get_nanbox_f64(),
        ty.get_nanbox_f64(),
        77.0,
        crate::value::js_nanbox_pointer(
            destructor.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    ));
    assert_eq!(
        crate::exception::catch_js_throw(|| method(resource.get_nanbox_f64(), b"dispose"))
            .unwrap_err(),
        77.0
    );
    method(resource.get_nanbox_f64(), b"dispose");
    assert_eq!(field(counter.get_nanbox_f64(), b"count"), 1.0);
    for value in [88.0, 99.0] {
        payload::js_component_host_resource_create(
            owner.get_nanbox_f64(),
            ty.get_nanbox_f64(),
            value,
            crate::value::js_nanbox_pointer(
                destructor.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
    }
    assert_eq!(
        crate::exception::catch_js_throw(|| context::finish_disposal(
            owner.get_nanbox_f64(),
            perry_abi::component_host::RELEASED_ERROR
        ))
        .unwrap_err(),
        88.0
    );
    assert_eq!(field(counter.get_nanbox_f64(), b"count"), 3.0);
    context::finish_disposal(owner.get_nanbox_f64(), perry_abi::component_host::OK);
    assert_eq!(field(counter.get_nanbox_f64(), b"count"), 3.0);
    assert!(
        crate::exception::catch_js_throw(|| payload::js_component_host_resource_create(
            owner.get_nanbox_f64(),
            ty.get_nanbox_f64(),
            1.0,
            undefined()
        ))
        .is_err()
    );
}
