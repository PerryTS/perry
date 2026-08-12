//! #7900: the post-remark weak-to-strong READ race.
//!
//! A budgeted cycle performs its one-time `FinalRootRemark` and then keeps
//! opening mutator windows while weak processing (and, on the full path, the
//! sliced remembered-set rebuild) is still incomplete. `WeakRef.deref()` and
//! `WeakMap.get()` turn an unmarked weak target into a STRONG compiled-code
//! local through a **read** — a transition neither the incremental store
//! barrier nor allocate-black birth accounting observes, and one that no later
//! root scan can discover because the remark already ran. A subsequent weak
//! slice then tombstones the target and the sweep reclaims it while generated
//! code still holds the pointer.
//!
//! The fix is a weak-READ barrier (`crate::weakref::read_barrier`): every weak
//! read shades the value words it hands the mutator while an incremental mark
//! cycle is in flight, so the pending weak decision sees a marked target and
//! the pre-sweep drain traces its children.
//!
//! These tests are state-machine tests, not "does not throw" tests: each one
//! asserts the window it depends on actually opened (weak processing parked
//! mid-registry), that the read actually shaded something (a live subject —
//! see CLAUDE.md's "a gate must assert its subject was live"), and that the
//! target survives BOTH as a heap object and as the WeakRef's own answer.

use super::super::*;
use super::support::*;

fn trace_snapshot(kind: GcTriggerKind) -> GcTriggerSnapshot {
    GcTriggerSnapshot {
        kind,
        steps_before: Some(GcStepSnapshot::current()),
    }
}

fn run_cycle_in_single_unit_steps(state: &mut GcCycleState) {
    for _ in 0..200_000 {
        if state.phase() == GcCyclePhase::Complete {
            return;
        }
        state.step(GcWorkBudget::bounded(1));
    }
    panic!("GC cycle did not complete within step limit");
}

fn run_cycle_until_phase(state: &mut GcCycleState, target: GcCyclePhase) {
    for _ in 0..200_000 {
        if state.phase() == target {
            return;
        }
        state.step(GcWorkBudget::bounded(1));
    }
    panic!("GC cycle did not reach {target:?} within step limit");
}

/// A budgeted minor-fallback cycle: `low_pause_non_moving`, exactly as
/// `gc_start_budgeted_minor_fallback_cycle_with_snapshot` builds it (evacuation
/// refused, so `atomic_finalize_minor_prelude`'s non-moving assert holds).
fn start_budgeted_minor_fallback_state(trigger: GcTriggerSnapshot) -> GcCycleState {
    let prev_in_alloc = GC_FLAGS.with(|f| {
        let prev = f.get();
        f.set(prev | GC_FLAG_IN_ALLOC);
        prev & GC_FLAG_IN_ALLOC
    });
    let trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);
    let start = std::time::Instant::now();
    crate::arena::old_pages_begin_gc_cycle();
    clear_mark_seeds();
    GcCycleState::new_minor_fallback(
        trigger,
        trace,
        start,
        GcProgressKind::NormalIncremental,
        prev_in_alloc,
        gc_last_pause_us(),
        crate::process::get_rss_bytes(),
        /* evacuation_policy_allowed = */ false,
        /* force_evacuation = */ false,
        "low_pause_non_moving",
        OldPageDefragSelection::default(),
        crate::arena::OldArenaSourceBlockSelection::default(),
    )
}

/// Push the freshly-allocated holders/targets out of the block-persistence
/// window (the 5 most recent general blocks are conservatively force-marked),
/// so a weak-only target really is white when finalization decides.
fn age_out_of_block_persist_window() {
    let aged_from = crate::arena::general_block_count();
    while crate::arena::general_block_count().saturating_sub(aged_from) < 7 {
        for _ in 0..64 {
            let _ = crate::arena::arena_alloc_gc(4096, 8, GC_TYPE_STRING);
        }
    }
}

/// Allocate `count` WeakRefs over weak-only targets, rooting each WeakRef in
/// shadow slot `i`. Returns nothing: the refs are read back out of the shadow
/// stack so the test never holds a raw address across a collection.
fn seed_weak_refs(count: u32) {
    for slot in 0..count {
        let target = crate::object::js_object_alloc(0, 0);
        let weak_ref = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(target as usize)));
        js_shadow_slot_set(slot, ptr_bits(weak_ref as usize));
    }
}

/// A white object that no root reaches. Installed into a shadow slot only once
/// the one-shot root scan is over, so the ONLY thing that can mark it is
/// `FinalRootRemark` — which makes it a witness that the remark really ran,
/// rather than an assumption that `progress_kind.is_budgeted()` implies it.
fn alloc_remark_witness() -> usize {
    let (witness, _) = unsafe { alloc_nursery_test_object(1) };
    unsafe {
        let header = header_from_user_ptr(witness as *const u8);
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
    witness as usize
}

/// Drive `state` to the first mutator window that is parked INSIDE weak
/// processing, i.e. after `FinalRootRemark` and with holders still pending.
///
/// `witness` is installed into `witness_slot` at the AtomicFinalize boundary —
/// after RootScan and MarkPropagation are complete — and must be MARKED by the
/// time weak processing parks. `js_shadow_slot_set` performs no barrier, so a
/// marked witness proves a root scan ran after the store: the remark.
fn park_inside_weak_processing(
    state: &mut GcCycleState,
    holders: u32,
    witness: usize,
    witness_slot: u32,
) {
    run_cycle_until_phase(state, GcCyclePhase::AtomicFinalize);
    js_shadow_slot_set(witness_slot, ptr_bits(witness));
    let mut steps = 0usize;
    while state.atomic_finalize_subphase_for_tests() != Some("weak_processing") {
        state.step(GcWorkBudget::bounded(1));
        steps += 1;
        assert!(steps < 200_000, "weak processing was never reached");
    }
    let witness_flags = unsafe { (*header_from_user_ptr(witness as *const u8)).gc_flags };
    assert_ne!(
        witness_flags & GC_FLAG_MARKED,
        0,
        "SUBJECT-LIVE CHECK: a root installed after the one-shot root scan is \
         still white at weak processing, so FinalRootRemark did NOT run and this \
         test is not exercising the post-remark window"
    );
    assert_eq!(
        crate::weakref::test_support::full_weak_processing_work_units(),
        1,
        "the step that enters weak processing must consume exactly one holder"
    );
    assert_eq!(
        state.atomic_finalize_subphase_for_tests(),
        Some("weak_processing"),
        "the cycle must be PARKED mid-registry: {holders} holders, budget 1"
    );
    assert!(
        incremental_mark_barrier_active(),
        "the mark barrier must still be armed in a post-remark mutator window"
    );
}

/// The mutator's weak read. Returns `(shadow slot, target bits, target addr)`
/// for the first holder whose target is still pending (an already-processed
/// holder answers `undefined`, since every target here is weak-only).
fn mutator_weak_read(holders: u32) -> (u32, u64, usize) {
    for slot in 0..holders {
        let weak_ref = f64::from_bits(js_shadow_slot_get(slot));
        let bits = crate::weakref::js_weakref_deref(weak_ref).to_bits();
        if bits != crate::value::TAG_UNDEFINED {
            let addr = (bits & POINTER_MASK) as usize;
            return (slot, bits, addr);
        }
    }
    panic!("no holder was still pending in the window — the race was not set up");
}

/// #7900, full budgeted cycle (BarrierSeedDrain → FinalRootRemark →
/// RememberedSetRebuild → WeakProcessing): a target handed to the mutator by
/// `WeakRef.deref()` in a post-remark window must not be tombstoned or swept
/// by the slices that follow.
#[test]
fn weak_read_after_final_remark_survives_full_budgeted_cycle() {
    const HOLDERS: u32 = 8;
    let _guard = CopyingNurseryTestGuard::new(HOLDERS + 1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    crate::weakref::test_support::reset_weak_read_barrier_shades();

    seed_weak_refs(HOLDERS);
    let witness = alloc_remark_witness();
    age_out_of_block_persist_window();

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::ArenaBytes));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    park_inside_weak_processing(&mut state, HOLDERS, witness, HOLDERS);

    // ---- mutator window ----
    let (slot, bits, addr) = mutator_weak_read(HOLDERS);
    // Observability: a swept owner has its OVERFLOW_FIELDS entry cleared by the
    // dead-payload sweep arm, exactly how the production shape of this bug
    // (lost side-table fields) presented.
    crate::object::test_seed_overflow_fields_root(addr, 42f64.to_bits());
    assert!(
        crate::weakref::test_support::weak_read_barrier_shades() >= 1,
        "SUBJECT-LIVE CHECK: the weak read must have shaded an unmarked target. \
         Zero shades means the target was already marked and this run proves nothing"
    );
    // ---- collector resumes ----

    run_cycle_in_single_unit_steps(&mut state);
    let _ = state.take_outcome().expect("cycle should complete");

    assert!(
        crate::object::debug_overflow_entry_len(addr).is_some(),
        "#7900: a target handed to the mutator by WeakRef.deref() after the final \
         root remark was SWEPT while the mutator still held it"
    );
    let weak_ref = f64::from_bits(js_shadow_slot_get(slot));
    assert_eq!(
        crate::weakref::js_weakref_deref(weak_ref).to_bits(),
        bits,
        "#7900: a target the mutator strongly acquired through deref() was \
         tombstoned by a later weak slice"
    );
    crate::object::test_clear_overflow_fields_root();
}

/// #7900, budgeted MINOR ordering (BarrierSeedDrain → FinalRootRemark →
/// WeakProcessing → MinorPrelude → RememberedSetRebuild). The full path's
/// sliced remembered-set rebuild sits between the remark and the weak
/// decisions; the minor path has no such phase, so it pins the other ordering.
#[test]
fn weak_read_after_final_remark_survives_budgeted_minor_cycle() {
    const HOLDERS: u32 = 8;
    let _guard = CopyingNurseryTestGuard::new(HOLDERS + 1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    crate::weakref::test_support::reset_weak_read_barrier_shades();

    seed_weak_refs(HOLDERS);
    let witness = alloc_remark_witness();
    age_out_of_block_persist_window();

    let mut state = start_budgeted_minor_fallback_state(trace_snapshot(GcTriggerKind::ArenaBytes));
    park_inside_weak_processing(&mut state, HOLDERS, witness, HOLDERS);

    let (slot, bits, addr) = mutator_weak_read(HOLDERS);
    crate::object::test_seed_overflow_fields_root(addr, 42f64.to_bits());
    assert!(
        crate::weakref::test_support::weak_read_barrier_shades() >= 1,
        "SUBJECT-LIVE CHECK: the weak read must have shaded an unmarked target"
    );

    run_cycle_in_single_unit_steps(&mut state);
    let _ = state.take_outcome().expect("cycle should complete");

    assert!(
        crate::object::debug_overflow_entry_len(addr).is_some(),
        "#7900 (minor ordering): a deref()'d target was swept while the mutator held it"
    );
    let weak_ref = f64::from_bits(js_shadow_slot_get(slot));
    assert_eq!(
        crate::weakref::js_weakref_deref(weak_ref).to_bits(),
        bits,
        "#7900 (minor ordering): a deref()'d target was tombstoned by a later weak slice"
    );
    crate::object::test_clear_overflow_fields_root();
}

/// The same race reaching a `WeakMap`: the key the mutator presents to
/// `get()` can only be white if it was itself acquired weakly in this window,
/// which is exactly the shape here — `deref()` recovers a key, `get()` then
/// reads its entry. Both reads must shade, or the pending weak slice tombstones
/// the entry and the sweep takes the key the mutator is holding.
///
/// 8 entries + 8 key WeakRefs = 16 holders and a budget of 1, so at most one
/// holder is decided when the window opens and at least seven key/entry pairs
/// are intact. The test scans for one rather than assuming a registry iteration
/// order, so it can never silently skip itself.
#[test]
fn weak_map_read_after_final_remark_survives_budgeted_cycle() {
    const ENTRIES: u32 = 8;
    const MAP_SLOT: u32 = ENTRIES;
    const WITNESS_SLOT: u32 = ENTRIES + 1;
    let _guard = CopyingNurseryTestGuard::new(ENTRIES + 2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    crate::weakref::test_support::reset_weak_read_barrier_shades();

    let map = crate::weakref::js_weakmap_new();
    js_shadow_slot_set(MAP_SLOT, ptr_bits(map as usize));
    let map_bits = ptr_bits(map as usize);
    for slot in 0..ENTRIES {
        let key = crate::object::js_object_alloc(0, 0);
        let value = crate::object::js_object_alloc(0, 0);
        crate::weakref::js_weakmap_set(
            f64::from_bits(map_bits),
            f64::from_bits(ptr_bits(key as usize)),
            f64::from_bits(ptr_bits(value as usize)),
        );
        // One WeakRef per key, so the mutator can recover a key without ever
        // holding it strongly (a strongly-held key would be marked at the
        // remark and its entry could not be tombstoned).
        let key_ref = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(key as usize)));
        js_shadow_slot_set(slot, ptr_bits(key_ref as usize));
    }
    let witness = alloc_remark_witness();
    age_out_of_block_persist_window();

    let mut state = GcCycleState::new_full(trace_snapshot(GcTriggerKind::ArenaBytes));
    state.set_progress_kind(GcProgressKind::NormalIncremental);
    park_inside_weak_processing(&mut state, ENTRIES, witness, WITNESS_SLOT);

    // ---- mutator window: weak read #1 recovers a key, #2 reads its entry ----
    let map_value = f64::from_bits(js_shadow_slot_get(MAP_SLOT));
    let mut acquired = None;
    for slot in 0..ENTRIES {
        let key_bits =
            crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(slot))).to_bits();
        if key_bits == crate::value::TAG_UNDEFINED {
            continue;
        }
        let value_bits =
            crate::weakref::js_weakmap_get(map_value, f64::from_bits(key_bits)).to_bits();
        if value_bits != crate::value::TAG_UNDEFINED {
            acquired = Some((key_bits, value_bits));
            break;
        }
    }
    let (key_bits, value_bits) = acquired.expect(
        "with 16 holders and a one-unit budget at least seven key/entry pairs \
         must still be pending — the race was not set up",
    );
    let key_addr = (key_bits & POINTER_MASK) as usize;
    crate::object::test_seed_overflow_fields_root(key_addr, 42f64.to_bits());
    assert!(
        crate::weakref::test_support::weak_read_barrier_shades() >= 1,
        "SUBJECT-LIVE CHECK: the weak reads must have shaded at least one white value"
    );

    run_cycle_in_single_unit_steps(&mut state);
    let _ = state.take_outcome().expect("cycle should complete");

    assert!(
        crate::object::debug_overflow_entry_len(key_addr).is_some(),
        "#7900: a WeakMap key the mutator strongly acquired in a post-remark \
         window was swept while it still held it"
    );
    assert_eq!(
        crate::weakref::js_weakmap_get(
            f64::from_bits(js_shadow_slot_get(MAP_SLOT)),
            f64::from_bits(key_bits),
        )
        .to_bits(),
        value_bits,
        "#7900: a WeakMap entry the mutator read in the window was tombstoned"
    );
    crate::object::test_clear_overflow_fields_root();
}

/// Contract test for the acceptance criterion "no mutator window exists after
/// the last root observation unless weak reads participate in marking".
///
/// The barrier's kill state is the absence of a cycle: with no incremental mark
/// in progress a weak read must shade NOTHING (it would otherwise leave stray
/// marks that the next cycle reads as live). This pins both directions.
#[test]
fn weak_read_barrier_is_inert_outside_a_cycle() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::weakref::test_support::clear_weak_holders();
    crate::weakref::test_support::reset_weak_read_barrier_shades();

    let target = crate::object::js_object_alloc(0, 0);
    let weak_ref = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(target as usize)));
    js_shadow_slot_set(0, ptr_bits(weak_ref as usize));
    js_shadow_slot_set(1, ptr_bits(target as usize));

    assert!(!incremental_mark_barrier_active());
    let bits = crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(0))).to_bits();
    assert_ne!(bits, crate::value::TAG_UNDEFINED);
    assert_eq!(
        crate::weakref::test_support::weak_read_barrier_shades(),
        0,
        "a weak read outside a mark cycle must not mark anything: a stray mark \
         reads as live to the next cycle"
    );
    let header = unsafe { header_from_user_ptr(((bits & POINTER_MASK) as usize) as *const u8) };
    assert_eq!(
        unsafe { (*header).gc_flags } & GC_FLAG_MARKED,
        0,
        "weak read outside a cycle must leave the target unmarked"
    );
}
