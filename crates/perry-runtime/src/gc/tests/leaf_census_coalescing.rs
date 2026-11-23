//! The owed allocation checks must run without pricing a small leaf against
//! an almost-full Eden over and over. Occupancy, rather than a remembered
//! collection decision, bounds both resources.
use super::super::*;
use super::support::*;

const CAP: usize = 1024 * 1024;
const LEAF: usize = 64 * 1024;

unsafe fn allocate(bytes: usize) -> usize {
    let p = crate::arena::arena_alloc_gc(bytes, 8, GC_TYPE_STRING);
    p.write_bytes(0, bytes);
    p as usize
}

unsafe fn fill_small_to(limit: usize) {
    let _suppressed = GcSuppressScope::new();
    while crate::arena::copying_from_space_in_use_bytes() < limit {
        allocate(1024);
    }
}

unsafe fn owed_leaf_birth() -> usize {
    let scope = GcSuppressScope::new();
    let p = allocate(LEAF);
    gc_check_trigger();
    assert_ne!(GC_FLAGS.with(Cell::get) & policy::GC_FLAG_TRIGGER_OWED, 0);
    drop(scope);
    assert_eq!(GC_FLAGS.with(Cell::get) & policy::GC_FLAG_TRIGGER_OWED, 0);
    js_gc_loop_safepoint();
    p
}

#[test]
fn near_full_eden_coalesces_repeated_leaf_censuses_from_owed_checks() {
    std::thread::spawn(|| unsafe {
        let _heap = CopyingNurseryTestGuard::new(0);
        let _pacing = policy::force_moving_gc_pacing();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        fill_small_to(CAP - LEAF / 2);
        let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(CAP);
        let before = super::super::leaf_service::leaf_services_run();
        let mut leaves = Vec::new();
        // Every birth crosses the old combined window. No leaf yet fills a
        // quarter-window, so three O(small) censuses must become one later.
        for _ in 0..3 {
            leaves.push(owed_leaf_birth());
        }
        assert_eq!(
            super::super::leaf_service::leaf_services_run(),
            before,
            "small pressure must not repeatedly census young cells for tiny leaf recovery"
        );
        assert!(crate::arena::young_large_in_use_bytes() >= 3 * LEAF);
        // The fourth fills the batch. Eden is more than half its window
        // (CAP - LEAF / 2 of CAP), so the window left is smaller than the
        // Eden the census would read: the copying minor runs instead and
        // retires the dead leaves itself.
        let collections = gc_collection_count();
        leaves.push(owed_leaf_birth());
        assert_eq!(
            super::super::leaf_service::leaf_services_run(),
            before,
            "with less window left than Eden, no leaf-only census"
        );
        assert!(
            gc_collection_count() > collections,
            "the copying minor must run in the census's place"
        );
        assert_eq!(crate::arena::young_large_in_use_bytes(), 0);
        assert!(crate::arena::copying_from_space_in_use_bytes() < CAP - LEAF / 2);
        for p in leaves {
            assert_eq!(
                crate::arena::classify_heap_space(p),
                crate::arena::HeapSpace::Unknown
            );
        }
    })
    .join()
    .unwrap();
}

#[test]
fn leaf_service_runs_while_the_window_left_covers_eden() {
    std::thread::spawn(|| unsafe {
        let _heap = CopyingNurseryTestGuard::new(0);
        let _pacing = policy::force_moving_gc_pacing();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        // Eden is two fifths of the window: the window left (three fifths)
        // covers Eden, so the leaf-only service is the collection, not a
        // copying minor.
        fill_small_to(CAP * 2 / 5);
        let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(CAP);
        let services = super::super::leaf_service::leaf_services_run();
        let collections = gc_collection_count();
        let small = crate::arena::copying_from_space_in_use_bytes();
        for _ in 0..CAP / LEAF {
            owed_leaf_birth();
            if super::super::leaf_service::leaf_services_run() > services {
                assert_eq!(
                    super::super::leaf_service::leaf_services_run(),
                    services + 1
                );
                assert_eq!(
                    gc_collection_count(),
                    collections,
                    "a leaf-only service, not a copying minor"
                );
                assert_eq!(crate::arena::young_large_in_use_bytes(), 0);
                assert!(crate::arena::copying_from_space_in_use_bytes() >= small);
                return;
            }
        }
        panic!("leaves filling the window must run the leaf service");
    })
    .join()
    .unwrap();
}

#[test]
fn owed_small_cap_check_arms_and_collects_at_scope_exit() {
    std::thread::spawn(|| unsafe {
        let _heap = CopyingNurseryTestGuard::new(0);
        let _pacing = policy::force_moving_gc_pacing();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        fill_small_to(CAP);
        let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(CAP);
        let before = gc_collection_count();
        let scope = GcSuppressScope::new();
        gc_check_trigger();
        assert!(!GC_SAFEPOINT_PENDING.with(Cell::get));
        drop(scope);
        assert!(
            GC_SAFEPOINT_PENDING.with(Cell::get),
            "an owed young-cap check must arm the poll"
        );
        js_gc_loop_safepoint();
        assert!(
            gc_collection_count() > before,
            "the owed check must reach a real tracing minor"
        );
        assert!(crate::arena::copying_from_space_in_use_bytes() < CAP);
    })
    .join()
    .unwrap();
}

#[test]
fn coalescing_cannot_grow_small_or_leaf_occupancy_past_the_bound() {
    std::thread::spawn(|| unsafe {
        let _heap = CopyingNurseryTestGuard::new(0);
        let _pacing = policy::force_moving_gc_pacing();
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        fill_small_to(CAP - LEAF / 2);
        let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(CAP);
        let before = gc_collection_count();
        owed_leaf_birth();
        // Even if every later birth is small, the unchanged small cap must
        // force a tracing minor before another 1 KiB allocation beyond it.
        for _ in 0..(CAP / 4 + LEAF) / 1024 {
            if gc_collection_count() > before {
                break;
            }
            allocate(1024);
            gc_check_trigger();
            js_gc_loop_safepoint();
            let small = crate::arena::copying_from_space_in_use_bytes();
            let leaves = crate::arena::young_large_in_use_bytes();
            assert!(small + leaves <= CAP + CAP / 4 + 1024 + GC_HEADER_SIZE);
        }
        assert!(
            gc_collection_count() > before,
            "coalescing must not postpone the small cap"
        );
        assert!(crate::arena::copying_from_space_in_use_bytes() < CAP);

        // Now grow only leaves. A window's worth is an independent bound,
        // even with no small births to drive the tracing arm.
        let services = super::super::leaf_service::leaf_services_run();
        for _ in 0..CAP / LEAF + 1 {
            owed_leaf_birth();
            assert!(crate::arena::young_large_in_use_bytes() < CAP);
            if super::super::leaf_service::leaf_services_run() > services {
                return;
            }
        }
        panic!("leaf-only growth must collect within one allocation window");
    })
    .join()
    .unwrap();
}

#[test]
fn quiet_leaf_pressure_preserves_a_bound_even_with_excess_survivors() {
    for cap in [1_usize, 4, 1024, 1024 * 1024] {
        for survivor in [0, cap / 16, cap / 2, cap - 1] {
            let window = super::super::tenuring::eden_allocation_window_bytes(cap, survivor);
            let floor = window.div_ceil(4);
            for small in [survivor, cap - 1] {
                for leaves in [0, floor - 1, floor, window - 1, window, 2 * window] {
                    if !policy::leaf_window_due(cap, small, survivor, leaves).0 {
                        let bound = (survivor + window).max(cap + floor);
                        assert!(small + leaves < bound,
                            "cap={cap} small={small} survivor={survivor} leaves={leaves} bound={bound}");
                    }
                }
            }
        }
    }
}
