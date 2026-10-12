//! Script source, location and offsets are owner-local private state.
use super::super::*;
use super::support::*;
#[test]
fn vm_script_metadata_follows_its_moved_owner() {
    check_script(false);
}
#[test]
#[ignore = "negative control: erase the moved Script's metadata record"]
fn negative_control_objtables_vm_script() {
    check_script(true);
}
fn check_script(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::object::js_object_alloc(0, 0);
    crate::node_vm::owned_state::seed_script_for_test(owner as usize);
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize;
    assert_ne!(moved, owner as usize);
    if erase {
        crate::node_vm::owned_state::clear_script_for_test(moved);
    }
    assert_eq!(
        crate::node_vm::owned_state::script_fields_for_test(moved),
        Some(("37 + 5".to_owned(), "owned-script.ts".to_owned(), -2, 3))
    );
    let fresh = crate::object::js_object_alloc(0, 0);
    assert!(crate::node_vm::owned_state::script_fields_for_test(fresh as usize).is_none());
}
