//! Parse pressure: the tiny-parse thresholds, the collection a suppressed
//! parse leaves pending, and the parse-boundary and JSON-construction
//! scheduling.

use super::*;

pub(in crate::gc) const GC_SUPPRESSED_TINY_PARSE_BYTES: usize = 1024 * 1024;
pub(in crate::gc) const GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES: usize = 48 * 1024 * 1024;
pub(in crate::gc) const GC_SUPPRESSED_TINY_PARSE_FULL_GC_IN_USE_TRIGGER_BYTES: usize =
    24 * 1024 * 1024;
pub(in crate::gc) const GC_TINY_PARSE_BOUNDARY_POLL_INTERVAL: u8 = 64;

pub(in crate::gc) fn gc_suppressed_parse_is_tiny(parse_growth: usize) -> bool {
    parse_growth <= GC_SUPPRESSED_TINY_PARSE_BYTES
}

/// #9831: how much the arena must have grown since the last collection ended
/// before tiny-parse churn may force another one — priced by the part of the
/// productivity backoff the `ArenaBytes` arm computes and then discards.
///
/// `GC_STEP_BYTES` powers on at `GC_THRESHOLD_INITIAL_BYTES`, which equals the
/// trigger ceiling, and doubles on every collection that frees almost nothing.
/// The arm's own re-arm (`gc_finish_arena_trigger_collection`) clamps
/// `new_total + step` at the ceiling, so every doubling past the initial step
/// is backoff the arm has computed, stored, and cannot express. This rescales
/// the step so that its power-on value buys exactly the headroom floor, and
/// each discarded doubling buys the guard one more doubling — up to the same
/// ceiling, so a run of unproductive collections can never let the guard wait
/// longer than the arm itself would. A productive collection halves the step
/// toward its 16 MB floor, which maps below the headroom floor and clamps to
/// it: a churn loop whose collections pay keeps today's cadence.
pub(in crate::gc) fn tiny_parse_pressure_headroom_bytes(step: usize) -> usize {
    let floor = gc_trigger_headroom_floor_bytes();
    let ceiling = gc_trigger_absolute_ceiling_bytes();
    // 64-bit intermediate on purpose: `step` reaches 1 GiB and `floor` 16 MiB,
    // whose product does not fit a 32-bit `usize` (watchOS/visionOS are ILP32;
    // see `ladder_nursery_cap_bytes` for the same trap).
    let scaled = (step as u64).saturating_mul(floor as u64) / (GC_THRESHOLD_INITIAL_BYTES as u64);
    let scaled = scaled.min(usize::MAX as u64) as usize;
    scaled.max(floor).min(ceiling.max(floor))
}

/// #9831: whether tiny-parse churn has earned a forced collection.
///
/// The guard used to be `in_use >= in_use_trigger` alone — an absolute
/// threshold on a quantity no collection can lower below the live set. On a
/// program whose live set sits above it permanently (the compiled claude-code
/// TUI holds 59–297 MB of arena through one streamed reply) that made every
/// tiny `JSON.parse` the SSE stream consists of force a collection at the next
/// safepoint: 51 minors in one 66-delta reply, each freeing a median 131 KB,
/// while the adaptive step sat at its 1 GiB maximum saying "back off" and
/// nothing consulted it. The growth clause is what that step is for.
///
/// `base` is `arena_in_use_bytes()` as the last collection ended
/// (`GC_TINY_PARSE_PRESSURE_BASE_BYTES`); `in_use` is the same reading now.
pub(in crate::gc) fn tiny_parse_pressure_due_with(
    in_use: usize,
    in_use_trigger: usize,
    base: usize,
    step: usize,
) -> bool {
    in_use >= in_use_trigger
        && in_use >= base.saturating_add(tiny_parse_pressure_headroom_bytes(step))
}

/// Should a tiny-parse boundary collect under the generational collector?
///
/// The priced in-use guard ([`tiny_parse_pressure_due`]) answers "has the
/// arena grown enough to be worth a collection", on total in-use, which a
/// collection cannot lower below the live set (#9831). It never asked the
/// generational question: is the young generation at its scavenge cap? A loop
/// of tiny `JSON.parse` calls allocates only under the parser's suppression
/// window and at bounded inline-object births, so nothing else arms the
/// nursery safepoint for it, and the young generation grew to the guard's
/// 48 MB floor before its first minor: `small_record:parse` peaked at 80 MiB
/// against Node's 59 with 0–9 ‰ of each collection surviving. The nursery cap
/// is safe to consult where the absolute in-use guard was not: a minor lowers
/// the quantity it tests to the survivors, so it cannot fire again until the
/// cap has been refilled.
///
/// Medium-parse pacing (2026-09-14) adds the third arm for the same reason the
/// second one exists, one currency over: both of the first two are denominated
/// in ARENA bytes, and a lazily-parsed document's bytes are not in the arena at
/// all. See
/// [`external_side_parse_pressure_due`].
pub(in crate::gc) fn tiny_parse_generational_collection_due(
    in_use: usize,
    in_use_trigger: usize,
) -> bool {
    tiny_parse_pressure_due(in_use, in_use_trigger)
        || young_scavenge_cap_due()
        || external_side_parse_pressure_due()
}

/// The live [`tiny_parse_pressure_due_with`]: current base and step.
pub(in crate::gc) fn tiny_parse_pressure_due(in_use: usize, in_use_trigger: usize) -> bool {
    #[cfg(test)]
    if GC_TINY_PARSE_PRESSURE_TEST_FORCE.with(|cell| cell.replace(false)) {
        return true;
    }
    let base = GC_TINY_PARSE_PRESSURE_BASE_BYTES.with(Cell::get);
    let step = GC_STEP_BYTES.with(Cell::get);
    tiny_parse_pressure_due_with(in_use, in_use_trigger, base, step)
}

/// Make the next explicitly armed parse-boundary collection deterministic in
/// tests without allocating tens of megabytes merely to cross the production
/// pressure threshold. The predicate consumes the flag so post-parse
/// accounting still uses ordinary pricing.
#[cfg(test)]
pub(in crate::gc) fn force_tiny_parse_pressure_due_for_test() {
    GC_TINY_PARSE_PRESSURE_TEST_FORCE.with(|cell| cell.set(true));
}

/// The in-use reading the tiny-parse guard compares against in this collector
/// mode: the generational collector's guard sits higher than the full
/// mark-sweep one because a minor is the cheaper collection to force.
pub(super) fn tiny_parse_in_use_trigger_for_mode() -> usize {
    if gen_gc_enabled() {
        gc_tiny_parse_in_use_trigger_dyn_bytes()
    } else {
        gc_tiny_parse_full_gc_in_use_trigger_dyn_bytes()
    }
}

/// `PERRY_GC_DIAG=1` witness that the guard is the arm that forced a
/// collection (CLAUDE.md: a gate must assert its subject was live). One line
/// per forced collection, tagged with which parse boundary asked for it.
pub(super) fn diag_tiny_parse_forced_collection(site: &str, in_use: usize) {
    if !crate::gc::diagnostic_hooks::enabled() {
        return;
    }
    let base = GC_TINY_PARSE_PRESSURE_BASE_BYTES.with(Cell::get);
    let step = GC_STEP_BYTES.with(Cell::get);
    // Medium-parse pacing (2026-09-14): the side-allocation arm's own inputs,
    // so a diag reader can tell which of the three arms priced this collection
    // rather than re-deriving it — the same "assert the subject was live" rule.
    let external = external_side_live_bytes();
    let external_base = GC_LAST_COLLECTION_EXTERNAL_SIDE_BYTES.with(Cell::get);
    eprintln!(
        "[gc-tiny-parse] forced collection site={} in_use={} base={} headroom={} step={} \
         external_side={} external_base={} external_band={}",
        site,
        in_use,
        base,
        tiny_parse_pressure_headroom_bytes(step),
        step,
        external,
        external_base,
        external_side_parse_band_bytes(external_base)
    );
}

pub(in crate::gc) fn gc_bump_arena_trigger_target(
    bytes_now: usize,
    step: usize,
    is_tiny_parse: bool,
) -> usize {
    let bytes_step = step.min(gc_trigger_absolute_ceiling_bytes());
    let target = bytes_now.saturating_add(bytes_step);
    if is_tiny_parse {
        target.min(gc_trigger_absolute_ceiling_bytes())
    } else {
        target
    }
}

/// Run a full collection that was armed by tiny JSON parse churn.
///
/// This is separate from the raise-only post-parse trigger bump. Full
/// mark-sweep needs the collection to happen before the next suppressed parse,
/// not immediately after the previous one, otherwise the parse result is still
/// rooted and every churn block looks partially live.
#[inline(always)]
pub fn gc_collect_pending_suppressed_parse() -> bool {
    if !GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(Cell::get) {
        return false;
    }
    gc_collect_pending_suppressed_parse_slow();
    true
}

#[cold]
#[optimize(size)]
#[inline(never)]
fn gc_collect_pending_suppressed_parse_slow() {
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(false));
    if GC_FLAGS.with(|f| f.get()) & (GC_FLAG_IN_ALLOC | GC_FLAG_SUPPRESSED) != 0
        || gc_blocked_by_unsafe_zone()
    {
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
        return;
    }
    // #9831: the request was priced when it was made; a collection of any kind
    // since then has moved the base, and re-pricing here is what keeps the
    // boundary collection from stacking a second minor on top of it. A request
    // nothing has satisfied is still due and still collects.
    let in_use = crate::arena::arena_in_use_bytes();
    let due = if gen_gc_enabled() {
        tiny_parse_generational_collection_due(in_use, tiny_parse_in_use_trigger_for_mode())
    } else {
        tiny_parse_pressure_due(in_use, tiny_parse_in_use_trigger_for_mode())
    };
    if !due {
        return;
    }
    diag_tiny_parse_forced_collection("parse_boundary", in_use);

    let total = crate::arena::arena_total_bytes();
    GC_NEXT_TRIGGER_BYTES.with(|trigger| {
        if trigger.get() > total {
            trigger.set(total);
            GC_TRIGGER_ARMED.with(|a| a.set(true));
        }
    });
    gc_check_trigger();
}

/// Schedule a collection for the next JSON.parse boundary.
///
/// Direct parse + stringify churn creates a full JS object graph, then walks it
/// immediately. If the arena trigger fires during that stringify, copied-minor
/// has to copy the just-parsed tree even though it dies at the end of the loop
/// body. Deferring the collection to the next parse boundary lets the caller's
/// loop-scope roots clear first, so the collector reclaims the previous tree
/// without promoting or repeatedly copying transient JSON data.
pub fn gc_schedule_parse_boundary_collection_if_pressure() {
    if !gen_gc_enabled() {
        return;
    }
    // #9831: priced the same way as the post-parse guard above — see
    // `tiny_parse_pressure_due_with` — plus the young generation's own cap.
    if !tiny_parse_generational_collection_due(
        crate::arena::arena_in_use_bytes(),
        gc_tiny_parse_in_use_trigger_dyn_bytes(),
    ) {
        return;
    }
    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
}

/// Schedule pressure work after a bounded tiny JSON object birth.
///
/// The pressure predicate walks arena block state. Empty and bounded-inline
/// JSON objects are at most a few slots, while its minimum growth headroom is
/// 16 MiB, so checking every 64 completions keeps the maximum delay to 63
/// bounded births and removes the heap walk from their steady-state path.
/// Pending work is still serviced at the start of every parse, before a new
/// result is born.
#[inline(always)]
pub fn gc_schedule_tiny_parse_boundary_collection_if_pressure() {
    #[cfg(not(test))]
    {
        let poll = GC_TINY_PARSE_BOUNDARY_POLL_REMAINING.with(|remaining| {
            let (next, poll) = tiny_parse_boundary_poll_next(remaining.get());
            remaining.set(next);
            poll
        });
        if !poll {
            return;
        }
    }
    // Tests poll immediately so the forced-pressure hook stays deterministic.
    gc_schedule_parse_boundary_collection_if_pressure();
}

#[inline(always)]
pub(in crate::gc) const fn tiny_parse_boundary_poll_next(remaining: u8) -> (u8, bool) {
    if remaining == 0 {
        (GC_TINY_PARSE_BOUNDARY_POLL_INTERVAL - 1, true)
    } else {
        (remaining - 1, false)
    }
}

/// The allocating parser has proved this construction fits the allowance.
/// Separate from the tiny-object completion hook so leaf parses pay no extra
/// nursery-policy reads on their hot path.
pub(in crate::gc) fn gc_schedule_json_construction_grace(in_use: usize) {
    if in_use >= scavenge_nursery_cap_dueness_bytes()
        // Numerical pressure only: young_scavenge_cap_due can run the initial
        // object census. Keep that heap walk at the later scheduler check.
        && (crate::arena::arena_total_bytes() >= next_arena_trigger_base()
            || crate::arena::copying_from_space_in_use_bytes()
                >= scavenge_nursery_cap_dueness_bytes())
        && super::json_defer::note_completed_parse(in_use)
    {
        defer_nursery_cap_to_precise_safepoint();
    }
}
