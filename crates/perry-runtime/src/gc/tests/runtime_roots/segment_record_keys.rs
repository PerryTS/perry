//! The per-thread `{ segment, index, input(, isWordLike) }` keys arrays every
//! `Intl.Segmenter` segment record shares.
//!
//! Exactly the shape `iter_result_keys.rs` guards, for the same reason and
//! with the same failure mode: `scripts/gc_root_dominance_check.py` reads
//! emitted LLVM IR, so a thread-local holding a `*mut ArrayHeader` into the
//! heap is structurally invisible to it, and the runtime scanner is the only
//! thing between this cache and a use-after-free. Being a cache rather than a
//! register, it would go bad at collection #0 and stay bad — corrupting every
//! later segment record on the thread instead of failing intermittently.
//!
//! Marking alone is not enough: a marked but un-rewritten slot still hands out
//! a pre-move address after a copying minor, so there is a MARK test and a
//! REWRITE test, plus a registration check (a scanner a test can call directly
//! is a no-op in production until `gc_init` names it).

use super::*;
use crate::array::ArrayHeader;
use crate::intl::segmenter::{SegmentRecordShape, SEGMENT_RECORD_SHAPE_LIST};

/// Empties the cache on entry and exit and pins the GC triggers for the body,
/// exactly as `IterResultKeysGuard` does.
struct SegmentRecordKeysGuard {
    _triggers: GcTriggerThresholdTestGuard,
}

impl SegmentRecordKeysGuard {
    fn new() -> Self {
        let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        crate::intl::segmenter::reset_shared_segment_keys_for_test();
        Self {
            _triggers: triggers,
        }
    }
}

impl Drop for SegmentRecordKeysGuard {
    fn drop(&mut self) {
        crate::intl::segmenter::reset_shared_segment_keys_for_test();
    }
}

fn evacuate_array(from: *mut ArrayHeader) -> *mut ArrayHeader {
    let to = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
    unsafe {
        set_forwarding_address(header_from_user_ptr(from as *const u8), to);
    }
    to as *mut ArrayHeader
}

/// MARK. The cache is the ONLY reference to these arrays — the records that
/// point at them are short-lived while the cache outlives them — so an
/// unmarked slot is a swept slot, and every later segment record installs a
/// freed keys array as its shape.
#[test]
fn segment_record_keys_cache_is_marked_by_the_collector() {
    let _guard = SegmentRecordKeysGuard::new();
    clear_marks();
    clear_mark_seeds();

    let arrays = crate::intl::segmenter::populate_shared_segment_keys_for_test();
    let valid_ptrs = build_valid_pointer_set();

    crate::intl::segmenter::scan_segment_record_keys_roots_mut(&mut RuntimeRootVisitor::for_mark(
        &valid_ptrs,
    ));

    for (i, arr) in arrays.iter().enumerate() {
        assert!(!arr.is_null(), "keys slot {i} should have been populated");
        assert_marked_user_ptr(
            *arr as usize,
            &format!("segment-record keys array {i} (nothing else references it)"),
        );
    }

    clear_marks();
    clear_mark_seeds();
}

/// REWRITE, every slot. Marking keeps the array alive; only the rewrite makes
/// the slot name the surviving copy.
#[test]
fn every_segment_record_keys_slot_is_rewritten_by_the_collector() {
    let _guard = SegmentRecordKeysGuard::new();

    let before = crate::intl::segmenter::populate_shared_segment_keys_for_test();
    let valid_ptrs = build_valid_pointer_set();
    let expected: Vec<*mut ArrayHeader> = before.iter().map(|p| evacuate_array(*p)).collect();

    crate::intl::segmenter::scan_segment_record_keys_roots_mut(
        &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
    );

    for (i, shape) in SEGMENT_RECORD_SHAPE_LIST.into_iter().enumerate() {
        assert_eq!(
            crate::intl::segmenter::shared_segment_keys_peek_for_test(shape),
            expected[i],
            "segment-record keys slot {i} ({shape:?}) must be rewritten to the \
             relocated array. A marked-but-stale slot goes bad at collection #0 \
             and then EVERY segment record on this thread installs a from-space \
             keys array as its shape."
        );
    }
}

/// An empty cache is the state between process start and the first
/// `Intl.Segmenter` use, and every cycle in that window scans it. A null slot
/// must be skipped, not treated as an address.
#[test]
fn scanning_an_empty_segment_record_keys_cache_is_a_no_op() {
    let _guard = SegmentRecordKeysGuard::new();
    let valid_ptrs = build_valid_pointer_set();

    crate::intl::segmenter::scan_segment_record_keys_roots_mut(
        &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
    );

    for shape in SEGMENT_RECORD_SHAPE_LIST {
        assert!(
            crate::intl::segmenter::shared_segment_keys_peek_for_test(shape).is_null(),
            "scanning must not populate the {shape:?} keys slot"
        );
    }
}

/// …and it must actually be REGISTERED: an unregistered scanner is a no-op in
/// production, which is precisely the bug this cache would introduce.
#[test]
fn segment_record_keys_scanner_is_registered() {
    crate::gc::gc_init();
    let registered = |scanner: MutableRootScanner| {
        crate::gc::roots::MUTABLE_ROOT_SCANNERS.with(|scanners| {
            scanners
                .borrow()
                .iter()
                .any(|entry| entry.scanner as usize == scanner as usize)
        })
    };

    assert!(
        registered(
            crate::intl::segmenter::scan_segment_record_keys_roots_mut as MutableRootScanner
        ),
        "scan_segment_record_keys_roots_mut must be registered in gc_init — unregistered, \
         the shared segment-record keys arrays are swept by the first minor and every later \
         segment record installs a freed array as its shape"
    );
}

/// The cache must be STABLE: the second segment record reuses the array the
/// first one built. If it did not, nothing would have been saved — and,
/// because `shape_id_for_keys_ensure` keys the shape table on the array's
/// ADDRESS, a fresh array per record is also a fresh shape id per record,
/// which is what made every read of `.segment` an inline-cache miss.
#[test]
fn segment_record_keys_are_built_once_per_shape() {
    let _guard = SegmentRecordKeysGuard::new();

    let first = crate::intl::segmenter::populate_shared_segment_keys_for_test();
    let second = crate::intl::segmenter::populate_shared_segment_keys_for_test();

    assert_eq!(
        first, second,
        "the shared keys arrays must be built once per thread per shape; rebuilding them \
         per record restores both the per-record allocations and the one-shape-id-per-record \
         inline-cache miss"
    );
    assert_eq!(
        first.len(),
        SEGMENT_RECORD_SHAPE_LIST.len(),
        "every segment-record shape needs its own shared array"
    );
    assert_ne!(
        first[SegmentRecordShape::Plain as usize],
        first[SegmentRecordShape::WordLike as usize],
        "the two shapes must not share one array: `isWordLike` is present only for \
         word granularity"
    );
}
