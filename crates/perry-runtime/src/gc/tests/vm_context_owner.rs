//! VM context children follow the sandbox owner rather than global roots.
use super::super::*;
use super::support::*;
#[test]
fn vm_context_children_follow_the_moved_sandbox() {
    check_context();
}
fn check_context() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::object::js_object_alloc(0, 0);
    let child = crate::object::js_object_alloc(0, 0);
    crate::node_vm::owned_state::seed_context_for_test(
        owner as usize,
        crate::value::POINTER_TAG | child as u64,
    );
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize;
    assert_ne!(moved, owner as usize);
    let (returned, child_after, strings, wasm, microtasks) =
        crate::node_vm::owned_state::context_fields_for_test(moved).unwrap();
    assert_eq!(returned, crate::value::POINTER_TAG | moved as u64);
    assert_ne!(child_after & crate::value::POINTER_MASK, child as u64);
    assert_eq!((strings, wasm, microtasks), (false, true, true));
    let fresh = crate::object::js_object_alloc(0, 0);
    assert!(crate::node_vm::owned_state::context_fields_for_test(fresh as usize).is_none());
}
