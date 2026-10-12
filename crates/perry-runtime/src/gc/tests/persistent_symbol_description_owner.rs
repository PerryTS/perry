//! Persistent symbols own immutable native text instead of an address map.
use super::super::*;
use super::support::*;
#[test]
fn persistent_symbol_owns_text_across_collections() {
    check_description(false);
}
#[test]
#[ignore = "negative control: erase a persistent symbol's native text"]
fn negative_control_objtables_persistent_symbol_description() {
    check_description(true);
}
fn check_description(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let key = crate::string::js_string_from_bytes(b"objtables-persistent-owner".as_ptr(), 26);
    let symbol =
        unsafe { crate::symbol::js_symbol_for(crate::value::js_nanbox_string(key as i64)) };
    let addr = (symbol.to_bits() & crate::value::POINTER_MASK) as usize;
    js_shadow_slot_set(0, symbol.to_bits());
    let _ = gc_collect_minor();
    assert_eq!(js_shadow_slot_get(0), symbol.to_bits());
    if erase {
        crate::symbol::test_clear_persistent_description(addr);
    }
    assert_eq!(
        crate::symbol::registered_symbol_description(addr).as_deref(),
        Some("objtables-persistent-owner")
    );
    let fresh = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) };
    assert!(crate::symbol::registered_symbol_description(fresh as usize).is_none());
    assert!(!crate::symbol::is_global_registered_symbol(
        crate::symbol::well_known_symbol("iterator") as usize
    ));
    assert!(crate::symbol::is_global_registered_symbol(addr));
}
