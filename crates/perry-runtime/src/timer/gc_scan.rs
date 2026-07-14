//! Incremental GC root scanning for the timer registries — extracted from
//! `timer.rs`, which had crossed the 2000-line size gate.
//!
//! The collector walks the timeout / callback / interval / mock timer tables in
//! bounded steps so a large timer population cannot stall a GC increment.

use super::*;

pub(crate) fn new_timer_root_scan_state() -> Box<dyn Any> {
    Box::<TimerRootScanState>::default()
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
            TIMER_SCAN_TIMEOUTS => scan_timeout_timers_step(visitor, state, remaining),
            TIMER_SCAN_CALLBACKS => scan_callback_timers_step(visitor, state, remaining),
            TIMER_SCAN_INTERVALS => scan_interval_timers_step(visitor, state, remaining),
            TIMER_SCAN_MOCK_CALLBACKS => scan_mock_timers_step(visitor, state, remaining, false),
            TIMER_SCAN_MOCK_INTERVALS => scan_mock_timers_step(visitor, state, remaining, true),
            TIMER_SCAN_DONE => true,
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
fn consume_timer_root_work(remaining: &mut usize) -> bool {
    if *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    true
}

fn scan_timeout_timers_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut TimerRootScanState,
    remaining: &mut usize,
) -> bool {
    let mut q = TIMER_QUEUE.lock().unwrap();
    while state.index < q.len() {
        let timer = &mut q[state.index];
        // #6185: the budgeted/incremental scan obeys the same ownership rule as
        // the stop-the-world one — never mark (and, on an evacuating cycle, never
        // rewrite) a slot in another agent's arena. Skip the entry wholesale,
        // advancing the scan state exactly as if it had been fully scanned.
        if !crate::agent::owns(timer.owner) {
            state.index += 1;
            state.finish_timer();
            continue;
        }
        while state.slot < 2 {
            if !consume_timer_root_work(remaining) {
                return false;
            }
            match state.slot {
                0 => visitor.visit_raw_mut_ptr_slot(&mut timer.promise),
                1 => visitor.visit_nanbox_f64_slot(&mut timer.value),
                _ => false,
            };
            state.slot += 1;
        }
        state.index += 1;
        state.finish_timer();
    }
    true
}

fn scan_callback_timers_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut TimerRootScanState,
    remaining: &mut usize,
) -> bool {
    let mut q = CALLBACK_TIMERS.lock().unwrap();
    while state.index < q.len() {
        let timer = &mut q[state.index];
        // #6185: the budgeted/incremental scan obeys the same ownership rule as
        // the stop-the-world one — never mark (and, on an evacuating cycle, never
        // rewrite) a slot in another agent's arena. Skip the entry wholesale,
        // advancing the scan state exactly as if it had been fully scanned.
        if !crate::agent::owns(timer.owner) {
            state.index += 1;
            state.finish_timer();
            continue;
        }
        if state.slot == 0 {
            if !consume_timer_root_work(remaining) {
                return false;
            }
            if !timer.cleared && timer.callback != 0 {
                visitor.visit_i64_slot(&mut timer.callback);
            }
            state.slot = 1;
        }
        if state.slot == 1 {
            while state.arg_index < timer.args.len() {
                if !consume_timer_root_work(remaining) {
                    return false;
                }
                visitor.visit_nanbox_f64_slot(&mut timer.args[state.arg_index]);
                state.arg_index += 1;
            }
            state.slot = 2;
            state.arg_index = 0;
        }
        if state.slot == 2 {
            if !crate::async_context::scan_snapshot_roots_mut_step(
                &mut timer.context,
                visitor,
                &mut state.context_entry,
                &mut state.context_store,
                remaining,
            ) {
                return false;
            }
            state.slot = 3;
            state.context_entry = 0;
            state.context_store = 0;
        }
        if state.slot == 3 {
            while state.arg_index < timer.args.len() {
                if !consume_timer_root_work(remaining) {
                    return false;
                }
                visitor.visit_nanbox_f64_slot(&mut timer.args[state.arg_index]);
                state.arg_index += 1;
            }
            state.slot = 4;
            state.arg_index = 0;
        }
        state.index += 1;
        state.finish_timer();
    }
    true
}

fn scan_interval_timers_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut TimerRootScanState,
    remaining: &mut usize,
) -> bool {
    let mut q = INTERVAL_TIMERS.lock().unwrap();
    while state.index < q.len() {
        let timer = &mut q[state.index];
        // #6185: the budgeted/incremental scan obeys the same ownership rule as
        // the stop-the-world one — never mark (and, on an evacuating cycle, never
        // rewrite) a slot in another agent's arena. Skip the entry wholesale,
        // advancing the scan state exactly as if it had been fully scanned.
        if !crate::agent::owns(timer.owner) {
            state.index += 1;
            state.finish_timer();
            continue;
        }
        if state.slot == 0 {
            if !consume_timer_root_work(remaining) {
                return false;
            }
            if !timer.cleared && timer.callback != 0 {
                visitor.visit_i64_slot(&mut timer.callback);
            }
            state.slot = 1;
        }
        if state.slot == 1 {
            if !crate::async_context::scan_snapshot_roots_mut_step(
                &mut timer.context,
                visitor,
                &mut state.context_entry,
                &mut state.context_store,
                remaining,
            ) {
                return false;
            }
            state.slot = 2;
        }
        state.index += 1;
        state.finish_timer();
    }
    true
}

// Step twin of the MOCK_TIMERS block in `scan_timer_roots_mut`. Cycle-based
// collections run only the step scanner, so before these phases existed,
// `node:test` mocked-timer callbacks/args/contexts reachable only through
// MOCK_TIMERS were swept while scheduled — `.tick()` then invoked a freed
// closure. One function serves both lists (distinct structs, same fields).
fn scan_mock_timers_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut TimerRootScanState,
    remaining: &mut usize,
    intervals: bool,
) -> bool {
    let mut guard = MOCK_TIMERS.lock().unwrap();
    macro_rules! scan_mock_list {
        ($list:expr) => {{
            while state.index < $list.len() {
                let timer = &mut $list[state.index];
                if state.slot == 0 {
                    if !consume_timer_root_work(remaining) {
                        return false;
                    }
                    if !timer.cleared && timer.callback != 0 {
                        visitor.visit_i64_slot(&mut timer.callback);
                    }
                    state.slot = 1;
                }
                if state.slot == 1 {
                    while state.arg_index < timer.args.len() {
                        if !consume_timer_root_work(remaining) {
                            return false;
                        }
                        visitor.visit_nanbox_f64_slot(&mut timer.args[state.arg_index]);
                        state.arg_index += 1;
                    }
                    state.slot = 2;
                    state.arg_index = 0;
                }
                if state.slot == 2 {
                    if !crate::async_context::scan_snapshot_roots_mut_step(
                        &mut timer.context,
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
        }};
    }
    if intervals {
        scan_mock_list!(guard.intervals)
    } else {
        scan_mock_list!(guard.callbacks)
    }
    true
}
