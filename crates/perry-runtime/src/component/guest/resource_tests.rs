use super::*;

#[path = "resource_cleanup_tests.rs"]
mod cleanup_tests;
#[path = "resource_loan_tests.rs"]
mod loan_tests;

struct Registry<'a> {
    global: crate::gc::RuntimeHandle<'a>,
    previous: crate::gc::RuntimeHandle<'a>,
}
impl<'a> Registry<'a> {
    fn new(scope: &'a RuntimeHandleScope) -> Self {
        let global = scope.root_nanbox_f64(crate::object::js_get_global_this());
        let previous = scope.root_nanbox_f64(super::super::maybe_registry());
        let registry = scope.root_nanbox_f64(context::publish(serde_json::json!({}), undefined()));
        unsafe {
            bag::store(
                object_pointer(global.get_nanbox_f64()).unwrap() as usize,
                bag::COMPONENT_GUEST,
                registry.get_nanbox_f64(),
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
fn owned(frame: &mut Frame, ty: u32, handle: u32) -> u32 {
    let value = wrap(ty, handle, 0, true, undefined(), undefined());
    frame.add(value)
}

#[test]
fn duplicate_transfer_fails_without_consuming_the_first_owner() {
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let resource = scope.root_nanbox_f64(undefined());
    let mut frame = Frame::allocate_guest_frame();
    let slot = owned(&mut frame, 4, 11);
    assert_eq!(frame.guard(|f| lower(f, slot, 4, true, false)), 11);
    // A second reference can come from a different argument or nested list.
    let alias = frame.add(frame.get(slot));
    frame.guard(|f| lower(f, alias, 4, true, false));
    assert!(frame.error().is_some());
    frame.guard(commit);
    assert_eq!(get(data(frame.get(slot)), STATE), OWNED);
    // Abandoning the failed call does not consume ownership either.
    resource.set_nanbox_f64(frame.get(slot));
    drop(frame);
    let mut retry = Frame::allocate_guest_frame();
    let slot = retry.add(resource.get_nanbox_f64());
    assert_eq!(retry.guard(|f| lower(f, slot, 4, true, false)), 11);
    retry.guard(commit);
    assert!(retry.error().is_none());
    assert_eq!(get(data(retry.get(slot)), STATE), TRANSFERRED);
    retry.guard(|f| lower(f, slot, 4, true, false));
    assert!(retry.error().is_some());
}

#[test]
fn own_and_borrow_aliases_are_rejected_in_either_argument_order() {
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    for own_first in [false, true] {
        let mut frame = Frame::allocate_guest_frame();
        let slot = owned(&mut frame, 4, 7);
        let borrowed = wrap(4, 7, 0, false, undefined(), frame.get(slot));
        let borrowed = frame.add(borrowed);
        if own_first {
            frame.guard(|f| lower(f, slot, 4, true, false));
            frame.guard(|f| lower(f, borrowed, 4, false, false));
        } else {
            frame.guard(|f| lower(f, borrowed, 4, false, false));
            frame.guard(|f| lower(f, slot, 4, true, false));
        }
        assert!(frame.error().is_some());
        assert_eq!(get(data(frame.get(slot)), STATE), OWNED);
    }
}

#[test]
fn escaped_borrow_expires_and_owned_lift_survives_frame_exit() {
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let borrowed = scope.root_nanbox_f64(undefined());
    let owned = scope.root_nanbox_f64(undefined());
    let mut frame = Frame::allocate_guest_frame();
    let lifetime = frame.lifetime();
    borrowed.set_nanbox_f64(wrap(4, 7, 0, false, lifetime, undefined()));
    owned.set_nanbox_f64(wrap(4, 8, 0, true, undefined(), undefined()));
    live(borrowed.get_nanbox_f64());
    drop(frame);
    assert!(crate::exception::catch_js_throw(|| live(borrowed.get_nanbox_f64())).is_err());
    assert_eq!(get(live(owned.get_nanbox_f64()), HANDLE), 8.0);
}

#[test]
fn all_transfers_are_revalidated_before_any_owner_is_consumed() {
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let mut frame = Frame::allocate_guest_frame();
    let a = owned(&mut frame, 4, 7);
    let b = owned(&mut frame, 4, 8);
    frame.guard(|f| lower(f, a, 4, true, false));
    frame.guard(|f| lower(f, b, 4, true, false));
    // Model a later argument getter disposing an earlier staged resource.
    set(data(frame.get(b)), STATE, DISPOSED);
    frame.guard(commit);
    assert!(frame.error().is_some());
    assert_eq!(get(data(frame.get(a)), STATE), OWNED);
}

#[test]
fn private_capabilities_reject_forgery_wrong_types_and_cross_instance_use() {
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let mut frame = Frame::allocate_guest_frame();
    let slot = owned(&mut frame, 4, 7);
    frame.guard(|f| lower(f, slot, 5, true, false));
    assert!(frame.error().is_some());
    assert_eq!(get(data(frame.get(slot)), STATE), OWNED);
    let forged = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set_field(forged.get_nanbox_f64(), b"handle", 7.0);
    set_field(forged.get_nanbox_f64(), b"type", 4.0);
    assert!(crate::exception::catch_js_throw(|| live(forged.get_nanbox_f64())).is_err());
    let _other = Registry::new(&scope);
    assert!(crate::exception::catch_js_throw(|| live(frame.get(slot))).is_err());
}

#[test]
fn staged_resource_owners_and_borrows_survive_actual_moving_gc() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    let mut frame = Frame::allocate_guest_frame();
    let slot = owned(&mut frame, 4, 7);
    frame.guard(|f| lower(f, slot, 4, true, false));
    let before = frame.get(slot).to_bits();
    crate::gc::gc_collect_minor();
    assert_ne!(
        before,
        frame.get(slot).to_bits(),
        "resource wrapper must move"
    );
    frame.guard(commit);
    assert!(frame.error().is_none());
    assert_eq!(get(data(frame.get(slot)), STATE), TRANSFERRED);
}

per_test_global! {
    static DROPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
}
extern "C-unwind" fn payload_drop(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    payload: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    crate::gc::gc_collect_minor();
    assert_eq!(
        string(field(payload.get_nanbox_f64(), b"label")),
        "rooted payload 🌍"
    );
    DROPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    undefined()
}
extern "C-unwind" fn payload_drop_throw(
    _: *const crate::closure::ClosureHeader,
    _: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    crate::gc::gc_collect_minor();
    DROPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    crate::exception::js_throw(value.get_nanbox_f64())
}

unsafe extern "C" fn fake_resource(
    pointer: *mut c_void,
    ty: u32,
    operation: u32,
    value: u32,
) -> u32 {
    if operation == 0 {
        return value + 1000;
    }
    payload::perry_component_frame_resource_destroy(pointer, ty, value - 1000);
    0
}
unsafe extern "C" fn no_import(_: *mut c_void, _: u32, _: u32, _: u32) -> u32 {
    0
}

#[test]
fn exported_payload_survives_transfer_and_gc_then_is_destroyed_exactly_once() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuate = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::gc_init();
    let scope = RuntimeHandleScope::new();
    let _registry = Registry::new(&scope);
    unsafe {
        (*context::get(super::super::maybe_registry())).world = serde_json::json!({
            "imports": [], "exports": [{"resources":{"item":"e4"}}]
        });
    }
    let ops = perry_abi::component_guest::ComponentGuestOps {
        abi_version: perry_abi::component_guest::GUEST_ABI_VERSION,
        size: std::mem::size_of::<perry_abi::component_guest::ComponentGuestOps>(),
        contract: b"{}".as_ptr(),
        contract_len: 2,
        import: no_import,
        resource: fake_resource,
    };
    struct Restore(*mut perry_abi::component_guest::ComponentGuestOps);
    impl Drop for Restore {
        fn drop(&mut self) {
            super::super::GUEST_OPS.store(self.0, std::sync::atomic::Ordering::Release);
        }
    }
    let _restore = Restore(super::super::GUEST_OPS.swap(
        (&ops as *const perry_abi::component_guest::ComponentGuestOps).cast_mut(),
        std::sync::atomic::Ordering::AcqRel,
    ));
    DROPS.store(0, std::sync::atomic::Ordering::SeqCst);
    let object = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let label = "rooted payload 🌍";
    let label = scope.root_nanbox_f64(crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(label.as_ptr(), label.len() as u32) as i64,
    ));
    set_field(object.get_nanbox_f64(), b"label", label.get_nanbox_f64());
    let callback = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(unwind payload_drop, 1), 0) as i64,
    ));
    let resource = scope.root_nanbox_f64(payload::js_component_resource_create(
        9.0,
        object.get_nanbox_f64(),
        callback.get_nanbox_f64(),
    ));
    let fields = data(resource.get_nanbox_f64());
    let rep = get(fields, REP) as u32;
    let handle = get(fields, HANDLE) as u32;
    let before = object.get_nanbox_f64().to_bits();
    object.set_nanbox_f64(undefined());
    label.set_nanbox_f64(undefined());
    let mut frame = Frame::allocate_guest_frame();
    let slot = frame.add(resource.get_nanbox_f64());
    frame.guard(|f| lower(f, slot, 9, true, true));
    frame.guard(commit);
    assert!(frame.error().is_none());
    assert!(
        crate::exception::catch_js_throw(|| payload::js_component_resource_value(
            9.0,
            resource.get_nanbox_f64()
        ))
        .is_err()
    );
    // A fresh borrowed wrapper models the canonical host calling a guest method.
    let borrowed = unsafe {
        perry_component_frame_resource_lift((&mut frame as *mut Frame).cast(), 9, 0, rep, 0)
    };
    crate::gc::gc_collect_minor();
    let value = payload::js_component_resource_value(9.0, frame.get(borrowed));
    assert_ne!(
        value.to_bits(),
        before,
        "exported payload must actually move"
    );
    assert_eq!(string(field(value, b"label")), "rooted payload 🌍");
    drop(frame);
    let mut frame = Frame::allocate_guest_frame();
    unsafe {
        fake_resource((&mut frame as *mut Frame).cast(), 9, 1, handle);
    }
    assert!(frame.error().is_none());
    assert_eq!(DROPS.load(std::sync::atomic::Ordering::SeqCst), 1);
    unsafe {
        fake_resource((&mut frame as *mut Frame).cast(), 9, 1, handle);
    }
    assert!(frame.error().is_some());
    assert_eq!(DROPS.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(frame);

    // A throwing destructor releases its global payload root exactly once and
    // preserves the original (moving) heap exception through the C frame.
    object.set_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc_with_length(2) as i64,
    ));
    callback.set_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(unwind payload_drop_throw, 1), 0) as i64,
    ));
    resource.set_nanbox_f64(payload::js_component_resource_create(
        9.0,
        object.get_nanbox_f64(),
        callback.get_nanbox_f64(),
    ));
    let handle = get(data(resource.get_nanbox_f64()), HANDLE) as u32;
    let mut frame = Frame::allocate_guest_frame();
    let slot = frame.add(resource.get_nanbox_f64());
    frame.guard(|f| lower(f, slot, 9, true, true));
    frame.guard(commit);
    unsafe {
        fake_resource((&mut frame as *mut Frame).cast(), 9, 1, handle);
    }
    assert_eq!(
        frame.error().unwrap().to_bits(),
        object.get_nanbox_f64().to_bits()
    );
    assert_eq!(DROPS.load(std::sync::atomic::Ordering::SeqCst), 2);
    drop(frame);
    let mut retry = Frame::allocate_guest_frame();
    unsafe {
        fake_resource((&mut retry as *mut Frame).cast(), 9, 1, handle);
    }
    assert!(retry.error().is_some());
    assert_eq!(
        DROPS.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "throwing destructor must not run twice"
    );
}
