use super::super::*;
use super::support::*;

#[test]
fn test_large_buffer_and_typed_array_enter_valid_pointer_set() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();

    let buffer = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
    let typed_array = crate::typedarray::typed_array_alloc(
        crate::typedarray::KIND_UINT8,
        LARGE_OBJECT_THRESHOLD_BYTES as u32,
    ) as usize;
    assert!(crate::arena::pointer_in_old_gen(buffer));
    assert!(crate::arena::pointer_in_old_gen(typed_array));

    let valid_ptrs = build_valid_pointer_set();
    assert!(
        valid_ptrs.contains(&buffer),
        "large old Buffer must be in the valid pointer set"
    );
    assert!(
        valid_ptrs.contains(&typed_array),
        "large old TypedArray must be in the valid pointer set"
    );

    let buffer_data = buffer + std::mem::size_of::<crate::buffer::BufferHeader>();
    let typed_array_data = typed_array + std::mem::size_of::<crate::typedarray::TypedArrayHeader>();
    assert_eq!(valid_ptrs.enclosing_object(buffer_data), Some(buffer));
    assert_eq!(
        valid_ptrs.enclosing_object(typed_array_data),
        Some(typed_array)
    );

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_sweep_accounting_includes_large_buffer_and_typed_array() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let live_buffer = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
    let dead_typed_array = crate::typedarray::typed_array_alloc(
        crate::typedarray::KIND_UINT8,
        LARGE_OBJECT_THRESHOLD_BYTES as u32,
    ) as usize;
    let (live_header, live_total) = old_test_header_and_size(live_buffer);
    let (_dead_header, dead_total) = old_test_header_and_size(dead_typed_array);
    unsafe {
        (*live_header).gc_flags |= GC_FLAG_MARKED;
    }

    let sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert!(
        sweep.freed_bytes >= dead_total as u64,
        "dead old TypedArray should use the existing sweep dead decision"
    );
    assert_eq!(summary.live_bytes, live_total);
    assert_eq!(summary.dead_bytes, dead_total);
    assert_eq!(summary.reusable_bytes, 0);
    assert_eq!(summary.returned_bytes, 0);
    assert_eq!(summary.pinned_bytes, 0);
    assert_eq!(
        summary.live_object_count,
        crate::arena::old_object_page_overlaps(live_header as usize, live_total).len()
    );
    assert_eq!(
        summary.dead_object_count,
        crate::arena::old_object_page_overlaps(dead_typed_array - GC_HEADER_SIZE, dead_total,)
            .len()
    );

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_sweep_accounting_live_dead_fragmentation() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let live = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (live_header, live_total) = old_test_header_and_size(live);
    let (_dead_header, dead_total) = old_test_header_and_size(dead);
    unsafe {
        (*live_header).gc_flags |= GC_FLAG_MARKED;
    }

    let sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert!(
        sweep.freed_bytes >= dead_total as u64,
        "dead old object should use the existing sweep dead decision"
    );
    assert_eq!(summary.live_bytes, live_total);
    assert_eq!(summary.dead_bytes, dead_total);
    assert_eq!(summary.reusable_bytes, 0);
    assert_eq!(summary.returned_bytes, 0);
    assert_eq!(summary.pinned_bytes, 0);
    assert_eq!(summary.live_object_count, 1);
    assert_eq!(summary.dead_object_count, 1);
    assert_eq!(summary.pinned_object_count, 0);
    assert_eq!(summary.fragmented_pages, 1);
    assert_eq!(summary.evacuation_eligible_pages, 1);
}

#[test]
fn test_old_page_reclamation_telemetry_dead_old_object_not_reusable_or_returned() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (_dead_header, dead_total) = old_test_header_and_size(dead);

    let sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert!(summary.dead_bytes >= dead_total);
    assert_eq!(summary.reusable_bytes, 0);
    assert_eq!(summary.returned_bytes, 0);
    assert!(sweep.dead_bytes >= dead_total as u64);
    assert_eq!(sweep.reusable_bytes, 0);
    assert_eq!(sweep.returned_bytes, 0);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_reclamation_telemetry_dead_large_object_not_reusable_or_returned() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let dead_buffer = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
    let (_dead_header, dead_total) = old_test_header_and_size(dead_buffer);

    let sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert!(summary.dead_bytes >= dead_total);
    assert_eq!(summary.reusable_bytes, 0);
    assert_eq!(summary.returned_bytes, 0);
    assert!(sweep.dead_bytes >= dead_total as u64);
    assert_eq!(sweep.reusable_bytes, 0);
    assert_eq!(sweep.returned_bytes, 0);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_full_sweep_reclaims_dead_old_block_and_clears_page_index() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let live = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let dead = crate::arena::arena_alloc_gc_old(2 * 1024 * 1024, 8, GC_TYPE_STRING) as usize;
    let (live_header, _live_total) = old_test_header_and_size(live);
    let (dead_header, dead_total) = old_test_header_and_size(dead);
    let mut dead_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(dead_header as usize, dead_total) {
        dead_pages.insert(page);
    }
    unsafe {
        (*live_header).gc_flags |= GC_FLAG_MARKED;
    }
    let old_before = crate::arena::old_gen_in_use_bytes();

    let sweep = sweep_with_age_bump_and_old_reclaim(false, true);
    let summary = crate::arena::old_page_summary();
    let old_after = crate::arena::old_gen_in_use_bytes();

    assert!(
        sweep.freed_bytes >= dead_total as u64,
        "dead old object should be swept before block reclaim"
    );
    assert!(
        old_after < old_before,
        "dead old block reset/deallocation should lower old in-use bytes"
    );
    assert!(
        sweep.reusable_bytes > 0 || sweep.returned_bytes > 0,
        "dead old block should be reset for reuse or returned"
    );
    assert!(
        summary.reusable_bytes > 0 || summary.returned_bytes > 0,
        "old-page summary should expose current-cycle reclaim telemetry"
    );
    assert_eq!(
        crate::arena::old_arena_walk_objects_on_pages(&dead_pages, |_| {}),
        0,
        "dead old block pages must not retain stale object-index entries"
    );
    for page in dead_pages {
        assert!(
            crate::arena::old_page_meta_for_tests(page).is_none(),
            "dead old block page metadata should be cleared"
        );
    }
    unsafe {
        assert_eq!((*live_header).obj_type, GC_TYPE_STRING);
        assert_eq!((*live_header).gc_flags & GC_FLAG_MARKED, 0);
    }
}

#[test]
fn test_old_page_sweep_accounting_pinned_is_live_and_not_evacuation_eligible() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let pinned = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (pinned_header, pinned_total) = old_test_header_and_size(pinned);
    unsafe {
        (*pinned_header).gc_flags |= GC_FLAG_PINNED;
    }

    let _sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert_eq!(summary.live_bytes, pinned_total);
    assert_eq!(summary.dead_bytes, 0);
    assert_eq!(summary.pinned_bytes, pinned_total);
    assert_eq!(summary.live_object_count, 1);
    assert_eq!(summary.pinned_object_count, 1);
    assert_eq!(summary.evacuation_eligible_pages, 0);

    unsafe {
        (*pinned_header).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_old_page_sweep_accounting_spanning_object_distributes_bytes() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let user = crate::arena::arena_alloc_gc_old(4096 * 2 + 77, 8, GC_TYPE_STRING) as usize;
    let (header, total) = old_test_header_and_size(user);
    let overlaps = crate::arena::old_object_page_overlaps(header as usize, total);
    assert!(
        overlaps.len() > 1,
        "test object should span more than one old page"
    );
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED;
    }

    let _sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert_eq!(summary.live_bytes, total);
    assert_eq!(summary.dead_bytes, 0);
    assert_eq!(summary.live_object_count, overlaps.len());
    assert_eq!(summary.evacuation_eligible_pages, 0);
    for (page, bytes) in overlaps {
        let meta = crate::arena::old_page_meta_for_tests(page)
            .expect("spanned old page should have metadata");
        assert_eq!(meta.live_bytes, bytes);
        assert_eq!(meta.dead_bytes, 0);
        assert_eq!(meta.live_object_count, 1);
    }
}

#[test]
fn test_dirty_page_scan_accounts_old_page_dirty_slots() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    crate::arena::old_pages_begin_gc_cycle();

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);
    let meta = crate::arena::old_page_meta_for_tests(dirty_page)
        .expect("dirty old page should have metadata");
    let summary = crate::arena::old_page_summary();

    assert!(
        stats.dirty_slots_scanned >= 1,
        "remembered scan should visit at least the written old slot"
    );
    assert!(
        meta.dirty_slots >= 1,
        "old page metadata should count scanned dirty slots"
    );
    assert_eq!(summary.dirty_pages, 1);
    assert_eq!(summary.dirty_slots, meta.dirty_slots);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_dirty_slots_reset_lazily_across_cycles() {
    // #6181: `old_pages_begin_gc_cycle` no longer walks every old page to zero
    // `dirty_slots` — it bumps a per-cycle epoch, so last cycle's count reads
    // as zero without any per-page work, and the next dirty slot re-stamps
    // from 1. This asserts that epoch-scoped equivalence directly.
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();

    let (_old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);

    // Cycle A: two dirty-slot accountings land on the page.
    crate::arena::old_pages_begin_gc_cycle();
    crate::arena::old_page_account_dirty_slot(fields as usize);
    crate::arena::old_page_account_dirty_slot(fields as usize);
    let meta_a = crate::arena::old_page_meta_for_tests(dirty_page)
        .expect("old page should have metadata after allocation");
    assert_eq!(meta_a.dirty_slots, 2, "both accountings counted in cycle A");
    assert_eq!(crate::arena::old_page_summary().dirty_slots, 2);

    // Cycle B: a bare epoch bump (no per-page reset walk) must invalidate the
    // prior count — the page reads zero dirty slots even though nothing
    // touched its metadata.
    crate::arena::old_pages_begin_gc_cycle();
    let meta_b = crate::arena::old_page_meta_for_tests(dirty_page)
        .expect("old page metadata should persist across cycles");
    assert_eq!(
        meta_b.dirty_slots, 0,
        "epoch bump alone resets last cycle's dirty-slot count"
    );
    assert_eq!(crate::arena::old_page_summary().dirty_slots, 0);

    // A fresh dirty slot in cycle B re-stamps the page and counts from 1.
    crate::arena::old_page_account_dirty_slot(fields as usize);
    let meta_b2 = crate::arena::old_page_meta_for_tests(dirty_page)
        .expect("old page metadata should persist");
    assert_eq!(meta_b2.dirty_slots, 1, "re-stamp starts a fresh count");
    assert_eq!(crate::arena::old_page_summary().dirty_slots, 1);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_sweep_accounting_trace_json_includes_summary() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();

    let pinned = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (pinned_header, pinned_total) = old_test_header_and_size(pinned);
    unsafe {
        (*pinned_header).gc_flags |= GC_FLAG_PINNED;
    }

    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Direct,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");
    let event = trace.into_json(GcStepSnapshot::current());
    let old_pages = &event["old_pages"];

    assert!(old_pages["pages"].as_u64().unwrap_or(0) > 0);
    assert_eq!(old_pages["live_bytes"].as_u64(), Some(pinned_total as u64));
    assert_eq!(
        old_pages["pinned_bytes"].as_u64(),
        Some(pinned_total as u64)
    );
    assert_eq!(old_pages["dead_bytes"].as_u64(), Some(0));
    assert_eq!(old_pages["reusable_bytes"].as_u64(), Some(0));
    assert_eq!(old_pages["returned_bytes"].as_u64(), Some(0));
    assert_eq!(old_pages["pinned_object_count"].as_u64(), Some(1));
    assert_eq!(old_pages["evacuation_eligible_pages"].as_u64(), Some(0));
    assert!(event["sweep"]["dead_bytes"].as_u64().is_some());
    assert!(event["sweep"]["reusable_bytes"].as_u64().is_some());
    assert!(event["sweep"]["returned_bytes"].as_u64().is_some());
    assert_eq!(
        event["evacuation"]["released_original_reusable_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(
        event["evacuation"]["released_original_returned_bytes"].as_u64(),
        Some(0)
    );

    unsafe {
        (*pinned_header).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_old_page_defrag_policy_selection_prefers_fragmented_unpinned_pages() {
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

    let low_dead = meta(0x1000_0000, 100, 80, 20, 0);
    let high_dead = meta(0x1000_1000, 100, 10, 90, 0);
    let high_dead_more_live = meta(0x1000_2000, 100, 20, 80, 0);
    let pinned = meta(0x1000_3000, 100, 10, 90, 8);
    let empty = meta(0x1000_4000, 0, 0, 0, 0);
    let snapshot = [low_dead, high_dead_more_live, pinned, empty, high_dead];

    let selection = select_old_page_defrag_pages_from_snapshot(&snapshot, false);
    let high_dead_page = crate::arena::generation_page_for_addr(high_dead.page_base);
    let high_dead_more_live_page =
        crate::arena::generation_page_for_addr(high_dead_more_live.page_base);
    let low_dead_page = crate::arena::generation_page_for_addr(low_dead.page_base);

    assert_eq!(selection.candidate_pages, 3);
    assert_eq!(selection.selected_pages, 2);
    assert_eq!(selection.selected_live_bytes, 30);
    assert_eq!(selection.selected_reclaimable_bytes, 170);
    assert_eq!(selection.skipped_pinned_pages, 1);
    assert!(selection.pages.contains(&high_dead_page));
    assert!(selection.pages.contains(&high_dead_more_live_page));
    assert!(!selection.pages.contains(&low_dead_page));
    assert_eq!(
        selection.page_order,
        vec![high_dead_page, high_dead_more_live_page],
        "selected pages should be ordered by highest dead ratio, then lowest live bytes"
    );

    let forced = select_old_page_defrag_pages_from_snapshot(&snapshot, true);
    assert_eq!(forced.selected_pages, 3);
    assert!(forced.pages.contains(&low_dead_page));
    assert_eq!(forced.skipped_pinned_pages, 1);
}

#[test]
fn test_old_page_defrag_forced_moves_only_marked_old_objects_on_selected_pages() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let movable = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let unmarked = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let (movable_header, movable_total) = old_test_header_and_size(movable);
    let (unmarked_header, _) = old_test_header_and_size(unmarked);
    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(movable_header as usize, movable_total)
    {
        selected_pages.insert(page);
    }
    let source_blocks = crate::arena::old_arena_source_blocks_for_pages(&selected_pages);
    unsafe {
        (*movable_header).gc_flags |= GC_FLAG_MARKED;
    }

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(moved.old_page_moved_objects, 1);
    assert_eq!(moved.old_page_moved_bytes, movable_total);
    assert_eq!(new_headers.len(), 1);
    assert_eq!(original_headers, vec![movable_header]);
    assert!(
        old_object_pages_disjoint_from_selected(new_headers[0], movable_total, &selected_pages),
        "old-page copy must not land in any selected source page"
    );
    assert!(
        old_object_pages_disjoint_from_selected(
            new_headers[0],
            movable_total,
            &source_blocks.pages
        ),
        "old-page copy must not land in the selected source block"
    );
    unsafe {
        assert_ne!((*movable_header).gc_flags & GC_FLAG_FORWARDED, 0);
        assert_eq!(
            (*unmarked_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "unmarked old object on the selected page must not move"
        );
        assert!(crate::arena::pointer_in_old_gen(
            forwarding_address(movable_header) as usize
        ));
    }

    let released = release_evacuated_original_forwarding_stubs(&original_headers);
    assert_eq!(released.released_original_objects, 1);
    assert_eq!(released.released_original_reusable_bytes, 0);
    assert_eq!(released.released_original_returned_bytes, 0);
    clear_marks();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_old_page_defrag_copy_avoids_selected_pages_and_rebuilds_remembered_set() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let parent_total = unsafe { (*parent_header).size as usize };
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(parent_header as usize, parent_total) {
        selected_pages.insert(page);
    }
    let source_blocks = crate::arena::old_arena_source_blocks_for_pages(&selected_pages);
    unsafe {
        *fields = ptr_bits(child);
        (*parent_header).gc_flags |= GC_FLAG_MARKED;
    }
    js_write_barrier_slot(ptr_bits(parent_user), fields as u64, ptr_bits(child));

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(moved.old_page_moved_objects, 1);
    assert_eq!(new_headers.len(), 1);
    assert!(
        old_object_pages_disjoint_from_selected(new_headers[0], parent_total, &selected_pages),
        "forwarded old-page copy must land outside all selected source pages"
    );
    assert!(
        old_object_pages_disjoint_from_selected(new_headers[0], parent_total, &source_blocks.pages),
        "forwarded old-page copy must land outside the selected source block"
    );
    unsafe {
        let forwarded_page =
            crate::arena::generation_page_for_addr(forwarding_address(parent_header) as usize);
        assert!(
            !selected_pages.contains(&forwarded_page),
            "forwarded address page must not be a selected source page"
        );
    }

    let sticky = rebuild_evacuated_old_to_young_remembered_set(&new_headers);
    remembered_set_clear();
    sticky.restore();
    let released = release_evacuated_original_forwarding_stubs(&original_headers);
    assert_eq!(released.released_original_objects, 1);
    assert!(
        remembered_set_size() > 0,
        "rebuilt remembered set should keep the evacuated old-to-young edge dirty"
    );

    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(
        stats.newly_marked > 0,
        "rebuilt remembered set should mark the young child"
    );
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "young child should remain reachable through the moved old parent"
        );
    }

    clear_marks();
    remembered_set_clear();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_old_page_defrag_skips_pinned_old_objects() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let pinned = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let (pinned_header, pinned_total) = old_test_header_and_size(pinned);
    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(pinned_header as usize, pinned_total) {
        selected_pages.insert(page);
    }
    unsafe {
        (*pinned_header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_PINNED;
    }

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(moved.old_page_moved_objects, 0);
    assert!(new_headers.is_empty());
    assert!(original_headers.is_empty());
    unsafe {
        assert_eq!(
            (*pinned_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "pinned old object address must remain stable"
        );
        (*pinned_header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_PINNED);
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_old_page_defrag_skips_non_movable_buffer_and_typed_array() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let buffer = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
    let typed_array = crate::typedarray::typed_array_alloc(
        crate::typedarray::KIND_UINT8,
        LARGE_OBJECT_THRESHOLD_BYTES as u32,
    ) as usize;
    let (buffer_header, buffer_total) = old_test_header_and_size(buffer);
    let (typed_array_header, typed_array_total) = old_test_header_and_size(typed_array);
    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(buffer_header as usize, buffer_total) {
        selected_pages.insert(page);
    }
    for (page, _) in
        crate::arena::old_object_page_overlaps(typed_array_header as usize, typed_array_total)
    {
        selected_pages.insert(page);
    }
    unsafe {
        (*buffer_header).gc_flags |= GC_FLAG_MARKED;
        (*typed_array_header).gc_flags |= GC_FLAG_MARKED;
    }

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(moved.old_page_moved_objects, 0);
    assert_eq!(moved.old_page_moved_bytes, 0);
    assert!(new_headers.is_empty());
    assert!(original_headers.is_empty());
    unsafe {
        assert_eq!(
            (*buffer_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "old Buffer address must remain stable"
        );
        assert_eq!(
            (*typed_array_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "old TypedArray address must remain stable"
        );
        (*buffer_header).gc_flags &= !GC_FLAG_MARKED;
        (*typed_array_header).gc_flags &= !GC_FLAG_MARKED;
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_old_page_defrag_re_remembers_young_child_after_collection_clear() {
    let _defrag = OldDefragTestEnable::new();
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
    let _scan = ConservativeScanAutoGuard::new();
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = EnvVarGuard::set("PERRY_GC_FORCE_EVACUATE", "1");
    let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
    reset_shadow_stack();
    reset_global_roots();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let _dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    unsafe {
        (*parent_header).gc_flags |= GC_FLAG_MARKED;
    }
    let _ = sweep_with_age_bump(false);

    let frame = js_shadow_frame_push(1);
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);
    unsafe {
        *fields = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(parent_user), fields as u64, ptr_bits(child));
    js_shadow_slot_set(0, ptr_bits(parent_user));

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert!(
        trace.evacuation.old_page_moved_objects >= 1,
        "forced old-page defrag should move the rooted old parent"
    );
    let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(parent_after, parent_user);
    assert!(crate::arena::pointer_in_old_gen(parent_after));
    assert!(
        remembered_set_size() > 0,
        "moved old parent retaining a young child must be re-remembered after clear"
    );

    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(stats.newly_marked > 0);
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "rebuilt remembered set should mark the young child"
        );
    }

    js_shadow_frame_pop(frame);
}

#[test]
fn test_old_page_defrag_target_gate_emits_trace() {
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
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
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

    let _old_block_filler =
        crate::arena::arena_alloc_gc_old(2 * 1024 * 1024 - GC_HEADER_SIZE, 8, GC_TYPE_STRING);
    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let _dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let parent_total = unsafe { (*parent_header).size as usize };
    let mut parent_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(parent_header as usize, parent_total) {
        parent_pages.insert(page);
    }
    unsafe {
        (*parent_header).gc_flags |= GC_FLAG_MARKED;
    }
    let _ = sweep_with_age_bump(false);
    let selected_before = select_old_page_defrag_pages(true);
    assert!(
        parent_pages
            .iter()
            .any(|page| selected_before.pages.contains(page)),
        "forced old-page defrag policy should select the seeded fragmented parent page"
    );
    let source_blocks = crate::arena::old_arena_source_blocks_for_pages(&parent_pages);
    let source_pages = source_blocks.pages.iter().copied().collect::<Vec<_>>();

    let frame = js_shadow_frame_push(1);
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);
    unsafe {
        *fields = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(parent_user), fields as u64, ptr_bits(child));
    js_shadow_slot_set(0, ptr_bits(parent_user));

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        parent_after, parent_user,
        "forced old-page defrag should rewrite the shadow root to the moved parent"
    );
    assert!(crate::arena::pointer_in_old_gen(parent_after));
    assert!(
        remembered_set_size() > 0,
        "moved old parent retaining a young child must be re-remembered"
    );

    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(stats.newly_marked > 0);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert!(
        trace.evacuation_policy.snapshot.old_page_selected_pages > 0,
        "forced old-page defrag should report selected source pages"
    );
    assert!(
        trace.evacuation.old_page_moved_bytes > 0,
        "forced old-page defrag should report moved old-page bytes"
    );
    assert!(
        trace.sweep.reusable_bytes > 0 || trace.sweep.returned_bytes > 0,
        "forced old-page defrag should make the emptied source block reusable or returned"
    );
    assert!(
        trace.old_pages.reusable_bytes > 0 || trace.old_pages.returned_bytes > 0,
        "old-page telemetry should expose targeted source-block reclaim"
    );
    assert_eq!(
        crate::arena::old_arena_walk_objects_on_pages(&source_blocks.pages, |_| {}),
        0,
        "reclaimed old source block pages must not retain stale object-index entries"
    );
    for page in source_pages {
        assert!(
            crate::arena::old_page_meta_for_tests(page).is_none(),
            "reclaimed old source block page metadata should be cleared"
        );
    }

    js_shadow_frame_pop(frame);
    if gc_trace_enabled() {
        trace.emit(GcStepSnapshot::current());
    }
}

#[test]
fn test_old_page_defrag_trace_json_distinguishes_moved_from_reclaimable() {
    let mut trace = GcCycleTrace::new(
        GcCollectionKind::Minor,
        GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        },
    )
    .expect("test requested GC trace capture");
    trace.evacuation_policy.snapshot.old_page_candidate_pages = 2;
    trace.evacuation_policy.snapshot.old_page_selected_pages = 1;
    trace
        .evacuation_policy
        .snapshot
        .old_page_selected_live_bytes = 64;
    trace.evacuation_policy.snapshot.old_page_reclaimable_bytes = 192;
    trace.evacuation.old_page_moved_objects = 1;
    trace.evacuation.old_page_moved_bytes = 64;
    trace.evacuation.released_original_objects = 1;
    trace.evacuation.released_original_bytes = 64;
    trace.sweep.dead_bytes = 192;
    trace.sweep.freed_bytes = 192;
    trace.sweep.reusable_bytes = 128;
    trace.sweep.returned_bytes = 32;
    trace.sweep.deallocated_bytes = 32;

    let event = trace.into_json(GcStepSnapshot::current());

    assert_eq!(
        event["evacuation_policy"]["old_page_reclaimable_bytes"].as_u64(),
        Some(192)
    );
    assert_eq!(
        event["evacuation_policy"]["old_page_selected_live_bytes"].as_u64(),
        Some(64)
    );
    assert_eq!(
        event["evacuation"]["old_page_moved_bytes"].as_u64(),
        Some(64)
    );
    assert_eq!(
        event["evacuation"]["released_original_bytes"].as_u64(),
        Some(64)
    );
    assert_eq!(
        event["evacuation"]["released_original_reusable_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(
        event["evacuation"]["released_original_returned_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(event["sweep"]["dead_bytes"].as_u64(), Some(192));
    assert_eq!(event["sweep"]["freed_bytes"].as_u64(), Some(192));
    assert_eq!(event["sweep"]["reusable_bytes"].as_u64(), Some(128));
    assert_eq!(event["sweep"]["returned_bytes"].as_u64(), Some(32));
    assert_eq!(event["sweep"]["deallocated_bytes"].as_u64(), Some(32));
}

/// The delta-maintained `OLD_GEN_IN_USE_BYTES` cache must agree with the
/// O(blocks) recompute across every old-arena mutation class: small bump
/// allocations, oversized-block allocation, sweep-driven block reset,
/// block deallocation back to the OS, post-reset block reuse, and a full
/// explicit collection. (`old_gen_in_use_bytes()` additionally
/// debug-asserts the same invariant on every read, so every other GC
/// test — including the old-page defrag suite — cross-checks it too.)
#[test]
fn test_old_gen_in_use_bytes_delta_matches_recompute_across_alloc_and_reclaim() {
    let _isolation = copying_nursery_isolation_lock();
    // Keep alloc-point triggers quiet so the mid-test totals compare
    // deterministically; the explicit sweep/collect calls below still run.
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    fn assert_consistent(label: &str) {
        assert_eq!(
            crate::arena::old_gen_in_use_bytes(),
            crate::arena::old_gen_in_use_bytes_recomputed(),
            "{label}: cached old-gen in-use bytes drifted from the per-block recompute"
        );
    }

    assert_consistent("baseline");

    // Small old-arena bump allocations (also materializes the lazy
    // initial block on a fresh test thread).
    let live = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    for _ in 0..32 {
        let _ = unsafe { alloc_old_test_object(4) };
    }
    assert_consistent("after small old allocs");
    let before_oversized = crate::arena::old_gen_in_use_bytes();

    // Oversized allocation: forces a custom-sized fresh-block install +
    // bump through the fresh-block alloc path.
    let dead_big = crate::arena::arena_alloc_gc_old(2 * 1024 * 1024, 8, GC_TYPE_STRING) as usize;
    let (_, dead_total) = old_test_header_and_size(dead_big);
    assert_consistent("after oversized old alloc");
    assert!(
        crate::arena::old_gen_in_use_bytes() >= before_oversized + dead_total,
        "oversized alloc must raise the cached in-use total"
    );

    // Sweep + old-block reclaim: the oversized block dies (reset and/or
    // deallocated), the block holding `live` survives.
    let (live_header, _) = old_test_header_and_size(live);
    unsafe {
        (*live_header).gc_flags |= GC_FLAG_MARKED;
    }
    let old_before_sweep = crate::arena::old_gen_in_use_bytes();
    let _ = sweep_with_age_bump_and_old_reclaim(false, true);
    assert_consistent("after sweep + old block reclaim");
    assert!(
        crate::arena::old_gen_in_use_bytes() < old_before_sweep,
        "dead old block reclaim must lower the cached in-use total"
    );

    // Re-allocate after the reclaim so the reset-block reuse path
    // (forward scan into an offset-0 block) is delta-tracked too.
    for _ in 0..8 {
        let _ = unsafe { alloc_old_test_object(2) };
    }
    assert_consistent("after post-reclaim reuse allocs");

    // Full explicit collection end-to-end (mark, sweep, reclaim,
    // possible evacuation-policy pass).
    js_gc_collect();
    assert_consistent("after full explicit collection");

    clear_marks();
    remembered_set_clear();
}

/// #6181 (2026-07-09 GC audit): a minor collection must NOT run the
/// whole-heap old→young remembered-set rebuild. A minor's old→young RS is
/// maintained by the write barriers plus this cycle's `evacuation_sticky`
/// and reclaim's `restore_surviving_dirty_coverage`, so the from-scratch
/// O(all-objects) walk is skipped. A FULL cycle still runs it. Proven via
/// the trace's `remembered_set.rebuild_objects_scanned` object-visit counter,
/// which scales with the heap for a full cycle and is 0 for a minor.
#[test]
fn test_minor_skips_whole_heap_old_to_young_rebuild() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    // A substantial old-gen live set (pinned so it survives and is not
    // finalized). If a minor walked the old gen for the RS rebuild, the
    // object-visit counter would scale with this set.
    const OLD_OBJECTS: usize = 64;
    let mut old_headers = Vec::with_capacity(OLD_OBJECTS);
    for _ in 0..OLD_OBJECTS {
        let (obj, _fields) = unsafe { alloc_old_test_object(2) };
        let header = unsafe { header_from_user_ptr(obj as *const u8) };
        unsafe {
            (*header).gc_flags |= GC_FLAG_PINNED;
        }
        old_headers.push(header);
    }
    // A little nursery churn so the minor has real young work.
    for _ in 0..8 {
        let _ = unsafe { alloc_nursery_test_object(1) };
    }

    let minor_trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_eq!(
        minor_trace.old_to_young_rebuild_objects_scanned, 0,
        "a minor must not run the whole-heap old→young RS rebuild"
    );
    let minor_event = minor_trace.into_json(GcStepSnapshot::current());
    assert_eq!(
        minor_event["remembered_set"]["rebuild_objects_scanned"].as_u64(),
        Some(0),
        "minor RS rebuild object-visit count must be 0 in the trace JSON"
    );

    // A full cycle DOES run the rebuild — the counter proves it is wired and
    // scales with the (now-large) heap, so the minor's 0 is a genuine skip
    // rather than the counter being dead.
    let full_outcome = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Direct,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let full_trace = full_outcome.trace.expect("full GC trace requested");
    assert!(
        full_trace.old_to_young_rebuild_objects_scanned >= OLD_OBJECTS,
        "a full cycle must walk the whole heap for the RS rebuild (got {}, expected >= {OLD_OBJECTS})",
        full_trace.old_to_young_rebuild_objects_scanned,
    );

    for header in old_headers {
        unsafe {
            (*header).gc_flags &= !GC_FLAG_PINNED;
        }
    }
    clear_marks();
    remembered_set_clear();
}

/// #6181: an old→young edge recorded before a minor must survive the minor
/// (young child kept via the remembered set) across repeated minors, even
/// though the minor no longer rebuilds the RS from a whole-heap walk. The
/// edge is carried entirely by the write barrier → reclaim's
/// `restore_surviving_dirty_coverage`. This is the under-remembering
/// (use-after-free) guard for Fix 2: if any minor dropped the edge, the
/// child would be swept and the RS root mark below would not reach it.
#[test]
fn test_minor_preserves_old_to_young_edge_across_minors() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    // Pinned old parent (stable address, always kept) holding a young child
    // reachable ONLY through the parent's field.
    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    unsafe {
        (*parent_header).gc_flags |= GC_FLAG_PINNED;
    }
    // Unrelated large old-gen set with no young children (makes the old gen
    // big enough that a whole-heap rebuild would be visibly costly).
    let mut other_old = Vec::new();
    for _ in 0..48 {
        let (obj, _f) = unsafe { alloc_old_test_object(1) };
        let h = unsafe { header_from_user_ptr(obj as *const u8) };
        unsafe {
            (*h).gc_flags |= GC_FLAG_PINNED;
        }
        other_old.push(h);
    }

    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    assert!(crate::arena::pointer_in_nursery(child));
    unsafe {
        *fields = ptr_bits(child);
        layout_note_slot(parent_user, 0, ptr_bits(child));
    }
    js_write_barrier_slot(ptr_bits(parent_user), fields as u64, ptr_bits(child));
    assert!(
        remembered_set_size() > 0,
        "barrier must record the old→young edge"
    );

    for cycle in 0..4 {
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_eq!(
            trace.old_to_young_rebuild_objects_scanned, 0,
            "cycle {cycle}: minor must skip the whole-heap RS rebuild"
        );
        // The child (reachable only via the old parent) must still be covered
        // by the remembered set: RS root marking reaches and marks it.
        assert!(
            remembered_set_size() > 0,
            "cycle {cycle}: old→young edge must survive the minor"
        );
        assert_eq!(
            unsafe { *fields },
            ptr_bits(child),
            "cycle {cycle}: non-moving minor must leave the parent's slot intact"
        );
        clear_marks();
        let valid_ptrs = build_valid_pointer_set();
        let stats = mark_remembered_set_roots(&valid_ptrs);
        assert!(
            stats.newly_marked > 0,
            "cycle {cycle}: RS root marking must reach the child"
        );
        unsafe {
            assert_ne!(
                (*child_header).gc_flags & GC_FLAG_MARKED,
                0,
                "cycle {cycle}: young child must be markable via the surviving RS edge"
            );
            // Clear the child's mark so the next minor re-derives its coverage
            // from the remembered set alone (not a stale mark).
            (*child_header).gc_flags &= !GC_FLAG_MARKED;
        }
    }

    unsafe {
        (*parent_header).gc_flags &= !GC_FLAG_PINNED;
        for h in other_old {
            (*h).gc_flags &= !GC_FLAG_PINNED;
        }
    }
    clear_marks();
    remembered_set_clear();
}
