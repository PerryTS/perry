//! The malloc-count trigger: its adaptive step, the bump a suppressed window
//! or JSON output pays at its end, and the sweep JSON output schedules.

use super::*;

/// Initial step for the malloc-count-based GC trigger. Adaptive: doubles
/// when >75% of malloc objects are garbage (loop-scoped temporaries),
/// halves when <25% are garbage (large live set). Capped at
/// `GC_MALLOC_COUNT_STEP_MAX` to bound memory between collections.
///
/// Originally a single hardcoded threshold (`GC_MALLOC_COUNT_THRESHOLD`);
/// issue #34 showed that triggering GC from `gc_malloc` (needed for
/// malloc-heavy workloads that don't push arena blocks — e.g.
/// @perry/postgres's `parseBigIntDecimal` bigint chain) combined with a
/// hardcoded threshold would thrash for any program whose live set
/// exceeded the threshold. Making it a per-cycle step fixes that.
///
/// Issue #58: the constant 10k step caused ~100 GC cycles for 500k-iter
/// string-concat loops where almost every object is dead. Adaptive
/// doubling ramps the step to 160k+ after a few mostly-garbage sweeps,
/// cutting GC cycles from ~100 to ~10.
pub(in crate::gc) const GC_MALLOC_COUNT_STEP_INITIAL: usize = 100_000;
pub(in crate::gc) const GC_MALLOC_COUNT_STEP_MAX: usize = 2_000_000;
pub(in crate::gc) const GC_MALLOC_COUNT_STEP_MIN: usize = 10_000;

/// Arrange for the next ordinary safepoint to sweep malloc-tracked objects.
///
/// Large JSON results use malloc-backed string storage so a discarded leaf can
/// be reclaimed by a generational malloc sweep instead of forcing a full trace
/// of the unchanged input graph. The caller invokes this only after the result
/// is completely allocated; no collection runs in the stringify boundary.
pub(crate) fn gc_schedule_malloc_sweep_after_json_output() {
    let current = malloc_object_count();
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(trigger.get().min(current)));
}

pub(crate) enum JsonOutputSweep {
    Pending,
    CompletedBeforeBoundary,
    CompletedAtBoundary,
}

/// Service completed JSON-output debt at a rooted input boundary. Distinguish
/// an earlier deferred completion so JSON can carry forward output bytes
/// produced after that request instead of dropping an extra buffer's debt.
pub(crate) fn gc_service_json_output_sweep() -> JsonOutputSweep {
    let was_due = malloc_object_count() >= GC_NEXT_MALLOC_TRIGGER.with(TriggerInput::get);
    gc_check_trigger();
    if malloc_object_count() >= GC_NEXT_MALLOC_TRIGGER.with(TriggerInput::get) {
        JsonOutputSweep::Pending
    } else if was_due {
        JsonOutputSweep::CompletedAtBoundary
    } else {
        JsonOutputSweep::CompletedBeforeBoundary
    }
}

crate::perry_thread_local! {
    /// Per-program adaptive malloc-count step. Mirrors `GC_STEP_BYTES`
    /// behaviour: doubles when mostly-garbage, halves when mostly-live.
    pub(in crate::gc) static GC_MALLOC_COUNT_STEP: std::cell::Cell<usize> =
        const { std::cell::Cell::new(GC_MALLOC_COUNT_STEP_INITIAL) };
}

/// Rebaseline the malloc-count AND arena-bytes triggers to the current
/// live set so that objects just created during a GC-suppressed window
/// (e.g. JSON.parse) don't immediately trip a collection on the next
/// allocation.
///
/// Pre-fix: only the malloc-count trigger was bumped. JSON.parse on the
/// 108 MB honest_bench fixture lifts arena_total to ~108 MB, the bytes
/// trigger is still at its initial 128 MB threshold, and the iterate+
/// rebuild pass that immediately follows trips bytes-based GC after
/// only ~20 MB of new allocations. The 4 mark/sweep cycles each walk
/// the entire 400 MB live heap (the records tree dominates) and add
/// ~800 ms of overhead to the workload. Bumping the bytes trigger by
/// the per-program step (initially 128 MB, grows up to 1 GB on
/// mostly-garbage sweep evidence) defers the first GC until the
/// post-parse working set itself doubles — for json_pipeline_full
/// that means iterate+rebuild completes inside one GC cycle instead
/// of four.
pub fn gc_bump_malloc_trigger() {
    gc_bump_malloc_trigger_inner(true);
}

/// JSON construction has finished: account for pressure without collecting
/// the newly built result before it can return to its caller.
pub(crate) fn gc_bump_json_malloc_trigger_deferred() {
    gc_bump_malloc_trigger_inner(false);
}

fn gc_bump_malloc_trigger_inner(collect_now: bool) {
    let current = MALLOC_STATE.with(|s| s.borrow().objects.len());
    use crate::arena::arena_total_bytes;
    let bytes_now = arena_total_bytes();
    let is_tiny_parse = gc_bump_malloc_trigger_with_snapshot(current, bytes_now);
    if is_tiny_parse {
        let use_gen_gc = gen_gc_enabled();
        // #9831: an absolute in-use guard alone forced a collection after
        // EVERY tiny parse on a program whose live set never drops below it.
        // The guard now also requires the arena to have grown past the
        // productivity-priced headroom since the last collection ended.
        let in_use = crate::arena::arena_in_use_bytes();
        let due = if use_gen_gc {
            tiny_parse_generational_collection_due(in_use, tiny_parse_in_use_trigger_for_mode())
        } else {
            tiny_parse_pressure_due(in_use, tiny_parse_in_use_trigger_for_mode())
        };
        if !due {
            return;
        }
        if use_gen_gc {
            if gc_blocked_by_unsafe_zone() {
                GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
                return;
            }
            GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
            diag_tiny_parse_forced_collection("parse_end", in_use);
            GC_NEXT_TRIGGER_BYTES.with(|trigger| {
                if trigger.get() > bytes_now {
                    trigger.set(bytes_now);
                    GC_TRIGGER_ARMED.with(|a| a.set(true));
                }
            });
            if collect_now {
                gc_check_trigger();
            }
        } else {
            crate::arena::arena_start_fresh_general_block();
            GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
        }
    }
}
