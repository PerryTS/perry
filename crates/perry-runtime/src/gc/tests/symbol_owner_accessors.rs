//! Symbol accessor pairs are traced children of the owner's ordinary slots.
use super::super::*;
use super::support::*;

extern "C" fn getter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    37.0
}

#[test]
fn symbol_accessor_pair_follows_owner_during_copying() {
    check_owned_accessor(false);
}

#[test]
#[ignore = "negative control: erase the moved owner's symbol accessor pair"]
fn negative_control_objtables_symbol_accessor() {
    check_owned_accessor(true);
}

fn check_owned_accessor(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = crate::array::js_array_alloc(4) as usize;
    let symbol = unsafe { crate::symbol::js_symbol_new_empty() };
    let symbol_addr = (symbol.to_bits() & crate::value::POINTER_MASK) as usize;
    let get = crate::closure::js_closure_alloc(crate::fn_info!(getter, 0), 0);
    unsafe {
        crate::symbol::set_symbol_accessor_property(
            f64::from_bits(crate::value::POINTER_TAG | owner as u64),
            symbol,
            crate::value::POINTER_TAG | get as u64,
            0,
        );
    }
    let owner = crate::array::clean_arr_ptr_mut(owner as *mut crate::array::ArrayHeader) as usize;
    js_shadow_slot_set(0, crate::value::POINTER_TAG | owner as u64);
    js_shadow_slot_set(1, symbol.to_bits());
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize;
    let symbol = (js_shadow_slot_get(1) & crate::value::POINTER_MASK) as usize;
    assert_ne!(moved, owner);
    if erase {
        unsafe {
            crate::object::shaped_symbols::delete(moved, symbol);
        }
    }
    let (get_after, set_after) =
        unsafe { crate::object::shaped_symbols::accessor(moved, symbol) }.unwrap();
    let lane = unsafe { crate::object::shaped_symbols::get(moved, symbol) }.unwrap();
    let pair = (lane & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader;
    assert_eq!(
        unsafe { (*pair).length } as usize,
        crate::object::accessor_pair::PAIR_LEN
    );
    let shared = unsafe { crate::object::accessor_pair::pair_of_value(lane) }.unwrap();
    assert_eq!((shared.get, shared.set), (get_after, set_after));
    assert_eq!(
        (
            shared.raw_get,
            shared.raw_set,
            shared.static_get,
            shared.static_set
        ),
        (0, 0, 0, 0)
    );
    assert_ne!(get_after & crate::value::POINTER_MASK, get as u64);
    assert_eq!(set_after, 0);
    assert_eq!(
        crate::closure::js_closure_call0(
            (get_after & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader,
            crate::closure::JsThis::from_f64(f64::from_bits(js_shadow_slot_get(0))),
        ),
        37.0
    );
    let fresh = crate::object::js_object_alloc(0, 0) as usize;
    assert!(unsafe { crate::object::shaped_symbols::accessor(fresh, symbol) }.is_none());
    assert_ne!(symbol_addr, 0);
}
