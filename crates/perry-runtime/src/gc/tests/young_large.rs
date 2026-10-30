//! Vertical leaf-space acceptance: subjects must exist, be collected, and reuse
//! their owner rather than merely survive a test in which no collector ran.
use super::super::*;
use super::support::*;
use crate::arena::{HeapGeneration, HeapSpace};

unsafe fn leaf(bytes: usize) -> usize {
    let p = crate::arena::arena_alloc_gc(bytes, 8, GC_TYPE_STRING);
    p.write_bytes(0, bytes);
    p as usize
}
fn string_bits(p: usize) -> u64 {
    STRING_TAG | p as u64
}

#[test]
fn large_leaf_has_dedicated_young_storage_and_no_payload_scan() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let small = young_leaf();
        let before = crate::arena::copying_from_space_in_use_bytes();
        let moving = crate::arena::moving_from_space_in_use_bytes();
        let p = leaf(LARGE_OBJECT_THRESHOLD_BYTES + 8);
        (p as *mut u64).write(string_bits(small));
        let header = header_from_user_ptr(p as *const u8);
        let total = (*header).size as usize;
        assert_eq!(
            crate::arena::classify_heap_generation(p),
            HeapGeneration::Nursery
        );
        assert_eq!((*header).gc_flags & GC_FLAG_TENURED, 0);
        assert!(crate::arena::is_large_storage(p));
        assert_eq!(
            crate::arena::copying_from_space_in_use_bytes(),
            before
        );
        assert_eq!(crate::arena::moving_from_space_in_use_bytes(), moving);
        let mut collector = CopyingNurseryCollector::new::<{ cfg!(test) }>(CopyingPointerSet::new());
        let classified = collector.ptrs.classify(p).unwrap();
        assert_eq!(classified.kind, CopyingPointerKind::YoungLarge);
        assert_eq!(collector.mark_addr::<{ cfg!(test) }>(p), Some(p));
        collector.drain::<{ cfg!(test) }>();
        assert!(collector.worklist.is_empty());
        assert_eq!(collector.stats.copied_bytes, 0);
        assert_eq!(
            (*header_from_user_ptr(small as *const u8)).gc_flags & GC_FLAG_MARKED,
            0,
            "a pointer-shaped word in leaf bytes is not an edge"
        );
        assert!(
            collector.ptrs.classify(p + 8).is_none(),
            "one object owns the entire extent"
        );
        collector.clear_marks();
    })
    .join()
    .unwrap();
}

#[test]
fn dead_large_leaf_retires_at_minor_and_successor_reuses_its_backing() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let p = leaf(18_090_024);
        let total = (*header_from_user_ptr(p as *const u8)).size as usize;
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert!(trace.sweep.freed_bytes >= total as u64);
        assert_eq!(crate::arena::classify_heap_space(p), HeapSpace::Unknown);
        assert_eq!(crate::arena::young_large_in_use_bytes(), 0);
        let next = leaf(18_090_024);
        assert_eq!(
            next, p,
            "dead backing must be reusable before the next fill"
        );
        assert_eq!(
            crate::arena::classify_heap_generation(next),
            HeapGeneration::Nursery
        );
    })
    .join()
    .unwrap();
}

#[test]
fn old_edge_retains_large_leaf_by_identity_and_a_later_full_reclaims_it() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let old = alloc_old_test_array(20_000).0;
        js_shadow_slot_set(0, ptr_bits(old as usize));
        let p = leaf(LARGE_OBJECT_THRESHOLD_BYTES + 8);
        crate::array::js_array_set_f64_extend(old, 0, f64::from_bits(string_bits(p)));
        assert!(remembered_child_needs_tracking(p));
        assert!(remembered_set_size() > 0);
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        let value = crate::array::js_array_get_f64(old, 0).to_bits();
        assert_eq!((value & POINTER_MASK) as usize, p);
        assert_eq!(
            crate::arena::classify_heap_generation(p),
            HeapGeneration::Old
        );
        assert!(crate::arena::is_large_storage(p));
        let header = header_from_user_ptr(p as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_TENURED, 0);
        assert_eq!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "promotion must finish mark cleanup"
        );
        crate::array::js_array_set_f64_extend(old, 0, 0.0);
        let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        });
        assert!(!crate::arena::pointer_in_nursery(p));
        assert_eq!(
            crate::arena::classify_heap_space(p),
            HeapSpace::Unknown,
            "old-large ownership is retired by the later full"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn nonmoving_minor_retires_dead_large_and_promotes_live_large_by_identity() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let dead = leaf(LARGE_OBJECT_THRESHOLD_BYTES + 8);
        let live = leaf(LARGE_OBJECT_THRESHOLD_BYTES + 8);
        js_shadow_slot_set(0, string_bits(live));
        js_gc_write_barriers_emitted(0);
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(
            !trace.copying_nursery.eligible,
            "fixture must take the nonmoving fallback"
        );
        assert_eq!(crate::arena::classify_heap_space(dead), HeapSpace::Unknown);
        assert_eq!((js_shadow_slot_get(0) & POINTER_MASK) as usize, live);
        assert_eq!(
            crate::arena::classify_heap_generation(live),
            HeapGeneration::Old
        );
        assert!(crate::arena::is_large_storage(live));
        assert_eq!(
            (*header_from_user_ptr(live as *const u8)).gc_flags & GC_FLAG_MARKED,
            0
        );
    })
    .join()
    .unwrap();
}

#[test]
fn large_birth_after_sweep_snapshot_survives_only_that_cycle() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let mut state = GcCycleState::new_full(GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        });
        for _ in 0..100_000 {
            if state.phase() == GcCyclePhase::Sweep {
                break;
            }
            state.step(GcWorkBudget::bounded(1));
        }
        assert_eq!(state.phase(), GcCyclePhase::Sweep);
        // Create the sweep cursor and its allocation snapshot, but leave it sliced.
        state.step(GcWorkBudget::bounded(1));
        let late = leaf(LARGE_OBJECT_THRESHOLD_BYTES + 8);
        for _ in 0..100_000 {
            if state.phase() == GcCyclePhase::Complete {
                break;
            }
            state.step(GcWorkBudget::bounded(1));
        }
        assert_eq!(state.phase(), GcCyclePhase::Complete);
        assert_eq!(
            crate::arena::classify_heap_generation(late),
            HeapGeneration::Nursery
        );
        assert_eq!(
            (*header_from_user_ptr(late as *const u8)).gc_flags & GC_FLAG_MARKED,
            0
        );
        let _ = collect_minor_trace(GcTriggerKind::Direct);
        assert_eq!(crate::arena::classify_heap_space(late), HeapSpace::Unknown);
    })
    .join()
    .unwrap();
}

#[test]
fn large_leaf_pressure_does_not_pretenure_the_special_json_output() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        seed_young_survival_for_tests(999);
        let base = gc_scavenge_nursery_cap_bytes();
        let moving_before = crate::arena::moving_from_space_in_use_bytes();
        let _large = leaf(base + LARGE_OBJECT_THRESHOLD_BYTES);
        let output = (base / 2).max(moving_before + 1024) as u32;
        assert!(crate::arena::young_in_use_bytes() >= base);
        assert_eq!(
            crate::arena::moving_from_space_in_use_bytes(),
            moving_before
        );
        assert!(
            !crate::string::json_leaf_prefers_arena(output),
            "a non-moving leaf is not the retained copying tree of the legacy JSON route"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn old_legacy_large_backing_still_classifies_secondary_objects() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let first =
            crate::arena::arena_alloc_gc_old(3 * crate::arena::BLOCK_SIZE + 64, 8, GC_TYPE_STRING)
                as usize;
        let next = crate::arena::arena_alloc_gc_old(56, 8, GC_TYPE_STRING) as usize;
        let a = crate::arena::classify_heap_storage_in_range(first).unwrap();
        let b = crate::arena::classify_heap_storage_in_range(next).unwrap();
        assert!(a.3 && b.3);
        assert_eq!(
            a.1, b.1,
            "fixture must exercise the packed legacy old extent"
        );
        assert_ne!(next - GC_HEADER_SIZE, b.1);
        assert_eq!(
            CopyingPointerSet::new().classify(next).unwrap().kind,
            CopyingPointerKind::Old
        );
    })
    .join()
    .unwrap();
}

#[test]
fn partial_large_leaf_backing_is_not_a_bump_target_or_densely_advised() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _suppression = GcSuppressScope::new();
        let large = leaf(32 * 1024);
        let (_, base, _, kind) = crate::arena::classify_heap_storage_in_range(large).unwrap();
        assert!(kind);
        let (used, size) = crate::arena::ARENA.with(|arena| {
            let arena = &*arena.get();
            let b = arena
                .blocks
                .iter()
                .find(|b| b.data as usize == base)
                .unwrap();
            (b.offset, b.size)
        });
        assert!(
            used < size && size < crate::arena::BLOCK_SIZE,
            "fixture needs a partial OS-page tail"
        );
        crate::arena::sync_inline_arena_state();
        let remaining = crate::arena::ARENA.with(|arena| {
            let arena = &*arena.get();
            let current = &arena.blocks[arena.current];
            current.size - current.offset
        });
        let _filler = crate::arena::arena_alloc(remaining - 8, 8);
        let next = crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
        assert!(
            !(base..base + size).contains(&next),
            "ordinary forward search must not pack the tail"
        );
        #[cfg(target_os = "linux")]
        {
            let maps = std::fs::read_to_string("/proc/self/smaps").unwrap();
            let mut inside = false;
            let mut flags = None;
            for line in maps.lines() {
                if let Some((a, b)) = line
                    .split_whitespace()
                    .next()
                    .and_then(|w| w.split_once('-'))
                {
                    if let (Ok(a), Ok(b)) =
                        (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16))
                    {
                        inside = (a..b).contains(&base);
                    }
                }
                if inside && line.starts_with("VmFlags:") {
                    flags = Some(line);
                    break;
                }
            }
            assert!(
                flags.unwrap().split_whitespace().any(|w| w == "nh"),
                "partial extents must retain the existing base-page advice"
            );
        }
    })
    .join()
    .unwrap();
}
