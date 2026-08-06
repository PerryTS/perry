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
//! These tests pin the claims the fix rests on:
//!
//! 1. old-generation growth no longer SCALES with tape size;
//! 2. the header stays small — and stays OLD-GEN and immovable, which is the
//!    contract every caller outside `json_tape` was already written against;
//! 3. the tape is freed the instant `materialized` is installed, with no
//!    collector involvement — the path `field_access` takes after #7537;
//! 4. the tape is freed by a full collection when its owner dies
//!    unmaterialized (the `roundtrip` shape), and NOT by a minor.

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

/// The header allocation no longer scales with the tape — but it stays in the
/// OLD generation and born tenured, exactly where a multi-megabyte inline-tape
/// header always landed.
///
/// That is the load-bearing half of this test, not a leftover. `json_tape_store`
/// keys a tape by its owner's address, and every caller outside `json_tape`
/// holds raw `*mut LazyArrayHeader` across allocations —
/// `json::stringify_api::try_stringify_lazy_array` reads `blob_bytes` off a raw
/// header and then allocates the result string. Letting the shrunken header
/// fall into the nursery made it movable for the first time and the copying
/// minor relocated it out from under those callers: `field_access` went
/// non-deterministic, emitting a JSON string of NUL bytes for
/// `JSON.stringify(parsed)` on 3 of 60 iterations. If a future change routes
/// the header allocation back through `arena_alloc_gc`, this fails.
#[test]
fn test_lazy_header_is_small_but_stays_old_gen_and_immovable() {
    let _guard = GcTestIsolationGuard::new();
    let blob = big_blob();
    let tape_bytes = tape_bytes_of(&blob);
    assert!(tape_bytes > 4 * crate::gc::LARGE_OBJECT_THRESHOLD_BYTES);

    let lazy = build_lazy(&blob);

    assert!(
        crate::arena::pointer_in_old_gen(lazy as usize),
        "the header must stay old-gen: callers outside json_tape hold raw \
         header pointers across allocations"
    );
    assert!(
        !crate::gc::gc_type_is_movable(crate::gc::GC_TYPE_LAZY_ARRAY),
        "a lazy array must not be movable — its tape is keyed by its address"
    );
    unsafe {
        let header = (lazy as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
        assert_ne!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "the header must be born tenured, as the large-object arm made it"
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

/// A lazy array that dies UNMATERIALIZED must still give its tape back. This
/// is the `roundtrip` shape: parse, stringify off the retained blob, drop.
/// The owner is old-gen, so a FULL collection is what proves it dead — which
/// is also why tape bytes stay in `external_side_live_bytes()`: they have to
/// be able to escalate that reclaim, exactly like a dead Map's entries buffer.
#[test]
fn test_dead_unmaterialized_owner_releases_its_tape_on_a_full_collection() {
    let _guard = GcTestIsolationGuard::new();
    let blob = big_blob();

    let bytes_before = crate::json_tape_store::registered_bytes();
    let lazy = build_lazy(&blob);
    assert!(
        crate::json_tape_store::registered_bytes() > bytes_before,
        "test premise: the lazy array owns tape bytes"
    );
    // Deliberately NOT rooted: the header is unreachable garbage.
    let _ = lazy;

    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));

    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        bytes_before,
        "a full collection must release a dead lazy array's tape"
    );
}

/// A minor must NOT release a live old-gen owner's tape, and must not move the
/// owner. Minors never trace the old generation, so "unmarked" says nothing
/// about an old header — treating one as dead there would free a tape out from
/// under a live lazy array.
#[test]
fn test_a_minor_neither_releases_nor_moves_a_live_owners_tape() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let blob = big_blob();

    let bytes_before = crate::json_tape_store::registered_bytes();
    let lazy = build_lazy(&blob);
    let owned = crate::json_tape_store::registered_bytes() - bytes_before;
    assert!(owned > 0, "test premise: the lazy array owns tape bytes");
    js_shadow_slot_set(0, ptr_bits(lazy as usize));

    let trace = collect_minor_trace(GcTriggerKind::ArenaBytes);
    assert!(
        trace.copying_nursery.eligible,
        "test premise: a COPYING minor must have run, or nothing was exercised"
    );

    assert_eq!(
        crate::json_tape_store::registered_bytes() - bytes_before,
        owned,
        "a minor must not touch a live old-gen owner's tape"
    );
    assert_eq!(
        (js_shadow_slot_get(0) & POINTER_MASK) as usize,
        lazy as usize,
        "the owner must not have moved"
    );
    unsafe {
        let tape = crate::json_tape::LazyArrayHeader::tape_slice(lazy);
        assert_eq!(tape[0].kind, crate::json_tape::KIND_ARR_START);
    }
    let arr = unsafe { crate::json_tape::force_materialize_lazy(lazy) };
    assert_eq!(unsafe { (*arr).length }, ELEMENTS);
    assert_eq!(
        crate::json_tape_store::registered_bytes(),
        bytes_before,
        "materializing must release the tape"
    );
}
