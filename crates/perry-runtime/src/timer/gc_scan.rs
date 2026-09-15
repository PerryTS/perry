//! Incremental GC root scanning for the agent timer store and the mock timers.
//!
//! The collector walks the store in bounded steps so a large timer population
//! cannot stall a GC increment.
//!
//! P3 made the walk a **slab index walk**. The old scan indexed into three
//! `Vec` queues, which every schedule and every cancel reordered underneath it:
//! resuming a partial scan after a `retain` could revisit or skip an entry.
//! Slab indices are stable — an entry keeps its index for its whole life and a
//! freed slot reads as empty — so a resumed increment lands where it left off.
//!
//! Ownership needs no filter here: `store::with_current` selects the calling
//! agent's partition, so a foreign agent's slots are not reachable from this
//! scan at all. That is the same rule #6185 enforced with an owner tag, now
//! structural.

use super::*;

pub(crate) fn new_timer_root_scan_state() -> Box<dyn Any> {
    Box::<TimerRootScanState>::default()
}

/// Scan every root the timer store holds for this agent, in one pass.
pub fn scan_timer_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut state = TimerRootScanState::default();
    let mut remaining = usize::MAX;
    while !scan_timer_roots_mut_step(visitor, &mut state, &mut remaining) {}
}

pub(crate) fn scan_timer_roots_mut_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut dyn Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<TimerRootScanState>()
        .expect("timer root scanner state type");
    while state.phase != TIMER_SCAN_DONE {
        let done = match state.phase {
            TIMER_SCAN_STORE => scan_store_step(visitor, state, remaining),
            TIMER_SCAN_MOCK_CALLBACKS => {
                super::mock::scan_mock_timers_step(visitor, state, remaining, false)
            }
            TIMER_SCAN_MOCK_INTERVALS => {
                super::mock::scan_mock_timers_step(visitor, state, remaining, true)
            }
            _ => true,
        };
        if !done {
            return false;
        }
        state.advance_to(state.phase.saturating_add(1));
    }
    true
}

#[inline]
pub(super) fn consume_timer_root_work(remaining: &mut usize) -> bool {
    if *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    true
}

/// Visit one slab entry's roots, resuming at `state.slot` / `state.arg_index`.
///
/// Returns false when the increment ran out of budget mid-entry; the state then
/// names exactly the slot to resume at.
fn scan_store_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut TimerRootScanState,
    remaining: &mut usize,
) -> bool {
    store::with_current(|timers| {
        while state.index < timers.slab_len() {
            let Some(entry) = timers.slab_entry_mut(state.index) else {
                state.index += 1;
                state.finish_timer();
                continue;
            };
            if entry.class == store::Class::Promise {
                while state.slot < 2 {
                    if !consume_timer_root_work(remaining) {
                        return false;
                    }
                    match state.slot {
                        0 => visitor.visit_raw_mut_ptr_slot(&mut entry.promise),
                        1 => visitor.visit_nanbox_f64_slot(&mut entry.value),
                        _ => false,
                    };
                    state.slot += 1;
                }
                state.index += 1;
                state.finish_timer();
                continue;
            }
            if state.slot == 0 {
                if !consume_timer_root_work(remaining) {
                    return false;
                }
                if entry.callback != 0 {
                    visitor.visit_i64_slot(&mut entry.callback);
                }
                state.slot = 1;
            }
            if state.slot == 1 {
                while state.arg_index < entry.args.len() {
                    if !consume_timer_root_work(remaining) {
                        return false;
                    }
                    visitor.visit_nanbox_f64_slot(&mut entry.args[state.arg_index]);
                    state.arg_index += 1;
                }
                state.slot = 2;
                state.arg_index = 0;
            }
            if state.slot == 2 {
                if !crate::async_context::scan_snapshot_roots_mut_step(
                    &mut entry.context,
                    visitor,
                    &mut state.context_entry,
                    &mut state.context_store,
                    remaining,
                ) {
                    return false;
                }
                state.slot = 3;
            }
            state.index += 1;
            state.finish_timer();
        }
        true
    })
}

pub(super) const TIMER_SCAN_STORE: u8 = 0;
pub(super) const TIMER_SCAN_MOCK_CALLBACKS: u8 = 1;
pub(super) const TIMER_SCAN_MOCK_INTERVALS: u8 = 2;
pub(super) const TIMER_SCAN_DONE: u8 = 3;

/// Where an incremental timer-root scan is up to: which table, which entry in
/// it, which slot of that entry, and how far into the entry's arguments and
/// async-context snapshot.
#[derive(Default)]
pub(super) struct TimerRootScanState {
    pub(super) phase: u8,
    pub(super) index: usize,
    pub(super) slot: u8,
    pub(super) arg_index: usize,
    pub(super) context_entry: usize,
    pub(super) context_store: usize,
}

impl TimerRootScanState {
    fn advance_to(&mut self, phase: u8) {
        self.phase = phase;
        self.index = 0;
        self.finish_timer();
    }

    pub(super) fn finish_timer(&mut self) {
        self.slot = 0;
        self.arg_index = 0;
        self.context_entry = 0;
        self.context_store = 0;
    }
}
