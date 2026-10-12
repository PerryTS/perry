//! Promise-owned context holders trace stores without rooting the promise.
use super::super::*;
use super::support::*;

#[test]
fn promise_context_follows_owner_and_releases_its_native_tokens() {
    check_owned_context(false);
}

#[test]
#[ignore = "negative control: erase the moved Promise context edge"]
fn negative_control_objtables_promise_context() {
    check_owned_context(true);
}

fn check_owned_context(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let promise = crate::promise::js_promise_new();
    let child = crate::object::js_object_alloc(0, 0);
    let snapshot = crate::async_context::test_snapshot_with_store(f64::from_bits(
        crate::value::POINTER_TAG | child as u64,
    ));
    let token_alive = crate::async_context::test_snapshot_token_lifetime(&snapshot);
    crate::promise::set_promise_context_snapshot(promise, snapshot);
    let holder = unsafe { (*promise).callback_context };
    assert!(gc_type_is_movable(GC_TYPE_ASYNC_SNAPSHOT));
    assert!(crate::arena::pointer_in_nursery(holder as usize));
    js_shadow_slot_set(0, crate::value::POINTER_TAG | promise as u64);
    let _ = collect_minor_trace(GcTriggerKind::Direct);
    let moved =
        (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut crate::promise::Promise;
    assert_ne!(moved, promise);
    assert_ne!(
        unsafe { (*moved).callback_context },
        holder,
        "the young holder moves with its promise"
    );
    if erase {
        crate::promise::clear_promise_context(moved);
    }
    let child_after = crate::async_context::test_snapshot_first_store(
        &crate::promise::context_for_promise(moved),
    )
    .unwrap();
    assert_ne!(
        child_after.to_bits() & crate::value::POINTER_MASK,
        child as u64
    );
    assert!(unsafe {
        crate::value::addr_class::try_read_tracked_gc_header(
            (child_after.to_bits() & crate::value::POINTER_MASK) as usize,
        )
    }
    .is_some());
    assert!(token_alive());
    crate::promise::clear_promise_context(moved);
    super::dead_owner_side_tables::full_gc_with_no_block_persistence();
    assert!(
        !token_alive(),
        "a detached holder must release its native Arc during full GC"
    );
}
