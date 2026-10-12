//! Map and Set stores are young, movable GC objects. A store that dies young
//! is reclaimed by the copying minor (its native index buffers included), a
//! live one moves with its header's payload alias re-derived, and an old
//! header that grows a fresh young store keeps every young value it holds.
use super::super::*;
use super::support::*;

fn map_from_slot(slot: u32) -> *mut crate::map::MapHeader {
    (js_shadow_slot_get(slot) & POINTER_MASK) as *mut crate::map::MapHeader
}

fn set_from_slot(slot: u32) -> *mut crate::set::SetHeader {
    (js_shadow_slot_get(slot) & POINTER_MASK) as *mut crate::set::SetHeader
}

#[test]
fn dead_young_map_and_set_stores_die_in_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let map = crate::map::js_map_alloc(8);
    crate::map::js_map_set(map, 1.0, 2.0);
    let set = crate::set::js_set_alloc(8);
    crate::set::js_set_add(set, 3.0);
    let map_store = unsafe { crate::map::test_map_store_word(map) };
    let set_store = unsafe { crate::set::test_set_store_word(set) };
    assert!(
        crate::arena::pointer_in_nursery(map_store),
        "Map store is born young"
    );
    assert!(
        crate::arena::pointer_in_nursery(set_store),
        "Set store is born young"
    );

    let maps_before = crate::map::test_map_side_deallocation_snapshot();
    let sets_before = crate::set::test_set_side_deallocation_snapshot();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let maps_after = crate::map::test_map_side_deallocation_snapshot();
    let sets_after = crate::set::test_set_side_deallocation_snapshot();

    // Sabotage: allocate the stores old again, or drop the from-space store
    // walk from `finalize_dead_copied_minor_from_space_side_allocations`, and
    // neither count moves.
    assert_eq!(maps_after.0 - maps_before.0, 1, "dead Map store finalized");
    assert_eq!(sets_after.0 - sets_before.0, 1, "dead Set store finalized");
}

#[test]
fn live_map_store_moves_and_header_alias_follows() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let map = crate::map::js_map_alloc(16);
    for i in 0..9 {
        crate::map::js_map_set(map, i as f64, (i * 10) as f64);
    }
    let store_before = unsafe { crate::map::test_map_store_word(map) };
    js_shadow_slot_set(0, ptr_bits(map as usize));
    let deallocs_before = crate::map::test_map_side_deallocation_snapshot();
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let map_after = map_from_slot(0);
    let store_after = unsafe { crate::map::test_map_store_word(map_after) };
    assert_ne!(store_after, store_before, "the young store must move");
    let (entries, _) = crate::map::test_map_side_allocation(map_after as usize)
        .expect("a live Map keeps its store");
    assert_eq!(unsafe { (*map_after).entries } as usize, entries);
    assert!(entries > store_after && entries < store_after + 512);
    assert_eq!(
        crate::map::test_map_side_deallocation_snapshot().0,
        deallocs_before.0,
        "a moved store hands its buffers to the copy; nothing is released"
    );
    for i in 0..9 {
        assert_eq!(crate::map::js_map_get(map_after, i as f64), (i * 10) as f64);
    }
}

#[test]
fn live_set_store_moves_and_header_alias_follows() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let set = crate::set::js_set_alloc(16);
    for i in 0..12 {
        crate::set::js_set_add(set, i as f64);
    }
    let store_before = unsafe { crate::set::test_set_store_word(set) };
    js_shadow_slot_set(0, ptr_bits(set as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let set_after = set_from_slot(0);
    let store_after = unsafe { crate::set::test_set_store_word(set_after) };
    assert_ne!(store_after, store_before, "the young store must move");
    let (elements, _) = crate::set::test_set_side_allocation(set_after as usize)
        .expect("a live Set keeps its store");
    assert_eq!(unsafe { (*set_after).elements } as usize, elements);
    for i in 0..12 {
        assert_ne!(crate::set::js_set_has(set_after, i as f64), 0);
    }
    assert_eq!(crate::set::js_set_has(set_after, 12.0), 0);
}

/// An old Map that grows publishes a young store into an old header. The
/// store then holds young strings that no old-parent barrier recorded (its own
/// parent was young when they were written); only the store's own trace finds
/// them once the minor copies it.
#[test]
fn old_map_growing_a_young_store_keeps_its_young_values() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let map = crate::map::js_map_alloc(2);
    js_shadow_slot_set(0, ptr_bits(map as usize));
    for _ in 0..8 {
        let header = map_from_slot(0) as usize;
        let store = unsafe { crate::map::test_map_store_word(map_from_slot(0)) };
        if crate::arena::pointer_in_old_gen(header) && crate::arena::pointer_in_old_gen(store) {
            break;
        }
        collect_minor_trace(GcTriggerKind::Direct);
    }
    let map = map_from_slot(0);
    assert!(
        crate::arena::pointer_in_old_gen(map as usize),
        "header tenured"
    );

    for i in 0..64u32 {
        let text = format!("young-value-{i}");
        let value = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        crate::map::js_map_set(
            map_from_slot(0),
            i as f64,
            f64::from_bits(string_bits(value as usize)),
        );
    }
    let store = unsafe { crate::map::test_map_store_word(map_from_slot(0)) };
    assert!(
        crate::arena::pointer_in_nursery(store),
        "growth publishes a young store"
    );

    for round in 0..3 {
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        let map = map_from_slot(0);
        let (entries, _) = crate::map::test_map_side_allocation(map as usize).unwrap();
        assert_eq!(unsafe { (*map).entries } as usize, entries, "round {round}");
        for i in 0..64u32 {
            let value = crate::map::js_map_get(map, i as f64);
            let text = format!("young-value-{i}");
            unsafe {
                assert_string_bytes(
                    (value.to_bits() & POINTER_MASK) as *const crate::StringHeader,
                    text.as_bytes(),
                );
            }
        }
    }
}
