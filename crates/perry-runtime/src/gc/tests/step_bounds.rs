//! #7903 — a "time-budgeted" step is only as bounded as its largest single
//! work unit.
//!
//! Two units used to be unbounded. Weak processing charged one unit per
//! registered *holder*, and a `FinalizationRegistry` is one holder however many
//! records it owns — so a single registry was an atomic, heap-sized unit behind
//! `js_gc_step_us`. Final root remark re-scans the roots and then drains
//! everything they newly reach, both with `usize::MAX`.
//!
//! These tests are adversarial in the sense the issue asked for: they build the
//! pathological shape rather than assert that an ordinary run stays quiet.
//! Every one of them asserts a **liveness counter** — that the sliced path
//! actually ran — before asserting the bound, because a run where the sliced
//! path never executed reports the same zeros as a run where it worked
//! perfectly.

use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(0));
}

extern "C" fn finreg_step_bounds_callback(
    _closure: *const crate::closure::ClosureHeader,
    _held: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Root a FinalizationRegistry in shadow slot 0 and give it `records`
/// registrations. Targets are deliberately unrooted: a registry whose records
/// all resolve is the cheap case, and the expensive one is what needs bounding.
fn rooted_registry_with_records(records: usize) {
    let cb = crate::closure::js_closure_alloc(finreg_step_bounds_callback as *const u8, 0);
    let reg = crate::weakref::js_finreg_new(f64::from_bits(ptr_bits(cb as usize)));
    js_shadow_slot_set(0, ptr_bits(reg as usize));
    for _ in 0..records {
        add_one_registration();
    }
}

/// Register one more record against the slot-0 registry. Used both to build the
/// fixture and, mid-cycle, as the *mutator* restructuring the entries array
/// under a live record cursor.
fn add_one_registration() {
    let target = crate::object::js_object_alloc(0, 0);
    let reg_v = f64::from_bits(js_shadow_slot_get(0));
    let _ = crate::weakref::js_finreg_register(
        reg_v,
        f64::from_bits(ptr_bits(target as usize)),
        f64::from_bits(crate::value::TAG_TRUE),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
}

/// Start a budgeted cycle and return once it is active.
fn start_budgeted_cycle(result: &mut JsGcStepResult) {
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    assert_eq!(
        js_gc_step_work_units(1, result),
        JS_GC_STEP_STATUS_ACTIVE,
        "old-gen pressure must start a budgeted cycle"
    );
}

/// Drive the active cycle one work unit at a time, calling `between` after each
/// step. Returns the number of steps taken.
fn drive_one_unit_at_a_time(mut between: impl FnMut(usize)) -> usize {
    let mut result = JsGcStepResult::default();
    let mut steps = 0usize;
    loop {
        let status = js_gc_step_work_units(1, &mut result);
        if status != JS_GC_STEP_STATUS_ACTIVE {
            return steps;
        }
        steps += 1;
        assert!(
            steps < 200_000,
            "budgeted cycle did not complete in a sane number of one-unit steps"
        );
        between(steps);
    }
}

/// The bound itself: one registry's record array must be spread across steps,
/// with no single step charging more records than its budget.
///
/// Before #7903 the whole array was scanned inside the one work unit that
/// resolved the holder, so `weak_max_records_per_step` would equal `RECORDS`
/// no matter how small the budget was.
#[test]
fn one_registry_record_array_is_sliced_across_budgeted_steps() {
    const RECORDS: usize = 64;
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    crate::weakref::test_support::clear_weak_holders();
    instruments::reset_step_bound_counters();

    rooted_registry_with_records(RECORDS);

    let mut result = JsGcStepResult::default();
    start_budgeted_cycle(&mut result);
    drive_one_unit_at_a_time(|_| {});

    // LIVENESS FIRST: without this the two bounds below are satisfied by a run
    // that never reached weak processing at all.
    assert!(
        instruments::weak_steps_sliced() > 0,
        "no step ended mid-registry — the sliced path did not run, so the \
         bounds asserted below are vacuous"
    );
    assert!(
        instruments::weak_records_scanned() >= RECORDS as u64,
        "every registered record must be scanned: scanned={} expected>={RECORDS}",
        instruments::weak_records_scanned()
    );
    assert!(
        instruments::weak_max_records_per_step() <= 1,
        "a one-work-unit step must charge at most one record; charged {}",
        instruments::weak_max_records_per_step()
    );
}

/// A bigger budget must scale the slice, not remove it: the per-step ceiling
/// tracks the budget rather than the registry size.
#[test]
fn record_slice_size_tracks_the_work_budget() {
    const RECORDS: usize = 128;
    const BUDGET: u64 = 8;
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    crate::weakref::test_support::clear_weak_holders();
    instruments::reset_step_bound_counters();

    rooted_registry_with_records(RECORDS);

    let mut result = JsGcStepResult::default();
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    assert_eq!(
        js_gc_step_work_units(BUDGET, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    let mut steps = 0usize;
    while js_gc_step_work_units(BUDGET, &mut result) == JS_GC_STEP_STATUS_ACTIVE {
        steps += 1;
        assert!(steps < 200_000, "budgeted cycle did not complete");
    }

    assert!(
        instruments::weak_steps_sliced() > 0,
        "the registry must still be sliced at budget {BUDGET}, not swallowed whole"
    );
    assert!(
        instruments::weak_max_records_per_step() <= BUDGET,
        "a {BUDGET}-unit step charged {} records",
        instruments::weak_max_records_per_step()
    );
    assert!(
        instruments::weak_max_records_per_step() < RECORDS as u64,
        "the whole {RECORDS}-record array went into one step — not sliced at all"
    );
}

/// The correctness half. The mutator restructures the entries array between two
/// slices; the cursor must notice and restart rather than resume against
/// indices that now denote different records.
///
/// `js_finreg_register` changes the array's length (and usually its identity
/// word), which is exactly the signal the cursor validates against.
#[test]
fn mutating_the_entries_array_between_slices_restarts_the_cursor() {
    const RECORDS: usize = 32;
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    crate::weakref::test_support::clear_weak_holders();
    instruments::reset_step_bound_counters();

    rooted_registry_with_records(RECORDS);

    let mut result = JsGcStepResult::default();
    start_budgeted_cycle(&mut result);
    // Mutate once, as soon as the first record slice has been charged.
    let mut mutated = false;
    drive_one_unit_at_a_time(|_| {
        if !mutated && instruments::weak_records_scanned() > 0 {
            mutated = true;
            add_one_registration();
        }
    });

    assert!(
        mutated,
        "the fixture never reached a record slice to mutate"
    );
    assert!(
        instruments::weak_registry_restarts() > 0,
        "restructuring the entries array under a live cursor was not detected — \
         a resumed cursor would silently skip records, leaving weak slots \
         un-tombstoned"
    );
}

/// The hard bound on the restart loop. A mutator that restructures the array in
/// every window would restart the scan forever; past `MAX_REGISTRY_RESTARTS`
/// the registry is finished in one atomic pass and *charged as such*.
#[test]
fn relentless_mutation_falls_back_to_a_bounded_atomic_finish() {
    const RECORDS: usize = 32;
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    crate::weakref::test_support::clear_weak_holders();
    instruments::reset_step_bound_counters();

    rooted_registry_with_records(RECORDS);

    let mut result = JsGcStepResult::default();
    start_budgeted_cycle(&mut result);
    // Restructure after EVERY step for as long as weak processing is running.
    drive_one_unit_at_a_time(|_| {
        if instruments::weak_records_scanned() > 0
            && instruments::weak_registry_atomic_finishes() == 0
        {
            add_one_registration();
        }
    });

    assert!(
        instruments::weak_registry_restarts() > 0,
        "the adversary never managed to invalidate a cursor"
    );
    assert!(
        instruments::weak_registry_atomic_finishes() > 0,
        "a registry mutated in every window restarted forever instead of \
         falling back to the bounded atomic finish"
    );
}

/// Final root remark is intentionally atomic (see
/// `docs/src/internals/gc-step-bounds.md`). The obligation this test enforces
/// is that it is *measured* rather than claimed: an atomic phase that nobody
/// times is indistinguishable from one that does not exist.
#[test]
fn final_root_remark_is_accounted_as_a_separate_atomic_phase() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    instruments::reset_step_bound_counters();

    let mut result = JsGcStepResult::default();
    start_budgeted_cycle(&mut result);
    drive_one_unit_at_a_time(|_| {});

    assert!(
        instruments::final_remark_count() > 0,
        "a completed budgeted cycle must record at least one atomic final \
         remark; zero means the phase is unmeasured again"
    );
}
