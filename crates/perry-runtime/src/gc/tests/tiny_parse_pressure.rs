//! #9831: the tiny-parse pressure guard prices the collections it forces.
//!
//! The guard used to be an absolute `arena_in_use_bytes() >= 48 MB` test, so
//! on a program whose live set never drops below that it forced a collection
//! after EVERY tiny `JSON.parse` — the adaptive step's backoff was computed
//! by each of those collections and consulted by none of them. These tests
//! pin the pricing: the headroom the guard demands between collections is the
//! part of the step the `ArenaBytes` arm's ceiling clamp discards, and the
//! guard is not due until the arena has grown that much since the last
//! collection ended.
//!
//! Sabotage-proved: restoring the absolute guard (dropping the growth clause
//! from `tiny_parse_pressure_due_with`) fails
//! `the_absolute_guard_alone_is_the_bug_the_growth_clause_exists_for` and
//! `growth_past_the_headroom_is_due`'s boundary half while the rest pass;
//! pricing the headroom at the raw step (`floor.max(step.min(ceiling))`)
//! fails `power_on_step_buys_exactly_the_headroom_floor`.

use super::super::heap_budget::{
    gc_trigger_absolute_ceiling_bytes, gc_trigger_headroom_floor_bytes,
};
use super::super::policy::{
    external_side_parse_band_bytes, external_side_parse_pressure_due_with,
    tiny_parse_boundary_poll_next, tiny_parse_pressure_due, tiny_parse_pressure_due_with,
    tiny_parse_pressure_headroom_bytes, GC_STEP_BYTES, GC_THRESHOLD_INITIAL_BYTES,
    GC_THRESHOLD_MAX_BYTES, GC_TINY_PARSE_BOUNDARY_POLL_INTERVAL,
    GC_TINY_PARSE_PRESSURE_BASE_BYTES,
};

const MB: usize = 1024 * 1024;

#[test]
fn bounded_tiny_parse_poll_has_exact_interval() {
    let mut remaining = GC_TINY_PARSE_BOUNDARY_POLL_INTERVAL - 1;
    for completion in 1..=GC_TINY_PARSE_BOUNDARY_POLL_INTERVAL * 2 {
        let (next, poll) = tiny_parse_boundary_poll_next(remaining);
        remaining = next;
        assert_eq!(
            poll,
            completion % GC_TINY_PARSE_BOUNDARY_POLL_INTERVAL == 0,
            "completion {completion}"
        );
    }
}

/// Restores the two live cells the guard reads, so a test that moves them
/// cannot leak its state into the next one (the suite is single-threaded, but
/// the cells outlive the test).
struct LiveCellsGuard {
    step: usize,
    base: usize,
}

impl LiveCellsGuard {
    fn set(step: usize, base: usize) -> Self {
        Self {
            step: GC_STEP_BYTES.with(|cell| cell.replace(step)),
            base: GC_TINY_PARSE_PRESSURE_BASE_BYTES.with(|cell| cell.replace(base)),
        }
    }
}

impl Drop for LiveCellsGuard {
    fn drop(&mut self) {
        GC_STEP_BYTES.with(|cell| cell.set(self.step));
        GC_TINY_PARSE_PRESSURE_BASE_BYTES.with(|cell| cell.set(self.base));
    }
}

#[test]
fn power_on_step_buys_exactly_the_headroom_floor() {
    // The step powers on at the trigger ceiling. That is not evidence of an
    // unproductive collection — nothing has run yet — so the guard keeps the
    // cadence it always had: the headroom floor, not the ceiling.
    assert_eq!(
        tiny_parse_pressure_headroom_bytes(GC_THRESHOLD_INITIAL_BYTES),
        gc_trigger_headroom_floor_bytes(),
        "a step that has never been priced must buy the floor, not the ceiling"
    );
}

#[test]
fn a_productive_collection_keeps_the_headroom_floor() {
    // Productive collections halve the step toward its own 16 MB floor. Every
    // step at or below the power-on value maps to the headroom floor.
    let floor = gc_trigger_headroom_floor_bytes();
    for step in [16 * MB, 32 * MB, 64 * MB, GC_THRESHOLD_INITIAL_BYTES / 2] {
        assert_eq!(
            tiny_parse_pressure_headroom_bytes(step),
            floor,
            "step {step} is a productive reading and must keep the floor"
        );
    }
}

#[test]
fn each_doubling_the_ceiling_discards_doubles_the_headroom() {
    let floor = gc_trigger_headroom_floor_bytes();
    let ceiling = gc_trigger_absolute_ceiling_bytes();
    let mut previous = tiny_parse_pressure_headroom_bytes(GC_THRESHOLD_INITIAL_BYTES);
    for doublings in 1..=3u32 {
        let step = GC_THRESHOLD_INITIAL_BYTES << doublings;
        let headroom = tiny_parse_pressure_headroom_bytes(step);
        let expected = (floor << doublings).min(ceiling);
        assert_eq!(
            headroom, expected,
            "{doublings} discarded doubling(s) must buy floor << {doublings}, bounded by the ceiling"
        );
        assert!(
            headroom >= previous,
            "backing off further must never shrink the headroom"
        );
        if ceiling >= (floor << doublings) {
            assert!(
                headroom > previous,
                "an unproductive collection must earn more headroom than the reading before it"
            );
        }
        previous = headroom;
    }
}

#[test]
fn headroom_is_bounded_by_the_absolute_ceiling() {
    let ceiling = gc_trigger_absolute_ceiling_bytes();
    let saturated = tiny_parse_pressure_headroom_bytes(GC_THRESHOLD_MAX_BYTES);
    assert!(
        saturated <= ceiling,
        "a saturated step ({saturated}) must not let the guard wait longer than the arm's ceiling ({ceiling})"
    );
    // On the unconstrained desktop budget the saturated step (1 GiB, three
    // doublings past the 128 MB initial) reaches the 128 MB ceiling exactly;
    // under a small `PERRY_GC_HEAP_LIMIT` the ceiling is lower and the clamp
    // binds earlier. Either way the saturated reading IS the ceiling.
    if ceiling <= gc_trigger_headroom_floor_bytes() << 3 {
        assert_eq!(saturated, ceiling);
    }
}

#[test]
fn below_the_in_use_trigger_is_never_due() {
    let trigger = 48 * MB;
    // Even with zero base and the most productive step, the guard stays off
    // below its in-use trigger: small heaps are the regular arms' business.
    assert!(!tiny_parse_pressure_due_with(
        trigger - 1,
        trigger,
        0,
        16 * MB
    ));
    assert!(!tiny_parse_pressure_due_with(0, trigger, 0, 16 * MB));
}

#[test]
fn the_absolute_guard_alone_is_the_bug_the_growth_clause_exists_for() {
    // The measured shape: a 60 MB live set above the 48 MB trigger, a tiny
    // parse that grew the arena by a few KB since the collection that just
    // ran, and a step saturated at its maximum because those collections free
    // nothing. The old guard said "collect" here after every parse.
    let trigger = 48 * MB;
    let base = 60 * MB;
    let in_use = base + 4096;
    assert!(
        !tiny_parse_pressure_due_with(in_use, trigger, base, GC_THRESHOLD_MAX_BYTES),
        "a few KB of growth past a collection that freed nothing must not force another"
    );
    // Nor with a productive step: 4 KB is below the headroom floor too.
    assert!(!tiny_parse_pressure_due_with(
        in_use,
        trigger,
        base,
        16 * MB
    ));
}

#[test]
fn growth_past_the_headroom_is_due() {
    let trigger = 48 * MB;
    let base = 60 * MB;
    for step in [16 * MB, GC_THRESHOLD_INITIAL_BYTES, GC_THRESHOLD_MAX_BYTES] {
        let headroom = tiny_parse_pressure_headroom_bytes(step);
        let boundary = base + headroom;
        assert!(
            tiny_parse_pressure_due_with(boundary, trigger, base, step),
            "growth of exactly the headroom ({headroom}) at step {step} is due"
        );
        assert!(
            !tiny_parse_pressure_due_with(boundary - 1, trigger, base, step),
            "one byte short of the headroom ({headroom}) at step {step} is not"
        );
    }
}

#[test]
fn the_live_predicate_reads_the_step_and_the_base() {
    let trigger = 48 * MB;
    let base = 60 * MB;
    let floor = gc_trigger_headroom_floor_bytes();
    let ceiling = gc_trigger_absolute_ceiling_bytes();

    // Saturated step: the guard waits for the ceiling's worth of growth.
    let _cells = LiveCellsGuard::set(GC_THRESHOLD_MAX_BYTES, base);
    if ceiling > floor {
        assert!(!tiny_parse_pressure_due(base + floor, trigger));
    }
    assert!(tiny_parse_pressure_due(base + ceiling.max(floor), trigger));

    // Productive step: the floor is enough again.
    GC_STEP_BYTES.with(|cell| cell.set(16 * MB));
    assert!(tiny_parse_pressure_due(base + floor, trigger));
    assert!(!tiny_parse_pressure_due(base + floor - 1, trigger));
}

#[test]
fn a_finished_collection_moves_the_base_to_the_post_collection_reading() {
    use super::super::js_gc_collect;
    // Whatever the base was, a completed collection re-baselines it to the
    // arena's post-collection in-use reading — the same reading the guard
    // compares against at the next parse boundary. Assert the identity of the
    // two readings, not merely that the cell moved: a base recorded in other
    // units (the live census) would count every swept hole as growth.
    let _cells = LiveCellsGuard::set(GC_STEP_BYTES.with(|cell| cell.get()), usize::MAX);
    js_gc_collect();
    let base = GC_TINY_PARSE_PRESSURE_BASE_BYTES.with(|cell| cell.get());
    assert_ne!(
        base,
        usize::MAX,
        "a finished collection must record the base"
    );
    assert_eq!(
        base,
        crate::arena::arena_in_use_bytes(),
        "the base must be the post-collection `arena_in_use_bytes()` reading"
    );
}

/// The nursery cap schedules a tiny-parse boundary collection on its own, well
/// below the priced in-use guard, and only while the cap is actually due.
#[test]
fn a_due_nursery_cap_schedules_the_boundary_collection_below_the_in_use_guard() {
    use super::super::policy::{
        gc_schedule_parse_boundary_collection_if_pressure, ScavengeNurseryCapTestGuard,
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING,
    };
    use super::support::*;
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let pending = || GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(std::cell::Cell::get);
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|p| p.set(false));
    let filler = [b'j'; 64];
    crate::string::js_string_from_bytes(filler.as_ptr(), filler.len() as u32);
    let in_use = crate::arena::arena_in_use_bytes();
    assert!(
        !tiny_parse_pressure_due(in_use, 48 * MB),
        "fixture: the priced in-use guard must not be due, or this proves nothing"
    );
    // Medium-parse pacing (2026-09-14) added a third arm to the same
    // predicate. Pin it quiet for the duration, so a side-allocation band that
    // happened to be due could not make this test's negative half pass for the
    // wrong reason.
    let _external_base = ExternalBaseGuard::set(
        crate::gc::policy::external_side_live_bytes() + gc_trigger_headroom_floor_bytes(),
    );
    assert!(
        !crate::gc::policy::external_side_parse_pressure_due(),
        "fixture: the side-allocation arm must not be due, or this proves nothing"
    );

    {
        let _cap = ScavengeNurseryCapTestGuard::due_at_bytes(usize::MAX);
        gc_schedule_parse_boundary_collection_if_pressure();
        assert!(
            !pending(),
            "neither the guard nor the cap is due: nothing scheduled"
        );
    }
    {
        let _cap = ScavengeNurseryCapTestGuard::due_at_bytes(1);
        gc_schedule_parse_boundary_collection_if_pressure();
        assert!(
            pending(),
            "a due nursery cap schedules the boundary collection"
        );
    }
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|p| p.set(false));
}

// ───────────────────────────────────────────────────────────────────────────
// Medium-parse pacing (2026-09-14): the side-allocation arm.
//
// Both pre-existing arms are denominated in ARENA bytes, and a lazily-parsed
// document's bytes are not in the arena. The readings below are the measured
// ones from `records_array_16k:parse` at `origin/main` (`PERRY_GC_DIAG=1`,
// 11 284 iterations of a 13 197-byte fixture): every one of the eight
// collections the row ran was a full mark-sweep from `alloc_point_old_reclaim`,
// fired at `external_side=33 570 480` with `arena_total` 3–8 MB, `old_in_use=0`
// and `from_space` never above 6 857 064 against a 16 MB nursery cap.
//
// Sabotage-proved: deleting the `|| external_side_parse_pressure_due()` arm
// from `tiny_parse_generational_collection_due` fails
// `side_allocation_pressure_schedules_the_boundary_collection_below_both_arena_arms`
// while `the_measured_defect_is_invisible_to_both_arena_denominated_arms` — the
// twin that says what the harm IS — keeps passing, which is the point: the two
// together say "nothing else was going to collect this". Pricing the band at a
// bare floor (`gc_trigger_headroom_floor_bytes()`, dropping the `.max(baseline)`)
// fails `a_collection_that_cannot_free_the_side_bytes_doubles_the_band` and
// `a_genuinely_large_live_side_set_is_not_due_at_the_floor`.
// ───────────────────────────────────────────────────────────────────────────

/// The measured `records_array_16k:parse` readings at the moment `origin/main`
/// finally collected.
const MEASURED_EXTERNAL_SIDE_BYTES: usize = 33_570_480;
const MEASURED_ARENA_IN_USE_BYTES: usize = 3 * MB;

/// Restores the side-allocation band's base cell, which the live predicate and
/// every finished collection write.
struct ExternalBaseGuard(usize);

impl ExternalBaseGuard {
    fn set(base: usize) -> Self {
        Self(
            super::super::policy::GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES
                .with(|cell| cell.replace(base)),
        )
    }
}

impl Drop for ExternalBaseGuard {
    fn drop(&mut self) {
        super::super::policy::GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES.with(|cell| cell.set(self.0));
    }
}

/// The twin that says what the harm is. Nothing here asserts the new arm — it
/// asserts that WITHOUT it the measured workload has no arm at all, which is
/// what made 32 MB of dead tape per cycle the row's steady state.
#[test]
fn the_measured_defect_is_invisible_to_both_arena_denominated_arms() {
    use super::super::policy::ScavengeNurseryCapTestGuard;
    let _cells = LiveCellsGuard::set(GC_THRESHOLD_INITIAL_BYTES, 0);
    // The priced in-use guard reads `arena_in_use_bytes()`: 3 MB against a
    // 48 MB trigger.
    assert!(
        !tiny_parse_pressure_due(MEASURED_ARENA_IN_USE_BYTES, 48 * MB),
        "3 MB of arena is far below the in-use guard — it can never fire here"
    );
    // The nursery cap reads `copying_from_space_in_use_bytes()`: the row's
    // high-water was 6.9 MB against the 16 MB cap, because a lazy array puts
    // ~1.1 KB in the nursery per parse and ~24 KB in a side allocation.
    let _cap = ScavengeNurseryCapTestGuard::due_at_bytes(usize::MAX);
    assert!(
        !super::super::policy::young_scavenge_cap_due(),
        "the nursery cap cannot see a byte of the 32 MB the process is holding"
    );
}

#[test]
fn the_side_allocation_arm_is_due_at_the_measured_readings() {
    assert!(
        external_side_parse_pressure_due_with(MEASURED_EXTERNAL_SIDE_BYTES, 0),
        "32 MB of side allocation past an empty base must be due"
    );
}

#[test]
fn an_empty_base_is_due_exactly_at_the_headroom_floor() {
    let floor = gc_trigger_headroom_floor_bytes();
    assert!(
        external_side_parse_pressure_due_with(floor, 0),
        "the floor's worth of side allocation past an empty base is due"
    );
    assert!(
        !external_side_parse_pressure_due_with(floor - 1, 0),
        "one byte short of the floor is not"
    );
    assert!(
        !external_side_parse_pressure_due_with(0, 0),
        "a program that has allocated no side buffers is never due"
    );
}

#[test]
fn a_collection_that_cannot_free_the_side_bytes_doubles_the_band() {
    // The livelock shape this band is built against: a lazy array whose sparse
    // cache is at or past `LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES` is born
    // OLD (`json_tape::lazy_cluster_is_old`), so the nursery collection this arm
    // schedules cannot prove its owner dead and its tape survives. The base then
    // re-bases at the surviving value and the next band is twice as far away,
    // rather than being due again at the very next parse.
    let survived = 24 * MB;
    assert!(
        !external_side_parse_pressure_due_with(survived, survived),
        "a collection that freed nothing must not be immediately due again"
    );
    assert!(
        external_side_parse_pressure_due_with(2 * survived, survived),
        "doubling past the surviving value is due"
    );
    assert!(
        !external_side_parse_pressure_due_with(2 * survived - 1, survived),
        "one byte short of doubling is not"
    );
}

#[test]
fn a_genuinely_large_live_side_set_is_not_due_at_the_floor() {
    // A retained multi-MB `Map` contributes to the same counter. Its bytes must
    // not force a collection at every parse boundary for the rest of the run.
    let live_map = 64 * MB;
    let floor = gc_trigger_headroom_floor_bytes();
    assert!(
        !external_side_parse_pressure_due_with(live_map + floor, live_map),
        "the band must grow with a genuinely large live side set, not stay at the floor"
    );
    assert_eq!(
        external_side_parse_band_bytes(live_map),
        live_map,
        "past the floor the band is the baseline itself (100% growth)"
    );
    assert_eq!(
        external_side_parse_band_bytes(0),
        floor,
        "an empty baseline buys exactly the headroom floor"
    );
}

/// The wiring test: the arm reaches the boundary-collection scheduler, and does
/// so below both arena-denominated arms.
#[test]
fn side_allocation_pressure_schedules_the_boundary_collection_below_both_arena_arms() {
    use super::super::policy::{
        gc_schedule_parse_boundary_collection_if_pressure, ScavengeNurseryCapTestGuard,
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING,
    };
    use super::support::*;
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _cells = LiveCellsGuard::set(GC_THRESHOLD_INITIAL_BYTES, 0);
    let pending = || GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(std::cell::Cell::get);
    let _cap = ScavengeNurseryCapTestGuard::due_at_bytes(usize::MAX);
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|p| p.set(false));

    let in_use = crate::arena::arena_in_use_bytes();
    assert!(
        !tiny_parse_pressure_due(in_use, 48 * MB),
        "fixture: the priced in-use guard must not be due, or this proves nothing"
    );
    {
        // A base one floor above the live reading keeps the side arm off too,
        // so all three arms are quiet: the scheduler must do nothing.
        let _base = ExternalBaseGuard::set(
            super::super::policy::external_side_live_bytes() + gc_trigger_headroom_floor_bytes(),
        );
        assert!(
            !super::super::policy::external_side_parse_pressure_due(),
            "fixture: the side arm must start quiet, or the negative half is vacuous"
        );
        gc_schedule_parse_boundary_collection_if_pressure();
        assert!(!pending(), "no arm is due: nothing scheduled");
    }
    {
        // Now only the side arm is due — an empty base with side bytes already
        // a floor past it, which is the `records_array_16k:parse` shape.
        let _base = ExternalBaseGuard::set(0);
        crate::gc::gc_note_external_side_alloc(gc_trigger_headroom_floor_bytes());
        // The alloc notice itself pokes `gc_check_trigger`, whose collection
        // would re-base the cell it just crossed; re-pin the base being tested.
        let _repin = ExternalBaseGuard::set(0);
        assert!(
            super::super::policy::external_side_parse_pressure_due(),
            "fixture: the side arm must be due, or the positive half is vacuous"
        );
        gc_schedule_parse_boundary_collection_if_pressure();
        let scheduled = pending();
        crate::gc::gc_note_external_side_free(gc_trigger_headroom_floor_bytes());
        assert!(
            scheduled,
            "a due side-allocation band schedules the boundary collection"
        );
    }
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|p| p.set(false));
}

/// The band is self-correcting only if the base is recorded AFTER the sweep and
/// the from-space pass have released what they can. Assert the identity of the
/// two readings, not merely that the cell moved.
#[test]
fn a_finished_collection_moves_the_external_base_to_the_post_collection_reading() {
    use super::super::js_gc_collect;
    use super::super::policy::{external_side_live_bytes, GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES};
    let _base = ExternalBaseGuard::set(usize::MAX);
    js_gc_collect();
    let base = GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES.with(|cell| cell.get());
    assert_ne!(
        base,
        usize::MAX,
        "a finished collection must record the side-allocation base"
    );
    assert_eq!(
        base,
        external_side_live_bytes(),
        "the base must be the POST-collection `external_side_live_bytes()` reading"
    );
}

/// The band's counterweight: the cheap collections it schedules must not hide
/// pressure from the arm that pays for arena-capacity release.
///
/// Sabotage-proved (run 2026-09-14, each against the whole
/// `tiny_parse_pressure` filter): reverting
/// `external_side_old_reclaim_pressure_bytes` to the bare
/// `external_side_live_bytes()` read fails BOTH tests below; deleting the reset
/// from `finish_full_old_reclaim_baseline` fails
/// `a_full_collection_clears_the_drained_debt` alone.
#[test]
fn a_drained_side_byte_still_pays_old_reclaim_until_the_next_full() {
    use super::super::policy::{
        external_side_live_bytes, external_side_old_reclaim_pressure_bytes,
        GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL,
    };
    use super::support::*;
    let _isolation = GcTestIsolationGuard::new();
    let restore = GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(|cell| cell.replace(0));
    let live_before = external_side_live_bytes();
    assert_eq!(
        external_side_old_reclaim_pressure_bytes(),
        live_before,
        "fixture: with no drained debt the term is the live reading"
    );

    const BYTES: usize = 3 * MB;
    crate::gc::gc_note_external_side_alloc(BYTES);
    let charged = external_side_old_reclaim_pressure_bytes();
    assert_eq!(charged, live_before + BYTES);
    // Reporting a release lowers the live reading while preserving the
    // cumulative term, whether the release came from a collector or mutator.
    crate::gc::gc_note_external_side_free(BYTES);
    assert_eq!(
        external_side_live_bytes(),
        live_before,
        "the live reading must fall by what was released"
    );
    assert_eq!(
        external_side_old_reclaim_pressure_bytes(),
        charged,
        "old-reclaim must still be charged for a byte a full has not yet paid for"
    );
    GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(|cell| cell.set(restore));
}

#[test]
fn a_full_collection_clears_the_drained_debt() {
    use super::super::js_gc_collect;
    use super::super::policy::{
        external_side_live_bytes, external_side_old_reclaim_pressure_bytes,
        GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL,
    };
    let restore = GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(|cell| cell.replace(7 * MB));
    assert_ne!(
        external_side_old_reclaim_pressure_bytes(),
        external_side_live_bytes(),
        "fixture: the debt must be non-zero, or this proves nothing"
    );
    js_gc_collect();
    assert_eq!(
        GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(crate::gc::TriggerInput::get),
        0,
        "the full that the debt was held for pays it"
    );
    assert_eq!(
        external_side_old_reclaim_pressure_bytes(),
        external_side_live_bytes(),
        "with the debt paid the term is the live reading again"
    );
    GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL.with(|cell| cell.set(restore));
}
