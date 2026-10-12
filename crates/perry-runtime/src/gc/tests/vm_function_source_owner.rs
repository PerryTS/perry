//! Compiled-function source text follows a closure's own private bag.
use super::super::*;
use super::support::*;
extern "C" fn callback(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
    37.0
}
#[test]
fn vm_function_source_follows_the_moved_closure() {
    check_source(false);
}
#[test]
#[ignore = "negative control: erase the moved closure's private source slot"]
fn negative_control_objtables_vm_function_source() {
    check_source(true);
}
fn check_source(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::closure::js_closure_alloc(crate::fn_info!(callback, 0), 0);
    crate::node_vm::owned_state::set_function_source(owner as usize, "function () { return 37; }");
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize;
    assert_ne!(moved, owner as usize);
    if erase {
        crate::node_vm::owned_state::clear_function_source(moved);
    }
    assert_eq!(
        crate::node_vm::compiled_function_source_for_closure(moved).as_deref(),
        Some("function () { return 37; }")
    );
    let fresh = crate::closure::js_closure_alloc(crate::fn_info!(callback, 0), 0);
    assert!(crate::node_vm::compiled_function_source_for_closure(fresh as usize).is_none());
}
