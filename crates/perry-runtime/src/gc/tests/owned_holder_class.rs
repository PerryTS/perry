//! O1 separates collecting nonmoving holders from ordinary movable backing.
use super::super::*;
use super::support::*;

unsafe fn cell(collecting_holder: bool) -> usize {
    let p = if collecting_holder {
        crate::arena::arena_alloc_gc_collecting_holder(40, 8, GC_TYPE_STRING)
    } else {
        crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING)
    };
    std::ptr::write_bytes(p, 0, 40);
    p as usize
}

#[test]
fn collecting_holder_publication_blocks_emergency_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _publication = crate::gc::GcSuppressScope::new();
    assert!(!super::super::gc_try_emergency_reclaim());
}

#[test]
fn collecting_holder_blocks_are_excluded_and_holes_reuse_their_class() {
    check(false);
}

#[test]
#[ignore = "negative control: remove the old-block allocation class"]
fn negative_control_objtables_holder_class() {
    check(true);
}

fn check(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let _defrag = super::super::oldgen_defrag::OldDefragTestEnable::new();
    let holder = unsafe { cell(true) };
    let dead_holder = unsafe { cell(true) };
    let ordinary = unsafe { cell(false) };
    let dead_ordinary = unsafe { cell(false) };
    js_shadow_slot_set(0, ptr_bits(holder));
    js_shadow_slot_set(1, ptr_bits(ordinary));
    js_gc_collect();
    let holder_base = crate::arena::classify_heap_space_in_range(holder)
        .unwrap()
        .1;
    let ordinary_base = crate::arena::classify_heap_space_in_range(ordinary)
        .unwrap()
        .1;
    assert_ne!(holder_base, ordinary_base);
    if erase {
        crate::arena::OLD_ARENA.with(|arena| unsafe {
            let arena = &mut *arena.get();
            let block = arena
                .blocks
                .iter_mut()
                .find(|b| b.data as usize == holder_base)
                .unwrap();
            block.extent_kind = crate::arena::ArenaBlockKind::OldBlock;
        });
    }
    let movable = crate::arena::old_arena_movable_block_ranges();
    assert!(
        crate::arena::old_arena_block_range_index(&movable, holder).is_none(),
        "the collecting-holder class must exclude its whole block"
    );
    assert!(crate::arena::old_arena_block_range_index(&movable, ordinary).is_some());
    let selected = select_old_page_defrag_pages(true);
    assert!(!selected
        .pages
        .contains(&crate::arena::generation_page_for_addr(holder)));
    assert!(selected
        .pages
        .contains(&crate::arena::generation_page_for_addr(ordinary)));
    let reused_holder = unsafe { cell(true) };
    let reused_ordinary = unsafe { cell(false) };
    assert_eq!(
        reused_holder, dead_holder,
        "reuse stays in the collecting-holder class"
    );
    assert_eq!(
        reused_ordinary, dead_ordinary,
        "movable allocation cannot consume a holder hole"
    );
}
