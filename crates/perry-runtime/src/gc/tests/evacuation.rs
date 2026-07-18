use super::super::*;
use super::support::*;

fn arena_block_index_for_user(user: usize) -> Option<usize> {
    let mut found = None;
    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let current_user = unsafe { (header_ptr as *mut u8).add(GC_HEADER_SIZE) as usize };
        if current_user == user {
            found = Some(block_idx);
        }
    });
    found
}

#[test]
fn test_cons_pinned_cleared_after_minor_gc() {
    // Allocate something to give the GC sweep work to do.
    let _ = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    // Pre-populate CONS_PINNED to simulate a prior GC's leftover.
    CONS_PINNED.with(|s| {
        s.borrow_mut().insert(0xDEAD_BEEF);
    });
    assert!(cons_pinned_count() >= 1);
    let _ = gc_collect_minor();
    assert_eq!(
        cons_pinned_count(),
        0,
        "minor GC must clear CONS_PINNED after collection"
    );
}

#[test]
fn test_pin_currently_marked_captures_marked_objects() {
    // Manually mark an arena object, then run the pinning
    // scan. The pinned set should contain the marked header.
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    clear_marks();
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED;
    }
    let stats = pin_currently_marked_as_conservative();
    assert!(
        is_conservatively_pinned(header),
        "marked header should land in CONS_PINNED"
    );
    assert_eq!(stats.pinned_roots, 1);
    assert_eq!(stats.pinned_bytes, unsafe { (*header).size as usize });
    // Cleanup for test isolation.
    unsafe {
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_pin_currently_marked_skips_unmarked() {
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    clear_marks();
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *const GcHeader };
    // Ensure unmarked.
    unsafe {
        assert_eq!((*(header as *mut GcHeader)).gc_flags & GC_FLAG_MARKED, 0);
    }
    let stats = pin_currently_marked_as_conservative();
    assert_eq!(stats.pinned_roots, 0);
    assert_eq!(stats.pinned_bytes, 0);
    assert!(
        !is_conservatively_pinned(header),
        "unmarked header should NOT land in CONS_PINNED"
    );
}

#[test]
fn test_conservative_pin_stats_exclude_legacy_copy_only_scanner_pins() {
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    clear_marks();
    let conservative_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let legacy_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let conservative_header = unsafe { header_from_user_ptr(conservative_user) as *mut GcHeader };
    let legacy_header = unsafe { header_from_user_ptr(legacy_user) as *mut GcHeader };
    unsafe {
        (*conservative_header).gc_flags |= GC_FLAG_MARKED;
    }

    let stats = pin_currently_marked_as_conservative();
    let conservative_bytes = unsafe { (*conservative_header).size as usize };
    assert_eq!(stats.pinned_roots, 1);
    assert_eq!(stats.pinned_bytes, conservative_bytes);

    let valid_ptrs = build_valid_pointer_set();
    let legacy_bits = POINTER_TAG | (legacy_user as u64 & POINTER_MASK);
    let legacy_bytes = mark_copy_only_scanner_bits(legacy_bits, &valid_ptrs, true);
    assert_eq!(
        legacy_bytes,
        Some(unsafe { (*legacy_header).size as usize })
    );
    assert_eq!(
        cons_pinned_count(),
        2,
        "evacuation set still contains both conservative and legacy pins"
    );
    assert_eq!(
        stats.pinned_roots, 1,
        "conservative pin stats must not absorb later legacy scanner pins"
    );
    assert_eq!(stats.pinned_bytes, conservative_bytes);

    clear_marks();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_evacuation_policy() {
    fn snapshot(
        tenured: usize,
        candidate: usize,
        candidate_objects: usize,
        pinned: usize,
        rss: u64,
        previous_pause_us: u64,
        pre_evac_pause_us: u64,
    ) -> EvacuationPolicySnapshot {
        EvacuationPolicySnapshot {
            tenured_still_in_nursery_bytes: tenured,
            candidate_bytes: candidate,
            candidate_objects,
            reclaimable_candidate_bytes: candidate,
            reclaimable_candidate_objects: candidate_objects,
            conservative_pinned_bytes: pinned,
            rss_bytes: rss,
            previous_pause_us,
            pre_evac_pause_us,
            ..EvacuationPolicySnapshot::default()
        }
    }

    fn decide(
        snapshot: EvacuationPolicySnapshot,
        considered: bool,
        force: bool,
    ) -> EvacuationPolicyDecision {
        evacuation_policy_final_decision(
            EvacuationPolicyDecision {
                allowed: true,
                considered,
                force,
                enabled: false,
                reason: "test",
                snapshot,
            },
            snapshot,
        )
    }

    let zero_candidates = decide(
        snapshot(MIN_TENURED_NURSERY_BYTES, 0, 0, 0, 0, 0, 0),
        true,
        false,
    );
    assert!(!zero_candidates.enabled);
    assert_eq!(zero_candidates.reason, "zero_candidates");

    let productive = decide(
        snapshot(
            MIN_TENURED_NURSERY_BYTES * 2,
            MIN_CANDIDATE_BYTES * 2,
            2,
            0,
            0,
            0,
            0,
        ),
        true,
        false,
    );
    assert!(productive.enabled);
    assert_eq!(productive.reason, "nursery_pressure");

    let rss_pressure = decide(
        snapshot(
            MIN_CANDIDATE_BYTES,
            MIN_CANDIDATE_BYTES,
            1,
            0,
            RSS_PRESSURE_BYTES,
            0,
            0,
        ),
        true,
        false,
    );
    assert!(rss_pressure.enabled);
    assert_eq!(rss_pressure.reason, "rss_pressure");

    let pinned_dominated = decide(
        snapshot(
            MIN_TENURED_NURSERY_BYTES * 4,
            MIN_CANDIDATE_BYTES,
            1,
            MIN_TENURED_NURSERY_BYTES * 3,
            0,
            0,
            0,
        ),
        true,
        false,
    );
    assert!(!pinned_dominated.enabled);
    assert_eq!(
        pinned_dominated.reason,
        "reclaimable_candidate_ratio_below_threshold"
    );

    let retained_stub_dominated = decide(
        EvacuationPolicySnapshot {
            tenured_still_in_nursery_bytes: MIN_TENURED_NURSERY_BYTES * 2,
            candidate_bytes: MIN_CANDIDATE_BYTES * 2,
            candidate_objects: 16,
            reclaimable_candidate_bytes: 0,
            reclaimable_candidate_objects: 0,
            retained_forwarded_stub_bytes: 64,
            retained_forwarded_stub_objects: 1,
            conservative_pinned_bytes: 0,
            rss_bytes: 0,
            previous_pause_us: 0,
            pre_evac_pause_us: 0,
            ..EvacuationPolicySnapshot::default()
        },
        true,
        false,
    );
    assert!(
        !retained_stub_dominated.enabled,
        "movable bytes alone must not enable evacuation when retained forwarded stubs keep the candidate blocks live"
    );
    assert_eq!(
        retained_stub_dominated.reason,
        "zero_reclaimable_candidates"
    );

    let pause_skip = decide(
        snapshot(
            MIN_TENURED_NURSERY_BYTES,
            MIN_CANDIDATE_BYTES,
            1,
            0,
            0,
            MAX_PREVIOUS_PAUSE_US + 1,
            0,
        ),
        true,
        false,
    );
    assert!(!pause_skip.enabled);
    assert_eq!(pause_skip.reason, "pause_budget_exceeded");

    let hard_rss_override = decide(
        snapshot(
            MIN_TENURED_NURSERY_BYTES,
            MIN_CANDIDATE_BYTES,
            1,
            0,
            RSS_HARD_PRESSURE_BYTES,
            MAX_PREVIOUS_PAUSE_US + 1,
            0,
        ),
        true,
        false,
    );
    assert!(hard_rss_override.enabled);
    assert_eq!(hard_rss_override.reason, "rss_hard_pressure");

    let force = decide(snapshot(0, 64, 1, 0, 0, 0, 0), true, true);
    assert!(force.enabled);
    assert_eq!(force.reason, "force");

    let low_pressure = evacuation_policy_initial_decision(
        0,
        RSS_PRESSURE_BYTES - 1,
        0,
        0,
        true,
        false,
        EVACUATION_POLICY_DISABLED_REASON,
        true,
        0,
    );
    assert!(!low_pressure.considered);
    assert!(!low_pressure.enabled);
    assert_eq!(low_pressure.reason, "low_pressure");

    let pressure_barriers_inactive = evacuation_policy_initial_decision(
        MIN_TENURED_NURSERY_BYTES,
        RSS_HARD_PRESSURE_BYTES,
        0,
        0,
        true,
        false,
        EVACUATION_POLICY_DISABLED_REASON,
        false,
        0,
    );
    assert!(!pressure_barriers_inactive.considered);
    assert!(!pressure_barriers_inactive.enabled);
    assert_eq!(pressure_barriers_inactive.reason, "barriers_inactive");

    let force_barriers_inactive = evacuation_policy_initial_decision(
        0,
        0,
        0,
        0,
        true,
        true,
        EVACUATION_POLICY_DISABLED_REASON,
        false,
        1,
    );
    assert!(force_barriers_inactive.force);
    assert!(!force_barriers_inactive.considered);
    assert!(!force_barriers_inactive.enabled);
    assert_eq!(force_barriers_inactive.reason, "barriers_inactive");

    let disabled = evacuation_policy_initial_decision(
        MIN_TENURED_NURSERY_BYTES,
        RSS_HARD_PRESSURE_BYTES,
        0,
        0,
        false,
        true,
        EVACUATION_POLICY_DISABLED_REASON,
        false,
        0,
    );
    assert!(!disabled.considered);
    assert!(!disabled.enabled);
    assert_eq!(disabled.reason, "disabled");
}

/// 2026-07-09 GC audit W3: the volume gate must pass on the block-granule
/// metric (`releasable_block_bytes`) even when the candidate OBJECTS' own
/// bytes are far below `MIN_CANDIDATE_BYTES` — the "500 blocks pinned by a
/// few hundred bytes of scattered tenured survivors each" shape — and hard
/// RSS pressure must bypass the candidate-volume/ratio/pause gates
/// entirely whenever any movable candidate exists.
#[test]
fn test_evacuation_policy_releasable_block_bytes_gate_and_hard_rss_bypass() {
    fn decide(snapshot: EvacuationPolicySnapshot) -> EvacuationPolicyDecision {
        evacuation_policy_final_decision(
            EvacuationPolicyDecision {
                allowed: true,
                considered: true,
                force: false,
                enabled: false,
                reason: "test",
                snapshot,
            },
            snapshot,
        )
    }

    // Scattered-survivor shape: ~500 blocks each pinned by ~1 KB of
    // movable tenured candidates. Object-bytes metric (~512 KB) is far
    // below the 8 MB bar, but evacuating the candidates releases
    // ~500 MB of block granules.
    let scattered = EvacuationPolicySnapshot {
        tenured_still_in_nursery_bytes: 512 * 1024,
        candidate_bytes: 512 * 1024,
        candidate_objects: 500,
        reclaimable_candidate_bytes: 512 * 1024,
        reclaimable_candidate_objects: 500,
        releasable_block_bytes: 500 * 1024 * 1024,
        ..EvacuationPolicySnapshot::default()
    };
    let scattered_decision = decide(scattered);
    assert!(
        scattered_decision.enabled,
        "block-granule metric must clear the volume gate on its own"
    );
    assert_eq!(scattered_decision.reason, "releasable_block_bytes");

    // The granule metric also skips the object-bytes ratio gate: same
    // shape with a huge tenured denominator would fail the 25% ratio.
    let low_ratio = EvacuationPolicySnapshot {
        tenured_still_in_nursery_bytes: MIN_TENURED_NURSERY_BYTES * 8,
        ..scattered
    };
    let low_ratio_decision = decide(low_ratio);
    assert!(
        low_ratio_decision.enabled,
        "ratio gate must not veto a block-granule pass"
    );
    assert_eq!(low_ratio_decision.reason, "releasable_block_bytes");

    // Both metrics below the bar still refuses, preserving the existing
    // telemetry reason.
    let both_small = EvacuationPolicySnapshot {
        tenured_still_in_nursery_bytes: MIN_TENURED_NURSERY_BYTES,
        candidate_bytes: 64 * 1024,
        candidate_objects: 8,
        reclaimable_candidate_bytes: 64 * 1024,
        reclaimable_candidate_objects: 8,
        releasable_block_bytes: MIN_CANDIDATE_BYTES - 1,
        ..EvacuationPolicySnapshot::default()
    };
    let both_small_decision = decide(both_small);
    assert!(!both_small_decision.enabled);
    assert_eq!(
        both_small_decision.reason,
        "reclaimable_candidate_bytes_below_threshold"
    );

    // Hard RSS pressure bypasses the volume/ratio/pause gates: candidate
    // volume below every bar, zero reclaimable/releasable bytes, pause
    // budget blown — still evacuates, because movable candidates exist
    // and RSS is past the hard threshold.
    let hard_rss_bypass = EvacuationPolicySnapshot {
        tenured_still_in_nursery_bytes: 64 * 1024,
        candidate_bytes: 4096,
        candidate_objects: 4,
        reclaimable_candidate_bytes: 0,
        reclaimable_candidate_objects: 0,
        releasable_block_bytes: 0,
        rss_bytes: RSS_HARD_PRESSURE_BYTES,
        previous_pause_us: MAX_PREVIOUS_PAUSE_US + 1,
        ..EvacuationPolicySnapshot::default()
    };
    let bypass_decision = decide(hard_rss_bypass);
    assert!(
        bypass_decision.enabled,
        "hard RSS pressure must bypass the candidate-volume gates"
    );
    assert_eq!(bypass_decision.reason, "rss_hard_pressure");

    // ...but zero movable candidates still refuses even under hard RSS:
    // there is nothing evacuation could move.
    let no_candidates = EvacuationPolicySnapshot {
        tenured_still_in_nursery_bytes: 64 * 1024,
        candidate_bytes: 0,
        candidate_objects: 0,
        rss_bytes: RSS_HARD_PRESSURE_BYTES,
        ..EvacuationPolicySnapshot::default()
    };
    let no_candidates_decision = decide(no_candidates);
    assert!(!no_candidates_decision.enabled);
    assert_eq!(no_candidates_decision.reason, "zero_candidates");

    // Object-bytes pass with below-bar granule metric keeps the legacy
    // pressure-based reason strings.
    let object_pass = EvacuationPolicySnapshot {
        tenured_still_in_nursery_bytes: MIN_TENURED_NURSERY_BYTES * 2,
        candidate_bytes: MIN_CANDIDATE_BYTES * 2,
        candidate_objects: 16,
        reclaimable_candidate_bytes: MIN_CANDIDATE_BYTES * 2,
        reclaimable_candidate_objects: 16,
        releasable_block_bytes: 0,
        ..EvacuationPolicySnapshot::default()
    };
    let object_pass_decision = decide(object_pass);
    assert!(object_pass_decision.enabled);
    assert_eq!(object_pass_decision.reason, "nursery_pressure");
}

/// The old-page defrag selection must report the page-granule bytes its
/// selected pages would release (page size minus pinned bytes) so the
/// policy's `releasable_block_bytes` metric sees whole granules, not
/// just the dead object bytes.
#[test]
fn test_old_page_defrag_selection_reports_releasable_granule_bytes() {
    fn meta(
        page_base: usize,
        allocated_bytes: usize,
        live_bytes: usize,
        dead_bytes: usize,
        pinned_bytes: usize,
    ) -> crate::arena::OldPageMeta {
        crate::arena::OldPageMeta {
            page_base,
            page_end: page_base + 4096,
            allocated_bytes,
            live_bytes,
            dead_bytes,
            object_count: 1,
            live_object_count: usize::from(live_bytes > 0),
            dead_object_count: usize::from(dead_bytes > 0),
            pinned_bytes,
            pinned_object_count: usize::from(pinned_bytes > 0),
            dirty_slots: 0,
            dirty_slots_epoch: 0,
            dirty: false,
            evacuation_eligible: false,
        }
    }

    let selected_a = meta(0x2000_0000, 100, 10, 90, 0);
    let selected_b = meta(0x2000_1000, 100, 20, 80, 0);
    let unselected_low_dead = meta(0x2000_2000, 100, 80, 20, 0);
    let pinned = meta(0x2000_3000, 100, 10, 90, 8);
    let snapshot = [selected_a, selected_b, unselected_low_dead, pinned];

    let selection = select_old_page_defrag_pages_from_snapshot(&snapshot, false);
    assert_eq!(selection.selected_pages, 2);
    assert_eq!(
        selection.selected_releasable_block_bytes,
        2 * 4096,
        "each selected (unpinned) page contributes its full page granule"
    );
}

#[test]
fn test_evacuation_policy_snapshot_excludes_retained_forwarded_stub_blocks() {
    clear_marks();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let mut pair = None;
    for _ in 0..64 {
        let candidate = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT) as usize;
        let stub = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY) as usize;
        let candidate_block = arena_block_index_for_user(candidate);
        let stub_block = arena_block_index_for_user(stub);
        if candidate_block.is_some()
            && candidate_block == stub_block
            && candidate_block.unwrap() < crate::arena::general_block_count()
        {
            pair = Some((candidate, stub));
            break;
        }
    }
    let (candidate, stub) =
        pair.expect("test setup should find two nursery allocations in one general block");
    let candidate_header = unsafe { header_from_user_ptr(candidate as *const u8) };
    let stub_header = unsafe { header_from_user_ptr(stub as *const u8) };
    let stub_target = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
    unsafe {
        (*candidate_header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
        set_forwarding_address(stub_header, stub_target);
    }

    let old_page_selection = OldPageDefragSelection::default();
    let snapshot = evacuation_policy_snapshot_after_mark(
        EvacuationPolicySnapshot::default(),
        false,
        0,
        &old_page_selection,
    );
    let candidate_size = unsafe { (*candidate_header).size as usize };
    let stub_size = unsafe { (*stub_header).size as usize };
    assert!(
        snapshot.candidate_bytes >= candidate_size,
        "marked tenured object should be a movable candidate"
    );
    assert_eq!(
        snapshot.reclaimable_candidate_bytes, 0,
        "candidate sharing a block with a retained forwarded stub is not block-reclaimable"
    );
    assert!(
        snapshot.retained_forwarded_stub_bytes >= stub_size,
        "policy snapshot should report retained forwarded stubs that keep blocks live"
    );

    unsafe {
        (*candidate_header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
        (*stub_header).gc_flags &= !GC_FLAG_FORWARDED;
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_evacuate_tenured_skips_pinned() {
    // An object that's MARKED + TENURED + CONS_PINNED must
    // NOT be evacuated.
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
    }
    // Pin it.
    CONS_PINNED.with(|s| s.borrow_mut().insert(header as usize));
    let n = evacuate_tenured_nursery_objects();
    assert_eq!(n.objects, 0, "pinned tenured object must not be evacuated");
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "FORWARDED flag must not be set on pinned object"
        );
    }
    // Cleanup
    unsafe {
        (*header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_evacuate_tenured_skips_unmarked() {
    // TENURED but not MARKED → dead this cycle, sweep handles it.
    // Evacuation must skip.
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    unsafe {
        (*header).gc_flags |= GC_FLAG_TENURED; // no MARK
    }
    let _n = evacuate_tenured_nursery_objects();
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "unmarked object must not be evacuated"
        );
    }
    unsafe {
        (*header).gc_flags &= !GC_FLAG_TENURED;
    }
}

#[test]
fn test_evacuate_tenured_marks_forwarded_and_copies_payload() {
    // The happy path: marked + tenured + not pinned → evacuated.
    // Verify (a) GC_FLAG_FORWARDED set on nursery header,
    // (b) forwarding_address points into OLD_ARENA,
    // (c) payload bytes copied.
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    // Write a sentinel pattern into the user payload so we can
    // confirm it survives the copy.
    unsafe {
        let p = user as *mut u64;
        *p = 0xCAFE_BABE_DEAD_BEEF;
        *p.add(1) = 0x1234_5678_9ABC_DEF0;
        (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
    }
    let n = evacuate_tenured_nursery_objects();
    assert_eq!(
        n.objects, 1,
        "tenured non-pinned marked object must evacuate"
    );
    unsafe {
        assert_ne!((*header).gc_flags & GC_FLAG_FORWARDED, 0);
        let new_user = forwarding_address(header);
        // Verify old_user points into nursery, new_user points into OLD.
        assert!(
            crate::arena::pointer_in_old_gen(new_user as usize),
            "forwarding address should point into OLD_ARENA"
        );
        assert!(
            !crate::arena::pointer_in_old_gen(user as usize),
            "old (nursery) location should NOT be in OLD_ARENA"
        );
        // Verify payload was copied.
        let new_p = new_user as *const u64;
        // Note: payload starts at user_ptr offset 0, but the
        // forwarding write at the OLD slot overwrites the first 8
        // bytes with the new address. So the payload at the OLD
        // location is partially clobbered now — we can only
        // verify the NEW location's payload.
        assert_eq!(*new_p, 0xCAFE_BABE_DEAD_BEEF);
        assert_eq!(*new_p.add(1), 0x1234_5678_9ABC_DEF0);
    }
    unsafe {
        (*header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
    }
}

#[test]
fn test_release_evacuated_original_forwarding_stub_before_sweep() {
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    clear_marks();
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
    }
    let total = unsafe { (*header).size as usize };
    let mut evacuated_new_headers = Vec::new();
    let mut evacuated_original_headers = Vec::new();
    let moved = evacuate_tenured_nursery_objects_collecting(
        false,
        &mut evacuated_new_headers,
        &mut evacuated_original_headers,
    );
    assert_eq!(moved.moved_objects, 1);
    assert_eq!(moved.moved_bytes, total);
    assert_eq!(evacuated_original_headers, vec![header]);
    unsafe {
        assert_ne!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "evacuation must install a forwarding stub for rewrite"
        );
    }

    let released = release_evacuated_original_forwarding_stubs(&evacuated_original_headers);
    assert_eq!(released.released_original_objects, 1);
    assert_eq!(released.released_original_bytes, total);
    assert_eq!(released.released_original_reusable_bytes, 0);
    assert_eq!(released.released_original_returned_bytes, 0);
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "GC-evacuation originals should release FORWARDED before sweep"
        );
    }

    let sweep = sweep_with_age_bump(false);
    assert_eq!(sweep.dead_bytes, sweep.freed_bytes);
    assert!(
        sweep.freed_bytes >= total as u64,
        "released evacuation original should contribute to sweep reclaimable bytes"
    );
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_sweep_reports_and_retains_non_evacuation_forwarded_stub() {
    clear_marks();
    let stub = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
    let target = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
    let stub_header = unsafe { header_from_user_ptr(stub) as *mut GcHeader };
    let total = unsafe { (*stub_header).size as usize };
    unsafe {
        set_forwarding_address(stub_header, target);
        (*stub_header).gc_flags |= GC_FLAG_MARKED;
    }
    for _ in 0..90_000 {
        let _ = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    }

    let sweep = sweep_with_age_bump(false);
    assert!(
        sweep.retained_forwarded_stub_objects >= 1,
        "sweep should count retained non-evacuation forwarding stubs"
    );
    assert!(
        sweep.retained_forwarded_stub_bytes >= total,
        "sweep should report bytes retained by non-evacuation forwarding stubs"
    );
    unsafe {
        assert_ne!(
            (*stub_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "sweep must not clear array-growth forwarding stubs"
        );
        (*stub_header).gc_flags &= !GC_FLAG_FORWARDED;
    }
}

#[test]
fn test_sweep_reclaims_unreached_old_forwarded_stub() {
    clear_marks();
    let stub = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
    let target = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
    let stub_header = unsafe { header_from_user_ptr(stub) as *mut GcHeader };
    let total = unsafe { (*stub_header).size as usize };
    unsafe {
        set_forwarding_address(stub_header, target);
    }
    for _ in 0..90_000 {
        let _ = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    }

    let sweep = sweep_with_age_bump(false);
    assert!(
        sweep.freed_bytes >= total as u64,
        "unreached old forwarding stub should be reclaimable"
    );
    unsafe {
        assert_eq!(
            (*stub_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "sweep should release stale unreachable forwarding stubs"
        );
    }
}

#[test]
fn test_forced_evacuation_barriers_inactive_does_not_forward_candidate() {
    struct ResetGcTestState;

    impl Drop for ResetGcTestState {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            clear_marks();
            clear_mark_seeds();
            CONS_PINNED.with(|s| s.borrow_mut().clear());
        }
    }

    let _reset = ResetGcTestState;
    let _isolation = copying_nursery_isolation_lock();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::inactive();
    reset_shadow_stack();
    reset_global_roots();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    if !gc_force_evacuate_enabled() {
        return;
    }
    assert!(
        !generated_write_barriers_emitted(),
        "this canary must verify the barriers-inactive evacuation gate"
    );

    let frame = js_shadow_frame_push(1);
    let (parent, _) = unsafe { alloc_nursery_test_object(0) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };

    unsafe {
        (*parent_header).gc_flags |= GC_FLAG_TENURED;
    }
    js_shadow_slot_set(0, ptr_bits(parent_user));

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_eq!(
        parent_after, parent_user,
        "forced evacuation must not move candidates when generated barriers are inactive"
    );
    unsafe {
        assert_eq!(
            (*parent_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "barriers-inactive policy gate must leave the nursery candidate unforwarded"
        );
    }
    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::BarriersInactive,
        false,
    );
    assert_eq!(trace.copying_nursery.copied_objects, 0);
    assert_eq!(trace.copying_nursery.copied_bytes, 0);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert!(trace.evacuation_policy.force);
    assert!(!trace.evacuation_policy.considered);
    assert!(!trace.evacuation_policy.enabled);
    assert_eq!(trace.evacuation_policy.reason, "barriers_inactive");
    assert_eq!(trace.evacuation.objects, 0);
    assert_eq!(trace.evacuation.bytes, 0);
    assert_eq!(trace.evacuation.moved_objects, 0);
    assert_eq!(trace.evacuation.moved_bytes, 0);

    js_shadow_frame_pop(frame);
    if gc_trace_enabled() {
        trace.emit(GcStepSnapshot::current());
    }
}

#[test]
fn test_evacuated_old_parent_re_remembers_young_child_canary() {
    struct ResetGcTestState;

    impl Drop for ResetGcTestState {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            clear_marks();
            clear_mark_seeds();
            CONS_PINNED.with(|s| s.borrow_mut().clear());
        }
    }

    let _reset = ResetGcTestState;
    let _isolation = copying_nursery_isolation_lock();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
    reset_shadow_stack();
    reset_global_roots();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    if !gc_force_evacuate_enabled() {
        return;
    }
    assert!(
        generated_write_barriers_emitted(),
        "this canary must exercise policy evacuation with generated barriers active"
    );

    let frame = js_shadow_frame_push(1);
    let (parent, fields) = unsafe { alloc_nursery_test_object(1) };
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);

    unsafe {
        *fields = ptr_bits(child);
        (*parent_header).gc_flags |= GC_FLAG_TENURED;
    }
    js_shadow_slot_set(0, ptr_bits(parent_user));
    CONS_PINNED.with(|s| {
        s.borrow_mut().insert(child_header as usize);
    });

    let _ = gc_collect_minor();

    let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        parent_after, parent_user,
        "rooted parent should be rewritten to its evacuated old-gen copy"
    );
    assert!(
        crate::arena::pointer_in_old_gen(parent_after),
        "evacuated parent should live in old-gen"
    );
    unsafe {
        assert_eq!(
            (*parent_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "original nursery parent should release its GC forwarding pointer after rewrite"
        );
    }

    let parent_after_fields = unsafe {
        (parent_after as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *mut u64
    };
    let child_after = unsafe { (*parent_after_fields & POINTER_MASK) as usize };
    assert_eq!(
        child_after, child,
        "evacuated parent should still point at the pinned nursery child"
    );
    assert!(
        crate::arena::pointer_in_nursery(child_after),
        "child should remain young after parent evacuation"
    );

    assert!(
        remembered_set_size() > 0,
        "evacuated old parent retaining a nursery child must be re-remembered after the collection clear"
    );

    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(
        stats.newly_marked > 0,
        "remembered scan should mark the nursery child reachable only from the evacuated old parent"
    );
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "remembered scan should mark the pinned nursery child"
        );
    }

    clear_marks();
    CONS_PINNED.with(|s| {
        s.borrow_mut().insert(child_header as usize);
    });
    let _ = gc_collect_minor();

    let parent_after_second = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_eq!(
        parent_after_second, parent_after,
        "second minor GC should keep using the evacuated old parent"
    );
    let child_after_second = unsafe { (*parent_after_fields & POINTER_MASK) as usize };
    assert_eq!(
        child_after_second, child,
        "second minor GC should keep the nursery child alive through the rebuilt remembered entry"
    );
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_TENURED,
            0,
            "second minor GC should mark and age the nursery child"
        );
    }

    js_shadow_frame_pop(frame);
}
