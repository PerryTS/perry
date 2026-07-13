//! Agent-ownership bookkeeping for the timer queues (#6185 Tier 2).
//!
//! The queues in `timer.rs` are process-global, but every pointer they hold —
//! promise, callback closure, NaN-boxed arg — belongs to the arena of the agent
//! that scheduled the timer. `timer.rs` enforces that at every read (tick,
//! next-deadline, liveness, active-handle count) and `gc_scan.rs` enforces it in
//! the collector. This module owns the two pieces that are purely about
//! ownership: per-agent event-loop liveness, and what happens to an agent's
//! timers when the agent itself goes away.

use super::{timer_has_ref_state, CALLBACK_TIMERS, INTERVAL_TIMERS, TIMER_QUEUE};

// ── Per-agent event-loop liveness ────────────────────────────────────────────
//
// #6185: a timer owned by another agent can never be fired by this one, so it
// must not keep this agent's event loop spinning. Pre-fix these counted every
// timer on the process-global queues, so one agent's pending work kept every
// other agent's loop alive.

pub(super) fn has_refed_promise_timer() -> bool {
    TIMER_QUEUE
        .lock()
        .unwrap()
        .iter()
        .any(|timer| timer.has_ref && crate::agent::owns(timer.owner))
}

pub(super) fn has_refed_callback_timer() -> bool {
    CALLBACK_TIMERS.lock().unwrap().iter().any(|timer| {
        !timer.cleared && crate::agent::owns(timer.owner) && timer_has_ref_state(timer.id)
    })
}

pub(super) fn has_refed_interval_timer() -> bool {
    INTERVAL_TIMERS.lock().unwrap().iter().any(|timer| {
        !timer.cleared && crate::agent::owns(timer.owner) && timer_has_ref_state(timer.id)
    })
}

/// Drop every timer owned by `agent`. Called from `crate::agent::retire_agent`
/// when a `perry/thread` worker exits.
///
/// A timer scheduled inside a worker can never legally fire: the worker has no
/// event loop of its own (Tier 1, #6276, rejects `await` in a worker body, so it
/// cannot pump), and no other agent may run its callback — the closure lives in
/// the worker's arena, which is unmapped at exit. Pre-#6185 such a timer *did*
/// fire, on whichever thread pumped next, dereferencing that freed arena.
///
/// Dropping the entry is therefore the honest end state, not a loss of work: it
/// is what "the thread ended before the timer came due" already meant. Purging
/// is safe precisely because nothing else can own these entries — no other
/// thread may dereference pointers into this agent's arena.
pub(crate) fn purge_agent_timers(agent: crate::agent::AgentId) {
    TIMER_QUEUE.lock().unwrap().retain(|t| t.owner != agent);
    CALLBACK_TIMERS.lock().unwrap().retain(|t| t.owner != agent);
    INTERVAL_TIMERS.lock().unwrap().retain(|t| t.owner != agent);
}
