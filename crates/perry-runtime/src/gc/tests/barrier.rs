use super::super::barrier::RememberedSetClearState;
use super::super::*;
use super::support::*;

unsafe fn alloc_old_test_map(
    capacity: u32,
) -> (*mut crate::map::MapHeader, *mut u64, std::alloc::Layout) {
    let map = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::map::MapHeader>(),
        8,
        GC_TYPE_MAP,
    ) as *mut crate::map::MapHeader;
    let layout = std::alloc::Layout::from_size_align((capacity as usize * 16).max(8), 8)
        .expect("valid map entries layout");
    let entries = std::alloc::alloc_zeroed(layout) as *mut u64;
    assert!(!entries.is_null());
    (*map).size = 0;
    (*map).capacity = capacity;
    (*map).entries = entries as *mut f64;
    (map, entries, layout)
}

unsafe fn retire_old_test_map(
    map: *mut crate::map::MapHeader,
    entries: *mut u64,
    layout: std::alloc::Layout,
) {
    (*map).size = 0;
    (*map).capacity = 0;
    (*map).entries = std::ptr::null_mut();
    std::alloc::dealloc(entries as *mut u8, layout);
}

unsafe fn field_index_not_on_last_page(fields: *mut u64, field_count: u32) -> usize {
    assert!(field_count > 1);
    let last_page =
        crate::arena::generation_page_for_addr(fields.add(field_count as usize - 1) as usize);
    for i in 0..field_count as usize {
        if crate::arena::generation_page_for_addr(fields.add(i) as usize) != last_page {
            return i;
        }
    }
    panic!("test object did not span multiple field pages");
}

unsafe fn field_indices_on_distinct_pages(fields: *mut u64, field_count: u32) -> (usize, usize) {
    assert!(field_count > 1);
    let first = field_index_not_on_last_page(fields, field_count);
    let first_page = crate::arena::generation_page_for_addr(fields.add(first) as usize);
    for i in 0..field_count as usize {
        if crate::arena::generation_page_for_addr(fields.add(i) as usize) != first_page {
            return (first, i);
        }
    }
    panic!("test object did not span two field pages");
}

fn remembered_maintenance_entry_count() -> usize {
    let dirty_old = DIRTY_OLD_PAGES.with(|s| s.borrow().len());
    let external_dirty =
        EXTERNAL_DIRTY_SLOT_PAGES.with(|s| s.borrow().values().map(Vec::len).sum::<usize>());
    let fallback = REMEMBERED_SET.with(|s| s.borrow().len());
    dirty_old + external_dirty + fallback
}

#[test]
fn test_write_barrier_old_to_young_records() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let parent_nanbox = POINTER_TAG | (old as u64);
    let child_nanbox = POINTER_TAG | (young as u64);
    let dirty_page = crate::arena::generation_page_for_addr(old - GC_HEADER_SIZE);
    assert_eq!(remembered_set_size(), 0);
    assert!(!old_page_dirty_for(dirty_page));
    js_write_barrier(parent_nanbox, child_nanbox);
    assert_eq!(
        remembered_set_size(),
        1,
        "old→young write must dirty the remembered page"
    );
    assert!(
        old_page_dirty_for(dirty_page),
        "old-page metadata should mirror the remembered dirty page"
    );
    // Same write again should NOT double-count (dirty pages dedup).
    js_write_barrier(parent_nanbox, child_nanbox);
    assert_eq!(
        remembered_set_size(),
        1,
        "duplicate barrier call must dedup the dirty page"
    );
    assert!(old_page_dirty_for(dirty_page));
}

#[test]
fn test_write_barrier_slot_marks_dirty_page_and_dedups() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);
    assert!(!old_page_dirty_for(dirty_page));
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(remembered_dirty_page_count(), 1);
    assert!(
        old_page_dirty_for(dirty_page),
        "old-page metadata should mirror the remembered dirty page"
    );
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(
        remembered_dirty_page_count(),
        1,
        "same dirty page should be logged once"
    );
    assert!(old_page_dirty_for(dirty_page));
}

#[test]
fn test_barriered_slot_store_api_trace_counters() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let tracing = gc_trace_enabled();
    let _ = take_write_barrier_trace_counters();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(2048) };
    let child_bits = POINTER_TAG | young as u64;
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);

    unsafe {
        layout_init_pointer_free(old_obj as *mut u8);
    }
    runtime_store_jsvalue_slot(old_obj as usize, fields as usize, 0, child_bits);

    unsafe {
        assert_eq!(*fields, child_bits);
    }
    assert_eq!(
        test_layout_pointer_slot_count(old_obj as usize, 2048),
        Some(1)
    );
    assert_eq!(remembered_dirty_page_count(), 1);
    assert!(old_page_dirty_for(dirty_page));

    let counters = take_write_barrier_trace_counters();
    if tracing {
        assert_eq!(counters.calls, 1);
        assert_eq!(counters.old_to_young_slow_hits, 1);
        assert_eq!(counters.remembered_set_insert_attempts, 1);
        assert_eq!(counters.dirty_page_mark_attempts, 1);
        assert_eq!(counters.new_dirty_pages, 1);
    }
}

#[test]
fn test_remembered_set_clear_state_slices_maintenance_entries() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();

    let old = crate::arena::arena_alloc_gc_old(24 * 1024, 8, GC_TYPE_STRING) as usize;
    let old_header = unsafe { header_from_user_ptr(old as *const u8) };
    let old_total = unsafe { (*old_header).size as usize };
    let dirty_pages: Vec<usize> =
        crate::arena::old_object_page_overlaps(old_header as usize, old_total)
            .into_iter()
            .map(|(page, _)| page)
            .take(3)
            .collect();
    assert!(
        dirty_pages.len() >= 3,
        "test old object should span at least three old pages"
    );
    for &page in &dirty_pages {
        mark_dirty_old_page(page);
        assert!(old_page_dirty_for(page));
    }

    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
        let mut pages = s.borrow_mut();
        pages.insert(0x1000, vec![0x10, 0x20]);
        pages.insert(0x2000, vec![0x30]);
    });
    REMEMBERED_SET.with(|s| {
        let mut headers = s.borrow_mut();
        headers.insert(0x40);
        headers.insert(0x50);
    });

    let initial = remembered_maintenance_entry_count();
    assert_eq!(initial, dirty_pages.len() + 5);

    let mut state = RememberedSetClearState::new();
    assert!(
        !state.step(1),
        "one cleanup unit must not drain all maintenance structures"
    );
    assert_eq!(remembered_maintenance_entry_count(), initial - 1);
    assert!(
        DIRTY_OLD_PAGES.with(|s| !s.borrow().is_empty()),
        "one cleanup unit should remove one dirty old page, not bulk-clear the set"
    );

    let mut calls = 1usize;
    while !state.step(1) {
        calls += 1;
        assert!(
            calls <= initial,
            "remembered cleanup should finish after one call per maintenance entry"
        );
    }

    assert!(calls > 1);
    assert_eq!(remembered_maintenance_entry_count(), 0);
    for page in dirty_pages {
        assert!(
            !old_page_dirty_for(page),
            "dirty old-page metadata should be clear after cleanup completes"
        );
    }
}

#[test]
fn test_write_barrier_young_to_young_skipped() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let parent = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child = unsafe { alloc_nursery_test_object(0).0 as usize };
    js_write_barrier(POINTER_TAG | (parent as u64), POINTER_TAG | (child as u64));
    assert_eq!(
        remembered_set_size(),
        0,
        "young→young write must not enter remembered set"
    );
}

#[test]
fn test_write_barrier_old_to_old_skipped() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let parent = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    js_write_barrier(POINTER_TAG | (parent as u64), POINTER_TAG | (child as u64));
    assert_eq!(
        remembered_set_size(),
        0,
        "old→old write must not enter remembered set (no inter-gen edge)"
    );
}

#[test]
fn test_write_barrier_old_to_young_string_tag() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let young_str = crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
    let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    // STRING_TAG should also fire the barrier — strings can be young.
    js_write_barrier(POINTER_TAG | (old as u64), STRING_TAG | (young_str as u64));
    assert_eq!(remembered_set_size(), 1);
}

#[test]
fn test_write_barrier_non_pointer_child_skipped() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    // INT32_TAG in child position.
    let int32_val = 0x7FFE_0000_0000_002A_u64;
    js_write_barrier(POINTER_TAG | (old as u64), int32_val);
    assert_eq!(
        remembered_set_size(),
        0,
        "non-pointer child must not enter remembered set"
    );
    // SHORT_STRING_TAG (SSO inline) — also not a heap pointer.
    let sso = 0x7FF9_0500_0000_0000_u64;
    js_write_barrier(POINTER_TAG | (old as u64), sso);
    assert_eq!(
        remembered_set_size(),
        0,
        "SSO child is inline data, not a heap pointer"
    );
    // Plain double in child position.
    js_write_barrier(POINTER_TAG | (old as u64), 3.14_f64.to_bits());
    assert_eq!(
        remembered_set_size(),
        0,
        "number child must not enter remembered set"
    );
}

#[test]
fn test_write_barrier_non_pointer_parent_skipped() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    js_write_barrier_slot(0x7FFE_0000_0000_002A_u64, 0, POINTER_TAG | young as u64);
    assert_eq!(
        remembered_set_size(),
        0,
        "non-pointer parent must not dirty remembered pages"
    );
}

#[test]
fn test_write_barrier_remembered_set_clear() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let dirty_page = crate::arena::generation_page_for_addr(old - GC_HEADER_SIZE);
    js_write_barrier(POINTER_TAG | (old as u64), POINTER_TAG | (young as u64));
    assert_eq!(remembered_set_size(), 1);
    assert!(old_page_dirty_for(dirty_page));
    remembered_set_clear();
    assert_eq!(remembered_set_size(), 0);
    assert!(
        !old_page_dirty_for(dirty_page),
        "old-page metadata dirty bit should clear with the remembered set"
    );
}

#[test]
fn test_write_barrier_slot_clear() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(remembered_dirty_page_count(), 1);
    assert!(old_page_dirty_for(dirty_page));
    remembered_set_clear();
    assert_eq!(remembered_dirty_page_count(), 0);
    assert_eq!(remembered_set_size(), 0);
    assert!(
        !old_page_dirty_for(dirty_page),
        "old-page metadata dirty bit should clear with the remembered set"
    );
}

#[test]
fn test_gc_collect_minor_restores_live_old_young_rs() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(remembered_set_size(), 1);
    let _freed = gc_collect_minor();
    assert!(
        remembered_set_size() > 0,
        "minor GC should restore remembered metadata for live old-to-young edges"
    );
    let stats = verify_old_to_young_edges_covered();
    assert_eq!(stats.missing_edges, 0);
}

#[test]
fn test_dirty_page_scan_marks_young_child() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
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
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 1);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert!(
        stats.dirty_slots_scanned >= 1,
        "dirty page should scan at least the written field slot"
    );
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let child_header = header_from_user_ptr(young as *const u8);
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_rejects_unbarriered_old_object_field() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let old_header = unsafe { header_from_user_ptr(old_obj as *const u8) };
    unsafe {
        *fields = ptr_bits(young);
        (*old_header).gc_flags |= GC_FLAG_MARKED;
    }

    let result = std::panic::catch_unwind(verify_old_to_young_edges_covered);

    assert!(
        result.is_err(),
        "unbarriered live old-to-young field must fail the verifier"
    );
    unsafe {
        (*old_header).gc_flags &= !GC_FLAG_MARKED;
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_accepts_barriered_old_object_field() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let old_header = unsafe { header_from_user_ptr(old_obj as *const u8) };
    unsafe {
        *fields = ptr_bits(young);
        (*old_header).gc_flags |= GC_FLAG_MARKED;
    }
    js_write_barrier_slot(ptr_bits(old_obj as usize), fields as u64, ptr_bits(young));

    let stats = verify_old_to_young_edges_covered();

    assert_eq!(stats.checked_old_to_young_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    unsafe {
        (*old_header).gc_flags &= !GC_FLAG_MARKED;
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_accepts_dirty_old_page_metadata() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let old_header = unsafe { header_from_user_ptr(old_obj as *const u8) };
    unsafe {
        *fields = ptr_bits(young);
        (*old_header).gc_flags |= GC_FLAG_MARKED;
    }
    mark_dirty_old_page(crate::arena::generation_page_for_addr(fields as usize));

    let stats = verify_old_to_young_edges_covered();

    assert_eq!(stats.checked_old_to_young_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    unsafe {
        (*old_header).gc_flags &= !GC_FLAG_MARKED;
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_rejects_object_fallback_only() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let old_header = unsafe { header_from_user_ptr(old_obj as *const u8) };
    unsafe {
        *fields = ptr_bits(young);
        (*old_header).gc_flags |= GC_FLAG_MARKED;
    }
    REMEMBERED_SET.with(|s| {
        s.borrow_mut().insert(old_header as usize);
    });

    let result = std::panic::catch_unwind(verify_old_to_young_edges_covered);

    assert!(
        result.is_err(),
        "test-only object fallback must not count as dirty-page coverage"
    );
    unsafe {
        (*old_header).gc_flags &= !GC_FLAG_MARKED;
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_accepts_barriered_array_element() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    let old_header = unsafe { header_from_user_ptr(old_arr as *const u8) };
    unsafe {
        *elements = ptr_bits(young);
        (*old_header).gc_flags |= GC_FLAG_MARKED;
    }
    js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(young));

    let stats = verify_old_to_young_edges_covered();

    assert_eq!(stats.checked_old_to_young_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    unsafe {
        (*old_header).gc_flags &= !GC_FLAG_MARKED;
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_accepts_map_external_slot() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(4) };
    let map_header = unsafe { header_from_user_ptr(map as *const u8) };
    unsafe {
        (*map).size = 1;
        *entries = ptr_bits(young);
        (*map_header).gc_flags |= GC_FLAG_MARKED;
    }
    write_barrier_slot_inner(
        ptr_bits(map as usize),
        entries as usize,
        ptr_bits(young),
        true,
    );

    let stats = verify_old_to_young_edges_covered();

    assert_eq!(stats.checked_old_to_young_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    unsafe {
        (*map_header).gc_flags &= !GC_FLAG_MARKED;
        retire_old_test_map(map, entries, layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_accepts_set_external_slot() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (set, elements, layout) = unsafe { alloc_old_test_set(1) };
    let set_header = unsafe { header_from_user_ptr(set as *const u8) };
    unsafe {
        (*set).size = 1;
        (*set_header).gc_flags |= GC_FLAG_MARKED;
    }
    runtime_store_external_jsvalue_slot(set as usize, elements as usize, ptr_bits(young));

    let stats = verify_old_to_young_edges_covered();

    assert_eq!(stats.checked_old_to_young_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    unsafe {
        (*set_header).gc_flags &= !GC_FLAG_MARKED;
        retire_old_test_set(set, elements, layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_accepts_promise_slot() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let promise = unsafe { alloc_old_test_promise() };
    let promise_header = unsafe { header_from_user_ptr(promise as *const u8) };
    unsafe {
        (*promise_header).gc_flags |= GC_FLAG_MARKED;
    }
    crate::promise::js_promise_resolve(promise, f64::from_bits(ptr_bits(young)));

    let stats = verify_old_to_young_edges_covered();

    assert_eq!(stats.checked_old_to_young_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    unsafe {
        (*promise_header).gc_flags &= !GC_FLAG_MARKED;
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_young_edge_verifier_trace_json_shape() {
    let _guard = GcTestIsolationGuard::new();
    let mut trace = GcCycleTrace::new(
        GcCollectionKind::Minor,
        GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        },
    )
    .expect("test requested GC trace capture");
    trace.old_young_edge_verifier = OldYoungEdgeVerifyStats {
        checked_old_objects: 3,
        checked_remembered_pages: 2,
        checked_old_to_young_edges: 1,
        missing_edges: 1,
        first_missing: Some(OldYoungEdgeMissing {
            parent: 0x1111,
            slot: 0x2222,
            child: 0x3333,
        }),
    };
    trace.record_phase("old_young_edge_verify", std::time::Duration::from_micros(7));

    let event = trace.into_json(GcStepSnapshot::current());

    assert_eq!(event["old_young_edge_verifier"]["checked_old_objects"], 3);
    assert_eq!(
        event["old_young_edge_verifier"]["checked_remembered_pages"],
        2
    );
    assert_eq!(
        event["old_young_edge_verifier"]["checked_old_to_young_edges"],
        1
    );
    assert_eq!(event["old_young_edge_verifier"]["missing_edges"], 1);
    assert_eq!(
        event["old_young_edge_verifier"]["first_missing"]["parent"],
        0x1111
    );
    assert_eq!(event["phase_us"]["old_young_edge_verify"], 7);
}

#[test]
fn test_dirty_page_scan_skips_pointer_free_old_object_payload_slots() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let (old_obj, fields) = unsafe { alloc_old_test_object(2048) };
    let dirty_idx = unsafe { field_index_not_on_last_page(fields, 2048) };
    let dirty_slot = unsafe { fields.add(dirty_idx) };
    unsafe {
        layout_init_pointer_free(old_obj as *mut u8);
        *dirty_slot = 42.0_f64.to_bits();
        mark_dirty_old_page(crate::arena::generation_page_for_addr(dirty_slot as usize));
    }

    assert_eq!(
        test_layout_pointer_slot_count(old_obj as usize, 2048),
        Some(0)
    );
    assert_eq!(test_heap_child_slot_count(old_obj as *mut u8), 0);

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 1);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(
        stats.dirty_slots_scanned, 0,
        "pointer-free old objects must not read payload slots during dirty-page scans"
    );
    assert_eq!(stats.dirty_slot_ranges_scanned, 0);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_array_scan_is_slot_range_bounded() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_arr, elements) = unsafe { alloc_old_test_array(2048) };
    let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(elements, 2048) };
    let dirty_slot = unsafe { elements.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
        *elements.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old_arr as u64,
        dirty_slot as u64,
        POINTER_TAG | dirty_child as u64,
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(stats.dirty_slot_ranges_scanned, 1);
    assert!(
        stats.dirty_slots_scanned <= 512,
        "one dirty page should scan at most one 4 KiB page of u64 slots"
    );
    unsafe {
        let dirty_header = header_from_user_ptr(dirty_child as *const u8);
        let clean_header = header_from_user_ptr(clean_child as *const u8);
        assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!((*clean_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_scan_ignores_clean_old_pages() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (dirty_obj, dirty_fields) = unsafe { alloc_old_test_object(2048) };
    let dirty_idx = unsafe { field_index_not_on_last_page(dirty_fields, 2048) };
    let dirty_slot = unsafe { dirty_fields.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
    }
    let (_clean_obj, clean_fields) = unsafe { alloc_old_test_object(2048) };
    let clean_idx = unsafe { field_index_not_on_last_page(clean_fields, 2048) };
    unsafe {
        *clean_fields.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }

    js_write_barrier_slot(
        POINTER_TAG | dirty_obj as u64,
        dirty_slot as u64,
        POINTER_TAG | dirty_child as u64,
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 1);
    assert_eq!(
        stats.old_objects_considered, 1,
        "clean old pages must not feed objects into the dirty scan"
    );
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(stats.dirty_slot_ranges_scanned, 1);
    assert!(
        stats.dirty_slots_scanned <= 512,
        "one dirty field page should not scan the whole old object"
    );
    unsafe {
        let dirty_header = header_from_user_ptr(dirty_child as *const u8);
        let clean_header = header_from_user_ptr(clean_child as *const u8);
        assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!(
            (*clean_header).gc_flags & GC_FLAG_MARKED,
            0,
            "young child stored only on a clean old page should not be marked"
        );
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_scan_dedupes_object_spanning_dirty_pages() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let young_a = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let young_b = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(2048) };
    let (idx_a, idx_b) = unsafe { field_indices_on_distinct_pages(fields, 2048) };
    let slot_a = unsafe { fields.add(idx_a) };
    let slot_b = unsafe { fields.add(idx_b) };
    unsafe {
        *slot_a = POINTER_TAG | young_a as u64;
        *slot_b = POINTER_TAG | young_b as u64;
    }

    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        slot_a as u64,
        POINTER_TAG | young_a as u64,
    );
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        slot_b as u64,
        POINTER_TAG | young_b as u64,
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 2);
    assert_eq!(
        stats.old_objects_considered, 1,
        "one object spanning two dirty pages should be considered once"
    );
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(stats.dirty_slot_pages_considered, 2);
    assert!(stats.dirty_slot_ranges_scanned <= 2);
    assert!(
        stats.dirty_slots_scanned <= 1024,
        "two dirty field pages should bound scanning to two pages"
    );
    assert_eq!(stats.newly_marked, 2);
    unsafe {
        let header_a = header_from_user_ptr(young_a as *const u8);
        let header_b = header_from_user_ptr(young_b as *const u8);
        assert_ne!((*header_a).gc_flags & GC_FLAG_MARKED, 0);
        assert_ne!((*header_b).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_map_entry_scan_is_external_range_bounded() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(2048) };
    unsafe {
        (*map).size = 2048;
    }
    let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(entries, 4096) };
    let dirty_slot = unsafe { entries.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
        *entries.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }
    write_barrier_slot_inner(
        POINTER_TAG | map as u64,
        dirty_slot as usize,
        POINTER_TAG | dirty_child as u64,
        true,
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 1);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(stats.dirty_slot_ranges_scanned, 1);
    assert!(
        stats.dirty_slots_scanned <= 512,
        "one dirty map entries page should not scan the whole map"
    );
    unsafe {
        let dirty_header = header_from_user_ptr(dirty_child as *const u8);
        let clean_header = header_from_user_ptr(clean_child as *const u8);
        assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!((*clean_header).gc_flags & GC_FLAG_MARKED, 0);
        retire_old_test_map(map, entries, layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_lazy_array_external_cache_scan_marks_bitmap_selected_child() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();

    let cached_length = 4usize;
    let lazy = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::json_tape::LazyArrayHeader>(),
        8,
        GC_TYPE_LAZY_ARRAY,
    ) as *mut crate::json_tape::LazyArrayHeader;
    let cache_bytes = cached_length * std::mem::size_of::<crate::value::JSValue>();
    let cache =
        crate::arena::arena_alloc_gc(cache_bytes, 8, GC_TYPE_STRING) as *mut crate::value::JSValue;
    let bitmap =
        crate::arena::arena_alloc_gc(std::mem::size_of::<u64>(), 8, GC_TYPE_STRING) as *mut u64;
    let selected_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let unselected_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;

    unsafe {
        std::ptr::write_bytes(cache as *mut u8, 0, cache_bytes);
        *bitmap = 0;
        (*lazy).cached_length = cached_length as u32;
        (*lazy).magic = crate::json_tape::LAZY_ARRAY_MAGIC;
        (*lazy).root_idx = 0;
        (*lazy).tape_len = 0;
        (*lazy).blob_str = std::ptr::null();
        (*lazy).materialized = std::ptr::null_mut();
        (*lazy).materialized_elements = cache;
        (*lazy).materialized_bitmap = bitmap;
        (*lazy).walk_idx = u32::MAX;
        (*lazy).walk_tape_pos = 0;
        (*lazy).cumulative_walk_steps = 0;

        *(cache.add(1) as *mut u64) = ptr_bits(selected_child);
        *(cache.add(2) as *mut u64) = ptr_bits(unselected_child);
        *bitmap = 1u64 << 1;
    }

    let lazy_header = unsafe { header_from_user_ptr(lazy as *const u8) };
    let dirty_cache_page = crate::arena::generation_page_for_addr(unsafe { cache.add(1) } as usize);
    assert!(mark_dirty_external_slot_page(
        lazy_header as usize,
        dirty_cache_page
    ));

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(
        stats.newly_marked, 1,
        "external lazy-array cache page should mark bitmap-selected nursery values"
    );
    unsafe {
        let selected_header = header_from_user_ptr(selected_child as *const u8);
        let unselected_header = header_from_user_ptr(unselected_child as *const u8);
        assert_ne!((*selected_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!(
            (*unselected_header).gc_flags & GC_FLAG_MARKED,
            0,
            "unset cache bitmap entries must not be treated as live slots"
        );
    }

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_map_external_dedupes_and_clears() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(16) };
    unsafe {
        (*map).size = 16;
        *entries.add(1) = POINTER_TAG | young as u64;
    }
    let slot = unsafe { entries.add(1) };
    write_barrier_slot_inner(
        POINTER_TAG | map as u64,
        slot as usize,
        POINTER_TAG | young as u64,
        true,
    );
    write_barrier_slot_inner(
        POINTER_TAG | map as u64,
        slot as usize,
        POINTER_TAG | young as u64,
        true,
    );
    assert_eq!(remembered_set_size(), 1);
    remembered_set_clear();
    assert_eq!(remembered_set_size(), 0);
    unsafe {
        retire_old_test_map(map, entries, layout);
    }
}

#[test]
fn test_dirty_page_map_realloc_span_marks_new_entries_pages() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(1024) };
    unsafe {
        (*map).size = 1024;
        *entries.add(1023) = POINTER_TAG | young as u64;
    }
    let new_layout = std::alloc::Layout::from_size_align(2048 * 16, 8).unwrap();
    let new_entries = unsafe { std::alloc::alloc_zeroed(new_layout) as *mut u64 };
    assert!(!new_entries.is_null());
    unsafe {
        std::ptr::copy_nonoverlapping(entries, new_entries, 2048);
        (*map).entries = new_entries as *mut f64;
        (*map).capacity = 2048;
    }
    dirty_external_slot_span(map as usize, new_entries as usize, 2048);

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(stats.dirty_pages_scanned >= 1);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let header = header_from_user_ptr(young as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
        retire_old_test_map(map, new_entries, new_layout);
        std::alloc::dealloc(entries as *mut u8, layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_set_external_slot_marks_child() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (set, elements, layout) = unsafe { alloc_old_test_set(1) };
    unsafe {
        (*set).size = 1;
    }
    runtime_store_external_jsvalue_slot(set as usize, elements as usize, ptr_bits(young));

    assert!(
        remembered_set_size() > 0,
        "Set append should dirty the exact external slot page"
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let header = header_from_user_ptr(young as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
    }

    unsafe {
        retire_old_test_set(set, elements, layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_rewrite_remembered_dirty_range_updates_set_external_entry_span() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();

    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (set, elements, layout) = unsafe { alloc_old_test_set(2048) };
    unsafe {
        (*set).size = 2048;
    }
    let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(elements, 2048) };
    let dirty_slot = unsafe { elements.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
        *elements.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }
    write_barrier_slot_inner(
        POINTER_TAG | set as u64,
        dirty_slot as usize,
        POINTER_TAG | dirty_child as u64,
        true,
    );

    let valid_ptrs = build_valid_pointer_set();
    let new_dirty_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let new_clean_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(dirty_child as *const u8),
            new_dirty_child as *mut u8,
        );
        set_forwarding_address(
            header_from_user_ptr(clean_child as *const u8),
            new_clean_child as *mut u8,
        );
    }

    rewrite_remembered_dirty_ranges(&valid_ptrs);

    unsafe {
        assert_eq!(*dirty_slot, POINTER_TAG | new_dirty_child as u64);
        assert_eq!(
            *elements.add(clean_idx),
            POINTER_TAG | clean_child as u64,
            "Set external dirty rewrite should stay bounded to the logged element page"
        );
        retire_old_test_set(set, elements, layout);
    }
    remembered_set_clear();
}

#[test]
fn test_dirty_page_promise_value_slot_marks_child() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let promise = unsafe { alloc_old_test_promise() };
    crate::promise::js_promise_resolve(promise, f64::from_bits(ptr_bits(young)));

    assert!(
        remembered_set_size() > 0,
        "Promise value setter should dirty the exact fixed field slot"
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let header = header_from_user_ptr(young as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    remembered_set_clear();
}

pub(super) fn assert_heap_child_marked(ptr: *const u8, label: &str) {
    assert!(!ptr.is_null(), "{label} should not be null");
    unsafe {
        let header = header_from_user_ptr(ptr);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "{label} should be marked through the Promise remembered-set edge"
        );
    }
}

fn assert_marked_user_ptr(ptr: usize, label: &str) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "{label} should be marked by the active incremental barrier"
        );
    }
}

fn mark_user_ptr(ptr: usize) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        (*header).gc_flags |= GC_FLAG_MARKED;
    }
}

fn clear_mark_user_ptr(ptr: usize) {
    unsafe {
        let header = header_from_user_ptr(ptr as *const u8);
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
}

#[test]
fn test_incremental_barrier_marks_object_field_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = unsafe { alloc_nursery_test_object(0).0 as usize };
    let (obj, fields) = unsafe { alloc_old_test_object(1) };
    mark_user_ptr(obj as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    runtime_store_jsvalue_slot(obj as usize, fields as usize, 0, ptr_bits(child));
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(child, "object field child");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.missing_edges, 0);
    clear_mark_user_ptr(obj as usize);
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_incremental_barrier_marks_array_element_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = unsafe { alloc_nursery_test_object(0).0 as usize };
    let (arr, elements) = unsafe { alloc_old_test_array(1) };
    mark_user_ptr(arr as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    runtime_store_jsvalue_slot(arr as usize, elements as usize, 0, ptr_bits(child));
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(child, "array element child");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.missing_edges, 0);
    clear_mark_user_ptr(arr as usize);
    clear_marks();
    remembered_set_clear();
}

/// Regression: a uniquely-owned (refcount==1) string stored into an object
/// field or array element must be demoted to shared (refcount==0) by the
/// write-barrier choke point, so a later `js_string_append` on the original
/// local allocates fresh instead of mutating the buffer in place and
/// corrupting the stored alias. The manifesting shape is a heap-stored snapshot
/// (`slot = s`) whose source is then grown (`s += chunk`): without the demote,
/// the append rewrites the slot the snapshot still points at, so a later
/// equality check against the snapshot wrongly sees the two as identical.
#[test]
fn test_store_demotes_unique_string_to_shared() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();

    // Build a uniquely-owned string via append (refcount becomes 1).
    let a = crate::string::js_string_from_bytes(b"john".as_ptr(), 4);
    let b = crate::string::js_string_from_bytes(b".".as_ptr(), 1);
    let unique = crate::string::js_string_append(a, b);
    assert_eq!(
        unsafe { (*unique).refcount },
        1,
        "js_string_append yields a uniquely-owned string"
    );

    // Store it into an object field slot via the choke point under test.
    let (obj, fields) = unsafe { alloc_old_test_object(1) };
    mark_user_ptr(obj as usize);
    runtime_store_jsvalue_slot(
        obj as usize,
        fields as usize,
        0,
        string_bits(unique as usize),
    );

    // The fix demotes the stored string to shared (refcount==0).
    assert_eq!(
        unsafe { (*unique).refcount },
        0,
        "string stored into an object field is demoted to shared"
    );

    // A subsequent append must allocate fresh and leave the stored buffer intact.
    let c = crate::string::js_string_from_bytes(b"doe".as_ptr(), 3);
    let grown = crate::string::js_string_append(unique, c);
    assert_ne!(
        grown, unique,
        "append after store allocates fresh — no in-place mutation of the aliased buffer"
    );
    assert_eq!(
        unsafe { (*unique).byte_len },
        5,
        "the stored buffer ('john.') was not grown in place"
    );
    let slot_bits = unsafe { std::ptr::read(fields as *const u64) };
    assert_eq!(
        slot_bits,
        string_bits(unique as usize),
        "the object field still references the original, unmutated string"
    );

    // Same guarantee for an array element store (shares the choke point).
    let unique2 = crate::string::js_string_append(
        crate::string::js_string_from_bytes(b"a".as_ptr(), 1),
        crate::string::js_string_from_bytes(b"b".as_ptr(), 1),
    );
    assert_eq!(unsafe { (*unique2).refcount }, 1);
    let (arr, elements) = unsafe { alloc_old_test_array(1) };
    mark_user_ptr(arr as usize);
    runtime_store_jsvalue_slot(
        arr as usize,
        elements as usize,
        0,
        string_bits(unique2 as usize),
    );
    assert_eq!(
        unsafe { (*unique2).refcount },
        0,
        "string stored into an array element is demoted to shared"
    );

    // ...and the same in-place-mutation guarantee as the object case above.
    let d = crate::string::js_string_from_bytes(b"c".as_ptr(), 1);
    let grown2 = crate::string::js_string_append(unique2, d);
    assert_ne!(
        grown2, unique2,
        "append after array-element store allocates fresh — no in-place mutation of the aliased buffer"
    );
    assert_eq!(
        unsafe { (*unique2).byte_len },
        2,
        "the stored array-element buffer ('ab') was not grown in place"
    );
    let elem_bits = unsafe { std::ptr::read(elements as *const u64) };
    assert_eq!(
        elem_bits,
        string_bits(unique2 as usize),
        "the array element still references the original, unmutated string"
    );

    clear_mark_user_ptr(obj as usize);
    clear_mark_user_ptr(arr as usize);
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_incremental_barrier_marks_closure_capture_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let closure = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 1);
    mark_user_ptr(closure as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    crate::closure::js_closure_set_capture_ptr(closure, 0, child as i64);
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(child, "closure capture child");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.missing_edges, 0);
    clear_mark_user_ptr(closure as usize);
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_incremental_barrier_marks_closure_static_prototype_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let proto = unsafe { alloc_nursery_test_object(0).0 as usize };
    let closure = crate::closure::js_closure_alloc(test_no_capture_singleton_func as *const u8, 0);
    mark_user_ptr(closure as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    crate::closure::closure_set_static_prototype(closure as usize, ptr_bits(proto));
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(proto, "closure static prototype");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.checked_edges, 1);
    assert_eq!(stats.missing_edges, 0);
    clear_mark_user_ptr(closure as usize);
    crate::closure::test_clear_closure_side_tables();
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_incremental_barrier_marks_external_map_and_set_slots() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let map_child = unsafe { alloc_nursery_test_object(0).0 as usize };
    let set_child = unsafe { alloc_nursery_test_object(0).0 as usize };
    let (map, entries, map_layout) = unsafe { alloc_old_test_map(1) };
    let (set, elements, set_layout) = unsafe { alloc_old_test_set(1) };
    unsafe {
        (*map).size = 1;
        (*set).size = 1;
    }
    mark_user_ptr(map as usize);
    mark_user_ptr(set as usize);
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    runtime_store_external_jsvalue_slot(map as usize, entries as usize, ptr_bits(map_child));
    runtime_store_external_jsvalue_slot(set as usize, elements as usize, ptr_bits(set_child));
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(map_child, "map external slot child");
    assert_marked_user_ptr(set_child, "set external slot child");
    let stats = verify_marked_heap_no_unmarked_children();
    assert_eq!(stats.missing_edges, 0);
    unsafe {
        retire_old_test_map(map, entries, map_layout);
        retire_old_test_set(set, elements, set_layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_mark_invariant_verifier_rejects_incremental_barrier_bypass() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (obj, fields) = unsafe { alloc_old_test_object(1) };
    mark_user_ptr(obj as usize);
    unsafe {
        *fields = ptr_bits(child);
    }

    let result = std::panic::catch_unwind(verify_marked_heap_no_unmarked_children);

    assert!(
        result.is_err(),
        "raw pointer-capable stores into marked parents must fail the mark verifier"
    );
    clear_mark_user_ptr(obj as usize);
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_store_outside_incremental_mark_keeps_generational_behavior_only() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    assert!(!incremental_mark_barrier_active());
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (obj, fields) = unsafe { alloc_old_test_object(1) };

    runtime_store_jsvalue_slot(obj as usize, fields as usize, 0, ptr_bits(child));

    unsafe {
        let child_header = header_from_user_ptr(child as *const u8);
        assert_eq!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "inactive incremental barrier must not mark the stored child"
        );
    }
    assert!(
        remembered_set_size() > 0,
        "inactive incremental barrier must leave old-to-young remembered-set behavior intact"
    );
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_promise_pointer_field_stores_dirty_old_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    reset_remembered_set();
    clear_marks();
    let promise = unsafe { alloc_old_test_promise() };
    let callback = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 0);
    let next = crate::promise::js_promise_then(promise, callback, std::ptr::null());

    assert!(
        remembered_set_size() > 0,
        "js_promise_then should dirty old Promise pointer fields"
    );
    let valid_ptrs = build_valid_pointer_set();
    mark_remembered_set_roots(&valid_ptrs);
    assert_heap_child_marked(callback as *const u8, "then fulfillment callback");
    assert_heap_child_marked(next as *const u8, "then next promise");

    reset_remembered_set();
    clear_marks();
    let inner = unsafe { alloc_old_test_promise() };
    let outer = crate::promise::js_promise_new();
    crate::promise::js_promise_resolve_with_promise(outer, inner);

    assert!(
        remembered_set_size() > 0,
        "js_promise_resolve_with_promise should dirty old Promise next"
    );
    let valid_ptrs = build_valid_pointer_set();
    mark_remembered_set_roots(&valid_ptrs);
    assert_heap_child_marked(outer as *const u8, "resolved outer promise");

    reset_remembered_set();
    clear_marks();
    let promise = unsafe { alloc_old_test_promise() };
    let fulfill = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 0);
    let reject = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 0);
    crate::promise::js_promise_attach_handlers(promise, fulfill, reject);

    assert!(
        remembered_set_size() > 0,
        "js_promise_attach_handlers should dirty old Promise callback fields"
    );
    let valid_ptrs = build_valid_pointer_set();
    mark_remembered_set_roots(&valid_ptrs);
    assert_heap_child_marked(fulfill as *const u8, "attached fulfillment callback");
    assert_heap_child_marked(reject as *const u8, "attached rejection callback");

    reset_remembered_set();
    clear_marks();
    let promise = unsafe { alloc_old_test_promise() };
    let on_finally = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 0);
    let _next = crate::promise::js_promise_finally(promise, on_finally);
    let (fulfill_wrap, reject_wrap) = unsafe { ((*promise).on_fulfilled, (*promise).on_rejected) };

    assert!(
        remembered_set_size() > 0,
        "js_promise_finally should dirty old Promise wrapper fields"
    );
    let valid_ptrs = build_valid_pointer_set();
    mark_remembered_set_roots(&valid_ptrs);
    assert_heap_child_marked(fulfill_wrap as *const u8, "finally fulfillment wrapper");
    assert_heap_child_marked(reject_wrap as *const u8, "finally rejection wrapper");

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_error_cause_slot_marks_child() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let error = unsafe { alloc_old_test_error() };
    unsafe {
        crate::error::error_set_cause(error, f64::from_bits(ptr_bits(young)));
    }

    assert!(
        remembered_set_size() > 0,
        "Error cause setter should dirty the exact fixed field slot"
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let header = header_from_user_ptr(young as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_rewrite_remembered_dirty_range_updates_unmarked_old_parent_slot() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | child as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | child as u64,
    );
    let valid_ptrs = build_valid_pointer_set();
    let new_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(child as *const u8),
            new_child as *mut u8,
        );
        let old_header = header_from_user_ptr(old_obj as *const u8);
        assert_eq!(
            (*old_header).gc_flags & GC_FLAG_MARKED,
            0,
            "test must prove dirty rewrite does not depend on marked parent walk"
        );
    }

    rewrite_remembered_dirty_ranges(&valid_ptrs);

    unsafe {
        assert_eq!(
            *fields,
            POINTER_TAG | new_child as u64,
            "dirty old parent slot should be rewritten even when parent is unmarked"
        );
    }
    remembered_set_clear();
}

#[test]
fn test_rewrite_remembered_dirty_range_updates_map_external_entry_span() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(2048) };
    unsafe {
        (*map).size = 2048;
    }
    let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(entries, 4096) };
    let dirty_slot = unsafe { entries.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
        *entries.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }
    write_barrier_slot_inner(
        POINTER_TAG | map as u64,
        dirty_slot as usize,
        POINTER_TAG | dirty_child as u64,
        true,
    );
    let valid_ptrs = build_valid_pointer_set();
    let new_dirty_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let new_clean_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(dirty_child as *const u8),
            new_dirty_child as *mut u8,
        );
        set_forwarding_address(
            header_from_user_ptr(clean_child as *const u8),
            new_clean_child as *mut u8,
        );
    }

    rewrite_remembered_dirty_ranges(&valid_ptrs);

    unsafe {
        assert_eq!(*dirty_slot, POINTER_TAG | new_dirty_child as u64);
        assert_eq!(
            *entries.add(clean_idx),
            POINTER_TAG | clean_child as u64,
            "external dirty rewrite should stay bounded to the logged entry page"
        );
        retire_old_test_map(map, entries, layout);
    }
    remembered_set_clear();
}

#[test]
fn test_rewrite_remembered_fallback_header_updates_fields() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | child as u64;
    }
    REMEMBERED_SET.with(|s| {
        s.borrow_mut().insert(old_obj as usize - GC_HEADER_SIZE);
    });
    let valid_ptrs = build_valid_pointer_set();
    let new_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(child as *const u8),
            new_child as *mut u8,
        );
    }

    rewrite_remembered_dirty_ranges(&valid_ptrs);

    unsafe {
        assert_eq!(*fields, POINTER_TAG | new_child as u64);
    }
    remembered_set_clear();
}

#[test]
fn test_object_hashset_fallback_still_scans() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    clear_marks();
    let (old_obj, _fields) = unsafe { alloc_old_test_object(1) };
    let old_header = old_obj as usize - GC_HEADER_SIZE;
    REMEMBERED_SET.with(|s| {
        s.borrow_mut().insert(old_header);
    });
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.entries_scanned, 1);
    assert_eq!(stats.valid_roots, 1);
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let header = header_from_user_ptr(old_obj as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_gc_collect_minor_keeps_dirty_page_child_alive() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
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
    let _ = gc_collect_minor();
    unsafe {
        let child_header = header_from_user_ptr(young as *const u8);
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_HAS_SURVIVED,
            0,
            "dirty-page remembered scan should keep the young child alive through minor GC"
        );
    }
    remembered_set_clear();
}

#[test]
fn test_minor_gc_promotes_after_two_survivals() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    // Allocate an arena object and pin it so it survives every GC.
    let user_ptr = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    unsafe {
        let header = header_from_user_ptr(user_ptr);
        (*header).gc_flags |= GC_FLAG_PINNED;
        // Initial state: not yet survived, not tenured.
        assert_eq!((*header).gc_flags & GC_FLAG_HAS_SURVIVED, 0);
        assert_eq!((*header).gc_flags & GC_FLAG_TENURED, 0);
    }
    // First minor GC: object survives, gets HAS_SURVIVED bit.
    let _ = gc_collect_minor();
    unsafe {
        let header = header_from_user_ptr(user_ptr);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_HAS_SURVIVED,
            0,
            "first survival should set HAS_SURVIVED"
        );
        assert_eq!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "first survival should not yet tenure"
        );
    }
    // Second minor GC: HAS_SURVIVED + survives → TENURED, clear HAS_SURVIVED.
    let _ = gc_collect_minor();
    unsafe {
        let header = header_from_user_ptr(user_ptr);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "second survival should tenure"
        );
        assert_eq!(
            (*header).gc_flags & GC_FLAG_HAS_SURVIVED,
            0,
            "tenuring should clear HAS_SURVIVED"
        );
    }
    // Third minor GC: stays tenured idempotently.
    let _ = gc_collect_minor();
    unsafe {
        let header = header_from_user_ptr(user_ptr);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "tenured stays tenured across subsequent collections"
        );
    }
}

// Regression (2026-07 GC audit, old→malloc hole): the fallback minor's
// remembered-set scan used to seed only NURSERY children; a malloc-GC child
// on a dirty old page stayed unmarked and the minor malloc sweep freed it.
// Exercises both halves of the fix: the barrier recording the old→malloc
// edge and `try_mark_young_user_ptr_as_seed` accepting malloc children.
#[test]
fn test_remembered_set_scan_marks_malloc_child_of_old_parent() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_remembered_set();
    clear_marks();
    activate_malloc_registry_for_tests();

    let malloc_child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(malloc_child);
    }
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(malloc_child as usize);
    }
    js_write_barrier_slot(
        ptr_bits(old_arr as usize),
        elements as u64,
        ptr_bits(malloc_child as usize),
    );
    assert!(
        remembered_set_size() > 0,
        "old→malloc store must dirty the remembered page"
    );

    let valid_ptrs = build_valid_pointer_set();
    mark_remembered_set_roots(&valid_ptrs);
    assert_heap_child_marked(malloc_child as *const u8, "malloc child of old parent");

    reset_remembered_set();
    clear_marks();
}
