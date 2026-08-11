//! Live-allocation accounting for public heap telemetry and GC pacing (#7879).

use super::super::*;
use super::support::*;

#[test]
fn one_tiny_survivor_does_not_charge_a_partially_dead_block_to_heap_used() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        reset_global_roots();
        let _root_reset = ShadowAndGlobalRootResetGuard;

        // More than one block of short-lived objects, followed by one rooted
        // survivor in the allocator's recent-block safety window. A full sweep
        // can prove the preceding objects dead but deliberately cannot reset
        // that partially-live block, so block.offset retains their high-water.
        for _ in 0..40_000 {
            std::hint::black_box(young_leaf());
        }
        let survivor_bytes = b"heap_accounting_survivor";
        let survivor = crate::string::js_string_from_bytes(
            survivor_bytes.as_ptr(),
            survivor_bytes.len() as u32,
        ) as usize;
        let mut root_slot = string_bits(survivor);
        js_gc_register_global_root(&mut root_slot as *mut u64 as i64);

        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));

        let high_water = crate::arena::arena_in_use_bytes() as u64;
        let mut heap_used = 0_u64;
        let mut heap_total = 0_u64;
        crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);

        assert!(heap_total >= high_water);
        assert_eq!(
            super::super::policy::pacing_arena_in_use_bytes(),
            heap_used as usize,
            "major-GC pacing and public heapUsed must consume the same live metric"
        );
        assert!(
            high_water >= heap_used.saturating_add((crate::arena::BLOCK_SIZE / 2) as u64),
            "one tiny survivor must not make heapUsed report its block's high-water: \
             heapUsed={heap_used} high_water={high_water} heapTotal={heap_total}"
        );
        unsafe {
            assert_string_bytes(
                (root_slot & POINTER_MASK) as *const crate::StringHeader,
                survivor_bytes,
            );
        }
    })
    .join()
    .expect("heap-accounting test thread must not panic");
}

#[test]
fn allocations_after_a_census_count_bump_growth_and_old_hole_reuse() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        reset_global_roots();
        let _root_reset = ShadowAndGlobalRootResetGuard;

        let dead = unsafe { alloc_old_test_promise() };
        let rooted = unsafe { alloc_old_test_promise() };
        let dead_total = unsafe { (*header_from_user_ptr(dead as *const u8)).size as usize };
        let mut root_slot = ptr_bits(rooted as usize);
        js_gc_register_global_root(&mut root_slot as *mut u64 as i64);

        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));

        let live_before_reuse = crate::arena::arena_live_allocated_bytes();
        let high_water_before_reuse = crate::arena::arena_in_use_bytes();
        let old_free_before_reuse = old_free_bytes();
        assert!(old_free_before_reuse >= dead_total);

        let replacement = unsafe { alloc_old_test_promise() };
        std::hint::black_box(replacement);
        let old_free_after_reuse = old_free_bytes();
        let reused = old_free_before_reuse - old_free_after_reuse;
        assert_eq!(reused, dead_total);
        assert_eq!(
            crate::arena::arena_in_use_bytes(),
            high_water_before_reuse,
            "exact-fit old-hole reuse must not move a block bump pointer"
        );
        assert_eq!(
            crate::arena::arena_live_allocated_bytes(),
            live_before_reuse + reused,
            "consuming a hole must still increase live allocated bytes"
        );

        let live_before_bump = crate::arena::arena_live_allocated_bytes();
        let young = young_leaf();
        let young_total = unsafe { (*header_from_user_ptr(young as *const u8)).size as usize };
        assert_eq!(
            crate::arena::arena_live_allocated_bytes(),
            live_before_bump + young_total,
            "a post-census bump allocation must advance live allocated bytes"
        );
    })
    .join()
    .expect("heap-accounting delta test thread must not panic");
}

#[test]
fn copying_minors_preserve_prior_promotions_in_the_live_census() {
    std::thread::spawn(|| {
        let _copying = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        reset_global_roots();
        let _root_reset = ShadowAndGlobalRootResetGuard;

        for _ in 0..10_000 {
            std::hint::black_box(young_leaf());
        }
        let survivor_bytes = b"copying_census_survivor";
        let survivor = crate::string::js_string_from_bytes(
            survivor_bytes.as_ptr(),
            survivor_bytes.len() as u32,
        ) as usize;
        js_shadow_slot_set(0, string_bits(survivor));

        let mut saw_promotion = false;
        for cycle in 0..=GC_COPY_PROMOTION_SURVIVALS {
            let pre_live = crate::arena::arena_live_allocated_bytes();
            let from_space = crate::arena::copying_from_space_in_use_bytes();
            let trace = collect_minor_trace(GcTriggerKind::Direct);
            assert!(trace.copying_nursery.eligible);

            let expected_live = pre_live
                .saturating_sub(from_space)
                .saturating_add(trace.copying_nursery.copied_bytes)
                .saturating_add(trace.copying_nursery.promoted_bytes);
            assert_eq!(
                crate::arena::arena_live_allocated_bytes(),
                expected_live,
                "copying-minor cycle {cycle} must preserve non-from-space live bytes and add survivors"
            );
            assert_eq!(
                super::super::policy::pacing_arena_in_use_bytes(),
                expected_live
            );
            saw_promotion |= trace.copying_nursery.promoted_bytes != 0;
        }

        assert!(saw_promotion, "test must drive its survivor into old-gen");
        let survivor_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(survivor_after, survivor);
        unsafe {
            assert_string_bytes(survivor_after as *const crate::StringHeader, survivor_bytes);
        }
    })
    .join()
    .expect("copying-minor census test thread must not panic");
}
