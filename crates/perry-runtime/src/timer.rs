//! Timer support for `setTimeout` / `setInterval` / `setImmediate`.
//!
//! turnloop P3 replaced the three process-global `Mutex<Vec<_>>` queues this
//! module used to keep with **one store per JS agent** (`timer/store.rs`): a
//! slab plus two `(deadline, seq)` min-heaps and a FIFO check queue. What that
//! removed:
//!
//! - **the full-queue scans.** Every tick, deadline computation and liveness
//!   question walked all three queues filtering on owner, `cleared` and ref
//!   state. Insert, cancel and expiry are now O(log n), and the earliest
//!   deadline is the heap root.
//! - **the owner filter.** Partitioning by agent *is* the filter: `owns(owner)`
//!   is exactly `owner == current_agent()` (`agent.rs`), so selecting the
//!   calling agent's partition answers the same question structurally. Android's
//!   split — TypeScript on the `perry-native` thread, the pump on the UI thread
//!   — still works, because both resolve to `PRIMARY_AGENT`.
//! - **the detached expiry batch.** The phases (`timer/phases.rs`) pop one entry
//!   at a time, so #8036's batch-wide rooting is gone and a `clearTimeout` from
//!   inside a sibling's callback cancels its target — which is what Node does
//!   (measured against 26.5.1).
//! - **the tombstones.** A cancelled entry leaves the heap immediately
//!   (DESIGN D6), rather than surviving as a `cleared` flag until the next scan.
//!
//! The store is still process-global and `Mutex`-protected rather than
//! thread-local, because a pump thread acting for an agent must reach that
//! agent's timers (the Android case above).
//!
//! Phase ownership: this module schedules and cancels; `timer/phases.rs` runs
//! the timers and check phases in Node's order.

mod async_lifecycle;
mod mock;
mod phases;
mod store;

use crate::promise::{js_promise_new, Promise};
use async_lifecycle::enqueue_destroy_ids;
use mock::{
    mock_clear_immediate, mock_clear_interval, mock_clear_timeout, schedule_mock_callback_timer,
    schedule_mock_interval_timer,
};
use std::any::Any;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};
use store::{Class, Entry};

pub use mock::{
    js_mock_timers_date_now, js_mock_timers_enable, js_mock_timers_real_now_ms,
    js_mock_timers_reset, js_mock_timers_run_all, js_mock_timers_set_time, js_mock_timers_tick,
    MOCK_TIMERS_ALL_APIS, MOCK_TIMERS_API_DATE, MOCK_TIMERS_API_SET_IMMEDIATE,
    MOCK_TIMERS_API_SET_INTERVAL, MOCK_TIMERS_API_SET_TIMEOUT,
};

extern "C" {
    fn js_stdlib_has_active_handles() -> i32;
}

static START_TIME: Mutex<Option<Instant>> = Mutex::new(None);

// Opt-in event-path counters printed with `PERRY_MT_PROFILE=1`. Keeping these
// beside the store makes registrations, phase runs, and firings independently
// visible instead of asking a sampling profiler to catch sub-microsecond work.
pub static PROFILE_PROMISE_TIMER_REGISTRATIONS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_CALLBACK_TIMER_REGISTRATIONS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_INTERVAL_TIMER_REGISTRATIONS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_PROMISE_TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_CALLBACK_TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_INTERVAL_TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_PROMISE_TIMERS_FIRED: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_CALLBACK_TIMERS_FIRED: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_INTERVAL_TIMERS_FIRED: AtomicU64 = AtomicU64::new(0);

/// Initialize the timer system (called once at startup)
fn ensure_initialized() {
    let mut st = START_TIME.lock().unwrap();
    if st.is_none() {
        *st = Some(Instant::now());
    }
}

/// Get current time in milliseconds since program start
#[no_mangle]
pub extern "C" fn js_timer_now() -> f64 {
    ensure_initialized();
    let st = START_TIME.lock().unwrap();
    st.map(|start| start.elapsed().as_millis() as f64)
        .unwrap_or(0.0)
}

// ── Scheduling ──────────────────────────────────────────────────────────────

/// Schedule a timer that resolves a promise after delay_ms milliseconds
/// Returns the promise that will be resolved
#[no_mangle]
pub extern "C" fn js_set_timeout(delay_ms: f64) -> *mut Promise {
    schedule_promise_timer(delay_ms, 0.0, true)
}

/// Schedule a timer with a specific resolve value
#[no_mangle]
pub extern "C" fn js_set_timeout_value(delay_ms: f64, value: f64) -> *mut Promise {
    schedule_promise_timer(delay_ms, value, true)
}

/// Schedule a promise timer with explicit event-loop liveness.
#[no_mangle]
pub extern "C" fn js_set_timeout_value_ref(
    delay_ms: f64,
    value: f64,
    has_ref: i32,
) -> *mut Promise {
    schedule_promise_timer(delay_ms, value, has_ref != 0)
}

fn schedule_promise_timer(delay_ms: f64, value: f64, has_ref: bool) -> *mut Promise {
    crate::promise::bump(&PROFILE_PROMISE_TIMER_REGISTRATIONS);
    ensure_initialized();

    let promise = js_promise_new();
    let deadline = Instant::now() + Duration::from_millis(normalize_timer_delay(delay_ms));
    store::with_current(|timers| {
        timers.insert_timer(Entry::promise(deadline, promise, value, has_ref));
    });
    sync_loop_timer();
    promise
}

/// Whether anything other than this agent's own timers keeps its loop alive.
fn other_event_sources_keep_loop_alive() -> bool {
    unsafe { js_stdlib_has_active_handles() != 0 }
}

/// Whether an unref'd timer may still contribute a wake deadline: it does while
/// something else keeps the loop running, and never on its own.
fn should_run_unref_promise_timers() -> bool {
    store::has_refed_timers() || store::has_refed_check() || other_event_sources_keep_loop_alive()
}

// ── The legacy per-class tick entries ───────────────────────────────────────
//
// P3 merged the three expiry-ordered queues into one heap, so "run the promise
// timers", "run the callback timers" and "run the intervals" are no longer
// separable: an interval and a timeout that are both due must interleave by
// deadline (Node walks one ordered structure; measured). These three exported
// symbols therefore all drive the same timers phase — the first call in a round
// does the work and the others find nothing due, which is exactly what a host
// that calls all three in sequence already expected.
//
// `js_callback_timer_tick` additionally runs the check phase, because the
// native-UI hosts (iOS/tvOS/watchOS/visionOS/Android/GTK4/WinUI) drive their
// timer pump as `js_callback_timer_tick(); js_interval_timer_tick();` and have
// no poll phase of their own to separate the two. The generated event loop does
// not use these: it calls `js_event_loop_timers_phase` and
// `js_event_loop_check_phase` at the right points in the iteration.

/// Run the timers phase: every due promise timer, `setTimeout` and
/// `setInterval` callback, in deadline order.
#[no_mangle]
pub extern "C" fn js_timer_tick() -> i32 {
    crate::promise::bump(&PROFILE_PROMISE_TIMER_TICKS);
    phases::run_timers_phase()
}

/// The generated event loop's timers phase.
#[no_mangle]
pub extern "C" fn js_event_loop_timers_phase() -> i32 {
    crate::promise::bump(&PROFILE_PROMISE_TIMER_TICKS);
    phases::run_timers_phase()
}

/// The callback half of the generated event loop's poll phase: the native
/// completion callbacks (`fs`, `dns`, `crypto`), which run after the I/O pump
/// and before the check phase.
#[no_mangle]
pub extern "C" fn js_event_loop_poll_callbacks() -> i32 {
    phases::run_poll_callbacks()
}

/// The generated event loop's check phase (`setImmediate`), which runs AFTER
/// the poll phase.
#[no_mangle]
pub extern "C" fn js_event_loop_check_phase() -> i32 {
    crate::promise::bump(&PROFILE_CALLBACK_TIMER_TICKS);
    phases::run_check_phase()
}

/// Timers phase, then the poll callbacks, then the check phase — the whole
/// iteration's callback work, for a host that has no phases of its own.
#[no_mangle]
pub extern "C" fn js_callback_timer_tick() -> i32 {
    crate::promise::bump(&PROFILE_CALLBACK_TIMER_TICKS);
    phases::run_timers_phase() + phases::run_poll_callbacks() + phases::run_check_phase()
}

/// Timers phase. Kept as its own symbol for the native-UI hosts, which call it
/// after `js_callback_timer_tick`.
#[no_mangle]
pub extern "C" fn js_interval_timer_tick() -> i32 {
    crate::promise::bump(&PROFILE_INTERVAL_TIMER_TICKS);
    phases::run_timers_phase()
}

/// Compatibility entry used by generated startup drains.
#[no_mangle]
pub extern "C" fn js_timer_tick_if_refed() -> i32 {
    js_timer_tick()
}

/// #5437: timer entry for the codegen `await` busy-wait loop. An `await` is a
/// yield point — the real event loop would run due timers and immediates there
/// — but the per-thread dispatch guard blocks the phases whenever the awaiting
/// code itself runs inside a timer/immediate callback (every HTTP request
/// handler does). React's server renderer schedules its render/flush work via
/// `setImmediate`, so a busy-wait await in the request path deadlocked forever.
/// Suspend the guard for this round: callbacks fired here still enter/leave the
/// dispatch depth themselves, so a *plain* nested phase inside one of them stays
/// guarded exactly as before.
#[no_mangle]
pub extern "C" fn js_await_loop_tick_timers() -> i32 {
    let saved = TIMER_CALLBACK_DISPATCH_DEPTH.with(|depth| {
        let v = depth.get();
        depth.set(0);
        v
    });
    let fired =
        phases::run_timers_phase() + phases::run_poll_callbacks() + phases::run_check_phase();
    TIMER_CALLBACK_DISPATCH_DEPTH.with(|depth| {
        depth.set(depth.get().saturating_add(saved));
    });
    fired
}

// ── Liveness and deadlines ──────────────────────────────────────────────────

/// Does a ref'd timers-phase entry keep this agent's event loop alive?
#[no_mangle]
pub extern "C" fn js_timer_has_pending() -> i32 {
    i32::from(store::has_refed_timers())
}

/// Does a ref'd check-phase entry keep this agent's event loop alive?
#[no_mangle]
pub extern "C" fn js_callback_timer_has_pending() -> i32 {
    i32::from(store::has_refed_check())
}

/// Same question as `js_timer_has_pending`; the generated loop's liveness
/// disjunction still asks all three.
#[no_mangle]
pub extern "C" fn js_interval_timer_has_pending() -> i32 {
    i32::from(store::has_refed_timers())
}

/// Is there anything in the check queue at all, ref'd or not?
///
/// The event loop must not park while this is true: Node computes a zero poll
/// timeout whenever the immediate queue is non-empty, so an immediate queued by
/// a check callback runs on the very next turn instead of after a park.
#[no_mangle]
pub extern "C" fn js_immediate_has_pending() -> i32 {
    i32::from(store::check_pending())
}

/// The earliest deadline this agent must wake for, as an exact `Instant`.
///
/// turnloop P0 made this the precise park's input (no millisecond truncation);
/// P3 made it the heap root instead of a scan of three queues.
pub(crate) fn next_timer_deadline() -> Option<Instant> {
    let (refed, unrefed) = store::with_current_existing(|timers| timers.deadline_candidates())?;
    match (refed, unrefed) {
        // No unref'd timer: the question below never arises. This is the hot
        // path — the deadline is recomputed on every schedule and cancel so the
        // loop's armed timer stays in step, and `should_run_unref_*` reaches
        // into stdlib.
        (refed, None) => refed,
        (Some(refed), Some(unrefed)) if unrefed >= refed => Some(refed),
        (refed, Some(unrefed)) => {
            if should_run_unref_promise_timers() {
                Some(unrefed)
            } else {
                refed
            }
        }
    }
}

/// The legacy C deadline shape: whole milliseconds until the next deadline
/// (0 when due), or -1 when there is none. Kept for embedders and the legacy
/// park; the primary agent's precise park reads the `Instant` directly.
fn whole_ms_until(at: Option<Instant>, now: Instant) -> f64 {
    match at {
        None => -1.0,
        Some(at) if at <= now => 0.0,
        Some(at) => (at - now).as_millis() as f64,
    }
}

/// Get the time until the next timer fires (in ms), or -1 if none.
#[no_mangle]
pub extern "C" fn js_timer_next_deadline() -> f64 {
    whole_ms_until(next_timer_deadline(), Instant::now())
}

/// Same value as `js_timer_next_deadline`; kept so `js_wait_for_event` and
/// embedders that ask per class keep resolving.
#[no_mangle]
pub extern "C" fn js_callback_timer_next_deadline() -> f64 {
    js_timer_next_deadline()
}

/// Same value as `js_timer_next_deadline`.
#[no_mangle]
pub extern "C" fn js_interval_timer_next_deadline() -> f64 {
    js_timer_next_deadline()
}

/// Any entry at all, including unref'd ones and check-phase work: the "is a
/// timer phase worth running" predicate.
pub(crate) fn timer_phase_work_pending() -> bool {
    store::any_pending()
}

/// Drop every timer owned by `agent`, called from `crate::agent::retire_agent`
/// when a `perry/thread` worker exits.
///
/// A timer scheduled inside a worker can never legally fire: the worker has no
/// event loop of its own and no other agent may run its callback — the closure
/// lives in the worker's arena, which is unmapped at exit. Dropping the
/// partition is the honest end state, not a loss of work.
pub(crate) fn purge_agent_timers(agent: crate::agent::AgentId) {
    store::purge_agent(agent);
}

/// Sleep for the specified number of milliseconds.
/// This is a blocking sleep - use sparingly
#[no_mangle]
pub extern "C" fn js_sleep_ms(ms: f64) {
    if ms > 0.0 {
        std::thread::sleep(Duration::from_millis(ms as u64));
    }
}

/// Re-arm this agent's turnloop timer at the store's earliest deadline.
///
/// turnloop P3: the loop owns a single timer handle whose expiry is a real
/// `OpResult::Timer` completion, so a park that ends at a JS timer deadline is
/// a turnloop completion rather than a timeout Perry computed for itself
/// (DESIGN §9). It is deliberately `set_ref(false)`: Perry's own keep-alive
/// counters decide whether the loop stays alive, and an armed deadline must
/// never by itself answer `Loop::alive()`.
fn sync_loop_timer() {
    crate::event_pump::arm_agent_timer(next_timer_deadline());
}

/// Re-arm after the agent loop was rebuilt underneath the store (a P1 profile
/// upgrade discards the old loop and its timer handle with it).
pub(crate) fn resync_loop_timer() {
    sync_loop_timer();
}

// ── Handle kinds, ref state and the dispatch guard ──────────────────────────

/// Whether a timer handle came from `setTimeout`/`setInterval` or
/// `setImmediate`. Only the `hasRef()`/constructor registries need the
/// distinction now; the store's `Class` carries it for scheduling.
#[derive(Clone, Copy, Eq, PartialEq)]
enum CallbackTimerKind {
    Timeout,
    Immediate,
}

// Shared id counter across callback timers AND intervals so a handle id is
// globally unique. Node treats Timeout/Interval as the same internal Timer
// type, so `clearTimeout(intervalHandle)` and `clearInterval(timeoutHandle)`
// are tolerated.
static NEXT_TIMER_ID: Mutex<i64> = Mutex::new(1);

mod gc_scan;
mod handle_object;
mod ref_states;
#[cfg(test)] // #7680: not re-exported; reach via `crate::timer::test_shared_queues::`
pub(crate) mod test_shared_queues;

pub use gc_scan::scan_timer_roots_mut;
pub(crate) use gc_scan::{new_timer_root_scan_state, scan_timer_roots_mut_step};
pub use ref_states::is_known_timer_id;
use ref_states::register_scheduled_timer;

pub(crate) use handle_object::{scan_timer_prototype_roots_mut, try_timer_method_fast_dispatch};
// `crate::timer::`-qualified only from unit tests (`timer/tests_inline.rs`,
// `gc/tests/handle_bound_method_name.rs`, `timer/ref_states.rs`'s test module);
// an unconditional `pub(crate) use` would be an unused import in a lib build
// and `-D warnings` would reject it.
use handle_object::{timer_handle_id, timer_object};
#[cfg(test)]
pub(crate) use handle_object::{timer_handle_parts, TIMEOUT_CLASS_ID};

static WARNED_NEGATIVE_TIMER_DELAY: AtomicBool = AtomicBool::new(false);
static WARNED_NAN_TIMER_DELAY: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TIMER_CALLBACK_DISPATCH_DEPTH: std::cell::Cell<u32> =
        const { std::cell::Cell::new(0) };
}

fn in_timer_callback_dispatch() -> bool {
    TIMER_CALLBACK_DISPATCH_DEPTH.with(|depth| depth.get() > 0)
}

fn enter_timer_callback_dispatch() {
    TIMER_CALLBACK_DISPATCH_DEPTH.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
}

fn leave_timer_callback_dispatch() {
    TIMER_CALLBACK_DISPATCH_DEPTH.with(|depth| {
        depth.set(depth.get().saturating_sub(1));
    });
}

fn timer_handle_value(id: i64) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(id as *mut u8).bits())
}

fn with_timer_uncaught_trap<F: FnOnce()>(f: F) {
    let trap_buf = crate::exception::js_try_push();
    let mut f = Some(f);
    // The jmp_buf is armed inside a C trampoline frame (#9305 — a raw
    // `setjmp` in a Rust frame is unsound). Loop shape: a throw from the
    // timer callback lands in the trampoline, and the uncaught path then
    // runs under the NEXT arm — so a throw out of an 'uncaughtException'
    // listener lands here again instead of targeting a dead frame,
    // matching the raw shape where the still-armed setjmp caught it.
    loop {
        let completed = crate::exception::arm_trap_and_run(trap_buf, || {
            if let Some(f) = f.take() {
                f();
            } else {
                let exc = crate::exception::js_get_exception();
                crate::exception::js_clear_exception();
                crate::os::emit_process_uncaught_exception(exc);
            }
        });
        if completed.is_some() {
            break;
        }
    }
    crate::exception::js_try_end();
}

fn call_timer_callback(
    id: i64,
    callback: i64,
    args: &[f64],
    context: &crate::async_context::AsyncContextSnapshot,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle =
        scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader);
    let arg_handles = scope.root_nanbox_f64_slice(args);
    let previous = crate::async_context::enter_context(context);
    let mut previous = previous;
    let previous_roots = crate::async_context::root_snapshot(&scope, &previous);
    let a = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let cb = callback_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
    let prev_this =
        scope.root_nanbox_f64(crate::object::js_implicit_this_set(timer_handle_value(id)));
    with_timer_uncaught_trap(|| unsafe {
        crate::closure::js_closure_call_array(cb as i64, a.as_ptr(), a.len() as i64);
    });
    crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
    crate::async_context::refresh_snapshot_from_roots(&mut previous, &previous_roots);
    crate::async_context::restore_context(previous);
}

fn next_timer_id() -> i64 {
    let mut next = NEXT_TIMER_ID.lock().unwrap();
    let current = *next;
    *next += 1;
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(crate::hot_diag::ReceiverReprFamily::Timer);
    }
    current
}

/// Call one timer callback with its arguments, under the uncaught-exception
/// trap and with the timer handle installed as `this`. `scope` already roots
/// nothing of this entry's: both the closure and the arguments are rooted here,
/// and re-read immediately before the call, because installing the receiver is
/// itself a collecting boundary.
fn call_timer_callback_entry(
    scope: &crate::gc::RuntimeHandleScope,
    id: i64,
    callback: i64,
    args: &[f64],
) {
    let callback_handle =
        scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader);
    let arg_handles = scope.root_nanbox_f64_slice(args);
    let prev_this =
        scope.root_nanbox_f64(crate::object::js_implicit_this_set(timer_handle_value(id)));
    with_timer_uncaught_trap(|| {
        let a = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
        let cb = callback_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
        unsafe {
            crate::closure::js_closure_call_array(cb as i64, a.as_ptr(), a.len() as i64);
        }
    });
    crate::object::js_implicit_this_set(prev_this.get_nanbox_f64());
}

fn timer_delay_text(delay_ms: f64) -> String {
    if delay_ms.is_infinite() && delay_ms.is_sign_positive() {
        "Infinity".to_string()
    } else if delay_ms.is_infinite() && delay_ms.is_sign_negative() {
        "-Infinity".to_string()
    } else {
        delay_ms.to_string()
    }
}

fn timer_warning_string(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

fn emit_timer_delay_warning(kind: &str, message: String) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let message_handle = scope.root_nanbox_f64(timer_warning_string(&message));
    let kind_handle = scope.root_nanbox_f64(timer_warning_string(kind));
    crate::process::js_process_emit_warning(
        message_handle.get_nanbox_f64(),
        kind_handle.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
}

fn coerce_timer_delay(delay_value: f64) -> f64 {
    let value = crate::value::JSValue::from_bits(delay_value.to_bits());
    if value.is_undefined() {
        1.0
    } else {
        crate::builtins::js_number_coerce(delay_value)
    }
}

fn normalize_timer_delay(delay_value: f64) -> u64 {
    const TIMEOUT_MAX: f64 = 2_147_483_647.0;
    let delay_ms = coerce_timer_delay(delay_value);
    if delay_ms > TIMEOUT_MAX {
        emit_timer_delay_warning(
            "TimeoutOverflowWarning",
            format!(
                "{} does not fit into a 32-bit signed integer.\nTimeout duration was set to 1.",
                timer_delay_text(delay_ms)
            ),
        );
        1
    } else if delay_ms < 0.0 {
        if !WARNED_NEGATIVE_TIMER_DELAY.swap(true, Ordering::AcqRel) {
            emit_timer_delay_warning(
                "TimeoutNegativeWarning",
                format!(
                    "{} is a negative number.\nTimeout duration was set to 1.",
                    timer_delay_text(delay_ms)
                ),
            );
        }
        1
    } else if delay_ms.is_nan() {
        if !WARNED_NAN_TIMER_DELAY.swap(true, Ordering::AcqRel) {
            emit_timer_delay_warning(
                "TimeoutNaNWarning",
                "NaN is not a number.\nTimeout duration was set to 1.".to_string(),
            );
        }
        1
    } else {
        delay_ms.max(0.0) as u64
    }
}

/// `ref()`/`unref()` on a timer handle: record the state for `hasRef()` and
/// apply it to the queued entry, which moves it between the ref'd and unref'd
/// heaps (and therefore changes the agent's keep-alive count).
fn set_timer_ref_state(id: i64, has_ref: bool) {
    record_timer_ref_state(id, has_ref);
    store::with_current(|timers| timers.set_ref(id, has_ref));
    sync_loop_timer();
}

/// Record `id`'s ref state in the `hasRef()` registry only. For callers that
/// hold a timer lock or have already set the queued entry's `refed` directly.
fn record_timer_ref_state(id: i64, has_ref: bool) {
    // #10447: was `insert_bounded`, which evicted the oldest ids by insertion
    // order whether or not they were still scheduled — so 65,536 later timers
    // silently undid a live timer's `unref()` and dropped the id out of
    // `is_known_timer_id`, which is what makes `.hasRef()`/`.ref()`/`.unref()`
    // stop dispatching. `set_timer_ref_state` bounds the registry the same way
    // but only ever evicts RETIRED ids; a scheduled id is pinned.
    //
    // The keep-alive half of #10447 never reached this branch — P3 reads
    // `store::has_refed_timers()`, not this registry — but the handle-method
    // half did, and this is the fix for it.
    ref_states::set_timer_ref_state(id, has_ref);
}

// #340/#341: `timer_constructor_value` stood here. It fabricated a fresh
// `{ name: "Timeout" }` object on EVERY `t.constructor` read, because a small
// registry id has no prototype to carry one. The handle is an ordinary object
// now and its prototype owns a real `constructor`, so that read is an ordinary
// property lookup and the per-read allocation is gone with it.

#[no_mangle]
pub extern "C" fn js_timer_has_ref(timer_id: i64) -> i32 {
    // Node's `Timeout.hasRef()` returns the current ref state, which is
    // `true` by default and stays `true` after `clearTimeout` unless the
    // user explicitly called `.unref()` on the handle.
    // #10447: the registry's own accessor, which keeps Node's "an id we do not
    // hold reads as ref'd" default in one place instead of restating it here.
    ref_states::timer_has_ref_state(timer_id) as i32
}

#[no_mangle]
pub extern "C" fn js_timer_ref(timer_id: i64) {
    set_timer_ref_state(timer_id, true);
}

#[no_mangle]
pub extern "C" fn js_timer_unref(timer_id: i64) {
    set_timer_ref_state(timer_id, false);
}

/// Reschedule a Timeout using its original delay, matching Node's
/// `Timeout.refresh()` semantics. For intervals, resets the next-deadline
/// cursor to one full interval from now. The handle's ref state is left alone —
/// Node's `refresh()` re-inserts the timer and never re-refs it.
#[no_mangle]
pub extern "C" fn js_timer_refresh(timer_id: i64) {
    let now = Instant::now();
    if store::with_current(|timers| timers.refresh(timer_id, now)) {
        sync_loop_timer();
    }
}

/// Issue #2013 — validate the first argument of `setTimeout`/`setInterval`
/// /`setImmediate` so a non-callable value throws Node's
/// `TypeError [ERR_INVALID_ARG_TYPE]` shape instead of segfaulting on
/// the downstream pointer-deref of the unboxed handle. `value` is the
/// caller's NaN-boxed JS value (codegen passes the full f64 before the
/// `unbox_to_i64` that the existing FFIs require). `fn_name` is the
/// JS function name reported in the error message
/// (`"setTimeout"` / `"setInterval"` / `"setImmediate"`).
///
/// Returns the raw closure pointer (extracted via `unbox_to_i64`) for
/// the callable case so the codegen can pass it straight to the
/// scheduling entry without a second unbox.
#[no_mangle]
pub unsafe extern "C" fn js_timer_validate_callback(value: f64, fn_name_idx: i32) -> i64 {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value.to_bits();
    if (bits & !POINTER_MASK) == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return ptr as i64;
        }
    }
    // Promise executor resolve/reject callbacks are passed through this runtime
    // as raw closure pointer bits rather than NaN-boxed pointers. They are still
    // callable JS functions, so accept them after proving the candidate is a
    // Perry-managed closure. Do not call `is_closure_ptr` on arbitrary JS bits:
    // short strings and doubles can otherwise look pointer-ish enough to
    // segfault during validation.
    if let Some(ptr) = raw_closure_pointer(bits) {
        return ptr as i64;
    }
    // 0 = setTimeout, 1 = setInterval, 2 = setImmediate, anything
    // else falls back to the generic "callback" wording.
    let fn_name: &str = match fn_name_idx {
        0 => "setTimeout",
        1 => "setInterval",
        2 => "setImmediate",
        _ => "timer",
    };
    let message = format!(
        "The \"callback\" argument must be of type function. Received {}",
        crate::fs::validate::describe_received(value)
    );
    // `setTimeout` / `setInterval` / `setImmediate` all surface the
    // bad-callback case as ERR_INVALID_ARG_TYPE — the message body
    // varies a touch but the code does not.
    let _ = fn_name;
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn raw_closure_pointer(bits: u64) -> Option<usize> {
    const RAW_PTR_MAX: u64 = 0x0000_FFFF_FFFF_FFFF;
    if !(0x10000..=RAW_PTR_MAX).contains(&bits) || bits & 0x7 != 0 {
        return None;
    }
    let ptr = bits as usize;
    // #7531: band, not magnitude floor (0x1008 admitted every handle band).
    if !crate::value::addr_class::is_plausible_heap_addr(ptr) {
        return None;
    }
    let header_addr = ptr - crate::gc::GC_HEADER_SIZE;
    let header = header_addr as *const crate::gc::GcHeader;
    let tracked_malloc = crate::gc::gc_malloc_header_is_tracked(header);
    let arena_payload = !matches!(
        crate::arena::classify_heap_space(ptr),
        crate::arena::HeapSpace::Unknown
    );
    let arena_header = !matches!(
        crate::arena::classify_heap_space(header_addr),
        crate::arena::HeapSpace::Unknown
    );
    if !tracked_malloc && !(arena_payload && arena_header) {
        return None;
    }
    unsafe {
        if (*header).obj_type != crate::gc::GC_TYPE_CLOSURE {
            return None;
        }
        let size = (*header).size as usize;
        if size < crate::gc::GC_HEADER_SIZE || size as u64 > (1u64 << 34) {
            return None;
        }
        let is_arena = (*header).gc_flags & crate::gc::GC_FLAG_ARENA != 0;
        if tracked_malloc == is_arena {
            return None;
        }
    }
    crate::closure::is_closure_ptr(ptr).then_some(ptr)
}
#[no_mangle]
pub extern "C" fn js_set_timeout_callback(callback: i64, delay_ms: f64) -> i64 {
    let id = schedule_callback_timer(
        callback,
        delay_ms,
        Vec::new(),
        "Timeout",
        Class::Timeout,
        None,
    );
    timer_object(id, CallbackTimerKind::Timeout)
}

#[no_mangle]
pub extern "C" fn js_set_immediate_callback(callback: i64) -> i64 {
    let id = schedule_callback_timer(
        callback,
        0.0,
        Vec::new(),
        "Immediate",
        Class::Immediate,
        None,
    );
    timer_object(id, CallbackTimerKind::Immediate)
}

fn schedule_callback_timer(
    callback: i64,
    delay_ms: f64,
    args: Vec<f64>,
    type_name: &str,
    class: Class,
    trigger_async_id: Option<u64>,
) -> i64 {
    crate::promise::bump(if class == Class::Interval {
        &PROFILE_INTERVAL_TIMER_REGISTRATIONS
    } else {
        &PROFILE_CALLBACK_TIMER_REGISTRATIONS
    });
    let handle_kind = match class {
        Class::Immediate | Class::Pending => CallbackTimerKind::Immediate,
        _ => CallbackTimerKind::Timeout,
    };
    if class == Class::Interval {
        if let Some(id) = schedule_mock_interval_timer(callback, delay_ms, args.clone()) {
            return id;
        }
    } else if let Some(id) =
        schedule_mock_callback_timer(callback, delay_ms, args.clone(), handle_kind)
    {
        return id;
    }
    ensure_initialized();

    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle =
        scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader);
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let delay_ms = normalize_timer_delay(delay_ms);
    let deadline = Instant::now() + Duration::from_millis(delay_ms);

    let id = next_timer_id();
    // #10447: registering PINS this id in the handle registry — it marks the id
    // scheduled, so it cannot be evicted while queued. The returned guard is
    // handed to the queue entry below and retires the id when that entry is
    // dropped. This replaces `record_timer_handle_kind`, whose own table had the
    // same evicting cap plus an O(n) `min()` scan per insert. (#340/#341 took
    // the KIND out of this registry — the handle object carries it now.)
    let scheduled = register_scheduled_timer(id);

    let mut context = crate::async_context::capture_context();
    let context_roots = crate::async_context::root_snapshot(&scope, &context);
    let (ids, callback) =
        callback_handle.across_const::<crate::closure::ClosureHeader, _>(
            || match trigger_async_id {
                Some(trigger_async_id) => crate::async_hooks::init_resource_with_trigger(
                    type_name,
                    timer_handle_value(id),
                    true,
                    trigger_async_id,
                ),
                None => crate::async_hooks::init_resource(type_name, timer_handle_value(id), true),
            },
        );
    crate::async_context::refresh_snapshot_from_roots(&mut context, &context_roots);

    let entry = Entry::callback(
        id,
        class,
        deadline,
        delay_ms,
        callback as i64,
        crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles),
        context,
        ids.async_id,
        ids.trigger_async_id,
        Some(scheduled),
    );
    store::with_current(|timers| {
        match class {
            c if c.is_timer() => timers.insert_timer(entry),
            Class::Pending => timers.insert_pending(entry),
            _ => timers.insert_check(entry),
        };
    });
    record_timer_ref_state(id, true);
    sync_loop_timer();

    id
}

/// JS-style setTimeout that takes a callback function, delay, and a buffer
/// of trailing arguments (`setTimeout(resolve, delay, res)`; refs #665).
#[no_mangle]
pub unsafe extern "C" fn js_set_timeout_callback_args(
    callback: i64,
    delay_ms: f64,
    args_ptr: *const f64,
    n_args: i32,
) -> i64 {
    let id = schedule_callback_timer(
        callback,
        delay_ms,
        args_from_raw(args_ptr, n_args),
        "Timeout",
        Class::Timeout,
        None,
    );
    timer_object(id, CallbackTimerKind::Timeout)
}

#[no_mangle]
pub unsafe extern "C" fn js_set_immediate_callback_args(
    callback: i64,
    args_ptr: *const f64,
    n_args: i32,
) -> i64 {
    let id = schedule_callback_timer(
        callback,
        0.0,
        args_from_raw(args_ptr, n_args),
        "Immediate",
        Class::Immediate,
        None,
    );
    timer_object(id, CallbackTimerKind::Immediate)
}

/// Copy a codegen-supplied trailing-argument buffer; the caller may free it as
/// soon as the scheduling entry returns.
unsafe fn args_from_raw(args_ptr: *const f64, n_args: i32) -> Vec<f64> {
    if args_ptr.is_null() || n_args <= 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, n_args as usize).to_vec()
    }
}

/// Schedule a native Node-style completion callback as its own async-hooks
/// provider. Native stdlib operations use the ordinary check queue for deferred
/// delivery, but must expose their actual provider name (for example
/// `PBKDF2REQUEST`) and execute with that provider's async id/resource rather
/// than masquerading as an `Immediate`.
pub fn schedule_native_callback(callback: i64, args: &[f64], provider_type: &'static str) -> i64 {
    schedule_callback_timer(
        callback,
        0.0,
        args.to_vec(),
        provider_type,
        Class::Pending,
        None,
    )
}

/// Schedule the final callback in a provider chain while emitting the eager
/// native preparation stages ahead of it. Node's `fs.readFile` is implemented
/// as four chained FSREQCALLBACK operations (open, stat, read, close); Perry
/// performs those syscalls eagerly, but the observable hook graph must retain
/// the same four-resource ancestry.
pub fn schedule_native_callback_chain(
    callback: i64,
    args: &[f64],
    provider_type: &'static str,
    resource_count: usize,
) -> i64 {
    let mut trigger = crate::async_hooks::execution_async_id_u64();
    for _ in 1..resource_count {
        let resource = crate::object::js_object_alloc_null_proto(0, 0);
        let ids = crate::async_hooks::init_resource_with_trigger(
            provider_type,
            crate::value::js_nanbox_pointer(resource as i64),
            true,
            trigger,
        );
        crate::async_hooks::before(ids.async_id, ids.trigger_async_id);
        crate::async_hooks::after(ids.async_id);
        crate::async_hooks::destroy(ids.async_id);
        trigger = ids.async_id;
    }
    schedule_callback_timer(
        callback,
        0.0,
        args.to_vec(),
        provider_type,
        Class::Pending,
        Some(trigger),
    )
}

/// JS-style setInterval that takes a callback function and interval.
#[no_mangle]
pub extern "C" fn setInterval(callback: i64, interval_ms: f64) -> i64 {
    let id = schedule_callback_timer(
        callback,
        interval_ms,
        Vec::new(),
        "Timeout",
        Class::Interval,
        None,
    );
    // node names an interval handle `Timeout` too, and `clearTimeout` /
    // `clearInterval` are interchangeable on it.
    timer_object(id, CallbackTimerKind::Timeout)
}

#[no_mangle]
pub unsafe extern "C" fn js_set_interval_callback_args(
    callback: i64,
    interval_ms: f64,
    args_ptr: *const f64,
    n_args: i32,
) -> i64 {
    let id = schedule_callback_timer(
        callback,
        interval_ms,
        args_from_raw(args_ptr, n_args),
        "Timeout",
        Class::Interval,
        None,
    );
    timer_object(id, CallbackTimerKind::Timeout)
}

/// How many `setTimeout`/`setInterval` handles this agent has outstanding —
/// `process.getActiveResourcesInfo()`.
pub fn active_timeout_resource_count() -> usize {
    store::with_current_existing(|timers| timers.timeout_resource_count()).unwrap_or(0)
        + mock::mock_timeout_resource_count()
}

// ── Cancellation ────────────────────────────────────────────────────────────

/// Remove a queued entry and queue its async-hooks `destroy`.
fn clear_entry(timer_id: i64, accept: fn(Class) -> bool) {
    let async_id = store::with_current(|timers| {
        timers
            .remove_by_id(timer_id, accept)
            .map(|entry| entry.async_id)
    });
    enqueue_destroy_ids([async_id, None]);
    sync_loop_timer();
}

/// Clear a Timeout by ID. Also clears intervals so Node's interchangeable
/// `clearTimeout(intervalHandle)` shape works. Immediate handles are distinct
/// and are only canceled by `clearImmediate`.
#[no_mangle]
pub extern "C" fn clearTimeout(timer_id: i64) {
    mock_clear_timeout(timer_id);
    clear_entry(timer_id, |class| {
        matches!(class, Class::Timeout | Class::Interval)
    });
}

/// Clear an interval timer by ID. Also clears Timeout callback timers so
/// Node's interchangeable `clearInterval(timeoutHandle)` shape works.
#[no_mangle]
pub extern "C" fn clearInterval(interval_id: i64) {
    mock_clear_interval(interval_id);
    clear_entry(interval_id, |class| {
        matches!(class, Class::Timeout | Class::Interval)
    });
}

/// Clear an Immediate by ID. Timeout/Interval handles are distinct and are not
/// canceled by `clearImmediate`.
#[no_mangle]
pub extern "C" fn clearImmediate(timer_id: i64) {
    mock_clear_immediate(timer_id);
    // A native completion callback shares the `Immediate` handle kind, so an
    // explicit `clearImmediate` of one keeps working exactly as it did when the
    // two shared a queue.
    clear_entry(timer_id, |class| {
        matches!(class, Class::Immediate | Class::Pending)
    });
}

/// Resolve a `clearTimeout`/`clearInterval` argument to a timer id. Accepts
/// both the Timeout/Immediate handle (POINTER_TAG, lower 48 bits = id) and the
/// primitive numeric id (`+timeout`), so `clearTimeout(+t)` works (#1213).
/// Returns `None` for nullish/other values (a no-op clear, matching Node).
fn arg_to_timer_id(arg: f64) -> Option<i64> {
    // #340/#341: the handle is an ordinary object carrying its id, so resolve
    // that first. The raw-id arms below stay for `clearTimeout(+t)` (#1213) and
    // for any value minted before this family migrated.
    if let Some(id) = timer_handle_id(arg) {
        return Some(id);
    }
    let v = crate::value::JSValue::from_bits(arg.to_bits());
    if v.is_int32() {
        Some(v.as_int32() as i64)
    } else if v.is_number() {
        let n = v.as_number();
        n.is_finite().then_some(n as i64)
    } else if let Some(s) = crate::node_submodules::diagnostics::decode_string_value(arg) {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse::<i64>().ok()
        } else {
            None
        }
    } else if v.is_pointer() {
        Some((arg.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64)
    } else {
        None
    }
}

/// `clearTimeout(handleOrId)` — accepts the handle or its numeric id (#1213).
#[no_mangle]
pub extern "C" fn js_clear_timeout_value(arg: f64) {
    if let Some(id) = arg_to_timer_id(arg) {
        clearTimeout(id);
    }
}

/// `clearInterval(handleOrId)` — accepts the handle or its numeric id (#1213).
#[no_mangle]
pub extern "C" fn js_clear_interval_value(arg: f64) {
    if let Some(id) = arg_to_timer_id(arg) {
        clearInterval(id);
    }
}

/// `clearImmediate(handleOrId)` — accepts the Immediate handle or primitive id.
#[no_mangle]
pub extern "C" fn js_clear_immediate_value(arg: f64) {
    if let Some(id) = arg_to_timer_id(arg) {
        clearImmediate(id);
    }
}

/// GC root scanner: mark all values reachable from this agent's timer store.
pub fn scan_timer_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_timer_roots_mut(&mut visitor);
}

/// `PERRY_GC_CENSUS`: the agent timer store.
pub(crate) fn timer_tables_census() -> Vec<crate::gc::census::SideTableRow> {
    // The handle registry is a bounded side table in its own right (#10447):
    // it outlives the queue entries, since a retired id stays queryable until
    // evicted. Leaving it out would under-report what timers retain.
    let mut rows = store::census_rows();
    rows.push(ref_states::ref_states_census());
    rows
}

#[path = "timer/tests_inline.rs"]
#[cfg(test)]
mod tests_inline;
// Helpers other modules reach as `crate::timer::…`; the extraction above
// moved their definitions, so re-export them at the original path.
#[cfg(test)]
pub(crate) use tests_inline::*;
