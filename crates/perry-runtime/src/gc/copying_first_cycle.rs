//! The first copying minor's promotion attempt, and its rollback (#7937).
//!
//! The steady-state in-place promotion policy (`gc::promote_in_place`) reads
//! the PREVIOUS cycle's measured young-survival ratio, so it always declines on
//! the first copying minor of a thread — and on the fully-live workloads that
//! one cycle was 58–81% of all GC pause.
//!
//! Cycle 0 instead ATTEMPTS the promotion and decides afterwards, from the
//! ratio its own trace just measured. That is possible because a promoting
//! cycle's trace IS a mark pass over the blocks it would keep:
//! `retag_young_for_in_place_promotion` runs before the trace, after which no
//! address in the heap classifies as `Nursery`, so `move_young` is unreachable
//! and the Cheney pass degenerates into marking.
//!
//! If the ratio disagrees, `gc::copying`'s decision point rolls the attempt
//! back — restoring the retag and clearing the marks, which is the whole of the
//! commitment — and this wrapper re-runs the cycle as an ordinary copying
//! minor.

use super::copying::{run_copied_minor_attempt, CopiedMinorEligibility};
use super::*;

/// How a copied-minor attempt ended. `RolledBack` is only ever produced by the
/// speculative first-cycle promotion, and it means the heap is in exactly the
/// state it was in when the attempt began.
pub(super) enum CopiedMinorAttempt {
    Done(Option<CopiedMinorFastPathOutcome>),
    RolledBack,
}

pub(super) fn gc_collect_minor_copying_fast_path_with_eligibility(
    trace: &mut Option<GcCycleTrace>,
    start: Instant,
    eligibility: CopiedMinorEligibility,
    trigger_kind: GcTriggerKind,
) -> Option<CopiedMinorFastPathOutcome> {
    match run_copied_minor_attempt(trace, start, eligibility, trigger_kind, true) {
        CopiedMinorAttempt::Done(outcome) => outcome,
        // The first-cycle attempt read its own trace and the ratio said
        // evacuate. The rollback restored the pre-cycle heap, so re-deriving
        // eligibility observes what the first attempt did and this is an
        // ordinary copying minor. Re-derived rather than reused because
        // `CopiedMinorEligibility` owns the pointer classifier the first
        // attempt consumed; the cost is one more preflight, on a cycle that by
        // construction has almost nothing live to walk.
        CopiedMinorAttempt::RolledBack => {
            let eligibility = CopiedMinorEligibility::evaluate(trigger_kind);
            match run_copied_minor_attempt(trace, start, eligibility, trigger_kind, false) {
                CopiedMinorAttempt::Done(outcome) => outcome,
                CopiedMinorAttempt::RolledBack => {
                    unreachable!("a non-speculative copied-minor attempt cannot roll back")
                }
            }
        }
    }
}
