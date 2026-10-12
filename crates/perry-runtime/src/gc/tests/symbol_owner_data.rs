//! Symbol data is an ordinary owner slot, never an independent root table.
use super::super::*;
use super::support::*;

#[test]
fn symbol_data_child_moves_with_exotic_owner() {
    check_owned_data(false);
}

#[test]
#[ignore = "negative control: erase the moved owner's symbol data slot"]
fn negative_control_objtables_symbol_data() {
    check_owned_data(true);
}

fn check_owned_data(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::promise::js_promise_new();
    let symbol = unsafe { crate::symbol::js_symbol_new_empty() };
    let symbol_addr = (symbol.to_bits() & crate::value::POINTER_MASK) as usize;
    let child = crate::object::js_object_alloc(0, 0);
    crate::symbol::store_object_symbol_property_root(
        owner as usize,
        symbol_addr,
        crate::value::POINTER_TAG | child as u64,
    );
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    js_shadow_slot_set(1, symbol.to_bits());
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize;
    let symbol = (js_shadow_slot_get(1) & crate::value::POINTER_MASK) as usize;
    assert_ne!(moved, owner as usize);
    if erase {
        unsafe {
            crate::object::shaped_symbols::delete(moved, symbol);
        }
    }
    let child_after = crate::symbol::symbol_property_root_bits(moved, symbol).unwrap()
        & crate::value::POINTER_MASK;
    assert_ne!(child_after, child as u64);
    assert!(
        unsafe { crate::value::addr_class::try_read_tracked_gc_header(child_after as usize) }
            .is_some()
    );
    let fresh = crate::promise::js_promise_new();
    assert!(crate::symbol::symbol_property_root_bits(fresh as usize, symbol).is_none());
}
