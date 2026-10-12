//! Symbol properties are ordinary traced shape slots, including private keys.
use super::super::*;
use super::support::*;

#[test]
fn event_symbol_properties_survive_a_moving_collection() {
    check_event_symbols(false);
}
#[test]
#[ignore = "negative control: omit the event symbol cache root scanner"]
fn negative_control_objtables_event_symbol_cache() {
    check_event_symbols(true);
}
fn check_event_symbols(omit_roots: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    if !omit_roots {
        gc_register_mutable_root_scanner(crate::event_target::state::scan_roots);
    }
    unsafe {
        let owner = crate::event_target::js_event_target_new();
        js_shadow_slot_set(0, ptr_bits(owner as usize));
        let (child, _) = alloc_nursery_test_object(0);
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
        crate::event_target::state::set_slot(
            owner,
            crate::event_target::state::LISTENERS,
            f64::from_bits(ptr_bits(child as usize)),
        );
        let keys = crate::object::object_keys(owner);
        assert_eq!(
            keys.count(),
            4,
            "EventTarget state must be ordinary own symbol properties"
        );
        assert_eq!(
            crate::object::object_live_slot_count(owner),
            crate::object::INLINE_SLOT_FLOOR as u32
        );
        // Accessing existing symbol state must not allocate managed strings
        // or another object. Distinct numbers exercise both loads and stores.
        let allocated = crate::arena::arena_live_allocated_bytes();
        for value in 0..128 {
            crate::event_target::state::set_slot(
                owner,
                crate::event_target::state::MAX_LISTENERS,
                value as f64,
            );
            assert_eq!(
                crate::event_target::state::get_slot(
                    owner,
                    crate::event_target::state::MAX_LISTENERS
                ),
                value as f64
            );
        }
        assert_eq!(crate::arena::arena_live_allocated_bytes(), allocated);
        let before =
            crate::event_target::state::get_slot(owner, crate::event_target::state::LISTENERS)
                .to_bits();
        let _ = gc_collect_minor();
        let after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
        assert_ne!(after, owner, "premise: the owner must move");
        let child_after =
            crate::event_target::state::get_slot(after, crate::event_target::state::LISTENERS)
                .to_bits();
        assert_ne!(
            child_after, before,
            "the symbol slot must mark and rewrite its child"
        );
        assert!(crate::value::addr_class::try_read_tracked_gc_header(
            (child_after & POINTER_MASK) as usize
        )
        .is_some());
        js_shadow_slot_set(0, 0);
    }
}

#[test]
fn promoted_event_symbol_property_barrier_keeps_a_new_child() {
    let _guard = CopyingNurseryTestGuard::new(1);
    gc_register_mutable_root_scanner(crate::event_target::state::scan_roots);
    let _tenuring = tenuring::set_survivals_for_test(tenuring::GC_TENURING_SURVIVALS_MAX);
    let mut owner = crate::event_target::js_event_target_new();
    js_shadow_slot_set(0, ptr_bits(owner as usize));
    for _ in 0..tenuring::GC_TENURING_SURVIVALS_MAX {
        let _ = gc_collect_minor();
        owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::ObjectHeader;
    }
    assert!(crate::arena::pointer_in_old_gen(owner as usize));
    unsafe {
        let (child, _) = alloc_nursery_test_object(0);
        let slots = (owner as *const u8).add(std::mem::size_of::<crate::ObjectHeader>());
        let page = crate::arena::generation_page_for_addr(slots as usize);
        crate::arena::old_page_clear_dirty(page);
        assert!(
            !old_page_dirty_for(page),
            "premise: no prior store may cover this slot"
        );
        crate::event_target::state::set_slot(
            owner,
            crate::event_target::state::LISTENERS,
            f64::from_bits(ptr_bits(child as usize)),
        );
        assert!(
            old_page_dirty_for(page),
            "symbol-slot stores must dirty their page"
        );
        let _ = gc_collect_minor();
        let after =
            crate::event_target::state::get_slot(owner, crate::event_target::state::LISTENERS)
                .to_bits()
                & POINTER_MASK;
        assert_ne!(
            after as usize, child as usize,
            "the child must move through the remembered edge"
        );
        assert!(crate::value::addr_class::try_read_tracked_gc_header(after as usize).is_some());
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}
