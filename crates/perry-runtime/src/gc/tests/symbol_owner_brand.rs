//! Symbol brands follow the owned GC header without address registration.
use super::super::*;
use super::support::*;
#[test]
fn symbol_brand_moves_with_its_owner() {
    check_brand(false);
}
#[test]
#[ignore = "negative control: erase the moved Symbol GC kind"]
fn negative_control_objtables_symbol_brand() {
    check_brand(true);
}
fn check_brand(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let owner = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) };
    assert!(crate::symbol::is_registered_symbol(owner as usize));
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    let _ = gc_collect_minor();
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, owner as usize);
    if erase {
        unsafe {
            (*header_from_user_ptr(moved as *const u8)).obj_type = GC_TYPE_STRING;
        }
    }
    assert!(crate::symbol::is_registered_symbol(moved));
    assert!(!crate::symbol::is_registered_symbol(owner as usize));
    let ordinary = crate::object::js_object_alloc(0, 0);
    assert!(!crate::symbol::is_registered_symbol(ordinary as usize));
    assert!(!crate::symbol::is_registered_symbol(0x7000_0000_0000_0000));
    assert!(crate::symbol::is_registered_symbol(
        crate::symbol::well_known_symbol("iterator") as usize
    ));
}
#[test]
fn a_weak_symbol_has_no_address_registry_to_keep_it_alive() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    let owner = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) };
    let reference = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(owner as usize)));
    js_shadow_slot_set(0, ptr_bits(reference as usize));
    let _ = gc_collect_minor();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(0))).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

#[test]
fn symbol_brand_rejects_matching_bytes_inside_another_allocation() {
    check_interior_brand(false);
}

#[test]
#[ignore = "negative control: publish a false Symbol allocation start"]
fn negative_control_objtables_symbol_allocation_start() {
    check_interior_brand(true);
}

fn check_interior_brand(plant: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (owner, fields) = unsafe { alloc_nursery_test_object(8) };
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    let prefix = fields.cast::<GcHeader>();
    let fake = unsafe { fields.add(1).cast::<crate::symbol::SymbolHeader>() };
    unsafe {
        std::ptr::write(
            prefix,
            GcHeader {
                obj_type: GC_TYPE_SYMBOL,
                gc_flags: GC_FLAG_ARENA,
                _reserved: 0,
                size: (GC_HEADER_SIZE + std::mem::size_of::<crate::symbol::SymbolHeader>()) as u32,
            },
        );
        std::ptr::write(
            fake,
            crate::symbol::SymbolHeader {
                magic: crate::symbol::SYMBOL_MAGIC,
                registered: 0,
                description: std::ptr::null_mut(),
                id: 73,
            },
        );
    }
    if plant {
        crate::arena::record_arena_object_start(prefix as usize, GC_TYPE_SYMBOL);
    }
    assert!(
        !crate::symbol::is_registered_symbol(fake as usize),
        "payload bytes cannot create a Symbol allocation identity",
    );
}
