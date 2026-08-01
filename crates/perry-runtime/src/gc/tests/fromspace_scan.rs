//! Teeth for the whole-heap from-space scan (#7035).
//!
//! Both directions are asserted deliberately. A verification instrument that
//! silently reports clean is worse than no instrument at all — that is the
//! defect #7035 records about `PERRY_GC_VERIFY_EVACUATION`, and it cost real
//! bisect cycles on #7022 because "verifier clean" was read as a negative
//! result. So these tests check that the scan (a) FINDS a planted un-rewritten
//! old->young reference, and (b) does NOT report it once the reference is gone.
//!
//! Deltas rather than absolute counts: a test process shares one thread-local
//! heap with whatever else the surrounding test allocated, so only the change
//! attributable to the planted reference is meaningful.

use super::super::fromspace_scan::*;
use super::super::*;

/// Plant a NaN-boxed reference to `young` in `holder`'s first payload word.
///
/// # Safety
/// `holder` must have at least 8 bytes of payload.
unsafe fn plant_reference(holder: *mut u8, young: *mut u8) {
    *(holder as *mut u64) = crate::value::POINTER_TAG | (young as u64 & crate::value::POINTER_MASK);
}

unsafe fn clear_reference(holder: *mut u8) {
    *(holder as *mut u64) = 0;
}

#[test]
fn fromspace_scan_finds_an_unrewritten_old_to_young_reference() {
    // An old-gen holder is outside from-space, so the scan inspects it; a
    // nursery target is inside from-space, so a surviving reference to it after
    // the rewrite pass is exactly what the scan exists to catch.
    let holder = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let young = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    unsafe {
        std::ptr::write_bytes(holder, 0, 64);
    }

    let baseline = scan_heap_for_fromspace_refs();

    // Plant the reference AND mark the target forwarded — i.e. the object moved
    // this cycle and this reference was not updated.
    unsafe {
        plant_reference(holder, young);
        let young_header = header_from_user_ptr(young) as *mut GcHeader;
        (*young_header).gc_flags |= GC_FLAG_FORWARDED;
    }

    let planted = scan_heap_for_fromspace_refs();
    assert!(
        planted.missing_rewrites > baseline.missing_rewrites,
        "the scan must report a planted un-rewritten old->young reference \
         (baseline missing_rewrites={}, planted={})",
        baseline.missing_rewrites,
        planted.missing_rewrites
    );

    // The negative direction: remove the reference and the report must fall
    // back. Without this half, a scan that reported every word as an offender
    // would pass the assertion above.
    unsafe {
        clear_reference(holder);
    }
    let cleared = scan_heap_for_fromspace_refs();
    assert!(
        cleared.missing_rewrites < planted.missing_rewrites,
        "removing the reference must lower the count again \
         (planted={}, cleared={})",
        planted.missing_rewrites,
        cleared.missing_rewrites
    );

    unsafe {
        let young_header = header_from_user_ptr(young) as *mut GcHeader;
        (*young_header).gc_flags &= !GC_FLAG_FORWARDED;
    }
}

#[test]
fn fromspace_scan_separates_dangling_from_missing_rewrite() {
    // A reference to a young object that was NOT forwarded is a different
    // defect class (the target was never evacuated and is about to be recycled)
    // and must be counted separately, because the two have different fixes.
    let holder = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let young = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    unsafe {
        std::ptr::write_bytes(holder, 0, 64);
        let young_header = header_from_user_ptr(young) as *mut GcHeader;
        (*young_header).gc_flags &= !GC_FLAG_FORWARDED;
    }

    let baseline = scan_heap_for_fromspace_refs();
    unsafe {
        plant_reference(holder, young);
    }
    let planted = scan_heap_for_fromspace_refs();

    assert!(
        planted.dangling > baseline.dangling,
        "a reference to a NON-forwarded from-space object must be counted as \
         dangling (baseline={}, planted={})",
        baseline.dangling,
        planted.dangling
    );
    assert_eq!(
        planted.missing_rewrites, baseline.missing_rewrites,
        "a non-forwarded target must NOT be counted as a missing rewrite"
    );

    unsafe {
        clear_reference(holder);
    }
}

#[test]
fn fromspace_scan_ignores_references_held_by_from_space_objects() {
    // A dead nursery object legitimately still points at its dead peers. If the
    // scan reported those it would drown the real signal — on the #7022
    // reproducer from-space holds tens of thousands of such objects.
    let dead_holder = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let young = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    unsafe {
        std::ptr::write_bytes(dead_holder, 0, 64);
    }

    let baseline = scan_heap_for_fromspace_refs();
    unsafe {
        plant_reference(dead_holder, young);
        let young_header = header_from_user_ptr(young) as *mut GcHeader;
        (*young_header).gc_flags |= GC_FLAG_FORWARDED;
    }
    let planted = scan_heap_for_fromspace_refs();

    assert_eq!(
        planted.missing_rewrites, baseline.missing_rewrites,
        "a from-space holder's reference into from-space must be ignored"
    );

    unsafe {
        clear_reference(dead_holder);
        let young_header = header_from_user_ptr(young) as *mut GcHeader;
        (*young_header).gc_flags &= !GC_FLAG_FORWARDED;
    }
}

/// #7154: a FORWARDED owner is a dead relocation stub. Its payload legitimately
/// still names pre-move addresses, so reporting it is a false positive — and
/// unlike a from-space owner it can sit in old-gen, where the space check does
/// not reach it. On a Perry-compiled zod workload this was the single largest
/// population in the residue, which is what made it worth a predicate.
///
/// The skip must also be COUNTED. A filter that shrinks the offender count
/// without saying so reads exactly like progress, which is the failure mode
/// this whole instrument exists to prevent.
#[test]
fn fromspace_scan_skips_but_counts_forwarded_owners_7154() {
    let holder = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let young = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    unsafe {
        std::ptr::write_bytes(holder, 0, 64);
    }

    let baseline = scan_heap_for_fromspace_refs();

    // Same planted offender as the positive test above...
    unsafe {
        plant_reference(holder, young);
        let young_header = header_from_user_ptr(young) as *mut GcHeader;
        (*young_header).gc_flags |= GC_FLAG_FORWARDED;
    }
    let reported = scan_heap_for_fromspace_refs();
    assert!(
        reported.missing_rewrites > baseline.missing_rewrites,
        "test premise: the planted reference must be reported while the holder \
         is live (baseline={}, reported={})",
        baseline.missing_rewrites,
        reported.missing_rewrites
    );

    // ...but once the HOLDER itself is forwarded it is dead, and the identical
    // planted reference must stop being an offender.
    unsafe {
        let holder_header = header_from_user_ptr(holder) as *mut GcHeader;
        (*holder_header).gc_flags |= GC_FLAG_FORWARDED;
    }
    let after = scan_heap_for_fromspace_refs();
    assert_eq!(
        after.missing_rewrites, baseline.missing_rewrites,
        "#7154: a FORWARDED owner is a dead relocation stub — its stale payload \
         must not be reported as a missing rewrite"
    );
    assert!(
        after.forwarded_owners_skipped > baseline.forwarded_owners_skipped,
        "#7154: the skip must be COUNTED, not silent — a suppressed population \
         that does not show up in the report reads as a fixed one \
         (baseline={}, after={})",
        baseline.forwarded_owners_skipped,
        after.forwarded_owners_skipped
    );

    unsafe {
        clear_reference(holder);
        let holder_header = header_from_user_ptr(holder) as *mut GcHeader;
        (*holder_header).gc_flags &= !GC_FLAG_FORWARDED;
        let young_header = header_from_user_ptr(young) as *mut GcHeader;
        (*young_header).gc_flags &= !GC_FLAG_FORWARDED;
    }
}
