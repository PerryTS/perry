//! Native wrap/type-tag state follows the property holder's private edge.
use super::super::*;
use super::support::*;
#[test]
fn node_api_metadata_moves_with_its_owner() {
    check_metadata();
}
fn check_metadata() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::object::js_object_alloc(0, 0);
    crate::node_api_host::test_seed_owned_metadata(owner as usize);
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, owner as usize);
    assert_eq!(
        crate::node_api_host::test_owned_metadata(moved),
        Some((37, 11, 13))
    );
    let fresh = crate::object::js_object_alloc(0, 0);
    assert_eq!(
        crate::node_api_host::test_owned_metadata(fresh as usize),
        None
    );
}
