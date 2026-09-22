//! turnloop P3: `node:test` mock timers.
//!
//! A separate, virtual-clock implementation of the timer APIs: `MockTimers`
//! replaces the real queues wholesale while enabled, so it keeps its own
//! `Vec`-backed state and never touches the agent timer store.

use super::gc_scan::{consume_timer_root_work, TimerRootScanState};
use super::ref_states::ScheduledTimerId;
use super::{
    call_timer_callback, next_timer_id, normalize_timer_delay, record_timer_ref_state,
    CallbackTimerKind,
};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MOCK_TIMERS_API_DATE: u32 = 1 << 0;
pub const MOCK_TIMERS_API_SET_TIMEOUT: u32 = 1 << 1;
pub const MOCK_TIMERS_API_SET_INTERVAL: u32 = 1 << 2;
pub const MOCK_TIMERS_API_SET_IMMEDIATE: u32 = 1 << 3;
pub const MOCK_TIMERS_ALL_APIS: u32 = MOCK_TIMERS_API_DATE
    | MOCK_TIMERS_API_SET_TIMEOUT
    | MOCK_TIMERS_API_SET_INTERVAL
    | MOCK_TIMERS_API_SET_IMMEDIATE;

struct MockCallbackTimer {
    id: i64,
    kind: CallbackTimerKind,
    due_ms: f64,
    callback: i64,
    args: Vec<f64>,
    context: crate::async_context::AsyncContextSnapshot,
    cleared: bool,
    /// #10447: pins this id in the ref-state registry for as long as the entry
    /// is queued. Not `Clone` — dropping it retires the id — which is why this
    /// struct is not `Clone` either.
    _scheduled: ScheduledTimerId,
}

unsafe impl Send for MockCallbackTimer {}

struct MockIntervalTimer {
    id: i64,
    callback: i64,
    interval_ms: u64,
    next_ms: f64,
    args: Vec<f64>,
    context: crate::async_context::AsyncContextSnapshot,
    cleared: bool,
    /// See `MockCallbackTimer::_scheduled`.
    _scheduled: ScheduledTimerId,
}

unsafe impl Send for MockIntervalTimer {}

struct MockTimersState {
    enabled: bool,
    apis: u32,
    current_ms: f64,
    callbacks: Vec<MockCallbackTimer>,
    intervals: Vec<MockIntervalTimer>,
}

static MOCK_TIMERS: Mutex<MockTimersState> = Mutex::new(MockTimersState {
    enabled: false,
    apis: 0,
    current_ms: 0.0,
    callbacks: Vec::new(),
    intervals: Vec::new(),
});

pub(super) fn throw_mock_timer_invalid_state(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_INVALID_STATE");
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(super) fn ensure_mock_timers_enabled() {
    if !MOCK_TIMERS.lock().unwrap().enabled {
        throw_mock_timer_invalid_state(
            "Invalid state: You should enable MockTimers first by calling the .enable function",
        );
    }
}

pub fn js_mock_timers_real_now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

pub fn js_mock_timers_date_now() -> Option<f64> {
    let state = MOCK_TIMERS.lock().unwrap();
    (state.enabled && (state.apis & MOCK_TIMERS_API_DATE) != 0).then_some(state.current_ms)
}

pub fn js_mock_timers_enable(apis: u32, now_ms: f64) {
    let mut state = MOCK_TIMERS.lock().unwrap();
    if state.enabled {
        throw_mock_timer_invalid_state("Invalid state: MockTimers is already enabled!");
    }
    state.enabled = true;
    state.apis = apis;
    state.current_ms = now_ms;
    state.callbacks.clear();
    state.intervals.clear();
}

pub fn js_mock_timers_reset() {
    let mut state = MOCK_TIMERS.lock().unwrap();
    state.enabled = false;
    state.apis = 0;
    state.current_ms = 0.0;
    state.callbacks.clear();
    state.intervals.clear();
}

pub fn js_mock_timers_set_time(now_ms: f64) {
    ensure_mock_timers_enabled();
    MOCK_TIMERS.lock().unwrap().current_ms = now_ms;
}

pub fn js_mock_timers_tick(ms: f64) {
    ensure_mock_timers_enabled();
    let target = {
        let state = MOCK_TIMERS.lock().unwrap();
        state.current_ms + ms
    };
    mock_timers_advance_to(target);
}

pub fn js_mock_timers_run_all() {
    ensure_mock_timers_enabled();
    let longest_due = {
        let state = MOCK_TIMERS.lock().unwrap();
        state
            .callbacks
            .iter()
            .filter(|timer| !timer.cleared)
            .map(|timer| timer.due_ms)
            .chain(
                state
                    .intervals
                    .iter()
                    .filter(|timer| !timer.cleared)
                    .map(|timer| timer.next_ms),
            )
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    };
    if let Some(target) = longest_due {
        mock_timers_advance_to(target);
    }
}

pub(super) fn schedule_mock_callback_timer(
    callback: i64,
    delay_ms: f64,
    args: Vec<f64>,
    kind: CallbackTimerKind,
) -> Option<i64> {
    let api = match kind {
        CallbackTimerKind::Timeout => MOCK_TIMERS_API_SET_TIMEOUT,
        CallbackTimerKind::Immediate => MOCK_TIMERS_API_SET_IMMEDIATE,
    };
    let mut state = MOCK_TIMERS.lock().unwrap();
    if !state.enabled || (state.apis & api) == 0 {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle =
        scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader);
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let delay = normalize_timer_delay(delay_ms);
    let id = next_timer_id();
    // #10447: the mock schedules a real id, so it pins it too. The guard goes
    // onto the queue entry below — a local `let _scheduled` would drop (and
    // retire the id) at the end of THIS function, before the timer ever fires.
    let scheduled = super::ref_states::register_scheduled_timer(id);
    let due_ms = state.current_ms + delay as f64;
    // `capture_context` allocates, and a struct literal evaluates its fields in
    // source order — reading the closure pointer first would leave a stale
    // address in `callback` if the capture moved it. `across_const` pairs the
    // allocating call with the reload.
    let (context, callback) = callback_handle
        .across_const::<crate::closure::ClosureHeader, _>(crate::async_context::capture_context);
    state.callbacks.push(MockCallbackTimer {
        id,
        kind,
        due_ms,
        callback: callback as i64,
        args: crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles),
        context,
        cleared: false,
        _scheduled: scheduled,
    });
    record_timer_ref_state(id, true);
    Some(id)
}

pub(super) fn schedule_mock_interval_timer(
    callback: i64,
    interval_ms: f64,
    args: Vec<f64>,
) -> Option<i64> {
    let mut state = MOCK_TIMERS.lock().unwrap();
    if !state.enabled || (state.apis & MOCK_TIMERS_API_SET_INTERVAL) == 0 {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle =
        scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader);
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let interval = normalize_timer_delay(interval_ms);
    let id = next_timer_id();
    // #10447: see `schedule_mock_callback_timer` — the guard rides the entry.
    let scheduled = super::ref_states::register_scheduled_timer(id);
    let next_ms = state.current_ms + interval as f64;
    // See `schedule_mock_callback_timer`: the capture allocates, so the closure
    // pointer is reloaded after it rather than read before.
    let (context, callback) = callback_handle
        .across_const::<crate::closure::ClosureHeader, _>(crate::async_context::capture_context);
    state.intervals.push(MockIntervalTimer {
        id,
        callback: callback as i64,
        interval_ms: interval,
        next_ms,
        args: crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles),
        context,
        cleared: false,
        _scheduled: scheduled,
    });
    record_timer_ref_state(id, true);
    Some(id)
}

pub(super) fn mock_timers_advance_to(target_ms: f64) {
    loop {
        let action = {
            let mut state = MOCK_TIMERS.lock().unwrap();
            state.callbacks.retain(|timer| !timer.cleared);
            state.intervals.retain(|timer| !timer.cleared);

            let mut best: Option<(f64, i64, bool, usize)> = None;
            for (idx, timer) in state.callbacks.iter().enumerate() {
                if timer.due_ms <= target_ms {
                    let candidate = (timer.due_ms, timer.id, false, idx);
                    if best
                        .is_none_or(|current| (candidate.0, candidate.1) < (current.0, current.1))
                    {
                        best = Some(candidate);
                    }
                }
            }
            for (idx, timer) in state.intervals.iter().enumerate() {
                if timer.next_ms <= target_ms {
                    let candidate = (timer.next_ms, timer.id, true, idx);
                    if best
                        .is_none_or(|current| (candidate.0, candidate.1) < (current.0, current.1))
                    {
                        best = Some(candidate);
                    }
                }
            }

            let Some((due_ms, _id, is_interval, idx)) = best else {
                state.current_ms = target_ms;
                return;
            };
            state.current_ms = due_ms;
            if is_interval {
                // The interval entry stays in the queue (it re-fires), so its
                // `_scheduled` pin is untouched here — nothing to carry.
                let timer = &mut state.intervals[idx];
                timer.next_ms = due_ms + timer.interval_ms.max(1) as f64;
                Some((
                    timer.id,
                    timer.callback,
                    timer.args.clone(),
                    timer.context.clone(),
                    None,
                ))
            } else {
                // #10447 follow-up: `remove` takes the WHOLE entry, including
                // its `_scheduled` pin. Move that pin into the action too and
                // hand it back below, instead of leaving it behind on `timer`
                // to drop (and retire the id) right here — before
                // `call_timer_callback` has even run, let alone finished. A
                // one-shot mock timer otherwise loses its own registry entry
                // if its callback churns more than the eviction cap's worth of
                // other timers while it is still dispatching.
                let timer = state.callbacks.remove(idx);
                Some((
                    timer.id,
                    timer.callback,
                    timer.args,
                    timer.context,
                    Some(timer._scheduled),
                ))
            }
        };
        if let Some((id, callback, args, context, _pin)) = action {
            call_timer_callback(id, callback, &args, &context);
            // `_pin` (the one-shot case's `ScheduledTimerId`, moved out of the
            // popped queue entry above) stays alive across that call and only
            // retires the id here, after the callback has returned.
        }
    }
}

pub(super) fn mock_clear_timeout(timer_id: i64) {
    let mut state = MOCK_TIMERS.lock().unwrap();
    for timer in state.callbacks.iter_mut() {
        if timer.id == timer_id && timer.kind == CallbackTimerKind::Timeout {
            timer.cleared = true;
        }
    }
    for timer in state.intervals.iter_mut() {
        if timer.id == timer_id {
            timer.cleared = true;
        }
    }
    state.callbacks.retain(|timer| !timer.cleared);
    state.intervals.retain(|timer| !timer.cleared);
}

pub(super) fn mock_clear_interval(timer_id: i64) {
    let mut state = MOCK_TIMERS.lock().unwrap();
    for timer in state.intervals.iter_mut() {
        if timer.id == timer_id {
            timer.cleared = true;
        }
    }
    for timer in state.callbacks.iter_mut() {
        if timer.id == timer_id && timer.kind == CallbackTimerKind::Timeout {
            timer.cleared = true;
        }
    }
    state.callbacks.retain(|timer| !timer.cleared);
    state.intervals.retain(|timer| !timer.cleared);
}

pub(super) fn mock_clear_immediate(timer_id: i64) {
    let mut state = MOCK_TIMERS.lock().unwrap();
    for timer in state.callbacks.iter_mut() {
        if timer.id == timer_id && timer.kind == CallbackTimerKind::Immediate {
            timer.cleared = true;
        }
    }
    state.callbacks.retain(|timer| !timer.cleared);
}

// ── GC roots and accounting for the mock tables ─────────────────────────────

pub(super) fn scan_mock_timers_step(
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

/// Queued mock `setTimeout`/`setInterval` handles — counted into
/// `process.getActiveResourcesInfo()` alongside the real store's.
pub(super) fn mock_timeout_resource_count() -> usize {
    let state = MOCK_TIMERS.lock().unwrap();
    state
        .callbacks
        .iter()
        .filter(|timer| !timer.cleared && timer.kind == CallbackTimerKind::Timeout)
        .count()
        + state
            .intervals
            .iter()
            .filter(|timer| !timer.cleared)
            .count()
}
