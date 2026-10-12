//! Symbol descriptor attributes are shape facts, including after evacuation.
use super::super::*;
use super::support::*;

#[test]
fn symbol_attrs_move_with_the_owner_shape() {
    check_symbol_attrs(false);
}

#[test]
#[ignore = "negative control: erase the moved symbol's readonly attribute"]
fn negative_control_objtables_symbol_attrs() {
    check_symbol_attrs(true);
}

fn check_symbol_attrs(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let original = crate::object::js_object_alloc(0, 0) as usize;
    js_shadow_slot_set(0, crate::value::POINTER_TAG | original as u64);
    let symbol = unsafe { crate::symbol::js_symbol_new_empty() };
    js_shadow_slot_set(1, symbol.to_bits());
    let symbol_addr = (symbol.to_bits() & crate::value::POINTER_MASK) as usize;
    crate::symbol::store_object_symbol_property_root(original, symbol_addr, 29.0f64.to_bits());
    crate::symbol::set_symbol_property_attrs(
        original,
        symbol_addr,
        crate::object::PropertyAttrs::new(false, false, true),
    );
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize;
    let symbol = (js_shadow_slot_get(1) & crate::value::POINTER_MASK) as usize;
    assert_ne!(moved, original);
    if erase {
        crate::symbol::set_symbol_property_attrs(
            moved,
            symbol,
            crate::object::PropertyAttrs::new(true, true, true),
        );
    }
    let attrs = crate::symbol::get_symbol_property_attrs(moved, symbol).unwrap();
    assert!(!attrs.writable());
    assert!(!attrs.enumerable());
    assert!(attrs.configurable());
    let fresh = crate::object::js_object_alloc(0, 0) as usize;
    assert!(crate::symbol::get_symbol_property_attrs(fresh, symbol).is_none());
}
