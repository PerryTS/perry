//! The coverage restore walks an old parent's in-body slots only on the
//! pre-clear dirty pages (`gc::restore_walk`).
use super::super::restore_walk::{
    parse_restore_walk, visit_old_parent_owed_slots, RestoreWalk, RestoreWalkTestGuard,
};
use super::super::*;
use super::support::*;

const SLOTS: u32 = 4096;

/// Element indices of `elements` on three distinct pages, none of them the
/// array's first page (which also holds the header).
unsafe fn three_pages(elements: *mut u64) -> [usize; 3] {
    let page = |i: usize| crate::arena::generation_page_for_addr(elements.add(i) as usize);
    let mut picked = Vec::new();
    let mut last = page(0);
    for i in 0..SLOTS as usize {
        if page(i) != last {
            last = page(i);
            picked.push(i + 7);
            if picked.len() == 3 {
                return [picked[0], picked[1], picked[2]];
            }
        }
    }
    panic!("fixture: the array spans at least four pages");
}

fn old_pages() -> Vec<usize> {
    let mut pages: Vec<usize> =
        barrier::DIRTY_OLD_PAGES.with(|s| s.borrow().iter().copied().collect());
    pages.sort_unstable();
    pages
}

fn snapshot_of(pages: &[usize]) -> RememberedDirtySnapshot {
    RememberedDirtySnapshot {
        dirty_old_pages: pages.iter().copied().collect(),
        external_dirty_entries: Vec::new(),
        dirty_pages: pages.iter().copied().collect(),
        fallback_headers: Vec::new(),
    }
}

#[test]
fn the_knob_parses_every_state() {
    assert_eq!(parse_restore_walk(None), RestoreWalk::DirtyPages);
    assert_eq!(parse_restore_walk(Some("")), RestoreWalk::DirtyPages);
    assert_eq!(parse_restore_walk(Some("1")), RestoreWalk::DirtyPages);
    assert_eq!(parse_restore_walk(Some("typo")), RestoreWalk::DirtyPages);
    for off in ["0", "off", "false", "no", " OFF "] {
        assert_eq!(parse_restore_walk(Some(off)), RestoreWalk::WholeOwner);
    }
    assert_eq!(parse_restore_walk(Some("verify")), RestoreWalk::Verify);
    assert_eq!(parse_restore_walk(Some(" Verify ")), RestoreWalk::Verify);
}

/// The bounded walk enumerates exactly the slots on the named pages, and the
/// whole-owner enumeration is its superset.
#[test]
fn the_bounded_walk_visits_only_slots_on_dirty_pages() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    unsafe {
        let (arr, elements) = alloc_old_test_array(SLOTS);
        let header = header_from_user_ptr(arr as *const u8) as *mut GcHeader;
        let [a, _, c] = three_pages(elements);
        let page_a = crate::arena::generation_page_for_addr(elements.add(a) as usize);
        let page_c = crate::arena::generation_page_for_addr(elements.add(c) as usize);
        let dirty: crate::fast_hash::PtrHashSet<usize> = [page_a, page_c].into_iter().collect();

        let mut visited = Vec::new();
        let clean = visit_old_parent_owed_slots::<{ cfg!(test) }>(header, &dirty, |slot| {
            visited.push(slot.slot as usize);
        });
        // Single-slot descriptors (the array's own header words) are always
        // visited; only the element range is bounded.
        let off_page = visited
            .iter()
            .filter(|&&slot| !dirty.contains(&crate::arena::generation_page_for_addr(slot)))
            .count();
        assert!(off_page <= 4, "{off_page} visited slots are on clean pages");
        assert!(
            visited.len() <= 1024 + off_page,
            "two pages hold at most 1024 slots, visited {}",
            visited.len()
        );
        assert!(visited.contains(&(elements.add(a) as usize)));
        assert!(visited.contains(&(elements.add(c) as usize)));

        let mut whole = 0usize;
        visit_gc_rewrite_slots::<{ cfg!(test) }>(header, |_| whole += 1);
        assert_eq!(
            visited.len() + clean,
            whole,
            "visited + skipped = every slot"
        );
        assert!(clean >= SLOTS as usize - 1024);

        // No dirty page: no element is owed, none is visited.
        let none = crate::fast_hash::new_ptr_hash_set();
        let mut count = 0usize;
        let clean = visit_old_parent_owed_slots::<{ cfg!(test) }>(header, &none, |_| count += 1);
        assert_eq!(count, off_page);
        assert_eq!(count + clean, whole);
    }
    remembered_set_clear();
}

/// Barriered young edges on three pages of an old array: the bounded restore
/// rebuilds exactly the pages the whole-owner restore rebuilds.
#[test]
fn the_bounded_restore_rebuilds_the_pages_the_whole_owner_restore_rebuilds() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    unsafe {
        let (arr, elements) = alloc_old_test_array(SLOTS);
        let young = [young_leaf(), young_leaf(), young_leaf()];
        for (&index, &child) in three_pages(elements).iter().zip(&young) {
            let slot = elements.add(index);
            *slot = string_bits(child);
            runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        }
        let barriered = old_pages();
        assert_eq!(barriered.len(), 3, "one dirty page per stored child");
        let snapshot = snapshot_of(&barriered);

        let mut rebuilt = Vec::new();
        for walk in [
            RestoreWalk::WholeOwner,
            RestoreWalk::DirtyPages,
            RestoreWalk::Verify,
        ] {
            let _walk = RestoreWalkTestGuard::pin(walk);
            remembered_set_clear();
            restore_surviving_dirty_coverage::<{ cfg!(test) }>(
                &snapshot,
                &Default::default(),
                "restore_walk_test",
            );
            rebuilt.push(old_pages());
        }
        assert_eq!(rebuilt[0], barriered, "whole-owner walk");
        assert_eq!(rebuilt[1], barriered, "bounded walk");
        assert_eq!(rebuilt[2], barriered, "bounded walk with its witness");
    }
    remembered_set_clear();
}

/// Plant a young edge on a page the snapshot does not name: a store that
/// skipped the barrier. Returns (snapshot, the unbarriered slot's page).
unsafe fn unbarriered_edge_fixture() -> (RememberedDirtySnapshot, usize) {
    let (arr, elements) = alloc_old_test_array(SLOTS);
    let [a, b, _] = three_pages(elements);
    let barriered = elements.add(a);
    *barriered = string_bits(young_leaf());
    runtime_write_barrier_slot(arr as usize, barriered as usize, *barriered);
    // No barrier call for this one.
    *elements.add(b) = string_bits(young_leaf());
    let lost_page = crate::arena::generation_page_for_addr(elements.add(b) as usize);
    let pages = old_pages();
    assert!(!pages.contains(&lost_page));
    (snapshot_of(&pages), lost_page)
}

/// Negative control for the custody rule: the bounded walk does not look at a
/// clean page, so an edge whose store skipped the barrier stays unremembered.
/// The whole-owner walk finds it, but only because the same object happens to
/// have another dirty page: the dirty scan before it never visited that slot.
#[test]
fn negative_control_an_unbarriered_edge_is_found_only_by_the_whole_owner_walk() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    unsafe {
        let (snapshot, lost_page) = unbarriered_edge_fixture();
        {
            let _walk = RestoreWalkTestGuard::pin(RestoreWalk::DirtyPages);
            remembered_set_clear();
            restore_surviving_dirty_coverage::<{ cfg!(test) }>(
                &snapshot,
                &Default::default(),
                "restore_walk_test",
            );
            assert!(!old_pages().contains(&lost_page));
        }
        {
            let _walk = RestoreWalkTestGuard::pin(RestoreWalk::WholeOwner);
            remembered_set_clear();
            restore_surviving_dirty_coverage::<{ cfg!(test) }>(
                &snapshot,
                &Default::default(),
                "restore_walk_test",
            );
            assert!(old_pages().contains(&lost_page));
        }
    }
    remembered_set_clear();
}

/// The witness is live: under `verify` the same unbarriered edge aborts the
/// restore instead of being skipped silently.
#[test]
#[should_panic(expected = "skipped the write barrier")]
fn the_witness_reports_an_unbarriered_edge() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    unsafe {
        let (snapshot, _) = unbarriered_edge_fixture();
        let _walk = RestoreWalkTestGuard::pin(RestoreWalk::Verify);
        remembered_set_clear();
        restore_surviving_dirty_coverage::<{ cfg!(test) }>(
            &snapshot,
            &Default::default(),
            "restore_walk_test",
        );
    }
}

/// A malloc parent keeps the whole-owner walk: its entries are owner
/// qualified, and an owner entry does not say which of its pages are dirty.
#[test]
fn a_malloc_parent_keeps_the_whole_owner_walk() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    let _walk = RestoreWalkTestGuard::pin(RestoreWalk::DirtyPages);
    reset_remembered_set();
    unsafe {
        let child = young_leaf();
        let mut captures = vec![crate::value::TAG_UNDEFINED; LARGE_OBJECT_THRESHOLD_BYTES / 8 + 64];
        captures[1] = string_bits(child);
        let closure = crate::closure::js_closure_alloc_init(
            std::ptr::null(),
            captures.len() as u32,
            captures.as_ptr(),
        );
        let header = header_from_user_ptr(closure as *const u8) as usize;
        let slot = crate::closure::closure_capture_slots_mut(closure).add(1);
        let page = crate::arena::generation_page_for_addr(slot as usize);
        activate_malloc_registry_for_tests();
        // The snapshot names two other pages of the owner, not the edge's.
        let snapshot = RememberedDirtySnapshot {
            dirty_old_pages: Default::default(),
            external_dirty_entries: vec![(page + 1, header), (page + 2, header)],
            dirty_pages: [page + 1, page + 2].into_iter().collect(),
            fallback_headers: Vec::new(),
        };
        remembered_set_clear();
        restore_surviving_dirty_coverage::<{ cfg!(test) }>(
            &snapshot,
            &Default::default(),
            "restore_walk_test",
        );
        assert!(EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
            s.borrow()
                .get(&page)
                .is_some_and(|owners| owners.contains(&header))
        }));
        remembered_set_clear();
    }
}

/// End to end, both walks: young strings stored (with the barrier) on three
/// pages of an old array survive copying minors until they are promoted, and
/// the array's slots follow them.
#[test]
fn young_children_of_a_multi_page_old_array_survive_copying_minors() {
    for walk in [
        RestoreWalk::Verify,
        RestoreWalk::DirtyPages,
        RestoreWalk::WholeOwner,
    ] {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _walk = RestoreWalkTestGuard::pin(walk);
        unsafe {
            let (arr, elements) = alloc_old_test_array(SLOTS);
            js_shadow_slot_set(0, ptr_bits(arr as usize));
            let indices = three_pages(elements);
            let mut expected = Vec::new();
            for (n, &index) in indices.iter().enumerate() {
                let text = format!("restore-walk-child-{n}-{walk:?}");
                let child = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
                let slot = elements.add(index);
                *slot = string_bits(child as usize);
                layout_note_slot(arr as usize, index, *slot);
                runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
                expected.push(text);
            }
            let before: Vec<usize> = indices
                .iter()
                .map(|&i| (*elements.add(i) & POINTER_MASK) as usize)
                .collect();
            for round in 0..6 {
                let trace = collect_minor_trace(GcTriggerKind::Direct);
                assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
                for (n, &index) in indices.iter().enumerate() {
                    let now = (*elements.add(index) & POINTER_MASK) as usize;
                    assert_string_bytes(now as *const crate::StringHeader, expected[n].as_bytes());
                    if round == 0 {
                        assert_ne!(now, before[n], "the first minor moves the child");
                    }
                }
            }
            for &index in &indices {
                let now = (*elements.add(index) & POINTER_MASK) as usize;
                assert!(crate::arena::pointer_in_old_gen(now), "promoted in the end");
            }
        }
        remembered_set_clear();
    }
}

fn ptr_from_slot(slot: u32) -> *mut crate::map::MapHeader {
    (js_shadow_slot_get(slot) & crate::value::POINTER_MASK) as *mut crate::map::MapHeader
}

fn on_fresh_thread(body: impl FnOnce() + Send + 'static) {
    std::thread::spawn(body).join().unwrap();
}

/// End to end through a real Map: its store outgrows one page, is promoted,
/// and keeps receiving young values at its tail across minors.
#[test]
fn a_large_old_map_store_keeps_its_young_tail_values() {
    on_fresh_thread(|| {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let map = crate::map::js_map_alloc(4);
        js_shadow_slot_set(0, ptr_bits(map as usize));
        let value = |i: usize| format!("tt-value-{i}");
        let mut stored = 0usize;
        for round in 0..12 {
            for _ in 0..600 {
                let text = value(stored);
                let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
                let map = ptr_from_slot(0);
                crate::map::js_map_set(map, stored as f64, f64::from_bits(string_bits(s as usize)));
                stored += 1;
            }
            let _ = collect_minor_trace(GcTriggerKind::Direct);
            let map = ptr_from_slot(0);
            for i in (0..stored).step_by(if round == 11 { 1 } else { 97 }) {
                let got = crate::map::js_map_get(map, i as f64).to_bits();
                unsafe {
                    assert_string_bytes(
                        (got & POINTER_MASK) as usize as *const crate::StringHeader,
                        value(i).as_bytes(),
                    );
                }
            }
        }
        remembered_set_clear();
    });
}
