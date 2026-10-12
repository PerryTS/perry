//! The imports token names a collecting cell, never an address registry.
use super::super::*;
use super::support::*;
#[test]
fn wasm_imports_follow_moving_wrapper_and_die_with_it() {
    check(false);
}
#[test]
#[ignore = "negative control: erase the context cell's imports edge"]
fn negative_control_objtables_wasm_import_objects() {
    check(true);
}
fn check(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::weakref::test_support::clear_weak_holders();
    let owner = crate::object::js_object_alloc(0, 0);
    let imports = crate::object::js_object_alloc(0, 0);
    let imports_value = f64::from_bits(ptr_bits(imports as usize));
    let context = crate::wasm_instance_state::new(imports_value);
    crate::wasm_instance_state::attach(f64::from_bits(ptr_bits(owner as usize)), context);
    let weak = crate::weakref::js_weakref_new(imports_value);
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    js_shadow_slot_set(1, ptr_bits(weak as usize));
    let _ = gc_collect_minor();
    let moved = f64::from_bits(js_shadow_slot_get(0));
    assert_ne!(js_shadow_slot_get(0) & POINTER_MASK, owner as u64);
    let context = crate::wasm_instance_state::from_owner(moved);
    let token = crate::wasm_instance_state::token(context);
    let imports = crate::wasm_instance_state::imports(token);
    assert_ne!(imports.to_bits(), imports_value.to_bits());
    if erase {
        unsafe {
            crate::wasm_instance_state::clear_imports(context);
        }
    }
    js_gc_collect();
    let retained = crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1)));
    assert_eq!(
        retained.to_bits(),
        imports.to_bits(),
        "only the owner's context retains imports"
    );
    let fresh = crate::object::js_object_alloc(0, 0);
    assert_eq!(
        crate::wasm_instance_state::from_owner(f64::from_bits(ptr_bits(fresh as usize))).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    js_shadow_slot_set(0, 0);
    js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}
