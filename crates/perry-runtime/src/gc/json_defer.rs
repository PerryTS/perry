//! A bounded lifetime grace period for a completed JSON parse.
//!
//! This is scheduling state, not a root or an ownership scheme. It holds no
//! heap pointers and never changes tracing, barriers, or object lifetimes.
//! Nursery pressure may wait for two loop safepoints or at most 8 MiB of
//! additional arena high-water space (less on small heaps). Repeated parses
//! share one allowance; only a completed collection makes another available.
//! A suppressed construction can overshoot the allowance by that document's
//! allocation, just as it can overshoot the existing collection triggers.

use std::cell::Cell;

const MAX_EXTRA_BYTES: usize = 8 * 1024 * 1024;
const MAX_SAFEPOINTS: u8 = 2;
const MIN_INPUT_BYTES: usize = 256 * 1024;

/// Only document-sized construction needs a lifetime grace period. Small
/// parses keep their existing fast scheduling path. Measure occupied bytes,
/// including allocations served by already-reserved blocks, rather than
/// treating block reservations as the amount this parse allocated.
pub(crate) struct JsonParseAllocation(Option<usize>);

impl JsonParseAllocation {
    #[inline]
    pub(crate) fn begin(input_bytes: usize) -> Self {
        if input_bytes < MIN_INPUT_BYTES
            || !super::gen_gc_enabled()
            || !super::gc_moving_loop_polls_enabled()
            || super::gc_budgeted_cycle_active()
        {
            return Self(None);
        }
        Self(Some(crate::arena::arena_live_allocated_bytes()))
    }

    #[inline]
    pub(crate) fn finish(self) {
        let Some(before) = self.0 else { return };
        let now = crate::arena::arena_live_allocated_bytes();
        if now.saturating_sub(before) > extra_bytes_for_budget(super::gc_heap_budget_bytes()) {
            // A large retained graph can itself exceed the allowance. Giving
            // it another whole parse before collection raised both CPU and
            // residency in the first measured implementation.
            return;
        }
        super::policy::gc_schedule_json_construction_grace(crate::arena::arena_in_use_bytes());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Deferral {
    Available,
    Waiting { limit_bytes: usize, polls_left: u8 },
    Spent,
}

crate::perry_thread_local! {
    // Byte counts and scheduling flags only; no managed values.
    static JSON_DEFERRAL: Cell<Deferral> = const { Cell::new(Deferral::Available) };
}

fn extra_bytes_for_budget(budget: Option<usize>) -> usize {
    budget.map_or(MAX_EXTRA_BYTES, |bytes| MAX_EXTRA_BYTES.min(bytes / 32))
}

/// Called only when a completed parse has ordinary nursery pressure due.
/// Returning true asks the caller to arm the existing precise safepoint poll.
pub(super) fn note_completed_parse(in_use_bytes: usize) -> bool {
    JSON_DEFERRAL.with(|state| match state.get() {
        Deferral::Available => {
            let extra = extra_bytes_for_budget(super::gc_heap_budget_bytes());
            if extra == 0 {
                state.set(Deferral::Spent);
                return false;
            }
            state.set(Deferral::Waiting {
                limit_bytes: in_use_bytes.saturating_add(extra),
                polls_left: MAX_SAFEPOINTS,
            });
            true
        }
        Deferral::Waiting { .. } => true,
        Deferral::Spent => false,
    })
}

/// Nursery-only callers ask this after their normal safety guards. Allocation
/// checks consume bytes; precise safepoints also consume the poll allowance.
/// Manual, old-generation, emergency and seeded collections bypass this gate.
pub(super) fn should_defer(at_safepoint: bool) -> bool {
    JSON_DEFERRAL.with(|state| {
        let Deferral::Waiting {
            limit_bytes,
            polls_left,
        } = state.get()
        else {
            return false;
        };
        if polls_left == 0 || crate::arena::arena_in_use_bytes() >= limit_bytes {
            state.set(Deferral::Spent);
            return false;
        }
        if at_safepoint {
            state.set(Deferral::Waiting {
                limit_bytes,
                polls_left: polls_left - 1,
            });
        }
        true
    })
}

/// OS pressure cancels the allowance even if the warning arrives at a point
/// where collection itself must wait. Further parses cannot renew it.
pub(super) fn cancel_until_collection() {
    JSON_DEFERRAL.with(|state| state.set(Deferral::Spent));
}

/// The collection-accounting funnel covers moving, full, manual and budgeted
/// completion alike. Reset here, rather than guessing from falling occupancy.
pub(super) fn collection_completed() {
    JSON_DEFERRAL.with(|state| state.set(Deferral::Available));
}

#[cfg(test)]
#[path = "tests/json_defer.rs"]
mod tests;
