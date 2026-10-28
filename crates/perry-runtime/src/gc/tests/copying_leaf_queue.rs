//! Leaf admission removes drain work, never mark clearing or forwarding.
use super::super::*;
use super::support::*;

#[test]
fn copied_leaves_are_not_queued_but_keep_their_survivor_and_clear_marks() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _heap_change = crate::gc::heap_generation::HeapChange::begin(
            crate::gc::heap_generation::HeapChangeKind::CopyingMinor,
        );
        let leaf = young_leaf();
        let mut collector = CopyingNurseryCollector::new(CopyingPointerSet::new());
        let moved = collector.mark_addr(leaf).unwrap();
        assert_ne!(leaf, moved, "premise: the leaf really moved");
        let header = header_from_user_ptr(moved as *const u8);
        assert_eq!(collector.moved_headers, vec![header]);
        assert!(collector.worklist.is_empty(), "a leaf owes no child scan");
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
        collector.clear_marks();
        assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!((*header).obj_type, GC_TYPE_STRING);
    })
    .join()
    .unwrap();
}

#[test]
fn promoted_leaves_are_not_queued_but_keep_survival_accounting() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let leaf = young_leaf();
        let promotion = crate::arena::retag_young_for_in_place_promotion(true);
        let mut collector = CopyingNurseryCollector::new(CopyingPointerSet::new());
        let ptr = collector.ptrs.classify(leaf).unwrap();
        assert_eq!(ptr.kind, CopyingPointerKind::PromotedYoung);
        assert_eq!(collector.mark_classified_addr(leaf, ptr), leaf);
        assert_eq!(collector.moved_headers, vec![ptr.header]);
        assert_eq!(collector.stats.in_place_promoted_objects, 1);
        assert!(collector.worklist.is_empty());
        collector.clear_marks();
        assert_eq!((*ptr.header).gc_flags & GC_FLAG_MARKED, 0);
        crate::arena::undo_in_place_promotion_retag(&promotion);
    })
    .join()
    .unwrap();
}

#[test]
fn nonmoving_leaves_are_marked_and_cleared_without_a_drain_entry() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let leaf = crate::arena::arena_alloc_gc_longlived(32, 8, GC_TYPE_STRING) as usize;
        let mut collector = CopyingNurseryCollector::new(CopyingPointerSet::new());
        let ptr = collector.ptrs.classify(leaf).unwrap();
        assert_eq!(ptr.kind, CopyingPointerKind::Longlived);
        assert_eq!(collector.mark_addr(leaf), Some(leaf));
        assert_eq!(collector.marked_headers, vec![ptr.header]);
        assert!(collector.worklist.is_empty());
        collector.clear_marks();
        assert_eq!((*ptr.header).gc_flags & GC_FLAG_MARKED, 0);
    })
    .join()
    .unwrap();
}

#[test]
fn a_forwarded_leaf_still_marks_and_scans_its_nonleaf_target() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _heap_change = crate::gc::heap_generation::HeapChange::begin(
            crate::gc::heap_generation::HeapChangeKind::CopyingMinor,
        );
        let stub = young_leaf();
        let (target, fields) = alloc_nursery_test_object(1);
        let child = young_leaf();
        *fields = string_bits(child);
        set_forwarding_address(header_from_user_ptr(stub as *const u8), target.cast());
        let mut collector = CopyingNurseryCollector::new(CopyingPointerSet::new());
        let moved = collector.mark_addr(stub).unwrap();
        assert_ne!(moved, target as usize);
        let header = header_from_user_ptr(moved as *const u8);
        assert!(
            collector.worklist.contains(&header),
            "the target has real edges"
        );
        collector.drain();
        let moved_fields = (moved as *const u8)
            .add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *const u64;
        let moved_child = (*moved_fields & POINTER_MASK) as usize;
        assert_ne!(moved_child, child, "the target's child really moved");
        assert_eq!(
            (*header_from_user_ptr(moved_child as *const u8)).obj_type,
            GC_TYPE_STRING
        );
        collector.clear_marks();
    })
    .join()
    .unwrap();
}
