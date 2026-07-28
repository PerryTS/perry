//! #6910 — the mutable-root MARK path and the mutable-root REWRITE path must
//! accept exactly the same word forms.
//!
//! Before the fix, shadow-stack slots were marked by `try_mark_value` (NaN-box
//! tags only) while `rewrite_mutable_root_slots` relocated them through
//! `try_rewrite_value` (NaN-boxed **or** bare address). A bare address in a
//! shadow slot was therefore a root to the rewrite, copying and
//! incremental-barrier paths but invisible to mark-sweep root marking — if it
//! was the only reference, the object was swept while live.
//!
//! Every test here pins the conservative native-stack scan to `Disabled`. The
//! unit-test build otherwise defaults to `Full`, whose scan accepts bare
//! addresses off the native stack and would rescue the object — which is
//! precisely why this class of bug ships green tests and a production-only
//! use-after-free (production resolves `Auto -> SkipDisabled`).

use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

/// Every bit pattern a mutable root slot can plausibly hold, over a single
/// target address. `expect_reference` is what BOTH the mark path and the
/// rewrite path must decide.
fn root_word_probes(addr: usize) -> Vec<(&'static str, u64, bool)> {
    vec![
        ("nan-boxed pointer", POINTER_TAG | addr as u64, true),
        ("nan-boxed string", STRING_TAG | addr as u64, true),
        ("nan-boxed bigint", BIGINT_TAG | addr as u64, true),
        // #6910: the form the rewrite path always accepted and the mark path
        // used to drop. Unboxed pointer reps (RFC §5.6) store this.
        ("bare address", addr as u64, true),
        ("undefined", crate::value::TAG_UNDEFINED, false),
        ("null", crate::value::TAG_NULL, false),
        ("int32 42", crate::value::INT32_TAG | 42, false),
        ("double 3.5", 3.5f64.to_bits(), false),
        ("double -1.0", (-1.0f64).to_bits(), false),
        ("zero", 0, false),
    ]
}

/// Which mutable-root registry to stage the probe word in. Both kinds are
/// walked by the same mark and rewrite passes and must behave identically.
#[derive(Clone, Copy)]
enum ProbeSlot {
    Shadow,
    Global,
}

/// Stage `bits` in a slot of the given kind, run `walk`, and hand back what
/// the slot holds afterwards. Every mutable-root registry is left empty.
fn with_staged_slot<R>(kind: ProbeSlot, bits: u64, walk: impl FnOnce() -> R) -> (R, u64) {
    reset_shadow_stack();
    reset_global_roots();
    let out = match kind {
        ProbeSlot::Shadow => {
            let frame = js_shadow_frame_push(1);
            js_shadow_slot_set(0, bits);
            let out = walk();
            let after = js_shadow_slot_get(0);
            js_shadow_frame_pop(frame);
            (out, after)
        }
        ProbeSlot::Global => {
            let mut slot: u64 = bits;
            js_gc_register_global_root(&mut slot as *mut u64 as i64);
            let out = walk();
            (out, slot)
        }
    };
    reset_shadow_stack();
    reset_global_roots();
    out
}

/// Did the REAL root-marking walk mark `target` when the slot held `bits`?
fn root_walk_marks(
    kind: ProbeSlot,
    bits: u64,
    target: usize,
    valid_ptrs: &ValidPointerSet,
) -> bool {
    unsafe {
        let header = header_from_user_ptr(target as *const u8);
        (*header).gc_flags &= !GC_FLAG_MARKED;
        let (_, _) = with_staged_slot(kind, bits, || {
            mark_mutable_root_slots(valid_ptrs, None, None);
        });
        let marked = (*header).gc_flags & GC_FLAG_MARKED != 0;
        (*header).gc_flags &= !GC_FLAG_MARKED;
        clear_mark_seeds();
        marked
    }
}

/// Did the REAL rewrite walk relocate the slot when it held `bits`?
fn root_walk_rewrites(kind: ProbeSlot, bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let (_, after) = with_staged_slot(kind, bits, || {
        rewrite_mutable_root_slots(valid_ptrs, None);
    });
    (after != bits).then_some(after)
}

/// The core #6910 invariant, driven through the collector's own walkers
/// (`mark_mutable_root_slots` / `rewrite_mutable_root_slots`) rather than the
/// decoder, so a call site that stops using the shared decoder fails here too.
#[test]
fn mutable_root_mark_and_rewrite_accept_the_same_word_forms() {
    let _guard = GcTestIsolationGuard::new();
    let _scan = ConservativeScanDisabledGuard::new();
    clear_marks();
    clear_mark_seeds();

    // Two distinct targets: the mark probe must see an un-forwarded object,
    // while the rewrite probe needs a forwarded one.
    let mark_target = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let rewrite_target = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let valid_ptrs = build_valid_pointer_set();
    let moved = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(rewrite_target as *const u8),
            moved as *mut u8,
        );
    }

    for kind in [ProbeSlot::Shadow, ProbeSlot::Global] {
        let name = match kind {
            ProbeSlot::Shadow => "shadow-stack slot",
            ProbeSlot::Global => "global root slot",
        };
        // Same probe list over two addresses, walked in lockstep.
        let mark_probes = root_word_probes(mark_target);
        let rewrite_probes = root_word_probes(rewrite_target);
        assert_eq!(mark_probes.len(), rewrite_probes.len());

        for ((label, mark_bits, expected), (_, rewrite_bits, _)) in
            mark_probes.into_iter().zip(rewrite_probes)
        {
            let marked = root_walk_marks(kind, mark_bits, mark_target, &valid_ptrs);
            let rewritten = root_walk_rewrites(kind, rewrite_bits, &valid_ptrs);

            assert_eq!(
                marked, expected,
                "{name}: mark walk disagreed with the contract for {label} (0x{mark_bits:x})"
            );
            assert_eq!(
                rewritten.is_some(),
                expected,
                "{name}: rewrite walk disagreed with the contract for {label} (0x{rewrite_bits:x})"
            );
            // The load-bearing assertion: whatever the rewrite path is willing
            // to relocate, the mark path must be willing to mark. A word the
            // rewrite path moves but marking skips is swept while live.
            assert!(
                rewritten.is_none() || marked,
                "{name}/{label}: rewrite relocates this word form but mark ignores it (#6910)"
            );
        }

        // A rewrite must preserve the stored representation: a bare address
        // stays bare, a NaN box keeps its tag.
        assert_eq!(
            root_walk_rewrites(kind, rewrite_target as u64, &valid_ptrs),
            Some(moved as u64),
            "{name}: bare address must rewrite to a bare address"
        );
        assert_eq!(
            root_walk_rewrites(kind, STRING_TAG | rewrite_target as u64, &valid_ptrs),
            Some(STRING_TAG | moved as u64),
            "{name}: NaN-boxed string must rewrite to a NaN-boxed string"
        );
    }

    unsafe {
        let header = header_from_user_ptr(rewrite_target as *const u8);
        (*header).gc_flags &= !GC_FLAG_FORWARDED;
    }
    clear_marks();
    clear_mark_seeds();
}

/// The end-to-end regression: a nursery/malloc object whose ONLY reference is
/// a bare address in a shadow-stack slot must survive a real collection.
///
/// Pre-fix this failed — the sweep freed `live` (the dead churn objects prove
/// the sweep ran) and the shadow slot was left dangling.
#[test]
fn bare_address_in_shadow_slot_survives_a_real_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let dead_headers = allocate_dead_malloc_churn_headers(8);
    let live = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live);
    }
    // Bare (untagged) user address — no POINTER_TAG. This is what an unboxed
    // `Ptr<Shape>` / `Str` slot would hold, and what module-variable globals
    // have always held.
    js_shadow_slot_set(0, live as u64);
    assert!(
        malloc_user_ptr_tracked(live),
        "precondition: live is tracked"
    );

    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    assert_eq!(
        tracked_malloc_headers_matching(&dead_headers),
        0,
        "the sweep must actually have run for this test to mean anything"
    );
    assert!(
        malloc_user_ptr_tracked(live),
        "an object reachable only through a bare address in a shadow-stack \
         slot must be marked, not swept (#6910)"
    );
    unsafe {
        assert_eq!(
            (*(live as *mut crate::closure::ClosureHeader)).type_tag,
            crate::closure::CLOSURE_MAGIC,
            "surviving object must still be intact"
        );
    }
}

/// Same contract for the other mutable-root kind. Module-variable globals have
/// always stored bare addresses, so this is the regression guard for folding
/// the old `mark_global_root_bits` into the shared decoder.
#[test]
fn bare_address_in_global_root_survives_a_real_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _scan = ConservativeScanDisabledGuard::new();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let dead_headers = allocate_dead_malloc_churn_headers(8);
    let live = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live);
    }
    // A registered global root slot holding the bare address, exactly as a
    // module-variable global does.
    let mut global_slot: u64 = live as u64;
    js_gc_register_global_root(&mut global_slot as *mut u64 as i64);

    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    assert_eq!(
        tracked_malloc_headers_matching(&dead_headers),
        0,
        "the sweep must actually have run for this test to mean anything"
    );
    assert!(
        malloc_user_ptr_tracked(live),
        "a bare address in a registered global root must still be marked"
    );
    assert_eq!(
        global_slot, live as u64,
        "an unmoved target must leave the bare global slot untouched"
    );
}

/// The decoder is the single place that knows the word forms; assert the two
/// forms it distinguishes round-trip through `encode` without changing
/// representation.
#[test]
fn decode_root_word_round_trips_each_representation() {
    // Any plausible-looking, 8-aligned, in-range address works — the decoder
    // is pure bit inspection and never dereferences.
    let addr: usize = 0x3_1234_5678_9AB0 & POINTER_MASK as usize;

    assert_eq!(
        decode_root_word(POINTER_TAG | addr as u64),
        Some(RootWord::Nanboxed {
            addr,
            tag: POINTER_TAG
        })
    );
    assert_eq!(
        decode_root_word(STRING_TAG | addr as u64),
        Some(RootWord::Nanboxed {
            addr,
            tag: STRING_TAG
        })
    );
    assert_eq!(
        decode_root_word(BIGINT_TAG | addr as u64),
        Some(RootWord::Nanboxed {
            addr,
            tag: BIGINT_TAG
        })
    );
    assert_eq!(decode_root_word(addr as u64), Some(RootWord::Bare { addr }));

    // Nothing else decodes to a reference.
    assert_eq!(decode_root_word(0), None);
    assert_eq!(decode_root_word(crate::value::TAG_UNDEFINED), None);
    assert_eq!(decode_root_word(crate::value::INT32_TAG | 7), None);
    assert_eq!(decode_root_word(1.5f64.to_bits()), None);
    // Below the first page: a small integer, not an address.
    assert_eq!(decode_root_word(0x40), None);
    // A NaN-boxed null pointer carries no reference.
    assert_eq!(decode_root_word(POINTER_TAG), None);

    let moved: usize = 0x3_1111_2222_3330 & POINTER_MASK as usize;
    assert_eq!(
        decode_root_word(STRING_TAG | addr as u64)
            .expect("string tag decodes")
            .encode(moved),
        STRING_TAG | moved as u64,
        "rewriting a NaN box must preserve its tag"
    );
    assert_eq!(
        decode_root_word(addr as u64)
            .expect("bare address decodes")
            .encode(moved),
        moved as u64,
        "rewriting a bare address must not add a tag"
    );
}
