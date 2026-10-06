//! An ordinary getter site can refill after its weak receiver shape retires.
use super::*;
use crate::object::field_get_set::runtime_read_site::RuntimeReadSite;
use crate::object::method_site::read_holder::probe::Key;
use crate::object::ObjectHeader;

extern "C" fn getter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    panic!("probing an accessor must not invoke it")
}

extern "C" fn replacement(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    panic!("probing a replacement accessor must not invoke it")
}

#[test]
fn accessor_site_refills_after_weak_receiver_shape_retirement() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "accessor_site_refills_after_weak_receiver_shape_retirement",
    ) {
        return;
    }
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    // This fresh-process fixture suppresses lazy initialization and clears
    // the scanner registry. Restore production rooting before using handles,
    // inherited getters and the realm across actual moving collections.
    gc_init();
    let scope = RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(getter, 0),
        0,
    ));
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        crate::object::set_accessor_descriptor(
            p as usize,
            "expired".to_string(),
            crate::object::AccessorDescriptor {
                get: closure.with_const_ptr(|c: *const crate::closure::ClosureHeader| {
                    crate::value::js_nanbox_pointer(c as i64).to_bits()
                }),
                ..Default::default()
            },
        )
    });
    let site = RuntimeReadSite::new();
    for generation in 0..24 {
        let retired = {
            let temporary = RuntimeHandleScope::new();
            let receiver = temporary.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
            receiver.with_mut_ptr::<ObjectHeader, _>(|r| {
                proto.with_const_ptr::<ObjectHeader, _>(|p| {
                    crate::object::js_object_set_prototype_of(
                        crate::value::js_nanbox_pointer(r as i64),
                        crate::value::js_nanbox_pointer(p as i64),
                    )
                })
            });
            let name = format!("receiver_{generation}");
            let key = temporary.root_string_ptr(crate::string::js_string_from_bytes(
                name.as_ptr(),
                name.len() as u32,
            ));
            receiver.with_mut_ptr::<ObjectHeader, _>(|r| {
                key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(r, k, 1.0))
            });
            receiver.with_const_ptr::<ObjectHeader, _>(|r| unsafe {
                assert_eq!(
                    site.probe_getter_code(r, Key::Name(b"expired")),
                    Some(getter as *const () as usize),
                );
                assert!(
                    site.probe_leaf(r).is_some(),
                    "retired shapes must not latch the site"
                );
                crate::object::shapes::object_shape_stamp(r)
            })
        };
        gc_collect_minor();
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
        assert!(
            crate::object::shapes::shape_record_by_id(retired).is_none(),
            "the departed receiver's shape is a weak memo, not a root"
        );
    }
    let next = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(replacement, 0),
        0,
    ));
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        crate::object::set_accessor_descriptor(
            p as usize,
            "expired".to_string(),
            crate::object::AccessorDescriptor {
                get: next.with_const_ptr(|c: *const crate::closure::ClosureHeader| {
                    crate::value::js_nanbox_pointer(c as i64).to_bits()
                }),
                ..Default::default()
            },
        )
    });
    let receiver = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    receiver.with_mut_ptr::<ObjectHeader, _>(|r| {
        proto.with_const_ptr::<ObjectHeader, _>(|p| {
            crate::object::js_object_set_prototype_of(
                crate::value::js_nanbox_pointer(r as i64),
                crate::value::js_nanbox_pointer(p as i64),
            )
        })
    });
    receiver.with_const_ptr::<ObjectHeader, _>(|r| unsafe {
        assert_eq!(
            site.probe_getter_code(r, Key::Name(b"expired")),
            Some(replacement as *const () as usize),
            "an expired receiver memo still rechecks the holder and getter pair",
        );
    });
}
