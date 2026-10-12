//! Set storage follows ordinary heap edges, including moving owners and death.
use super::super::*;
use super::support::*;

#[test]
fn set_owned_store_retains_values_after_moving_owner() {
    owned_edge(false);
}

#[test]
#[ignore = "negative control: erase the Set's sole owning store edge"]
fn negative_control_objtables_set_registry() {
    owned_edge(true);
}

fn owned_edge(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(3);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::weakref::test_support::clear_weak_holders();
    let set = crate::set::js_set_alloc(4);
    let value = unsafe { alloc_old_test_object(0).0 };
    crate::set::js_set_add(set, f64::from_bits(ptr_bits(value as usize)));
    let weak = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(value as usize)));
    js_shadow_slot_set(0, ptr_bits(set as usize));
    js_shadow_slot_set(1, ptr_bits(weak as usize));
    let store = unsafe { crate::set::test_set_store_word(set) };
    assert!(
        crate::arena::pointer_in_nursery(store),
        "stores are born young"
    );
    assert!(gc_type_is_movable(GC_TYPE_SET_STORE));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    assert_ne!(moved, set);
    assert_ne!(
        unsafe { crate::set::test_set_store_word(moved) },
        store,
        "the store moves too"
    );
    assert!(crate::set::is_registered_set(moved as usize));
    if erase {
        // GC_STORE_AUDIT(TEST): intentionally delete the edge for this control.
        unsafe {
            std::ptr::write((moved as *mut u8).add(32).cast::<usize>(), 0);
        }
    }
    js_gc_collect();
    let retained = crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1)));
    assert_ne!(
        retained.to_bits(),
        crate::value::TAG_UNDEFINED,
        "only Set-to-store-to-value edges retain this weak target"
    );
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::set::SetHeader;
    assert_eq!(crate::set::js_set_has(moved, retained), 1);
    let fresh = crate::set::js_set_alloc(4);
    assert_eq!(crate::set::js_set_has(fresh, retained), 0);
    js_shadow_slot_set(0, 0);
    js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(1))).to_bits(),
        crate::value::TAG_UNDEFINED,
        "no registry retains a dead Set"
    );
}
