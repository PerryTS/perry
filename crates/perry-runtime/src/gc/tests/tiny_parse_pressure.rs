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
    tiny_parse_pressure_due, tiny_parse_pressure_due_with, tiny_parse_pressure_headroom_bytes,
    GC_STEP_BYTES, GC_THRESHOLD_INITIAL_BYTES, GC_THRESHOLD_MAX_BYTES,
    GC_TINY_PARSE_PRESSURE_BASE_BYTES,
};

const MB: usize = 1024 * 1024;

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
