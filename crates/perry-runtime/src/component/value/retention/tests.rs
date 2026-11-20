use super::*;
use crate::component::{self, context, host_resource};

#[test]
fn retained_wrapper_identities_survive_sorted_encoding_growth_transfer_and_collection() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(context::publish(
        serde_json::json!({
            "imports":[{"resources":{"thing":"t0"}}]
        }),
        component::undefined(),
    ));
    let plan = scope.root_nanbox_f64(new_plan());
    let weak_z = scope.root_nanbox_f64(component::undefined());
    let weak_a = scope.root_nanbox_f64(component::undefined());
    let original_z;
    let descriptor;
    let event;
    {
        let temporary = RuntimeHandleScope::new();
        let ty = temporary.root_string_ptr(crate::string::js_string_from_bytes(b"t0".as_ptr(), 2));
        let ty = crate::value::js_nanbox_string(
            ty.get_raw_const_ptr::<crate::string::StringHeader>() as i64,
        );
        let z =
            temporary.root_nanbox_f64(host_resource::payload::js_component_host_resource_create(
                owner.get_nanbox_f64(),
                ty,
                41.0,
                component::undefined(),
            ));
        let a =
            temporary.root_nanbox_f64(host_resource::payload::js_component_host_resource_create(
                owner.get_nanbox_f64(),
                ty,
                42.0,
                component::undefined(),
            ));
        original_z = z.get_nanbox_f64().to_bits();
        weak_z.set_nanbox_f64(js_nanbox_pointer(
            crate::weakref::js_weakref_new(z.get_nanbox_f64()) as i64,
        ));
        weak_a.set_nanbox_f64(js_nanbox_pointer(
            crate::weakref::js_weakref_new(a.get_nanbox_f64()) as i64,
        ));
        let object = temporary.root_nanbox_f64(js_nanbox_pointer(crate::object::js_object_alloc(
            0, 0,
        ) as i64));
        component::set_field(object.get_nanbox_f64(), b"z", z.get_nanbox_f64());
        component::set_field(object.get_nanbox_f64(), b"a", a.get_nanbox_f64());
        let encoded = super::super::encode_for_retained(
            object.get_nanbox_f64(),
            owner.get_nanbox_f64(),
            plan.get_nanbox_f64(),
        );
        assert_eq!(
            encoded.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["a", "z"]
        );
        assert_eq!(len(plan.get_nanbox_f64()), 2);
        descriptor = identity(plan.get_nanbox_f64(), 0);
        assert_eq!(descriptor["$hostResource"], encoded["z"]["$hostResource"]);
        assert_eq!(
            identity(plan.get_nanbox_f64(), 1)["$hostResource"],
            encoded["a"]["$hostResource"]
        );
        assert_ne!(
            descriptor["$hostResource"], encoded["a"]["$hostResource"],
            "root order cannot be zipped with sorted JSON fields"
        );
        // Grow well beyond the initial private list capacity. The fixed holder
        // must publish each replacement header to its ordinary traced edge.
        for _ in 0..64 {
            retain(plan.get_nanbox_f64(), z.get_nanbox_f64(), &descriptor).unwrap();
        }
        let reference = serde_json::json!({"token":descriptor["$hostResource"], "type":descriptor["type"], "generation":descriptor["generation"]});
        event =
            serde_json::json!({"kind":"lower", "created":[reference], "transferred":[reference]});
        host_resource::payload::event(owner.get_nanbox_f64(), &event);
        assert!(
            crate::exception::catch_js_throw(|| host_resource::encode(
                z.get_nanbox_f64(),
                owner.get_nanbox_f64()
            ))
            .is_err(),
            "a transferred wrapper must no longer be re-encoded"
        );
    }
    crate::gc::gc_collect_minor();
    let moved_z = crate::array::js_array_get_f64(pair(plan.get_nanbox_f64(), 0), 0);
    assert_ne!(
        moved_z.to_bits(),
        original_z,
        "retained wrapper must actually move"
    );
    assert_eq!(len(plan.get_nanbox_f64()), 66);
    let mut wrong_generation = event.clone();
    wrong_generation["transferred"][0]["generation"] = serde_json::json!(99);
    assert!(transferred(plan.get_nanbox_f64(), &wrong_generation).is_empty());
    let slots = transferred(plan.get_nanbox_f64(), &event);
    assert_eq!(
        slots.len(),
        65,
        "all captured aliases must match their original identity"
    );
    for index in slots {
        release(plan.get_nanbox_f64(), index);
    }
    // The first evacuation may promote the wrapper. Reclaim both nursery
    // and old generations after its last retained alias is released.
    crate::gc::js_gc_collect();
    assert!(
        JSValue::from_bits(crate::weakref::js_weakref_deref(weak_z.get_nanbox_f64()).to_bits())
            .is_undefined(),
        "matching receipts must release all aliases of the transferred wrapper"
    );
    assert!(
        !JSValue::from_bits(crate::weakref::js_weakref_deref(weak_a.get_nanbox_f64()).to_bits())
            .is_undefined(),
        "a different owner's wrapper must remain retained"
    );
    assert_eq!(
        identity(plan.get_nanbox_f64(), 0),
        descriptor,
        "identity metadata survives release without re-encoding"
    );
    host_resource::payload::dispose_owner(owner.get_nanbox_f64());
}
