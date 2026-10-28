use super::*;
use crate::{component, value::js_nanbox_pointer};

extern "C-unwind" fn getter(
    closure: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let throws = crate::closure::js_closure_get_capture_f64(closure, 1) == 1.0;
    crate::gc::gc_collect_minor();
    if throws {
        crate::exception::js_throw(value.get_nanbox_f64());
    }
    value.get_nanbox_f64()
}

fn accessor(object: f64, key: &[u8], value: f64, throws: bool) {
    let scope = RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let value = scope.root_nanbox_f64(value);
    let function = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(unwind getter, 0),
        2,
    ));
    crate::closure::js_closure_set_capture_f64(
        function.get_raw_mut_ptr(),
        0,
        value.get_nanbox_f64(),
    );
    crate::closure::js_closure_set_capture_f64(
        function.get_raw_mut_ptr(),
        1,
        if throws { 1.0 } else { 0.0 },
    );
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        key.as_ptr(),
        key.len() as u32,
    ));
    crate::object::js_object_define_getter(
        object.get_nanbox_f64(),
        crate::value::js_nanbox_string(
            key.get_raw_const_ptr::<crate::string::StringHeader>() as i64
        ),
        js_nanbox_pointer(function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64),
    );
}

#[test]
fn typed_snapshots_ignore_unselected_getters_and_preserve_moving_getter_errors() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64
    ));
    let plan = scope.root_nanbox_f64(retention::new_plan());
    accessor(object.get_nanbox_f64(), b"wanted", 41.0, false);
    accessor(object.get_nanbox_f64(), b"ignored", 404.0, true);
    let args = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(1));
    crate::array::js_array_set_f64(args.get_raw_mut_ptr(), 0, object.get_nanbox_f64());
    let world = serde_json::json!({"types":{"r":{"ty":{"kind":"record","value":[{"name":"wanted","ty":{"kind":"u32"}}]}}}});
    let ty = serde_json::json!({"kind":"ref","value":"r"});
    let params = vec![serde_json::json!({"name":"item","ty":ty})];
    let before = object.get_nanbox_f64().to_bits();
    assert_eq!(
        arguments(
            js_nanbox_pointer(args.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64),
            &params,
            &world,
            component::undefined(),
            plan.get_nanbox_f64()
        ),
        serde_json::json!([{"wanted":41}])
    );
    assert_ne!(
        object.get_nanbox_f64().to_bits(),
        before,
        "the selected getter must actually collect and move its receiver"
    );
    assert_eq!(
        crate::exception::catch_js_throw(|| super::super::encode_for_retained(
            object.get_nanbox_f64(),
            component::undefined(),
            plan.get_nanbox_f64()
        ))
        .unwrap_err(),
        404.0,
        "the unselected getter must be live in the fixture"
    );
    // A payload-free option and a void function result must not inspect values
    // that their WIT contracts do not consume.
    let none = scope.root_nanbox_f64(js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64
    ));
    let tag = scope.root_string_ptr(crate::string::js_string_from_bytes(b"none".as_ptr(), 4));
    component::set_field(
        none.get_nanbox_f64(),
        b"tag",
        crate::value::js_nanbox_string(
            tag.get_raw_const_ptr::<crate::string::StringHeader>() as i64
        ),
    );
    accessor(none.get_nanbox_f64(), b"value", 404.0, true);
    assert_eq!(
        result(
            none.get_nanbox_f64(),
            &serde_json::json!({"kind":"option","value":{"kind":"u32"}}),
            &world,
            component::undefined(),
            plan.get_nanbox_f64()
        ),
        serde_json::json!({"tag":"none"})
    );
    assert_eq!(
        result(
            object.get_nanbox_f64(),
            &Value::Null,
            &world,
            component::undefined(),
            plan.get_nanbox_f64()
        ),
        Value::Null
    );
    let error = scope.root_nanbox_f64(js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64
    ));
    accessor(
        object.get_nanbox_f64(),
        b"wanted",
        error.get_nanbox_f64(),
        true,
    );
    let before_error = error.get_nanbox_f64().to_bits();
    let thrown = crate::exception::catch_js_throw(|| {
        result(
            object.get_nanbox_f64(),
            &ty,
            &world,
            component::undefined(),
            plan.get_nanbox_f64(),
        )
    })
    .unwrap_err();
    assert_ne!(
        error.get_nanbox_f64().to_bits(),
        before_error,
        "the thrown object must actually move"
    );
    assert_eq!(
        thrown.to_bits(),
        error.get_nanbox_f64().to_bits(),
        "selected getter errors retain their original identity"
    );
}
