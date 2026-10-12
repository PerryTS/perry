//! The snapshot closure owns its stores and native tokens, without a registry.
use super::super::*;
use super::support::*;

#[test]
fn als_snapshot_store_moves_and_tokens_die_with_the_closure_edge() {
    check_owned_snapshot(false);
}
#[test]
#[ignore = "negative control: clear the snapshot closure's holder capture"]
fn negative_control_objtables_als_snapshot() {
    check_owned_snapshot(true);
}
fn check_owned_snapshot(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = crate::object::js_object_alloc(0, 0);
    let snapshot = crate::async_context::test_snapshot_with_store(f64::from_bits(
        crate::value::POINTER_TAG | child as u64,
    ));
    let token_alive = crate::async_context::test_snapshot_token_lifetime(&snapshot);
    let owner = crate::async_hooks::context_snapshots::snapshot_closure(snapshot);
    let holder = crate::closure::js_closure_get_capture_bits(owner, 0);
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    let _ = gc_collect_minor();
    let moved =
        (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as *mut crate::closure::ClosureHeader;
    assert_ne!(moved, owner);
    // The holder is a young, movable object: it moves with its closure owner.
    assert!(crate::arena::pointer_in_nursery(
        (holder & crate::value::POINTER_MASK) as usize
    ));
    assert_ne!(
        crate::closure::js_closure_get_capture_bits(moved, 0),
        holder
    );
    if erase {
        crate::closure::js_closure_set_capture_bits(moved, 0, crate::value::TAG_UNDEFINED);
    }
    let child_after = crate::async_context::test_snapshot_first_store(
        &crate::async_hooks::context_snapshots::snapshot_for_closure(moved),
    )
    .unwrap();
    assert_ne!(
        child_after.to_bits() & crate::value::POINTER_MASK,
        child as u64
    );
    assert!(token_alive());
    crate::closure::js_closure_set_capture_bits(moved, 0, crate::value::TAG_UNDEFINED);
    super::dead_owner_side_tables::full_gc_with_no_block_persistence();
    assert!(
        !token_alive(),
        "a detached snapshot holder must release its native tokens"
    );
    let empty = crate::async_hooks::context_snapshots::snapshot_closure(Default::default());
    assert!(crate::async_hooks::context_snapshots::snapshot_for_closure(empty).is_empty());
}
