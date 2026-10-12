use super::support::*;
use crate::object::owned_state_bag::*;
#[test]
fn a_wasm_binding_preserves_existing_private_facts_when_the_bag_grows() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let owner = crate::object::js_object_alloc(0, 0);
    let value = crate::object::js_object_alloc(0, 1);
    unsafe {
        crate::object::js_object_set_field_f64(value, 0, 73.0);
        store(
            owner as usize,
            0,
            crate::value::js_nanbox_pointer(value as i64),
        );
    }
    let context = crate::wasm_instance_state::new(f64::from_bits(crate::value::TAG_UNDEFINED));
    crate::wasm_instance_state::attach(crate::value::js_nanbox_pointer(owner as i64), context);
    crate::gc::js_shadow_slot_set(0, crate::value::js_nanbox_pointer(owner as i64).to_bits());
    let _ = crate::gc::gc_collect_minor();
    let owner = (crate::gc::js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize;
    let context =
        crate::wasm_instance_state::from_owner(crate::value::js_nanbox_pointer(owner as i64));
    assert!(crate::wasm_instance_state::memory_span(context).is_some());
    unsafe {
        let value = (load(owner, 0).to_bits() & crate::value::POINTER_MASK)
            as *mut crate::object::ObjectHeader;
        assert_eq!(crate::object::js_object_get_field_f64(value, 0), 73.0);
    }
}
