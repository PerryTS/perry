//! #7187: the Rust-side write-barrier entry points hand
//! `write_barrier_slot_inner` a parent address they already hold. They used to
//! re-encode it as a bare `u64` so `decode_heap_addr` could re-derive it — and
//! that arm pays a full `classify_heap_generation` immediately before
//! `barrier_parent_needs_remembering` classifies the same address again.
//!
//! These tests pin the two things removing the round-trip depends on: that the
//! decoded route leaves the collector exactly the remembered set the NaN-boxed
//! route did, and that the filtering the round-trip was silently providing is
//! now explicit — `malloc_gc_parent_addr` DEREFERENCES its argument.

use super::super::barrier::{malloc_gc_parent_addr, remembered_dirty_page_count};
use super::super::*;
use super::support::*;

fn remembered_maintenance_entry_count() -> usize {
    let dirty_old = DIRTY_OLD_PAGES.with(|s| s.borrow().len());
    let external_dirty =
        EXTERNAL_DIRTY_SLOT_PAGES.with(|s| s.borrow().values().map(Vec::len).sum::<usize>());
    let fallback = REMEMBERED_SET.with(|s| s.borrow().len());
    dirty_old + external_dirty + fallback
}

/// The remembered-set state a barrier call produced, as the collector will
/// later read it. Compared between the decoded and NaN-boxed entry points —
/// counter names may differ (that IS the observable delta, pinned separately
/// below), but what reaches the collector must not.
fn remembered_state_fingerprint() -> (usize, usize, usize) {
    (
        remembered_set_size(),
        remembered_dirty_page_count(),
        remembered_maintenance_entry_count(),
    )
}

#[test]
fn runtime_write_barrier_slot_remembers_old_to_young_edge() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let child_bits = ptr_bits(young);
    unsafe {
        *fields = child_bits;
    }
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);
    assert!(!old_page_dirty_for(dirty_page));

    // The `note_array_slot` / `runtime_store_jsvalue_slot` entry point — the
    // one every born-old array element store goes through.
    runtime_write_barrier_slot(old_obj as usize, fields as usize, child_bits);

    assert_eq!(
        remembered_dirty_page_count(),
        1,
        "old→young store through the decoded entry point must dirty the slot page"
    );
    assert!(
        old_page_dirty_for(dirty_page),
        "old-page metadata should mirror the remembered dirty page"
    );

    reset_remembered_set();
}

#[test]
fn runtime_write_barrier_slot_matches_nanboxed_entry_point() {
    let _guard = GcTestIsolationGuard::new();
    activate_malloc_registry_for_tests();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let old_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let malloc_child = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(malloc_child);
    }

    // Every parent generation the barrier distinguishes, paired with every
    // child kind. `malloc_parent` is the case the removed `decode_heap_addr`
    // filter used to reject outright (classify == Unknown → address 0 →
    // `NonPointerParentSkips`); it now reaches `barrier_parent_needs_remembering`
    // and exits at `ParentNotOldSkips` instead. Same remembered-set effect.
    let malloc_parent = gc_malloc(
        std::mem::size_of::<crate::object::ObjectHeader>() + 8,
        GC_TYPE_OBJECT,
    );
    let children: [(&str, u64); 4] = [
        ("young", ptr_bits(young)),
        ("old", ptr_bits(old_child)),
        ("malloc", ptr_bits(malloc_child as usize)),
        ("primitive", 1.5f64.to_bits()),
    ];

    for (child_label, child_bits) in children {
        for parent_label in ["old", "nursery", "malloc"] {
            let (parent_addr, slot_addr) = unsafe {
                match parent_label {
                    "old" => {
                        let (obj, fields) = alloc_old_test_object(1);
                        (obj as usize, fields as usize)
                    }
                    "nursery" => {
                        let (obj, fields) = alloc_nursery_test_object(1);
                        (obj as usize, fields as usize)
                    }
                    _ => {
                        let fields = (malloc_parent as *mut u8)
                            .add(std::mem::size_of::<crate::object::ObjectHeader>());
                        (malloc_parent as usize, fields as usize)
                    }
                }
            };
            unsafe {
                std::ptr::write(slot_addr as *mut u64, child_bits);
            }

            reset_remembered_set();
            js_write_barrier_slot(ptr_bits(parent_addr), slot_addr as u64, child_bits);
            let nanboxed = remembered_state_fingerprint();

            reset_remembered_set();
            runtime_write_barrier_slot(parent_addr, slot_addr, child_bits);
            let decoded = remembered_state_fingerprint();

            assert_eq!(
                decoded, nanboxed,
                "parent={parent_label} child={child_label}: the decoded entry point must \
                 leave the collector exactly the remembered set the NaN-boxed one does"
            );
        }
    }

    reset_remembered_set();
    clear_marks();
}

#[test]
fn runtime_write_barrier_slot_malloc_parent_skips_as_not_old() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    activate_malloc_registry_for_tests();
    let tracing = gc_trace_enabled();
    let _ = take_write_barrier_trace_counters();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let malloc_parent = gc_malloc(
        std::mem::size_of::<crate::object::ObjectHeader>() + 8,
        GC_TYPE_OBJECT,
    );
    let slot = unsafe {
        (malloc_parent as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *mut u64
    };
    let child_bits = ptr_bits(young);
    unsafe {
        std::ptr::write(slot, child_bits);
    }

    runtime_write_barrier_slot(malloc_parent as usize, slot as usize, child_bits);

    // The invariant: a malloc-GC parent reached through the NON-external entry
    // point is not an old parent, so nothing is remembered. Unchanged by #7187
    // — only which skip counter reports it moved.
    assert_eq!(
        remembered_state_fingerprint(),
        (0, 0, 0),
        "non-external malloc-GC parent must record no old→young edge"
    );

    let counters = take_write_barrier_trace_counters();
    if tracing {
        assert_eq!(counters.calls, 1);
        assert_eq!(
            counters.non_pointer_parent_skips, 0,
            "the parent address is real — it must no longer be rejected as a non-pointer"
        );
        assert_eq!(
            counters.parent_not_old_skips, 1,
            "it is rejected for the reason that is actually true: not an old parent"
        );
    }

    reset_remembered_set();
    clear_marks();
}

#[test]
fn runtime_barrier_entry_points_reject_implausible_parents_without_dereferencing() {
    let _guard = GcTestIsolationGuard::new();
    reset_remembered_set();
    let tracing = gc_trace_enabled();
    let _ = take_write_barrier_trace_counters();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child_bits = ptr_bits(young);
    let mut slot = child_bits;
    let slot_addr = &mut slot as *mut u64 as usize;

    // `barrier_parent_needs_remembering` → `malloc_gc_parent_addr`
    // DEREFERENCES `parent - GC_HEADER_SIZE`. Every one of these owner shapes
    // reaches the runtime barrier entry points in the wild and none may be
    // dereferenced:
    //   - the `closure/dynamic_props` side-table owner key (real: its own unit
    //     test parks props under `0xC10C_AB1E_0000_1803`),
    //   - native handle-band ids (`#4740`, `#6271`),
    //   - a heap-shaped but unaligned word.
    let implausible: [(&str, usize); 6] = [
        ("closure prop owner key", 0xC10C_AB1E_0000_1803),
        ("common handle band", 0x1234),
        ("fetch handle band", 0x4_0010),
        ("proxy id band", 0xF_0008),
        ("above platform heap range", 0x9000_0000_0000),
        ("unaligned heap-shaped word", 0x0000_7f31_0000_1003),
    ];

    for (label, parent) in implausible {
        runtime_write_barrier_slot(parent, slot_addr, child_bits);
        runtime_write_barrier_external_slot(parent, slot_addr, child_bits);
        runtime_write_barrier_gc_slot(parent, slot_addr, child_bits);
        assert_eq!(
            remembered_state_fingerprint(),
            (0, 0, 0),
            "{label}: an implausible parent must record no edge"
        );
    }

    let counters = take_write_barrier_trace_counters();
    if tracing {
        assert_eq!(counters.calls, 18);
        assert_eq!(
            counters.non_pointer_parent_skips, 18,
            "every implausible parent must be rejected before any header read"
        );
        assert_eq!(counters.old_to_young_slow_hits, 0);
    }

    // The predicate itself, pinned: `malloc_gc_parent_addr` must answer false
    // without dereferencing. If the guard is removed this line segfaults
    // rather than failing, which is the point of asserting it here as well as
    // through the entry points above.
    for (label, parent) in implausible {
        assert!(
            !malloc_gc_parent_addr(parent),
            "{label}: must not be classified as a malloc-GC parent"
        );
    }

    reset_remembered_set();
}
