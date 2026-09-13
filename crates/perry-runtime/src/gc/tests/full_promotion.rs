//! #10182: a synchronous full mark-sweep promotes its Eden survivors in place.
//!
//! What these pin, in order of how badly a regression would bite:
//!
//! 1. **After a promoting full the young generation is EMPTY** and the next
//!    copying minor moves nothing of the tree the full proved live. The failure
//!    this exists to prevent is the one the cohort-bound experiment on #10182
//!    measured: a full that leaves a 58 MB tree in Eden, followed by a minor
//!    whose survival reads ~50% (the previous tree is still there), is an
//!    evacuating copy of the whole tree — the worst outcome on the table.
//! 2. **The promoted objects are first-class old-gen objects**: same address,
//!    `GC_FLAG_TENURED`, and indexed on their old page. An unindexed promoted
//!    object is a missed old→young edge one minor later.
//! 3. **The dead ones are NOT indexed.** The sweep invalidates their headers so
//!    the described page runs re-parse to exactly the survivors. Asserted by
//!    walking the index, because the `debug_assert` inside the run expansion is
//!    compiled out of the `--release` test build this suite runs under.
//! 4. **Both states of the decision**: a mostly-dead nursery declines (and says
//!    so in its own counter), and a thread that is not admitted never plans.
//!
//! Each test runs on a fresh thread: the arenas are thread-local, so that is
//! what makes "survival" a property of the heap the test built rather than of
//! whatever an earlier test left in Eden.

use super::super::promote_in_place::InPlacePromotionTestGuard;
use super::super::*;
use super::support::*;

const LIVE_LEAVES: u32 = 400;

/// Empty the young generation with one promoting copying minor, so the full
/// that follows measures only what the test allocates next.
fn empty_the_young_generation() {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        crate::arena::copying_from_space_in_use_bytes(),
        0,
        "precondition: the clearing minor promoted in place and left no young generation"
    );
}

fn full_collect() {
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));
}

fn user_to_header(user: usize) -> usize {
    unsafe { header_from_user_ptr(user as *const u8) as usize }
}

fn indexed_headers_on_pages_of(addrs: &[usize]) -> std::collections::BTreeSet<usize> {
    let mut pages = crate::fast_hash::new_ptr_hash_set();
    for &addr in addrs {
        pages.insert(crate::arena::generation_page_for_addr(addr));
    }
    let mut seen = std::collections::BTreeSet::new();
    crate::arena::old_arena_walk_objects_on_pages(&pages, |h| {
        seen.insert(h as usize);
    });
    seen
}

#[test]
fn a_full_over_a_live_nursery_promotes_it_in_place_and_leaves_no_young_generation() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(4);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _precise = ConservativeScanDisabledGuard::new();
        let _promote = InPlacePromotionTestGuard::enabled(1000);
        empty_the_young_generation();

        // One live tree: an array sized up front (no growth, so no abandoned
        // backing store and no forwarding stub) holding LIVE_LEAVES young
        // strings, plus ONE unrooted leaf in the middle of it — survival stays
        // well above 95%, and the dead leaf is the object assertion 3 needs.
        let mut arr = crate::array::js_array_alloc(LIVE_LEAVES);
        let mut leaves = Vec::with_capacity(LIVE_LEAVES as usize);
        let mut dead = 0usize;
        for i in 0..LIVE_LEAVES {
            if i == LIVE_LEAVES / 2 {
                dead = young_leaf();
            }
            let leaf = young_leaf();
            leaves.push(leaf);
            arr = crate::array::js_array_push_f64(arr, f64::from_bits(string_bits(leaf)));
        }
        let arr = arr as usize;
        js_shadow_slot_set(0, ptr_bits(arr));
        assert!(crate::arena::pointer_in_nursery(arr));
        assert!(crate::arena::pointer_in_nursery(dead));

        let cycles_before = crate::gc::full_promotion_cycles();
        let declined_before = crate::gc::full_promotion_declined_cycles();
        let objects_before = crate::gc::full_promoted_objects();
        full_collect();

        // Live subject first: a green run that never promoted proves nothing.
        assert_eq!(
            crate::gc::full_promotion_cycles() - cycles_before,
            1,
            "the full must have promoted its young generation (declined {} times)",
            crate::gc::full_promotion_declined_cycles() - declined_before
        );
        let promoted = crate::gc::full_promoted_objects() - objects_before;
        assert!(
            promoted > LIVE_LEAVES as u64,
            "the promotion must have covered the array and all {LIVE_LEAVES} leaves, got {promoted}"
        );
        assert!(
            super::super::promote_in_place::last_young_survival_permille()
                .is_some_and(|p| p >= 950),
            "the full's own measurement must have been fed to the predictor"
        );

        // (1) No young generation remains.
        assert_eq!(
            crate::arena::copying_from_space_in_use_bytes(),
            0,
            "a promoting full must leave the young generation empty"
        );

        // (2) Same address, old-gen, TENURED, indexed.
        assert_eq!((js_shadow_slot_get(0) & POINTER_MASK) as usize, arr);
        for &addr in std::iter::once(&arr).chain(leaves.iter()) {
            assert!(
                crate::arena::pointer_in_old_gen(addr),
                "{addr:#x} must classify as old-gen after the promotion"
            );
            let header = user_to_header(addr) as *const GcHeader;
            assert_ne!(
                unsafe { (*header).gc_flags } & GC_FLAG_TENURED,
                0,
                "a promoted object must carry GC_FLAG_TENURED (#7511)"
            );
        }
        let mut addrs = vec![arr];
        addrs.extend_from_slice(&leaves);
        addrs.push(dead);
        let indexed = indexed_headers_on_pages_of(&addrs);
        assert!(
            indexed.contains(&user_to_header(arr)),
            "the promoted array must be indexed on its old page"
        );
        for &leaf in &leaves {
            assert!(
                indexed.contains(&user_to_header(leaf)),
                "every promoted leaf must be indexed on its old page"
            );
        }

        // (3) The dead leaf was reclaimed by the sweep, invalidated, and is
        // not in the index a dirty-page scan would walk.
        let dead_header = user_to_header(dead);
        assert_eq!(
            unsafe { (*(dead_header as *const GcHeader)).obj_type },
            0,
            "the sweep must have invalidated the dead young header before the promotion"
        );
        assert!(
            !indexed.contains(&dead_header),
            "a dead object on a promoted block must not be indexed as live"
        );

        // The data itself is intact.
        for (i, &leaf) in leaves.iter().enumerate() {
            let bits = crate::array::js_array_get_f64(arr as *const _, i as u32).to_bits();
            assert_eq!((bits & POINTER_MASK) as usize, leaf);
        }

        // (1, continued) The next copying minor has nothing to move.
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_eq!(
            trace.copying_nursery.copied_objects, 0,
            "a minor after a promoting full must not evacuate the promoted tree"
        );
        assert_eq!((js_shadow_slot_get(0) & POINTER_MASK) as usize, arr);
        assert!(crate::arena::pointer_in_old_gen(arr));
    })
    .join()
    .expect("full-promotion test thread must not panic");
}

#[test]
fn a_full_over_a_mostly_dead_nursery_declines_and_keeps_its_young_generation() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(4);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _precise = ConservativeScanDisabledGuard::new();
        let _promote = InPlacePromotionTestGuard::enabled(1000);
        empty_the_young_generation();

        for _ in 0..2_000 {
            std::hint::black_box(young_leaf());
        }
        let survivor = young_leaf();
        js_shadow_slot_set(0, string_bits(survivor));

        let cycles_before = crate::gc::full_promotion_cycles();
        let declined_before = crate::gc::full_promotion_declined_cycles();
        full_collect();

        assert_eq!(
            crate::gc::full_promotion_declined_cycles() - declined_before,
            1,
            "the full was admitted, measured a mostly-dead nursery, and must say it declined"
        );
        assert_eq!(crate::gc::full_promotion_cycles(), cycles_before);
        assert!(
            super::super::promote_in_place::last_young_survival_permille()
                .is_some_and(|p| p < 950),
            "the declining measurement must still reach the copying minor's predictor"
        );
        assert!(
            crate::arena::pointer_in_nursery(survivor),
            "a declined promotion must leave the survivor young"
        );
    })
    .join()
    .expect("full-promotion test thread must not panic");
}

#[test]
fn a_full_on_a_thread_not_admitted_to_in_place_promotion_never_plans_one() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(4);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _precise = ConservativeScanDisabledGuard::new();
        // No `InPlacePromotionTestGuard`: `in_place_promotion_admissible` is
        // false, which is also the state `PERRY_GC_PROMOTE_IN_PLACE=0` produces.
        let survivor = young_leaf();
        js_shadow_slot_set(0, string_bits(survivor));
        let dead = young_leaf();

        let cycles_before = crate::gc::full_promotion_cycles();
        let declined_before = crate::gc::full_promotion_declined_cycles();
        full_collect();

        assert_eq!(crate::gc::full_promotion_cycles(), cycles_before);
        assert_eq!(
            crate::gc::full_promotion_declined_cycles(),
            declined_before,
            "an inadmissible full must not even take the measurement"
        );
        assert!(crate::arena::pointer_in_nursery(survivor));
        // And the sweep did not invalidate dead young headers it was not asked
        // to: that write is part of the promotion plan, not of every full.
        assert_ne!(
            unsafe { (*(user_to_header(dead) as *const GcHeader)).obj_type },
            0,
            "a full with no promotion plan must leave dead young headers as they were"
        );
    })
    .join()
    .expect("full-promotion test thread must not panic");
}
