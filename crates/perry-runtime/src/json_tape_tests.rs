//! Unit tests for the JSON tape builder, its materializer, and the
//! lazy-array header's access/flip policy.
//!
//! Split out of `json_tape.rs` so that file stays under the 2,000-line
//! CI cap (`scripts/check_file_size.sh`). Declared from there with
//! `#[cfg(test)] #[path = "json_tape_tests.rs"] mod tests;`, so
//! `use super::*` still names the tape module's private items.

use super::*;

/// Tape structure invariants on a simple object — exercises the
/// OBJ_START → KEY → scalar → OBJ_END chain and the backfilled
/// `link` for skip-over.
#[test]
fn tape_simple_object() {
    let input = br#"{"a":1,"b":"x"}"#;
    let tape = build_tape(input).unwrap();
    let kinds: Vec<u8> = tape.entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            KIND_OBJ_START,
            KIND_KEY,
            KIND_NUMBER,
            KIND_KEY,
            KIND_STRING,
            KIND_OBJ_END
        ]
    );
    // OBJ_START.link points at the matching OBJ_END (last entry).
    assert_eq!(tape.entries[0].link as usize, tape.entries.len() - 1);
    // OBJ_END.link points back at OBJ_START.
    assert_eq!(
        *tape.entries.last().unwrap(),
        TapeEntry {
            offset: tape.entries.last().unwrap().offset,
            kind: KIND_OBJ_END,
            link: 0
        }
    );
}

/// Nested structure — an array of objects. Each inner OBJ_START
/// must have its link pointing at its OWN OBJ_END, not the outer
/// ARR_END. This is the invariant Phase 3 (lazy indexed access)
/// relies on to skip past unwanted elements.
#[test]
fn tape_nested_array_of_objects() {
    let input = br#"[{"a":1},{"b":2},{"c":3}]"#;
    let tape = build_tape(input).unwrap();
    // ARR_START ... ARR_END outer
    assert_eq!(tape.entries[0].kind, KIND_ARR_START);
    assert_eq!(tape.entries.last().unwrap().kind, KIND_ARR_END);
    // Three object children — count OBJ_START entries.
    let n_objs = tape
        .entries
        .iter()
        .filter(|e| e.kind == KIND_OBJ_START)
        .count();
    assert_eq!(n_objs, 3);
    // Each OBJ_START's link points at an OBJ_END strictly before ARR_END.
    for (i, e) in tape.entries.iter().enumerate() {
        if e.kind == KIND_OBJ_START {
            let end = e.link as usize;
            assert!(end > i, "OBJ_START.link must point forward");
            assert!(
                end < tape.entries.len() - 1,
                "OBJ_END must precede outer ARR_END"
            );
            assert_eq!(tape.entries[end].kind, KIND_OBJ_END);
            assert_eq!(
                tape.entries[end].link as usize, i,
                "OBJ_END.link must point back"
            );
        }
    }
}

/// Escaped string in a key and value — tape should still emit
/// one KEY and one STRING entry; string decoding is deferred to
/// materialization and doesn't perturb the tape shape.
#[test]
fn tape_escaped_strings() {
    let input = br#"{"a\"b":"x\\y"}"#;
    let tape = build_tape(input).unwrap();
    assert_eq!(
        tape.entries.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec![KIND_OBJ_START, KIND_KEY, KIND_STRING, KIND_OBJ_END]
    );
}

/// Malformed inputs must return None (caller falls back to
/// direct parser with richer error messages).
#[test]
fn tape_malformed_returns_none() {
    assert!(build_tape(b"{").is_none(), "unclosed object");
    assert!(build_tape(b"[").is_none(), "unclosed array");
    assert!(build_tape(b"{a:1}").is_none(), "unquoted key");
    assert!(build_tape(b"{\"a\"}").is_none(), "missing colon");
    assert!(build_tape(b"0 trailing").is_none(), "trailing token");
    assert!(build_tape(b"01").is_none(), "leading zero");
    assert!(build_tape(b"1.").is_none(), "empty fraction");
    assert!(build_tape(b"1e+").is_none(), "empty exponent");
    assert!(build_tape(br#""\q""#).is_none(), "invalid escape");
    assert!(build_tape(b"\"line\nfeed\"").is_none(), "raw control byte");
    assert!(build_tape(b"").is_none(), "empty input");
}

/// Top-level scalar (allowed by JSON spec).
#[test]
fn tape_top_level_scalars() {
    assert_eq!(build_tape(b"42").unwrap().entries.len(), 1);
    assert_eq!(build_tape(b"true").unwrap().entries.len(), 1);
    assert_eq!(build_tape(br#""hi""#).unwrap().entries.len(), 1);
    assert_eq!(build_tape(b"null").unwrap().entries.len(), 1);
}

#[test]
fn recursive_materializer_reserves_exact_spill_per_object_depth() {
    let input = br#"{"a":1,"nested":{"n0":0,"n1":1,"n2":2,"n3":3,"n4":4},"b":2}"#;
    let tape = build_tape(input).expect("valid tape");
    let nested_key = crate::string::js_string_from_bytes(b"nested".as_ptr(), 6);

    crate::gc::gc_suppress();
    let value = unsafe { materialize(&tape, input) };
    let object = (value.bits() & crate::value::POINTER_MASK) as *const crate::ObjectHeader;
    let nested = crate::object::js_object_get_field_by_name(object, nested_key);
    let nested = (nested.bits() & crate::value::POINTER_MASK) as *const crate::ObjectHeader;

    unsafe {
        assert_eq!(
            (*object).field_count,
            crate::object::INLINE_SLOT_FLOOR as u32,
            "known width must not enlarge the primary object"
        );
        let spill =
            crate::object::test_spill_buffer_addr(object as usize) as *const crate::ArrayHeader;
        assert!(!spill.is_null());
        assert_eq!((*spill).capacity, 3, "count only outer-object keys");
        assert_eq!((*spill).length, 3);

        assert_eq!(
            (*nested).field_count,
            crate::object::INLINE_SLOT_FLOOR as u32
        );
        let nested_spill =
            crate::object::test_spill_buffer_addr(nested as usize) as *const crate::ArrayHeader;
        assert!(!nested_spill.is_null());
        assert_eq!(
            (*nested_spill).capacity,
            5,
            "reserve the nested width exactly"
        );
        assert_eq!((*nested_spill).length, 5);
    }
    crate::gc::gc_unsuppress();
}

#[test]
fn iterative_materializer_preserves_nested_objects_arrays_and_duplicate_keys() {
    let input = br#"{"a":[1,true,"x"],"a":{"b":2}}"#;
    let tape = build_tape(input).expect("valid tape");
    let saved_roots = crate::json::parse_root_save_len();
    crate::gc::gc_suppress();
    let value = unsafe { materialize_iterative(&tape.entries, input) }.expect("materializes");
    crate::json::parse_root_push(value);
    crate::gc::gc_unsuppress();

    let object = (value.bits() & crate::value::POINTER_MASK) as *const crate::ObjectHeader;
    let key_a = crate::string::js_string_from_bytes(b"a".as_ptr(), 1);
    let key_b = crate::string::js_string_from_bytes(b"b".as_ptr(), 1);
    let nested = crate::object::js_object_get_field_by_name(object, key_a);
    let nested = (nested.bits() & crate::value::POINTER_MASK) as *const crate::ObjectHeader;
    let b = crate::object::js_object_get_field_by_name(nested, key_b);
    assert_eq!(f64::from_bits(b.bits()), 2.0);

    crate::json::parse_root_restore(saved_roots);
}

#[test]
fn iterative_materializer_reserves_exact_spill_without_widening_object() {
    let input = br#"{"f0":0,"f1":1,"f2":2,"f3":3,"f4":4}"#;
    let tape = build_tape(input).expect("valid tape");

    crate::gc::gc_suppress();
    let value = unsafe { materialize_iterative(&tape.entries, input) }.expect("materializes");
    let object = (value.bits() & crate::value::POINTER_MASK) as *const crate::ObjectHeader;
    unsafe {
        assert_eq!(
            (*object).field_count,
            crate::object::INLINE_SLOT_FLOOR as u32
        );
        let spill =
            crate::object::test_spill_buffer_addr(object as usize) as *const crate::ArrayHeader;
        assert!(!spill.is_null());
        assert_eq!((*spill).capacity, 5);
        assert_eq!((*spill).length, 5);
    }
    crate::gc::gc_unsuppress();
}

/// `TapeEntry` is 12 bytes (u32 + u8 + padding + u32). Keeping
/// this compact matters for tape-size parity with parse output:
/// a 1 MB JSON blob with ~20k tokens should build a ~240 KB tape,
/// not a megabyte.
#[test]
fn tape_entry_layout() {
    assert!(
        std::mem::size_of::<TapeEntry>() <= 12,
        "TapeEntry grew beyond 12 bytes — check padding"
    );
}

/// #7539 requirement: the tape is POINTER-FREE BY CONSTRUCTION, which is what
/// licenses moving it out of the GC heap into a `json_tape_store` side
/// allocation that is never marked, scanned, or rewritten.
///
/// The claim is structural, not a convention, and this pins the structure:
/// every `TapeEntry` field is an integer, and `offset`/`link` are `u32` — too
/// narrow to hold a 48-bit heap address even if some future code tried. `kind`
/// is a `u8`. There is exactly one writer of the region
/// (`json_tape_store::allocate`'s `copy_nonoverlapping` from a
/// `&[TapeEntry]`), so nothing can smuggle a reference in behind it.
///
/// If someone widens a field to pointer size this fails, and the whole
/// direction has to be revisited: a tape that can carry a heap edge would need
/// tracing, and an untraced one would be a use-after-free.
#[test]
fn tape_entry_is_pointer_free_by_construction() {
    // On every 64-bit target a struct with a pointer-sized field has
    // alignment 8. `TapeEntry`'s alignment is 4, so no field it has — present
    // or future — can hold a `*mut`/`usize`/`u64`. That is the whole proof,
    // and it is checked by the compiler's own layout rules rather than by
    // reading the field list and trusting it.
    assert_eq!(
        std::mem::align_of::<TapeEntry>(),
        4,
        "TapeEntry gained a pointer-aligned field — it can no longer be \
         assumed pointer-free, and json_tape_store's untraced side \
         allocation would become a use-after-free"
    );
    assert!(
        std::mem::size_of::<TapeEntry>() <= 12,
        "TapeEntry grew — recheck the pointer-free claim above"
    );
    // Field widths, restated so a `u32 -> u64` widening fails here with a
    // message that says why rather than only at the alignment assert.
    let probe = TapeEntry {
        offset: u32::MAX,
        kind: u8::MAX,
        link: u32::MAX,
    };
    assert_eq!(std::mem::size_of_val(&probe.offset), 4);
    assert_eq!(std::mem::size_of_val(&probe.kind), 1);
    assert_eq!(std::mem::size_of_val(&probe.link), 4);
}

/// The tape lives outside the header allocation now, so the header stays small
/// no matter how big the blob is. That is the property keeping it out of
/// `arena_alloc_gc`'s large-object arm — and therefore out of the old
/// generation, where only a FULL collection could reclaim it.
#[test]
fn lazy_array_header_stays_small_regardless_of_tape_size() {
    assert!(
        std::mem::size_of::<LazyArrayHeader>() < crate::gc::LARGE_OBJECT_THRESHOLD_BYTES,
        "LazyArrayHeader must stay under the large-object threshold"
    );
    // Restated here because this file is where the header's shape is asserted:
    // `.length` is an inlined raw u32 load at offset 0 (codegen contract), and
    // #7537's scan-flip threshold must keep its shape.
    assert_eq!(std::mem::offset_of!(LazyArrayHeader, cached_length), 0);
    assert_eq!(scan_flip_threshold(10_000), 156);
    assert_eq!(scan_flip_threshold(10), 64);
}

/// A disowned tape reads as EMPTY rather than as freed memory. Every reader
/// checks `materialized` first, but the null guard is what makes a stale read
/// safe instead of a use-after-free, so pin it directly.
#[test]
fn disowned_tape_reads_as_empty() {
    let input = b"[1,2,3]";
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let lazy = with_built_tape(input, |tape| unsafe {
        alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
    })
    .expect("valid JSON should build a tape");

    assert!(!unsafe { LazyArrayHeader::tape_slice(lazy) }.is_empty());
    let arr = unsafe { force_materialize_lazy(lazy) };
    assert_eq!(unsafe { (*arr).length }, 3);

    unsafe {
        assert!((*lazy).tape.is_null());
        assert!(LazyArrayHeader::tape_slice(lazy).is_empty());
        let scope = crate::gc::RuntimeHandleScope::new();
        let source = TapeSource::Lazy {
            hdr_handle: scope.root_raw_mut_ptr(lazy),
        };
        assert!(
            source.entry(0).is_none(),
            "a disowned tape must yield no entries"
        );
    }
}

#[test]
fn force_materialize_numeric_lazy_array_preserves_raw_payload() {
    let input = br#"[1,2.5,3]"#;
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let lazy = with_built_tape(input, |tape| unsafe {
        alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
    })
    .expect("valid JSON should build a tape");

    let before = reparse_materializations();
    let arr = unsafe { force_materialize_lazy(lazy) };
    assert_eq!(
        reparse_materializations(),
        before + 1,
        "an uncached lazy array must batch-materialize via the #7478 reparse"
    );

    assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 1);
    assert_eq!(crate::array::js_array_numeric_get_f64_unboxed(arr, 0), 1.0);
    assert_eq!(crate::array::js_array_numeric_get_f64_unboxed(arr, 1), 2.5);
    assert_eq!(crate::array::js_array_numeric_get_f64_unboxed(arr, 2), 3.0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 3),
        Some(0)
    );
}

#[test]
fn force_materialize_lazy_array_cache_downgrades_for_pointer_values() {
    let input = br#"[1,2,3]"#;
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let lazy = with_built_tape(input, |tape| unsafe {
        alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
    })
    .expect("valid JSON should build a tape");

    unsafe {
        let cached = crate::string::js_string_from_bytes(b"cached".as_ptr(), 6);
        *(*lazy).materialized_elements.add(1) =
            JSValue::string_ptr(cached as *mut crate::StringHeader);
        *(*lazy).materialized_bitmap |= 1u64 << 1;
    }

    let before = reparse_materializations();
    let arr = unsafe { force_materialize_lazy(lazy) };
    assert_eq!(
        reparse_materializations(),
        before + 1,
        "1-of-3 cached is below the crossover, so this must reparse"
    );

    // The reparse produces a RawF64-layout array for `[1,2,3]`; patching
    // a STRING into slot 1 has to downgrade it, or the tracer would skip
    // a live pointer in an array flagged pointer-free.
    assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 3),
        Some(1)
    );
}

/// #7478 crossover. Once MOST elements are already in the sparse cache
/// the element-wise merge is the cheap producer — it copies the cached
/// JSValues and materializes only the remainder — so a reparse would
/// rebuild subtrees it is about to throw away. This pins the decision,
/// not just the values: without the counter assertion the test passes
/// either way and the crossover could silently invert.
#[test]
fn force_materialize_majority_cached_uses_the_merge_walk_not_a_reparse() {
    let input = br#"[10,20,30,40]"#;
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    let lazy = with_built_tape(input, |tape| unsafe {
        alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
    })
    .expect("valid JSON should build a tape");

    unsafe {
        *(*lazy).materialized_elements.add(0) = JSValue::number(1.5);
        *(*lazy).materialized_elements.add(1) = JSValue::number(2.5);
        *(*lazy).materialized_bitmap |= 0b11;
    }

    let before = reparse_materializations();
    let arr = unsafe { force_materialize_lazy(lazy) };
    assert_eq!(
        reparse_materializations(),
        before,
        "2-of-4 cached is at the crossover — the walk, not a reparse"
    );

    assert_eq!(
        crate::array::js_array_get(arr, 0).bits(),
        JSValue::number(1.5).bits()
    );
    assert_eq!(
        crate::array::js_array_get(arr, 1).bits(),
        JSValue::number(2.5).bits()
    );
    assert_eq!(
        crate::array::js_array_get(arr, 2).bits(),
        JSValue::number(30.0).bits()
    );
    assert_eq!(
        crate::array::js_array_get(arr, 3).bits(),
        JSValue::number(40.0).bits()
    );
}

/// Build a lazy array over `[{"x":0},{"x":1},…]` with `n` records.
/// Returns the header plus the retained blob text so the caller can
/// keep both alive for the duration of a test.
unsafe fn lazy_object_array(n: usize) -> *mut LazyArrayHeader {
    let mut json = String::from("[");
    for i in 0..n {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!("{{\"x\":{i}}}"));
    }
    json.push(']');
    let input = json.as_bytes();
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    with_built_tape(input, |tape| {
        alloc_lazy_array(tape, 0, count_array_length(tape, 0), text)
    })
    .expect("valid JSON should build a tape")
}

/// #7478 core acceptance. A sequential scan must hand off to the batch
/// producer PART WAY THROUGH, not at the end.
///
/// This asserts the subject ran, not merely that the values are right:
/// `reparse_materializations()` distinguishes the batch parser from the
/// element-wise merge walk, and the two produce identical values. Without
/// that counter the test would pass just as happily against the old
/// never-flips behavior.
#[test]
fn a_sequential_scan_flips_to_the_batch_parser_part_way_through() {
    let n = 200usize;
    let lazy = unsafe { lazy_object_array(n) };
    let threshold = scan_flip_threshold(n as u32) as usize;
    assert_eq!(threshold, 64, "n=200 sits on the floor arm of the rule");

    let before = reparse_materializations();
    let mut flipped_at = None;
    for i in 0..n {
        unsafe { lazy_get(lazy, i as u32) };
        if flipped_at.is_none() && unsafe { !(*lazy).materialized.is_null() } {
            flipped_at = Some(i);
        }
    }
    let flipped_at = flipped_at.expect("a full sequential scan must flip");
    assert_eq!(
        flipped_at,
        threshold - 1,
        "the flip must land on the read that completes the streak"
    );
    assert!(
        flipped_at < n / 2,
        "flipping must happen while most of the array is still unbuilt"
    );
    assert_eq!(
        reparse_materializations(),
        before + 1,
        "the flip must reach #7499's BATCH reparse — an element-wise \
         merge walk here would leave this counter untouched"
    );
}

/// An array too small for the streak to complete before the scan is nearly
/// over must not flip at all: the merge walk is already holding the tree by
/// then, so a reparse would rebuild what it is about to return. This pins
/// the "would the callee even pick the batch producer?" half of the trigger.
#[test]
fn an_array_whose_streak_can_only_complete_late_does_not_flip() {
    // n = 100: threshold is the floor (64), and 64 reads in the streak is
    // already past the half-way point, so the batch producer is not eligible.
    let n = 100usize;
    let lazy = unsafe { lazy_object_array(n) };
    assert_eq!(scan_flip_threshold(n as u32), 64);
    let before = reparse_materializations();
    for i in 0..n {
        unsafe { lazy_get(lazy, i as u32) };
    }
    assert!(
        unsafe { (*lazy).materialized.is_null() },
        "a late-completing streak must not trigger a redundant batch parse"
    );
    assert_eq!(reparse_materializations(), before);
}

/// The floor of `scan_flip_threshold` is what keeps a glance at the first
/// few records from dragging in a parse of the whole document. Without it
/// the proportional rule would trip at 1/64th of the array, which on a
/// 10k-record blob is 156 reads — but on a small one is 2.
#[test]
fn a_short_prefix_read_does_not_flip() {
    let n = 200usize;
    let lazy = unsafe { lazy_object_array(n) };
    let before = reparse_materializations();
    for i in 0..10 {
        unsafe { lazy_get(lazy, i) };
    }
    assert!(
        unsafe { (*lazy).materialized.is_null() },
        "10 reads out of 200 must stay lazy"
    );
    assert_eq!(
        reparse_materializations(),
        before,
        "no batch parse may be triggered by a short prefix read"
    );
}

/// The streak counts CONSECUTIVE ascending reads. A strided walk is not a
/// scan, and must not trip the new rule — it is left to the pre-existing
/// `cumulative_walk_steps` rule, which this access pattern also stays
/// under (61 reads × 2 steps = 122, against a 2n threshold of 400).
#[test]
fn a_strided_walk_does_not_trip_the_scan_flip() {
    let n = 200usize;
    let lazy = unsafe { lazy_object_array(n) };
    let before = reparse_materializations();
    let mut reads = 0;
    for i in (0..122).step_by(2) {
        unsafe { lazy_get(lazy, i) };
        reads += 1;
    }
    assert_eq!(reads, 61);
    assert_eq!(
        unsafe { (*lazy).sequential_streak },
        1,
        "a stride of 2 must never EXTEND a run — each read starts its own run \
         of length one, and 61 of them must not add up to a streak"
    );
    assert!(
        unsafe { (*lazy).materialized.is_null() },
        "a strided walk must not flip"
    );
    assert_eq!(reparse_materializations(), before);
}

/// `parsed[i] === parsed[i]` has to survive the flip. The elements handed
/// out before it came from the tape walk and live in the sparse cache;
/// the reparse builds fresh ones and then patches the cached values back
/// over its slots. If that patch loop were dropped, this test sees two
/// different pointers for the same index.
#[test]
fn element_identity_survives_the_scan_flip() {
    let n = 200usize;
    let lazy = unsafe { lazy_object_array(n) };
    // Read index 5 before the flip and remember the exact JSValue. It has to
    // be ROOTED, not just copied into a local: the scan below allocates 200
    // records and then a whole reparsed tree, and a raw copy of a pointer
    // JSValue held across that is exactly the stale-local shape the identity
    // assertion is supposed to be testing for. Unrooted, a moving collection
    // would fail this test for the wrong reason.
    let scope = crate::gc::RuntimeHandleScope::new();
    let early = scope.root_nanbox_u64(unsafe { lazy_get(lazy, 5) }.bits());
    assert!(
        unsafe { (*lazy).materialized.is_null() },
        "one read must not flip"
    );
    for i in 0..n {
        unsafe { lazy_get(lazy, i as u32) };
    }
    assert!(
        unsafe { !(*lazy).materialized.is_null() },
        "the scan must have flipped"
    );
    let late = unsafe { lazy_get(lazy, 5) };
    assert_eq!(
        early.get_nanbox_u64(),
        late.bits(),
        "the pre-flip element must stay identical across the flip"
    );
    // And the batch-produced elements must carry the right values.
    let arr = unsafe { (*lazy).materialized };
    assert_eq!(unsafe { (*arr).length }, n as u32);
}

/// The flip's eligibility test must count what is ACTUALLY in the sparse
/// cache, not assume the cache is the scanned prefix. With a read outside the
/// prefix already cached, approximating the count as `i + 1` undercounts, and
/// the trigger can fire on an array `force_materialize_lazy` then declines to
/// reparse — which does not merely waste the trigger, it materializes the
/// whole array early through the element-wise merge walk, the exact path the
/// flip exists to avoid.
///
/// 131 elements with index 130 read first: the streak completes at index 63,
/// where the true cache count is 65 (0..=63 plus 130). `65 * 2 = 130 < 131`
/// still admits the reparse by one, so the flip is correct here — and the
/// test pins that it is decided on 65 and not on the 64 the old arithmetic
/// would have used.
#[test]
fn the_flip_counts_the_whole_cache_not_just_the_scanned_prefix() {
    let n = 131usize;
    let lazy = unsafe { lazy_object_array(n) };
    assert_eq!(scan_flip_threshold(n as u32), 64);

    // A cold read well outside the prefix we are about to scan.
    unsafe { lazy_get(lazy, 130) };
    assert_eq!(
        unsafe { (*lazy).sequential_streak },
        1,
        "a lone cold read is a run of length one"
    );

    let before = reparse_materializations();
    for i in 0..64u32 {
        unsafe { lazy_get(lazy, i) };
    }
    assert!(
        unsafe { !(*lazy).materialized.is_null() },
        "the streak completes at index 63 and the cache is still under half"
    );
    assert_eq!(
        reparse_materializations(),
        before + 1,
        "the flip must have reached the batch reparse, not the merge walk"
    );
    // Every element still has to read back correctly, including the one that
    // was cached before the flip and patched back over the reparsed slot.
    let arr = unsafe { (*lazy).materialized };
    assert_eq!(unsafe { (*arr).length }, n as u32);
}

/// A lazy header whose tape root is not the blob's first value cannot
/// have its blob re-parsed (the blob is not that array's source), so the
/// reparse must decline and the tape walk must still produce the array.
#[test]
fn force_materialize_declines_reparse_when_the_tape_root_is_not_the_blob_root() {
    let input = br#"[[7,8],[9]]"#;
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    // root_idx 1 = the inner `[7,8]`, whose source is NOT the whole blob.
    let lazy = with_built_tape(input, |tape| unsafe {
        alloc_lazy_array(tape, 1, count_array_length(tape, 1), text)
    })
    .expect("valid JSON should build a tape");

    let before = reparse_materializations();
    let arr = unsafe { force_materialize_lazy(lazy) };
    assert_eq!(
        reparse_materializations(),
        before,
        "a non-root tape index must decline the reparse"
    );
    assert_eq!(unsafe { (*arr).length }, 2);
    assert_eq!(
        crate::array::js_array_get(arr, 0).bits(),
        JSValue::number(7.0).bits()
    );
    assert_eq!(
        crate::array::js_array_get(arr, 1).bits(),
        JSValue::number(8.0).bits()
    );
}
