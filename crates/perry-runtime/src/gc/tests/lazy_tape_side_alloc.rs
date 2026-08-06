//! The JSON tape is a side allocation, not old-generation arena bytes (#7539).
//!
//! A `LazyArrayHeader` used to carry its tape INLINE, so the whole allocation
//! was as large as the tape — ~2.4 MB for the 10 k-record `field_access`
//! fixture. That is over `LARGE_OBJECT_THRESHOLD_BYTES` (16 KB), so
//! `arena_alloc_gc` routed it into the OLD generation and stamped
//! `GC_FLAG_TENURED` on it, and old-gen bytes are reclaimable only by a FULL
//! collection. A tape that dies at the end of its loop iteration therefore
//! accumulated at ~2.4 MB per parse until `old_reclaim_pressure_due` fired.
//!
//! Measured at the parent commit with `PERRY_GC_TRACE=1` over 53 parses of
//! `benchmarks/json_polyglot/bench_field_access.ts`: 19 collections, 9 full,
//! **6 of those triggered by `old_gen_bytes`**. `bench.ts` (roundtrip, which
//! never materialises) is the cleanest attribution — its nursery peaks at
//! 4.1 MB while the OLD generation peaks at **39.6 MB** and fires 5
//! `old_gen_bytes` fulls. In that program the old generation IS the tape.
//!
//! These tests pin the four claims the fix rests on:
//!
//! 1. a multi-megabyte tape puts NOTHING in the old generation, and the header
//!    it belongs to is a small nursery object;
//! 2. the tape is freed the instant `materialized` is installed, with no
//!    collector involvement — the path `field_access` takes after #7537;
//! 3. the tape is freed when its owner dies, under both the copying minor
//!    (bulk from-space reset, no per-object finalizer) and the full sweep;
//! 4. an evacuated owner keeps its tape, and the tape still reads correctly.

use super::super::*;
use super::support::*;

fn build_lazy(input: &[u8]) -> *mut crate::json_tape::LazyArrayHeader {
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    crate::json_tape::with_built_tape(input, |tape| unsafe {
        crate::json_tape::alloc_lazy_array(
            tape,
            0,
            crate::json_tape::count_array_length(tape, 0),
            text,
        )
    })
    .expect("valid JSON should build a tape")
}

const ELEMENTS: u32 = 20_000;

/// `[0,1,...,N-1]` — one tape entry per element.
fn flat_blob() -> Vec<u8> {
    let mut blob = Vec::with_capacity(256 * 1024);
    blob.push(b'[');
    for i in 0..ELEMENTS {
        if i > 0 {
            blob.push(b',');
        }
        blob.extend_from_slice(i.to_string().as_bytes());
    }
    blob.push(b']');
    blob
}

/// `[[0],[1],...,[N-1]]` — same element COUNT (so the same sparse-cache size)
/// and nearly the same blob length, but three tape entries per element.
fn nested_blob() -> Vec<u8> {
    let mut blob = Vec::with_capacity(256 * 1024);
    blob.push(b'[');
    for i in 0..ELEMENTS {
        if i > 0 {
            blob.push(b',');
        }
        blob.push(b'[');
        blob.extend_from_slice(i.to_string().as_bytes());
        blob.push(b']');
    }
    blob.push(b']');
    blob
}

fn big_blob() -> Vec<u8> {
    flat_blob()
}

fn tape_bytes_of(blob: &[u8]) -> usize {
    crate::json_tape::build_tape(blob)
        .expect("valid JSON")
        .entries
        .len()
        * std::mem::size_of::<crate::json_tape::TapeEntry>()
}

/// The load-bearing claim: old-generation growth no longer SCALES with the
/// tape, because the tape is not a GC allocation at all.
///
/// Measuring one parse against zero would only prove that old-gen grew by less
/// than the tape — but a parse legitimately puts other things there (the
/// retained blob string and the sparse element cache are both well over
/// `LARGE_OBJECT_THRESHOLD_BYTES` at this size). So compare two blobs with the
/// SAME element count, and therefore the same cache and near-identical blob
/// bytes, whose tapes differ by ~3×. Before the fix the extra tape entries
/// landed in old-gen one-for-one; now the difference is only the few extra
/// bracket characters in the blob.
#[test]
fn test_old_generation_growth_does_not_scale_with_tape_size() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let flat = flat_blob();
    let nested = nested_blob();
    let flat_tape = tape_bytes_of(&flat);
    let nested_tape = tape_bytes_of(&nested);
    let tape_delta = nested_tape - flat_tape;
    assert!(
        flat_tape > 4 * crate::gc::LARGE_OBJECT_THRESHOLD_BYTES
            && tape_delta > 4 * crate::gc::LARGE_OBJECT_THRESHOLD_BYTES,
        "test premise: both tapes ({flat_tape} B, {nested_tape} B) and their \
         difference must be well over the large-object threshold, or this \
         test proves nothing"
    );
    let blob_delta = nested.len() - flat.len();

    let before_flat = crate::arena::old_gen_in_use_bytes();
    let _flat_lazy = build_lazy(&flat);
    let flat_growth = crate::arena::old_gen_in_use_bytes() - before_flat;

    let before_nested = crate::arena::old_gen_in_use_bytes();
    let _nested_lazy = build_lazy(&nested);
    let nested_growth = crate::arena::old_gen_in_use_bytes() - before_nested;

    let growth_delta = nested_growth.saturating_sub(flat_growth);
    assert!(
        growth_delta < tape_delta / 2,
        "old-gen growth tracked the tape: {tape_delta} B more tape produced \
         {growth_delta} B more old-gen (blob grew only {blob_delta} B, and \
         the sparse cache is identical at {ELEMENTS} elements)"
    );
}

/// The header itself is a small, untenured nursery object now — the property
/// that keeps it out of `arena_alloc_gc`'s large-object arm no matter how big
/// the blob is.
#[test]
fn test_lazy_header_is_a_small_nursery_object_for_a_huge_tape() {
    let _guard = GcTestIsolationGuard::new();
    let blob = big_blob();
    let tape_bytes = tape_bytes_of(&blob);
    assert!(tape_bytes > 4 * crate::gc::LARGE_OBJECT_THRESHOLD_BYTES);

    let lazy = build_lazy(&blob);

    assert!(
        crate::arena::pointer_in_nursery(lazy as usize),
        "the header should be nursery-resident once the tape moved out"
    );
    unsafe {
        let header = (lazy as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
        assert_eq!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "a small header must not be born tenured — being born tenured is \
             what made a per-iteration-dead tape reclaimable only by a full \
             collection"
        );
        assert!(
            ((*header).size as usize) < crate::gc::LARGE_OBJECT_THRESHOLD_BYTES,
            "the header allocation must not scale with the tape"
        );
        assert_eq!(
            (*lazy).tape_len as usize,
            tape_bytes / std::mem::size_of::<crate::json_tape::TapeEntry>()
        );
    }
    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        tape_bytes,
        "the tape bytes must be accounted to the side-allocation store"
    );
}

/// Installing `materialized` disowns the tape immediately. No collector runs
/// here at all — this is the deterministic half of the fix, and the half
/// `field_access` actually relies on: #7537 flips the scan to the batch parser
/// after a few hundred of 10 000 reads, so the tape is dead long before any
/// collection would have proved it.
#[test]
fn test_materialization_releases_the_tape_without_a_collection() {
    let _guard = GcTestIsolationGuard::new();
    let blob = big_blob();
    let lazy = build_lazy(&blob);

    let bytes_before = crate::json_tape_store::registered_bytes();
    assert!(
        bytes_before > 0,
        "test premise: the lazy array owns tape bytes"
    );
    let collections_before = gc_collection_count();

    let arr = unsafe { crate::json_tape::force_materialize_lazy(lazy) };
    assert!(!arr.is_null());

    assert_eq!(
        gc_collection_count(),
        collections_before,
        "the release must not depend on a collection running"
    );
    assert!(
        crate::json_tape_store::registered_bytes() < bytes_before,
        "materialization must hand the tape bytes back"
    );
    unsafe {
        assert!(
            (*lazy).tape.is_null(),
            "the disowned tape pointer must be nulled, not left dangling"
        );
        assert_eq!((*lazy).tape_len, 0);
        // The materialized array is still correct and still readable.
        assert_eq!((*arr).length, 20_000);
    }
    assert_eq!(
        crate::array::js_array_get(arr, 19_999).bits(),
        crate::value::JSValue::number(19_999.0).bits()
    );
    // A disowned tape reads as empty rather than as freed memory.
    assert!(unsafe { crate::json_tape::LazyArrayHeader::tape_slice(lazy).is_empty() });
}

/// A lazy array that dies UNMATERIALIZED must still give its tape back. This
/// is the `roundtrip` shape: parse, stringify off the retained blob, drop.
/// The owner dies in the nursery, so the copying minor's bulk from-space reset
/// is what reclaims it — and that path runs no per-object finalizer, which is
/// exactly why `json_tape_store` needs its own from-space pass.
#[test]
fn test_dead_unmaterialized_owner_releases_its_tape_on_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let blob = big_blob();

    let bytes_before = crate::json_tape_store::registered_bytes();
    let lazy = build_lazy(&blob);
    let owned = crate::json_tape_store::registered_bytes() - bytes_before;
    assert!(owned > 0, "test premise: the lazy array owns tape bytes");
    // Deliberately NOT rooted: the header is unreachable garbage.
    let _ = lazy;

    let trace = collect_minor_trace(GcTriggerKind::ArenaBytes);
    assert!(
        trace.copying_nursery.eligible,
        "test premise: this must be a COPYING minor — the bulk from-space \
         reset is the path that skips per-object finalizers, so a fallback \
         minor here would exercise nothing"
    );
    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        bytes_before,
        "a dead unmaterialized lazy array must release its tape"
    );
}

/// Same, through the full mark-sweep — the non-copying cycle kind, where the
/// registry pass at sweep entry is what sees the death.
#[test]
fn test_dead_unmaterialized_owner_releases_its_tape_on_a_full_collection() {
    let _guard = GcTestIsolationGuard::new();
    let blob = big_blob();

    let bytes_before = crate::json_tape_store::registered_bytes();
    let lazy = build_lazy(&blob);
    assert!(crate::json_tape_store::registered_bytes() > bytes_before);
    let _ = lazy;

    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));

    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        bytes_before,
        "a full collection must release a dead lazy array's tape"
    );
}

/// The header is small and nursery-resident now, so the copying minor really
/// does evacuate it — the old multi-megabyte header was born old and never
/// moved. The registry is keyed by the header address, so without the
/// `GcMoveHookKind::LazyArrayTape` rekey the survivor would read a tape it no
/// longer owns and leak the entry keyed at the stale address.
#[test]
fn test_evacuated_owner_keeps_and_still_reads_its_tape() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let input = br#"[10,20,30,40]"#;
    let lazy = build_lazy(input);
    js_shadow_slot_set(0, ptr_bits(lazy as usize));

    let owned_before = crate::json_tape_store::registered_bytes();
    let entries_before = unsafe { (*lazy).tape_len };
    assert!(owned_before > 0 && entries_before > 0);

    let _ = gc_collect_minor();

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, 0, "the rooted lazy array must survive");
    assert_ne!(
        moved, lazy as usize,
        "test premise: the header must actually have moved, or the rekey \
         path is untested"
    );
    let moved_hdr = moved as *mut crate::json_tape::LazyArrayHeader;
    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        owned_before,
        "an evacuated owner must keep owning exactly its tape bytes"
    );
    unsafe {
        assert_eq!((*moved_hdr).tape_len, entries_before);
        let tape = crate::json_tape::LazyArrayHeader::tape_slice(moved_hdr);
        assert_eq!(tape.len(), entries_before as usize);
        assert_eq!(tape[0].kind, crate::json_tape::KIND_ARR_START);
    }
    // And it still materializes to the right values through the moved header.
    let arr = unsafe { crate::json_tape::force_materialize_lazy(moved_hdr) };
    assert_eq!(
        crate::array::js_array_get(arr, 3).bits(),
        crate::value::JSValue::number(40.0).bits()
    );
    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        0,
        "materializing the moved header must release the rekeyed entry"
    );
}
