//! #10166 (brief 4): an array's named properties live in a pairs array that
//! its reserved front slot points at (`array/named_props.rs`). There is no
//! address-keyed table for reserved arrays, so their death story is ordinary — a
//! dead array's pairs die with it — and the value edge is a real child edge of
//! the array: `gc/layout_slot_visit.rs` emits the reserved slot as a fixed
//! child slot, and `store_pairs_pointer` is the one barriered store into it.
//!
//! Fault injections these cases exist to catch: dropping the fixed-slot visit
//! (the live-owner full-GC case and the live-move case fail), dropping the
//! barrier in `store_pairs_pointer` (the old-to-young edge case fails), and
//! dropping the reserve carry in `js_array_grow` (`array::named_props_tests`).

use super::super::*;
use super::dead_owner_side_tables::{
    alloc_malloc_test_object, alloc_nursery_test_array, full_gc, full_gc_with_no_block_persistence,
    register_array_side_table_scanners, set_array_named_property, ArraySideTableTestGuard,
};
use super::support::*;

#[test]
fn test_array_named_property_value_dies_with_its_dead_owner() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let value = unsafe { alloc_malloc_test_object() };
    // Spare capacity: the property takes the in-array reserve.
    let owner = crate::array::js_array_alloc(0);
    let owner = unsafe {
        set_array_named_property(owner, "payload", f64::from_bits(ptr_bits(value as usize)))
    };
    assert!(
        crate::arena::pointer_in_nursery(owner as usize),
        "test premise: the unrooted owner is a nursery array"
    );

    full_gc_with_no_block_persistence();
    assert!(
        !malloc_user_ptr_tracked(value as *mut u8),
        "nothing but the dead array's pairs referenced the value, so the first full \
         collection must reclaim it — there is no side table to keep it alive"
    );
}

#[test]
fn test_array_named_property_value_survives_full_gc_through_its_live_owner() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let value = unsafe { alloc_malloc_test_object() };
    // Spare capacity: the property takes the in-array reserve.
    let owner = crate::array::js_array_alloc(0);
    let owner = unsafe {
        set_array_named_property(owner, "payload", f64::from_bits(ptr_bits(value as usize)))
    };
    js_shadow_slot_set(0, ptr_bits(owner as usize));

    full_gc();
    full_gc();
    assert!(
        malloc_user_ptr_tracked(value as *mut u8),
        "the value is reachable ONLY through the live array's reserved slot: the \
         fixed child-slot visit in the Array arm must mark it"
    );
    let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::array::ArrayHeader;
    assert_eq!(
        unsafe { crate::array::array_named_property_get_by_name(owner, "payload") }
            .map(|v| v.to_bits()),
        Some(ptr_bits(value as usize))
    );
}

#[test]
fn test_array_named_dead_owner_cannot_leak_property_across_eden_page_reuse() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let dead = crate::array::js_array_alloc(0);
    let dead = unsafe {
        set_array_named_property(dead, "inherited", f64::from_bits(crate::value::TAG_TRUE))
    };
    let dead_addr = dead as usize;

    let _ = gc_collect_minor();
    // The Eden reset recycles the dead owner's bytes: fresh same-sized arrays
    // land back on its page. None of them may report the dead array's
    // property — the flag word is zeroed by the allocator and the reserve is
    // INSIDE the allocation, so there is no address-keyed state to inherit.
    let mut recycled = false;
    for _ in 0..8 {
        let candidate = crate::array::js_array_alloc(0);
        assert!(
            unsafe { crate::array::array_named_property_get_by_name(candidate, "inherited") }
                .is_none(),
            "an ordinary replacement array must not inherit the dead array's expando"
        );
        assert_eq!(
            unsafe { crate::array::test_named_props_state(candidate) },
            (false, 0, 0),
            "the allocator zeroes the flag word, so the reserve does not outlive its owner"
        );
        recycled |= (candidate as usize).abs_diff(dead_addr) < 4096;
    }
    assert!(
        recycled,
        "test premise: the copied-minor Eden reset must recycle the dead owner's page"
    );
}

#[test]
fn test_array_named_live_move_carries_the_reserve_and_rewrites_the_edges() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let arr = crate::array::js_array_alloc(0);
    let (value, _) = unsafe { alloc_nursery_test_object(0) };
    let old_value = value as usize;
    let arr = unsafe { set_array_named_property(arr, "kept", f64::from_bits(ptr_bits(old_value))) };
    let old_owner = arr as usize;
    let (_, old_pairs, _) = unsafe { crate::array::test_named_props_state(arr) };
    assert_ne!(old_pairs, 0);
    js_shadow_slot_set(0, ptr_bits(old_owner));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    let (flagged, new_pairs, count) = unsafe {
        crate::array::test_named_props_state(new_owner as *const crate::array::ArrayHeader)
    };
    assert!(flagged, "the flag rides `_reserved` through the move");
    assert_eq!(count, 1);
    assert_ne!(
        new_pairs, old_pairs,
        "the pairs edge must be rewritten to the moved pairs array"
    );
    assert!(crate::arena::pointer_in_nursery(new_pairs));
    let new_value_bits = unsafe {
        crate::array::array_named_property_get_by_name(
            new_owner as *const crate::array::ArrayHeader,
            "kept",
        )
    }
    .expect("the moved owner must retain its expando")
    .to_bits();
    let new_value = (new_value_bits & POINTER_MASK) as usize;
    assert_ne!(new_value, old_value, "the object value must be rewritten");
    assert!(crate::arena::pointer_in_nursery(new_value));
}

#[test]
fn test_array_named_property_store_into_old_array_records_the_old_to_young_edge() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    // An exact-capacity old array: the first expando has no slack to take, so
    // the reserve comes from a growth into another OLD allocation — the
    // hardest case, because the pairs array (young) is then reachable only
    // through a slot the minor cannot see without the barrier.
    let (old_arr, _) = unsafe { alloc_old_test_array(1) };
    // One slot of spare capacity, so the reserve is taken in place and the
    // array stays in the old generation (a full array uses the fallback table).
    unsafe { (*old_arr).length = 0 };
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let old_arr =
        unsafe { set_array_named_property(old_arr, "young", f64::from_bits(ptr_bits(young))) };
    assert!(
        crate::arena::pointer_in_old_gen(old_arr as usize),
        "test premise: the live head is still an old-generation array"
    );
    let (_, pairs, _) = unsafe { crate::array::test_named_props_state(old_arr) };
    assert!(
        crate::arena::pointer_in_nursery(pairs),
        "test premise: the pairs array is young, so the reserved slot is an old->young edge"
    );
    let old_header = unsafe { header_from_user_ptr(old_arr as *const u8) };
    unsafe {
        (*old_header).gc_flags |= GC_FLAG_MARKED;
    }

    let stats = verify_old_to_young_edges_covered();

    assert!(
        stats.checked_old_to_young_edges >= 1,
        "the reserved slot must be enumerated as a child slot of the old array"
    );
    assert_eq!(
        stats.missing_edges, 0,
        "`store_pairs_pointer` must have barriered the reserved-slot store"
    );
    unsafe {
        (*old_header).gc_flags &= !GC_FLAG_MARKED;
    }
    clear_marks();
    remembered_set_clear();
}

/// Inline mode (regex exec results): the VALUES sit in the array's own front
/// slots, so a minor that moves both the array and a young value must rewrite
/// the value slot through `visit_array_named_props_slots`'s inline arm.
#[cfg(feature = "regex-engine")]
#[test]
fn test_array_named_inline_value_is_rewritten_when_owner_and_value_move() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let arr = crate::array::js_array_alloc_named_props_reserved(
        0,
        crate::array::InlineKeySet::ExecResult,
    );
    let (value, _) = unsafe { alloc_nursery_test_object(0) };
    let old_value = value as usize;
    unsafe {
        crate::array::array_named_props_install_inline(
            arr,
            &[
                3.0,
                f64::from_bits(ptr_bits(old_value)),
                f64::from_bits(crate::value::TAG_UNDEFINED),
            ],
        );
    }
    let old_owner = arr as usize;
    js_shadow_slot_set(0, ptr_bits(old_owner));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    let owner = new_owner as *const crate::array::ArrayHeader;
    assert_eq!(
        unsafe { crate::array::test_named_props_inline_set(owner) },
        Some(crate::array::InlineKeySet::ExecResult),
        "the inline header word rides the move"
    );
    let new_value_bits = unsafe { crate::array::array_named_property_get_by_name(owner, "input") }
        .expect("the moved owner keeps its inline value")
        .to_bits();
    let new_value = (new_value_bits & POINTER_MASK) as usize;
    assert_ne!(new_value, old_value, "the inline value must be rewritten");
    assert!(crate::arena::pointer_in_nursery(new_value));
    assert_eq!(
        unsafe { crate::array::array_named_property_get_by_name(owner, "index") },
        Some(3.0)
    );
}

// ---- Fallback table: arrays that were FULL at their first named property ----
// (`FULL_ARRAY_NAMED_PROPS`). The capacity-0 fixture is full by construction.

#[test]
fn test_full_array_dead_owner_stops_rooting_value_after_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let value = unsafe { alloc_malloc_test_object() };
    let owner = unsafe { alloc_nursery_test_array() };
    let owner = unsafe {
        set_array_named_property(owner, "payload", f64::from_bits(ptr_bits(value as usize)))
    };
    assert!(crate::array::test_full_array_named_property_owner_exists(
        owner as usize
    ));

    full_gc();
    assert!(
        !crate::array::test_full_array_named_property_owner_exists(owner as usize),
        "the first full collection must prune the dead owner"
    );
    assert!(
        malloc_user_ptr_tracked(value as *mut u8),
        "the value was scanned before post-trace pruning and survives that cycle"
    );

    full_gc();
    assert!(
        !malloc_user_ptr_tracked(value as *mut u8),
        "without the dead owner entry, the next full collection must reclaim the value"
    );
}

#[test]
fn test_full_array_live_move_rekeys_owner_and_rewrites_object_value() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let arr = unsafe { alloc_nursery_test_array() };
    let old_owner = arr as usize;
    let (value, _) = unsafe { alloc_nursery_test_object(0) };
    let old_value = value as usize;
    let arr = unsafe { set_array_named_property(arr, "kept", f64::from_bits(ptr_bits(old_value))) };
    assert_eq!(
        arr as usize, old_owner,
        "a full array does not move to gain a property"
    );
    js_shadow_slot_set(0, ptr_bits(old_owner));

    let _ = gc_collect_minor();

    let new_owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(new_owner, old_owner, "test premise: the owner must move");
    let new_value_bits = unsafe {
        crate::array::array_named_property_get_by_name(
            new_owner as *const crate::array::ArrayHeader,
            "kept",
        )
    }
    .expect("the moved owner must retain its expando")
    .to_bits();
    let new_value = (new_value_bits & POINTER_MASK) as usize;
    assert_ne!(new_value, old_value, "the object value must be rewritten");
    assert!(crate::arena::pointer_in_nursery(new_value));
    assert!(crate::array::test_full_array_named_property_owner_exists(
        new_owner
    ));
    assert!(!crate::array::test_full_array_named_property_owner_exists(
        old_owner
    ));
}

#[test]
fn test_full_array_dead_owner_cannot_leak_property_across_exact_eden_reuse() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _side_tables = ArraySideTableTestGuard::new();
    register_array_side_table_scanners();
    crate::arena::arena_reset_all_blocks_to_zero();

    let dead = unsafe { alloc_nursery_test_array() };
    let dead = unsafe {
        set_array_named_property(dead, "inherited", f64::from_bits(crate::value::TAG_TRUE))
    };
    let dead_addr = dead as usize;
    assert!(crate::array::test_full_array_named_property_owner_exists(
        dead_addr
    ));
    let _ = gc_collect_minor();
    let replacement = unsafe { alloc_nursery_test_array() };
    assert_eq!(
        replacement as usize, dead_addr,
        "test premise: the Eden reset must reuse the exact fallback owner address"
    );
    assert!(!crate::array::test_full_array_named_property_owner_exists(
        dead_addr
    ));
    assert!(
        unsafe { crate::array::array_named_property_get_by_name(replacement, "inherited") }
            .is_none()
    );
}
