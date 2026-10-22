//! Async metadata follows its resource and existing native-provider lease.
use super::super::*;
use super::support::*;

#[test]
fn async_resource_state_follows_its_moving_owner_and_dies_with_it() {
    check(false);
}
#[test]
#[ignore = "negative control: remove the resource's private-state edge"]
fn negative_control_objtables_async_resource_state() {
    check(true);
}
fn check(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::async_hooks::reset_for_tests();
    crate::weakref::test_support::clear_weak_holders();
    let storage = crate::async_context::AsyncLocalStoragePayload::default();
    let token = storage.token();
    let store = crate::object::js_object_alloc(0, 1);
    {
        crate::object::js_object_set_field_f64(store, 0, 73.0);
    }
    crate::async_context::enter_with(token, crate::value::js_nanbox_pointer(store as i64));
    let owner = crate::object::js_object_alloc(0, 0);
    let ids = crate::async_hooks::init_resource(
        "owned-state",
        crate::value::js_nanbox_pointer(owner as i64),
        true,
    );
    let serial = crate::async_hooks::public_async_id(ids.async_id);
    assert!(serial > 1);
    crate::async_context::clear_store(token);
    let weak = crate::weakref::js_weakref_new(crate::value::js_nanbox_pointer(store as i64));
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    js_shadow_slot_set(1, ptr_bits(weak as usize));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, owner as usize);
    assert_eq!(crate::async_hooks::public_async_id(ids.async_id), serial);
    unsafe {
        if erase {
            crate::object::owned_state_bag::store(
                moved,
                crate::object::owned_state_bag::ASYNC_RESOURCE,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            );
        }
    }
    js_gc_collect();
    assert_ne!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_UNDEFINED,
        "only the resource's state edge retains its captured store"
    );
    js_shadow_slot_set(0, 0);
    js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert!(crate::async_hooks::resource_state::get(ids.async_id).is_none());
}

#[test]
fn scheduler_entry_owns_its_job_state() {
    check_scheduler(false);
}
#[test]
#[ignore = "negative control: the queue entry does not own its job's state token"]
fn negative_control_objtables_scheduler_entry_state() {
    check_scheduler(true);
}
/// A nextTick/queueMicrotask/timer job's state has one owner: its queue entry.
/// The resource holds no edge back, so a full collection while the job is
/// pending must keep the cell through the entry alone, and the cell dies with
/// the entry once the job is destroyed.
fn check_scheduler(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::async_hooks::reset_for_tests();
    crate::gc::gc_register_mutable_root_scanner_named(
        "test-queued-jobs",
        crate::builtins::scan_queued_microtask_roots_mut,
    );
    crate::async_hooks::test_set_hooks_active(true);
    let resource = crate::object::js_object_alloc(0, 0);
    let resource_value = crate::value::js_nanbox_pointer(resource as i64);
    let ids = crate::async_hooks::init_scheduled_resource("TickObject", resource_value, None);
    let serial = crate::async_hooks::public_async_id(ids.async_id);
    assert!(crate::async_hooks::resource_state::get(ids.async_id).is_some());
    assert_ne!(serial, ids.async_id, "a hooked job gets a state token");
    crate::builtins::test_seed_queued_job_owner(
        if erase { 0 } else { ids.async_id },
        resource_value,
    );
    js_gc_collect();
    js_gc_collect();
    assert!(
        crate::async_hooks::resource_state::get(ids.async_id).is_some(),
        "the pending job's queue entry must own its state"
    );
    assert_eq!(crate::async_hooks::public_async_id(ids.async_id), serial);
    // Run-and-destroy: the entry is gone and destroy forgot the payload.
    crate::builtins::test_clear_queued_jobs();
    crate::async_hooks::destroy_scheduled(ids.async_id);
    js_gc_collect();
    assert!(
        crate::async_hooks::resource_state::get(ids.async_id).is_none(),
        "the state dies with its last queue entry"
    );
    // Without hooks a job gets a bare serial and no cell at all.
    crate::async_hooks::test_set_hooks_active(false);
    let plain = crate::async_hooks::init_scheduled_resource("TickObject", resource_value, None);
    assert!(crate::async_hooks::resource_state::get(plain.async_id).is_none());
    assert_eq!(
        crate::async_hooks::public_async_id(plain.async_id),
        plain.async_id
    );
    crate::async_hooks::reset_for_tests();
}

#[test]
fn native_provider_state_is_leased_until_destroy() {
    check_native(false);
}
#[test]
#[ignore = "negative control: remove the native provider's existing pin"]
fn negative_control_objtables_native_async_resource_lease() {
    check_native(true);
}
fn check_native(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::async_hooks::reset_for_tests();
    let id =
        unsafe { crate::async_hooks::js_async_hooks_provider_init(b"native-state".as_ptr(), 12) };
    let serial = crate::async_hooks::public_async_id(id);
    if erase {
        unsafe {
            unpin_user_ptr(id as *mut u8);
        }
    }
    js_gc_collect();
    assert!(
        crate::async_hooks::resource_state::get(id).is_some(),
        "the native lease pin must retain the provider's state"
    );
    assert_eq!(crate::async_hooks::public_async_id(id), serial);
    crate::async_hooks::js_async_hooks_provider_destroy(id);
    js_gc_collect();
    assert!(crate::async_hooks::resource_state::get(id).is_none());
}

#[test]
fn native_async_context_without_resource_retains_its_captured_store() {
    let _guard = crate::gc::tests::support::CopyingNurseryTestGuard::new(1);
    let _triggers =
        crate::gc::tests::support::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = crate::gc::tests::support::ConservativeScanDisabledGuard::new();
    use crate::node_api_host::*;
    crate::node_api_host::reset_env_for_test();
    // The nursery fixture clears production scanners; restore this true C
    // environment owner for the test rather than relying on GC init's latch.
    crate::gc::gc_register_mutable_root_scanner_named(
        "test-node-api",
        crate::node_api_host::scan_node_api_roots_mut,
    );
    let env = current_env();
    crate::weakref::test_support::clear_weak_holders();
    let storage = crate::async_context::AsyncLocalStoragePayload::default();
    let token = storage.token();
    let store = crate::object::js_object_alloc(0, 0);
    crate::async_context::enter_with(token, crate::value::js_nanbox_pointer(store as i64));
    let mut name = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_create_string_utf8(env, c"native-context".as_ptr(), 14, &mut name) },
        NapiStatus::Ok
    );
    let mut context = std::ptr::null_mut();
    assert_eq!(
        unsafe { napi_async_init(env, std::ptr::null_mut(), name, &mut context) },
        NapiStatus::Ok
    );
    crate::async_context::clear_store(token);
    let weak = crate::weakref::js_weakref_new(crate::value::js_nanbox_pointer(store as i64));
    crate::gc::js_shadow_slot_set(0, crate::value::js_nanbox_pointer(weak as i64).to_bits());
    crate::gc::js_gc_collect();
    assert_ne!(
        crate::weakref::js_weakref_deref(f64::from_bits(crate::gc::js_shadow_slot_get(0)))
            .to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(unsafe { napi_async_destroy(env, context) }, NapiStatus::Ok);
    crate::gc::js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(crate::gc::js_shadow_slot_get(0)))
            .to_bits(),
        crate::value::TAG_UNDEFINED
    );
}
