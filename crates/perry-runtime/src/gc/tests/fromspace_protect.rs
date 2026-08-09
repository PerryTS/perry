//! Teeth for the #7154 detection-latency instruments: from-space quarantine
//! (`PERRY_GC_PROTECT_FROMSPACE`) and GC zeal (`PERRY_GC_ZEAL`).
//!
//! Every test asserts BOTH directions of its knob. The GC knob kill-policy in
//! CLAUDE.md requires an exercised OFF state for every knob, and the reason is
//! recorded right there: `PERRY_GC_FORCE_EVACUATE` was inert for every
//! `gc()`-driven test for months and nobody noticed, because only the ON arm was
//! ever asserted and the ON arm did nothing.
//!
//! These are *debug instruments*, so the correctness bar is higher than usual,
//! not lower: an instrument that reports clean when the heap is dirty is worse
//! than no instrument, and one that changes the collector when it is switched
//! off is a landmine in every future bisect.

use super::super::*;
use super::support::*;
use crate::arena::FromSpaceProtection;

// ---------------------------------------------------------------------------
// Knob parsing — pure, so both states are asserted without touching the
// process environment (the live readers cache in a `OnceLock`, so a test that
// set an env var would be at the mercy of which test ran first).
// ---------------------------------------------------------------------------

#[test]
fn protection_knob_parses_off_poison_and_protect() {
    use crate::arena::parse_protection_mode;
    // OFF is the default and every unrecognised spelling: a typo must not
    // silently enable an instrument that detaches arena blocks.
    for raw in [
        None,
        Some("0"),
        Some("off"),
        Some("false"),
        Some("yes"),
        Some(""),
    ] {
        assert_eq!(
            parse_protection_mode(raw),
            FromSpaceProtection::Off,
            "{raw:?} must leave from-space protection OFF"
        );
    }
    for raw in ["1", "on", "true"] {
        assert_eq!(
            parse_protection_mode(Some(raw)),
            FromSpaceProtection::ProtectPages,
            "{raw} must select mprotect + poison"
        );
    }
    assert_eq!(
        parse_protection_mode(Some("poison")),
        FromSpaceProtection::PoisonOnly,
        "`poison` must select the no-mprotect fallback"
    );
}

#[test]
fn quarantine_depth_rejects_zero_and_garbage() {
    use crate::arena::parse_quarantine_depth;
    // A depth of 0 would evict each set on the cycle it was created — the
    // instrument would read as ON and protect nothing.
    assert_eq!(parse_quarantine_depth(Some("0")), 1);
    assert_eq!(parse_quarantine_depth(Some("1")), 1);
    assert_eq!(parse_quarantine_depth(Some("16")), 16);
    assert_eq!(parse_quarantine_depth(Some("banana")), 4);
    assert_eq!(parse_quarantine_depth(None), 4);
}

#[test]
fn zeal_knob_parses_both_states() {
    use super::super::zeal::parse_zeal;
    for raw in [None, Some("0"), Some("off"), Some("false"), Some("2")] {
        assert!(!parse_zeal(raw), "{raw:?} must leave zeal OFF");
    }
    for raw in ["1", "on", "true"] {
        assert!(parse_zeal(Some(raw)), "{raw} must enable zeal");
    }
}

/// The gap this closes: `PERRY_GC_FROMSPACE_SCAN_ABORT=1` used to be completely
/// inert on its own — `run_fromspace_scan` returned at the
/// `fromspace_scan_enabled()` gate, so there was never anything to abort and the
/// run reported success. An investigator reaching for the abort switch mid-hunt
/// got a green run and no scan.
#[test]
fn fromspace_scan_abort_implies_the_scan_runs() {
    use super::super::fromspace_scan::resolve_scan_knobs;
    assert_eq!(resolve_scan_knobs(None, None), (false, false));
    assert_eq!(resolve_scan_knobs(Some("1"), None), (true, false));
    assert_eq!(
        resolve_scan_knobs(None, Some("1")),
        (true, true),
        "ABORT alone must turn the scan ON, not silently do nothing"
    );
    assert_eq!(resolve_scan_knobs(Some("1"), Some("1")), (true, true));
    assert_eq!(resolve_scan_knobs(Some("0"), Some("0")), (false, false));
}

// ---------------------------------------------------------------------------
// From-space quarantine, driven through a real copying minor.
// ---------------------------------------------------------------------------

/// The OFF arm. With the knob unset the copying minor must take the ordinary
/// reset path, retire nothing, and leave the quarantine untouched — a normal
/// build pays exactly nothing for this instrument existing.
#[test]
fn protection_off_retires_no_from_space() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let before = crate::arena::quarantine_stats();

    let live = young_leaf();
    js_shadow_slot_set(0, string_bits(live));
    let _ = gc_collect_minor();

    let after = crate::arena::quarantine_stats();
    assert_eq!(
        after.sets_retired, before.sets_retired,
        "with PERRY_GC_PROTECT_FROMSPACE off, a copying minor must recycle \
         from-space exactly as it always has"
    );
    assert_eq!(after.blocks_quarantined, before.blocks_quarantined);
}

/// The ON arm, in `poison` mode so the assertions can read the retired bytes
/// instead of faulting on them.
///
/// Three things are asserted together, because any one alone can pass
/// vacuously: (1) the subject was live — an object actually MOVED, so this was
/// a real evacuating minor; (2) the from-space bytes it moved out of are now
/// poison rather than recyclable; (3) the instrument says so in its counters.
#[test]
fn protection_poisons_the_from_space_an_object_moved_out_of() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);
    let before = crate::arena::quarantine_stats();

    let from_space_addr = young_leaf();
    js_shadow_slot_set(0, string_bits(from_space_addr));
    let _ = gc_collect_minor();
    let to_space_addr = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    // (1) subject-was-live: the minor evacuated, so `from_space_addr` really is
    // a retired address and not simply the object's current home.
    assert_ne!(
        to_space_addr, from_space_addr,
        "test premise: the copying minor must have MOVED the rooted object"
    );

    // (2) the retired bytes are poison, and present a header no dispatch can
    // mistake for a live object.
    let poison_word = unsafe { *(from_space_addr as *const u64) };
    assert_eq!(
        poison_word,
        crate::arena::QUARANTINE_POISON_WORD,
        "the retired from-space payload must read as poison, not as a \
         freshly-recycled object"
    );
    let header_obj_type = unsafe { *((from_space_addr - GC_HEADER_SIZE) as *const u8) };
    assert_eq!(
        header_obj_type,
        crate::arena::QUARANTINE_POISON_OBJ_TYPE,
        "the retired header must present the invalid-object sentinel"
    );

    // (3) the counters agree, so a future run can tell a protected cycle from
    // one where the instrument never engaged.
    let after = crate::arena::quarantine_stats();
    assert_eq!(
        after.sets_retired,
        before.sets_retired + 1,
        "exactly one from-space page-set must have been retired"
    );
    assert!(
        after.blocks_quarantined > before.blocks_quarantined,
        "at least one block must have been quarantined"
    );
    assert!(
        after.bytes_poisoned > before.bytes_poisoned,
        "poison-only mode must count poisoned bytes"
    );
    assert_eq!(
        after.bytes_protected, before.bytes_protected,
        "poison-only mode must NOT claim mprotected bytes"
    );

    // The survivor is untouched by any of this.
    assert_eq!(
        crate::arena::classify_heap_space(to_space_addr),
        crate::arena::active_survivor_space(),
        "the evacuated copy must still be a normal, readable survivor"
    );
}

/// The memory bound. Long runs must not OOM: the quarantine is a ring, and
/// evicted blocks go back into Eden rather than being freed (nothing that was
/// ever `mprotect`ed is handed to `dealloc`).
#[test]
fn quarantine_is_bounded_and_recycles_expired_blocks() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);
    let depth = crate::arena::quarantine_depth();
    let before = crate::arena::quarantine_stats();

    for _ in 0..(depth + 3) {
        let live = young_leaf();
        js_shadow_slot_set(0, string_bits(live));
        let _ = gc_collect_minor();
    }

    let after = crate::arena::quarantine_stats();
    assert!(
        after.sets_retired >= before.sets_retired + depth as u64,
        "test premise: enough minors must have run to overflow the ring \
         (before={}, after={}, depth={depth})",
        before.sets_retired,
        after.sets_retired
    );
    assert!(
        after.sets_held <= depth,
        "the quarantine must never hold more than PERRY_GC_PROTECT_FROMSPACE_DEPTH \
         page-sets (held={}, depth={depth})",
        after.sets_held
    );
    assert!(
        after.blocks_recycled > before.blocks_recycled,
        "expired sets must be recycled back into Eden, not leaked \
         (before={}, after={})",
        before.blocks_recycled,
        after.blocks_recycled
    );
}

/// **The sabotage test.** Everything above asserts the instrument *ran*; this
/// asserts it *detects the thing it was built for*, by planting the bug rather
/// than waiting for one.
///
/// The plant is the #7184 / #7192 shape reduced to its essence: a mutator keeps
/// a pre-collection address across an evacuating minor (there, a caller's
/// register or an out-of-frame shadow slot; here, a local `usize`), and then
/// dereferences it. Both arms run the identical plant, and the OFF arm is what
/// makes the ON arm mean anything:
///
/// - OFF — the from-space bytes are recycled, so the stale address reads as a
///   **valid, live, unrelated object**. The deref silently succeeds. That is
///   precisely why a #7154-class bug takes ten rounds to find, and it is the
///   red control: the bug is genuinely invisible without the instrument.
/// - ON — the same address reads as poison carrying an `obj_type` no dispatch
///   can accept. The stale use is caught at the use.
///
/// A regression that made the quarantine miss the retired range would flip the
/// ON arm to look like the OFF arm, and this test would fail. A regression that
/// made it protect bytes that were never retired would break the OFF arm.
#[test]
fn quarantine_catches_a_planted_stale_from_space_deref() {
    // --- red control: WITHOUT the instrument, the stale deref succeeds ------
    let recycled_is_live_object = {
        let _guard = CopyingNurseryTestGuard::new(1);
        let stale = young_leaf();
        js_shadow_slot_set(0, string_bits(stale));
        let _ = gc_collect_minor();
        let moved_to = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(
            moved_to, stale,
            "test premise: the minor must have MOVED the object, so `stale` is \
             genuinely a retired from-space address"
        );

        // The mutator keeps allocating, as it would after the bad collection.
        // The bump allocator hands the retired bytes straight back out.
        for _ in 0..8 {
            let _ = young_leaf();
        }

        // Deref the stale address. Without the instrument this reads whatever
        // now lives there — well-formed memory, not poison.
        let word = unsafe { *(stale as *const u64) };
        assert_ne!(
            word,
            crate::arena::QUARANTINE_POISON_WORD,
            "the OFF arm must NOT poison — otherwise the ON arm proves nothing"
        );
        word
    };

    // --- the instrument: the same plant is caught ---------------------------
    {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);
        let before = crate::arena::quarantine_stats();

        let stale = young_leaf();
        js_shadow_slot_set(0, string_bits(stale));
        let _ = gc_collect_minor();
        let moved_to = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(moved_to, stale, "test premise: the object must have MOVED");

        // Same mutator pressure as the red control — the retired bytes must
        // stay quarantined rather than being handed back out.
        for _ in 0..8 {
            let _ = young_leaf();
        }

        // Live-subject check before believing the verdict (CLAUDE.md, "a gate
        // must assert its subject was live").
        let after = crate::arena::quarantine_stats();
        assert_eq!(
            after.sets_retired,
            before.sets_retired + 1,
            "the instrument must actually have retired this minor's from-space"
        );

        let word = unsafe { *(stale as *const u64) };
        assert_eq!(
            word,
            crate::arena::QUARANTINE_POISON_WORD,
            "the planted stale deref must land on poison, not on recycled bytes"
        );
        let obj_type = unsafe { *((stale - GC_HEADER_SIZE) as *const u8) };
        assert_eq!(
            obj_type,
            crate::arena::QUARANTINE_POISON_OBJ_TYPE,
            "the retired header must present an obj_type no dispatch accepts"
        );
        assert_ne!(
            word, recycled_is_live_object,
            "the two arms must genuinely differ — if they agree, one of them is \
             not exercising what it claims"
        );
    }
}

// ---------------------------------------------------------------------------
// Zeal
// ---------------------------------------------------------------------------

/// Zeal's whole contract: collect at a safepoint where nothing is due. Both
/// arms, because the OFF arm is what proves the safepoint was genuinely idle —
/// without it, a passing ON arm could just be ordinary heap pressure.
#[test]
fn zeal_collects_at_a_safepoint_with_no_pressure_due() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_scan_fallback_counters();

    // OFF: an idle safepoint must collect nothing.
    {
        let _zeal = super::super::zeal::ZealGuard::set(false);
        gc_safepoint_moving_minor();
    }
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::NurseryMinor),
        0,
        "test premise: with no trigger due and zeal off, the safepoint must be idle"
    );

    // ON: the same idle safepoint must now run a minor.
    let forced_before = zeal_forced_collections();
    {
        let _zeal = super::super::zeal::ZealGuard::set(true);
        gc_safepoint_moving_minor();
    }
    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::NurseryMinor),
        1,
        "PERRY_GC_ZEAL=1 must force a minor at every safepoint"
    );
    assert!(
        zeal_forced_collections() > forced_before,
        "the forced collection must be COUNTED — a zeal run reporting 0 forced \
         collections exercised nothing, and a clean verdict from it is vacuous"
    );
}

// ---------------------------------------------------------------------------
// Zeal pacing (#7726). Zeal used to force a collection at EVERY back-edge poll.
// That was affordable only while the polls themselves were a compile-time
// opt-in nobody took; #7721 made them default-ON and the same instrument became
// ~511 us of fixed collection cost per loop iteration — 24 minutes for a 19 s
// program, which is an instrument nobody switches on.
//
// Both directions are asserted, per the kill-policy: the stride must BOUND the
// forced collections, and `=0` must still give the literal every-poll mode.
// ---------------------------------------------------------------------------

#[test]
fn zeal_alloc_stride_knob_parses_both_states() {
    use super::super::zeal::parse_zeal_alloc_kb;
    // Default when unset or unparseable — a typo must not silently select the
    // unusable every-poll mode.
    assert_eq!(parse_zeal_alloc_kb(None), 4096);
    assert_eq!(parse_zeal_alloc_kb(Some("banana")), 4096);
    assert_eq!(parse_zeal_alloc_kb(Some("")), 4096);
    // 0 is MEANINGFUL, not garbage: it restores pre-#7726 every-poll zeal.
    assert_eq!(parse_zeal_alloc_kb(Some("0")), 0);
    assert_eq!(parse_zeal_alloc_kb(Some("16")), 16 * 1024);
    assert_eq!(parse_zeal_alloc_kb(Some(" 64 ")), 64 * 1024);
}

/// ★ The regression test for #7726, and the one that would have caught it.
///
/// Drives a hot poll loop — the shape of every real workload under zeal — and
/// asserts the forced collections are BOUNDED well below the poll count. Before
/// the fix this ratio was exactly 1.0 (70,968 forced collections for 70,963
/// polls on the measured workload), so this assertion fails on the old code.
///
/// It is paired with two liveness assertions, because "fast" is trivially
/// achievable by collecting nothing and that would be a worse regression than
/// the one being fixed (CLAUDE.md, four ways a gate cannot fail — #4): the run
/// must still force collections, and those collections must still MOVE objects.
#[test]
fn zeal_pacing_bounds_forced_collections_but_still_moves_objects() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _polls = super::super::policy::MovingLoopPollsGuard::on();
    let _zeal = super::super::zeal::ZealGuard::set(true);
    let _stride = super::super::zeal::ZealStrideGuard::set(4096);
    super::super::zeal::reset_zeal_pacing_for_test();

    const POLLS: u64 = 2_000;
    let forced_before = zeal_forced_collections();
    let moved_before = moved_objects_total();
    let paced_before = zeal_polls_paced();

    for _ in 0..POLLS {
        // Allocate, root it, then poll — a loop body that produces new nursery
        // material every iteration, which is what makes the unpaced instrument
        // collect every time.
        let leaf = young_leaf();
        js_shadow_slot_set(0, string_bits(leaf));
        js_gc_loop_safepoint();
    }

    let forced = zeal_forced_collections() - forced_before;
    let moved = moved_objects_total() - moved_before;
    let paced = zeal_polls_paced() - paced_before;

    // THE BOUND. Each `young_leaf` is a few tens of bytes, so 2000 of them is
    // well under 200 KB; at a 4 KB stride that is a few dozen collections, not
    // 2000. A generous ceiling keeps this from being an allocator-size test
    // while still failing loudly on the pre-fix 1:1 behaviour.
    assert!(
        forced < POLLS / 4,
        "zeal must PACE its forced collections: {forced} forced for {POLLS} polls \
         (pre-#7726 this was 1:1, which cost 24 minutes on a 19 s program)"
    );
    assert_eq!(
        forced + paced,
        POLLS,
        "every poll must be accounted for as either forced or paced \
         (forced={forced} paced={paced})"
    );

    // LIVENESS 1: pacing must not have turned zeal off. A run that forces zero
    // collections is the vacuous-green shape, not a fix.
    assert!(
        forced > 0,
        "zeal must still force collections — a paced instrument that never \
         collects is a worse regression than the slow one it replaced"
    );
    // LIVENESS 2: and those collections must still RELOCATE. Zeal exists to
    // make an unrooted value move on its first exposure; a paced minor that
    // leaves survivors in place would surface nothing.
    assert!(
        moved > 0,
        "zeal's paced collections must still MOVE survivors (moved={moved})"
    );
}

/// The OFF state of the pacing knob, per the kill-policy: `=0` must restore the
/// literal every-poll semantics, which is the right setting for a small fixture
/// (`gc_instrument_smoke.sh` pins it) or a window executed exactly once.
#[test]
fn zeal_alloc_stride_zero_restores_every_poll_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _polls = super::super::policy::MovingLoopPollsGuard::on();
    let _zeal = super::super::zeal::ZealGuard::set(true);
    let _stride = super::super::zeal::ZealStrideGuard::set(0);
    super::super::zeal::reset_zeal_pacing_for_test();

    const POLLS: u64 = 32;
    let forced_before = zeal_forced_collections();
    for _ in 0..POLLS {
        let leaf = young_leaf();
        js_shadow_slot_set(0, string_bits(leaf));
        js_gc_loop_safepoint();
    }
    assert_eq!(
        zeal_forced_collections() - forced_before,
        POLLS,
        "PERRY_GC_ZEAL_ALLOC_KB=0 must collect at EVERY poll — that escape \
         hatch is what a once-executed bug window needs"
    );
}

/// The pacing is a monotone high-water mark, not a "bytes since last time"
/// delta, and the difference is the whole bound. If a forced collection
/// reclaims nothing — an escalation to a non-moving full mark-sweep, which
/// #7592 and #7682 both produced in the field — a delta-based pacer would find
/// the threshold still met and collect again at the very next poll, restoring
/// the livelock it was meant to remove. Rearming from the level measured AFTER
/// the collection makes the next one cost a full stride of genuinely new
/// allocation no matter what the collector managed to free.
#[test]
fn zeal_pacing_rearms_above_survivors_so_a_useless_collection_cannot_loop() {
    use super::super::zeal::{
        note_zeal_poll_collection, reset_zeal_pacing_for_test, zeal_poll_collection_due,
        ZealStrideGuard,
    };
    let _stride = ZealStrideGuard::set(4096);
    reset_zeal_pacing_for_test();

    // A collection that freed NOTHING: from-space still holds 1 MB afterwards.
    note_zeal_poll_collection(1024 * 1024);
    assert!(
        !zeal_poll_collection_due(1024 * 1024),
        "a collection that reclaimed nothing must NOT be immediately due again \
         — that is the #7592 livelock shape"
    );
    assert!(
        !zeal_poll_collection_due(1024 * 1024 + 4095),
        "still short of one full stride of new allocation"
    );
    assert!(
        zeal_poll_collection_due(1024 * 1024 + 4096),
        "one full stride of NEW material above the survivors makes it due again"
    );
}

/// A zealous minor that leaves survivors in place would move nothing, so it
/// could not surface a stale-pointer bug at all. Zeal therefore implies forced
/// evacuation, UNCONDITIONALLY.
///
/// ★ #7611 deleted the `PERRY_GEN_GC_EVACUATE=0` veto this test used to have a
/// second arm for. That veto was the one way an ambient environment variable
/// could silently turn zeal into a no-op: this very test used to take the
/// precedence arm and `return` without exercising zeal at all, which is the
/// vacuous-green shape the kill-policy exists to catch. There is now no
/// environment in which zeal does not force evacuation, so there is one arm and
/// it always runs.
#[test]
fn zeal_implies_forced_evacuation() {
    let _zeal_off = super::super::zeal::ZealGuard::set(false);
    let off = gc_force_evacuate_enabled();
    let _zeal_on = super::super::zeal::ZealGuard::set(true);
    assert!(
        gc_force_evacuate_enabled(),
        "zeal must force evacuation in every environment (force_off={off})"
    );
}

/// Zeal and protection are designed to compose — that pairing is what turns a
/// #7154 bug into an immediate fault instead of a cycle-late `TypeError`. This
/// asserts they actually run together rather than one disabling the other.
#[test]
fn zeal_and_protection_compose() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _mode = crate::arena::ProtectionModeGuard::set(FromSpaceProtection::PoisonOnly);
    let _zeal = super::super::zeal::ZealGuard::set(true);
    reset_scan_fallback_counters();
    let before = crate::arena::quarantine_stats();

    let from_space_addr = young_leaf();
    js_shadow_slot_set(0, string_bits(from_space_addr));
    gc_safepoint_moving_minor();

    assert_eq!(
        safepoint_drain_count(SafepointDrainKind::NurseryMinor),
        1,
        "zeal must have forced the minor"
    );
    let after = crate::arena::quarantine_stats();
    assert_eq!(
        after.sets_retired,
        before.sets_retired + 1,
        "the zeal-forced minor's from-space must have been quarantined"
    );
    assert_eq!(
        unsafe { *(from_space_addr as *const u64) },
        crate::arena::QUARANTINE_POISON_WORD,
        "zeal + protection: the address the value moved out of must be poison \
         immediately, not on some later cycle"
    );
}
