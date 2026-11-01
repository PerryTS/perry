use super::super::frame::*;
use super::super::resources;
use super::*;

struct Registry<'a> {
    global: crate::gc::RuntimeHandle<'a>,
    previous: crate::gc::RuntimeHandle<'a>,
}
impl<'a> Registry<'a> {
    fn new(scope: &'a RuntimeHandleScope) -> Self {
        let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
        let previous = scope.root_nanbox_f64(super::super::maybe_registry());
        let owner = scope.root_nanbox_f64(context::publish(serde_json::json!({
            "types": {
                "t0":{"ty":{"kind":"future","value":{"kind":"u32"}}},
                "t1":{"ty":{"kind":"stream","value":{"kind":"u32"}}},
                "e2":{"ty":{"kind":"future","value":{"kind":"u32"}}},
                "t3":{"ty":{"kind":"future","value":null}},
                "t4":{"ty":{"kind":"future","value":{"kind":"own","value":"t8"}}},
                "t5":{"ty":{"kind":"future","value":{"kind":"own","value":"e8"}}},
                "t6":{"ty":{"kind":"ref","value":"t0"}},
                "t7":{"ty":{"kind":"future","value":{"kind":"stream","value":{"kind":"future","value":{"kind":"u32"}}}}}
            }
        }), undefined()));
        unsafe {
            bag::store(
                object_pointer(global.get_nanbox_f64()).unwrap() as usize,
                bag::COMPONENT_GUEST,
                owner.get_nanbox_f64(),
            );
        }
        Self { global, previous }
    }
}
impl Drop for Registry<'_> {
    fn drop(&mut self) {
        unsafe {
            bag::store(
                object_pointer(self.global.get_nanbox_f64()).unwrap() as usize,
                bag::COMPONENT_GUEST,
                self.previous.get_nanbox_f64(),
            );
        }
    }
}

fn method(wrapper: f64) -> f64 {
    unsafe {
        crate::closure::js_native_call_value(
            field(wrapper, b"dispose"),
            crate::closure::plain_call_receiver(),
            std::ptr::null(),
            0,
        )
    }
}
unsafe fn pop() -> Option<(u32, u32)> {
    let frame = perry_component_frame_new();
    let (mut ty, mut handle) = (u32::MAX, u32::MAX);
    let status = perry_component_frame_endpoint_pop(frame, &mut ty, &mut handle);
    assert_eq!(perry_component_frame_failed(frame), 0);
    perry_component_frame_drop(frame);
    match status {
        0 => {
            assert_eq!((ty, handle), (0, 0));
            None
        }
        1 => Some((ty, handle)),
        _ => panic!("unexpected cleanup status {status}"),
    }
}

#[test]
fn typed_endpoint_transfer_rejects_duplicates_and_survives_moving_gc() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    unsafe {
        let frame = perry_component_frame_new();
        let value = perry_component_frame_endpoint_lift(frame, 0, 91);
        assert_eq!(
            perry_component_frame_endpoint_lower(frame, value, 5),
            91,
            "structurally equal exported endpoint is compatible"
        );
        let before = super::frame(frame).get(value).to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(
            before,
            super::frame(frame).get(value).to_bits(),
            "endpoint must really move"
        );
        resources::perry_component_frame_resource_commit(frame);
        assert_eq!(perry_component_frame_failed(frame), 0);
        assert_eq!(
            get(data(super::frame(frame).get(value)), STATE),
            TRANSFERRED
        );
        assert_eq!(perry_component_frame_endpoint_lower(frame, value, 0), 0);
        assert_eq!(perry_component_frame_failed(frame), 1);
        perry_component_frame_drop(frame);

        let frame = perry_component_frame_new();
        let value = perry_component_frame_endpoint_lift(frame, 14, 92);
        assert_eq!(perry_component_frame_endpoint_lower(frame, value, 14), 92);
        let alias = perry_component_frame_copy(frame, frame, value);
        perry_component_frame_endpoint_lower(frame, alias, 14);
        resources::perry_component_frame_resource_commit(frame);
        assert_eq!(perry_component_frame_failed(frame), 1);
        assert_eq!(get(data(super::frame(frame).get(value)), STATE), OPEN);
        perry_component_frame_drop(frame);
        assert_eq!(
            pop(),
            None,
            "transfer itself must never queue a canonical drop"
        );
    }
}

#[test]
fn endpoint_types_keep_resource_identity_and_reject_forgery_and_other_instances() {
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    assert!(same_type(super::super::registry(), 0, 12).unwrap());
    assert!(!same_type(super::super::registry(), 0, 2).unwrap());
    assert!(!same_type(super::super::registry(), 0, 6).unwrap());
    assert!(
        !same_type(super::super::registry(), 8, 10).unwrap(),
        "resource payloads stay nominal"
    );
    assert!(schema(super::super::registry(), u32::MAX).is_err());
    let wrapper = scope.root_nanbox_f64(wrap(0, 0));
    set_field(wrapper.get_nanbox_f64(), b"type", 2.0);
    set_field(wrapper.get_nanbox_f64(), b"handle", 999.0);
    assert_eq!(get(live(wrapper.get_nanbox_f64()), HANDLE), 0.0);
    assert!(JSValue::from_bits(field(wrapper.get_nanbox_f64(), b"then").to_bits()).is_undefined());
    let forged = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(forged.get_nanbox_f64(), b"type", 0.0);
    set_field(forged.get_nanbox_f64(), b"handle", 0.0);
    assert!(crate::exception::catch_js_throw(|| live(forged.get_nanbox_f64())).is_err());
    let _other = Registry::new(&scope);
    assert!(crate::exception::catch_js_throw(|| live(wrapper.get_nanbox_f64())).is_err());
    assert!(crate::exception::catch_js_throw(|| method(wrapper.get_nanbox_f64())).is_err());
}

#[test]
fn resource_and_endpoint_custody_are_revalidated_before_either_is_consumed() {
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    unsafe {
        for invalidate_endpoint in [false, true] {
            let frame = perry_component_frame_new();
            let resource = resources::perry_component_frame_resource_lift(frame, 4, 17, 0, 1);
            let endpoint = perry_component_frame_endpoint_lift(frame, 0, 18);
            resources::perry_component_frame_resource_lower(frame, resource, 4, 1, 0);
            perry_component_frame_endpoint_lower(frame, endpoint, 0);
            assert_eq!(perry_component_frame_failed(frame), 0);
            if invalidate_endpoint {
                release(data(super::frame(frame).get(endpoint)));
            } else {
                // Another conversion commits this resource after a later
                // argument getter runs, invalidating the original batch.
                let other = perry_component_frame_new();
                let copy = perry_component_frame_copy(other, frame, resource);
                resources::perry_component_frame_resource_lower(other, copy, 4, 1, 0);
                resources::perry_component_frame_resource_commit(other);
                assert_eq!(perry_component_frame_failed(other), 0);
                perry_component_frame_drop(other);
            }
            resources::perry_component_frame_resource_commit(frame);
            assert_eq!(perry_component_frame_failed(frame), 1);
            if invalidate_endpoint {
                let retry = perry_component_frame_new();
                let copy = perry_component_frame_copy(retry, frame, resource);
                assert_eq!(
                    resources::perry_component_frame_resource_lower(retry, copy, 4, 1, 0),
                    17
                );
                assert_eq!(
                    perry_component_frame_failed(retry),
                    0,
                    "resource was not consumed"
                );
                perry_component_frame_drop(retry);
                assert_eq!(pop(), Some((0, 18)));
            } else {
                assert_eq!(get(live(super::frame(frame).get(endpoint)), STATE), OPEN);
            }
            perry_component_frame_drop(frame);
        }
    }
}

#[test]
fn collection_and_disposal_queue_once_without_closing_reused_handles() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let kept = scope.root_nanbox_f64(wrap(0, 0));
    let before = kept.get_nanbox_f64().to_bits();
    {
        wrap(2, 31); // Unobserved readable, collected by the actual weak pass.
        let temporary = RuntimeHandleScope::new();
        let explicit = temporary.root_nanbox_f64(wrap(6, 32));
        method(explicit.get_nanbox_f64());
        method(explicit.get_nanbox_f64());
        let mut frame = Frame::allocate_guest_frame();
        let transferred = frame.add(wrap(0, 33));
        frame.guard(|f| lower(f, transferred, 0));
        frame.guard(resources::commit);
        assert!(frame.error().is_none());
    }
    let reused = scope.root_nanbox_f64(wrap(0, 33));
    crate::gc::gc_collect_minor();
    assert_ne!(before, kept.get_nanbox_f64().to_bits());
    // The collector itself only schedules finalization; no C drop can run.
    unsafe {
        assert_eq!(pop(), Some((6, 32)));
        assert_eq!(pop(), None);
    }
    crate::promise::js_promise_run_microtasks();
    unsafe {
        assert_eq!(pop(), Some((2, 31)));
        assert_eq!(pop(), None);
    }
    assert_eq!(get(live(reused.get_nanbox_f64()), HANDLE), 33.0);
    method(kept.get_nanbox_f64());
    method(reused.get_nanbox_f64());
    unsafe {
        assert_eq!(pop(), Some((0, 0)), "zero is a valid canonical identity");
        assert_eq!(pop(), Some((0, 33)));
        assert_eq!(pop(), None);
    }
    crate::gc::gc_collect_minor();
    crate::promise::js_promise_run_microtasks();
    unsafe {
        assert_eq!(pop(), None);
    }
}

#[test]
fn cleanup_pointer_errors_preserve_the_queue_and_disposed_instances_do_not_schedule_drops() {
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let wrapper = scope.root_nanbox_f64(wrap(0, 73));
    method(wrapper.get_nanbox_f64());
    unsafe {
        for same in [false, true] {
            let frame = perry_component_frame_new();
            let mut output = 99;
            let handle = if same {
                &mut output as *mut u32
            } else {
                std::ptr::null_mut()
            };
            assert_eq!(
                perry_component_frame_endpoint_pop(frame, &mut output, handle),
                -1
            );
            assert_eq!(output, 0);
            perry_component_frame_drop(frame);
        }
        assert_eq!(pop(), Some((0, 73)));
        let abandoned = scope.root_nanbox_f64(wrap(0, 74));
        (*context::get(super::super::registry())).alive = false;
        release(data(abandoned.get_nanbox_f64()));
        assert_eq!(pop(), None);
        assert!(crate::exception::catch_js_throw(|| live(abandoned.get_nanbox_f64())).is_err());
    }
}
