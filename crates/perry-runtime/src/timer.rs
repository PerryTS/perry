//! Timer support for setTimeout/setInterval
//!
//! Provides a simple timer queue that integrates with the Promise runtime.
//!
//! Uses global Mutex-protected state (not thread_local) so that timers
//! registered on one thread can be pumped from another. This is critical
//! on Android where TypeScript runs on the perry-native thread but the
//! timer pump fires on the UI thread.

use crate::promise::{js_promise_new, js_promise_resolve, Promise};
use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

/// A scheduled timer
struct Timer {
    /// When this timer should fire
    deadline: Instant,
    /// The promise to resolve when the timer fires
    promise: *mut Promise,
    /// The value to resolve with (typically undefined/0.0)
    value: f64,
}

// SAFETY: Promise pointers are only accessed from the pump thread
unsafe impl Send for Timer {}

// Global timer queues (Mutex-protected for cross-thread access)
static TIMER_QUEUE: Mutex<Vec<Timer>> = Mutex::new(Vec::new());
static START_TIME: Mutex<Option<Instant>> = Mutex::new(None);

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

/// Schedule a timer that resolves a promise after delay_ms milliseconds
/// Returns the promise that will be resolved
#[no_mangle]
pub extern "C" fn js_set_timeout(delay_ms: f64) -> *mut Promise {
    ensure_initialized();

    let promise = js_promise_new();
    let delay = Duration::from_millis(normalize_timer_delay(delay_ms));
    let deadline = Instant::now() + delay;

    TIMER_QUEUE.lock().unwrap().push(Timer {
        deadline,
        promise,
        value: 0.0, // setTimeout resolves with undefined
    });

    promise
}

/// Schedule a timer with a specific resolve value
#[no_mangle]
pub extern "C" fn js_set_timeout_value(delay_ms: f64, value: f64) -> *mut Promise {
    ensure_initialized();

    let promise = js_promise_new();
    let delay = Duration::from_millis(normalize_timer_delay(delay_ms));
    let deadline = Instant::now() + delay;

    TIMER_QUEUE.lock().unwrap().push(Timer {
        deadline,
        promise,
        value,
    });

    promise
}

/// Process any expired timers, resolving their promises
/// Returns the number of timers that fired
#[no_mangle]
pub extern "C" fn js_timer_tick() -> i32 {
    let now = Instant::now();
    let mut fired = 0;

    // Collect expired timers
    let expired: Vec<Timer> = {
        let mut queue = TIMER_QUEUE.lock().unwrap();
        let mut expired = Vec::new();
        let mut i = 0;
        while i < queue.len() {
            if queue[i].deadline <= now {
                expired.push(queue.remove(i));
            } else {
                i += 1;
            }
        }
        expired
    };

    // Resolve the expired timers' promises
    for timer in expired {
        let scope = crate::gc::RuntimeHandleScope::new();
        let promise_handle = scope.root_raw_mut_ptr(timer.promise);
        let value_handle = scope.root_nanbox_f64(timer.value);
        js_promise_resolve(
            promise_handle.get_raw_mut_ptr::<Promise>(),
            value_handle.get_nanbox_f64(),
        );
        fired += 1;
    }

    fired
}

/// Check if there are any pending timers
#[no_mangle]
pub extern "C" fn js_timer_has_pending() -> i32 {
    if TIMER_QUEUE.lock().unwrap().is_empty() {
        0
    } else {
        1
    }
}

/// Get the time until the next timer fires (in ms), or -1 if no timers
#[no_mangle]
pub extern "C" fn js_timer_next_deadline() -> f64 {
    let now = Instant::now();

    TIMER_QUEUE
        .lock()
        .unwrap()
        .iter()
        .map(|t| {
            if t.deadline <= now {
                0.0
            } else {
                (t.deadline - now).as_millis() as f64
            }
        })
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(-1.0)
}

/// Sleep for the specified number of milliseconds
/// This is a blocking sleep - use sparingly
#[no_mangle]
pub extern "C" fn js_sleep_ms(ms: f64) {
    if ms > 0.0 {
        std::thread::sleep(Duration::from_millis(ms as u64));
    }
}

/// A scheduled timer with a callback
#[derive(Clone, Copy, Eq, PartialEq)]
enum CallbackTimerKind {
    Timeout,
    Immediate,
}

struct CallbackTimer {
    /// Unique ID for this timer
    id: i64,
    /// Whether this callback came from `setTimeout` or `setImmediate`.
    kind: CallbackTimerKind,
    /// When this timer should fire
    deadline: Instant,
    /// Original delay (preserved so `refresh()` can reschedule with the
    /// same delay, matching Node's `Timeout.refresh()` semantics).
    delay_ms: u64,
    /// The closure pointer to call
    callback: i64,
    /// Trailing arguments to forward to the callback when it fires.
    /// Empty for the standard `setTimeout(fn, delay)` shape; non-empty
    /// when the call site is `setTimeout(fn, delay, ...args)` (JS spec
    /// allows trailing args that get passed to the callback — used in
    /// e.g. `setTimeout(resolve, delay, res)` inside Promise executors).
    /// Refs #665.
    args: Vec<f64>,
    /// AsyncLocalStorage context captured when the timer was scheduled.
    context: crate::async_context::AsyncContextSnapshot,
    /// async_hooks ids for this timer callback resource.
    async_id: u64,
    trigger_async_id: u64,
    /// Whether this timer has been cleared
    cleared: bool,
}

// SAFETY: closure pointers point to global compiled code data
unsafe impl Send for CallbackTimer {}

static CALLBACK_TIMERS: Mutex<Vec<CallbackTimer>> = Mutex::new(Vec::new());
// Shared id counter across callback timers AND intervals so a handle id is
// globally unique. Node treats Timeout/Interval as the same internal Timer
// type, so `clearTimeout(intervalHandle)` and `clearInterval(timeoutHandle)`
// are tolerated. With independent counters per queue (the previous design),
// id collisions across queues could cause `clearTimeout(intId)` to also
// clobber an unrelated Timeout with the same numeric id.
static NEXT_TIMER_ID: Mutex<i64> = Mutex::new(1);
static TIMER_REF_STATES: Mutex<Option<HashMap<i64, bool>>> = Mutex::new(None);
static WARNED_NEGATIVE_TIMER_DELAY: AtomicBool = AtomicBool::new(false);
static WARNED_NAN_TIMER_DELAY: AtomicBool = AtomicBool::new(false);
static WARNED_TIMER_TRACE_HINT: AtomicBool = AtomicBool::new(false);

fn timer_handle_value(id: i64) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(id as *mut u8).bits())
}

fn with_timer_uncaught_trap<F: FnOnce()>(f: F) {
    let trap_buf = crate::exception::js_try_push();
    // SAFETY: this setjmp frame is active only for the synchronous timer
    // callback invocation below. `js_throw` longjmps back here before the
    // frame is popped, matching the promise microtask runner's trap shape.
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut c_int) };
    if jumped == 0 {
        f();
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::os::emit_process_uncaught_exception(exc);
    }
    crate::exception::js_try_end();
}

fn next_timer_id() -> i64 {
    let mut next = NEXT_TIMER_ID.lock().unwrap();
    let current = *next;
    *next += 1;
    current
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

fn emit_timer_delay_warning(kind: &str, message: String) {
    eprintln!("(node:{}) {}: {}", std::process::id(), kind, message);
    if !WARNED_TIMER_TRACE_HINT.swap(true, Ordering::AcqRel) {
        eprintln!("(Use `node --trace-warnings ...` to show where the warning was created)");
    }
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

fn set_timer_ref_state(id: i64, has_ref: bool) {
    let mut slot = TIMER_REF_STATES.lock().unwrap();
    let map = slot.get_or_insert_with(HashMap::new);
    map.insert(id, has_ref);
}

/// Whether `id` corresponds to a timer that was scheduled by this runtime
/// (active or already cleared). Used by the small-handle method/property
/// fast paths in `object/*.rs` and by `js_number_coerce` to decide whether
/// to apply Timeout-shaped semantics to a NaN-boxed small pointer. Without
/// this gate, any small handle (UI widget, drizzle, etc.) would accidentally
/// route through timer dispatch.
///
/// Entries in `TIMER_REF_STATES` are inserted at schedule time and never
/// removed — clearing a timer marks it cleared in the queue but keeps the
/// id registered as "this was a timer" so post-clear `.hasRef()` / `+timer`
/// / `.unref()` still route through timer dispatch (Node keeps the
/// Timeout object alive after `clearTimeout` and methods still work).
pub fn is_known_timer_id(id: i64) -> bool {
    if id <= 0 {
        return false;
    }
    TIMER_REF_STATES
        .lock()
        .unwrap()
        .as_ref()
        .map(|map| map.contains_key(&id))
        .unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn js_timer_has_ref(timer_id: i64) -> i32 {
    // Node's `Timeout.hasRef()` returns the current ref state, which is
    // `true` by default and stays `true` after `clearTimeout` unless the
    // user explicitly called `.unref()` on the handle. Default `true` for
    // any non-timer id is harmless since the dispatcher gates on
    // `is_known_timer_id` first.
    TIMER_REF_STATES
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|map| map.get(&timer_id).copied())
        .unwrap_or(true) as i32
}

#[no_mangle]
pub extern "C" fn js_timer_ref(timer_id: i64) {
    set_timer_ref_state(timer_id, true);
}

#[no_mangle]
pub extern "C" fn js_timer_unref(timer_id: i64) {
    set_timer_ref_state(timer_id, false);
}

/// Reschedule a Timeout (or revive a cleared one) using its original
/// delay, matching Node's `Timeout.refresh()` semantics. For intervals,
/// resets the next-deadline cursor to one full interval from now.
#[no_mangle]
pub extern "C" fn js_timer_refresh(timer_id: i64) {
    let now = Instant::now();

    {
        let mut timers = CALLBACK_TIMERS.lock().unwrap();
        if let Some(timer) = timers.iter_mut().find(|t| t.id == timer_id) {
            timer.deadline = now + Duration::from_millis(timer.delay_ms);
            timer.cleared = false;
            set_timer_ref_state(timer_id, true);
            return;
        }
    }

    let mut intervals = INTERVAL_TIMERS.lock().unwrap();
    if let Some(timer) = intervals.iter_mut().find(|t| t.id == timer_id) {
        timer.next_deadline = now + Duration::from_millis(timer.interval_ms);
        timer.cleared = false;
        set_timer_ref_state(timer_id, true);
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
    if ptr < crate::gc::GC_HEADER_SIZE + 0x1000 {
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
        if size < crate::gc::GC_HEADER_SIZE || size > (1usize << 34) {
            return None;
        }
        let is_arena = (*header).gc_flags & crate::gc::GC_FLAG_ARENA != 0;
        if tracked_malloc == is_arena {
            return None;
        }
    }
    crate::closure::is_closure_ptr(ptr).then_some(ptr)
}

/// JS-style setTimeout that takes a callback function and delay
/// The callback is a closure pointer that will be called with no arguments
/// Returns a timer ID
#[no_mangle]
pub extern "C" fn js_set_timeout_callback(callback: i64, delay_ms: f64) -> i64 {
    schedule_callback_timer(
        callback,
        delay_ms,
        Vec::new(),
        "Timeout",
        CallbackTimerKind::Timeout,
    )
}

#[no_mangle]
pub extern "C" fn js_set_immediate_callback(callback: i64) -> i64 {
    schedule_callback_timer(
        callback,
        0.0,
        Vec::new(),
        "Immediate",
        CallbackTimerKind::Immediate,
    )
}

fn schedule_callback_timer(
    callback: i64,
    delay_ms: f64,
    args: Vec<f64>,
    type_name: &str,
    kind: CallbackTimerKind,
) -> i64 {
    ensure_initialized();

    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handle =
        scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader);
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let delay_ms = normalize_timer_delay(delay_ms);
    let deadline = Instant::now() + Duration::from_millis(delay_ms);

    let id = next_timer_id();

    let ids = crate::async_hooks::init_resource(
        type_name,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
    );

    CALLBACK_TIMERS.lock().unwrap().push(CallbackTimer {
        id,
        kind,
        deadline,
        delay_ms,
        callback: callback_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>() as i64,
        args: crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles),
        context: crate::async_context::capture_context(),
        async_id: ids.async_id,
        trigger_async_id: ids.trigger_async_id,
        cleared: false,
    });
    set_timer_ref_state(id, true);

    id
}

/// JS-style setTimeout that takes a callback function, delay, and a buffer
/// of trailing arguments. The callback is invoked as `callback(...args)`
/// when the timer fires. The args buffer is copied into the timer record
/// before this function returns (caller may free `args_ptr` immediately).
///
/// Refs #665: `setTimeout(resolve, delay, res)` and similar shapes inside
/// Promise executors couldn't reach codegen because the existing
/// `js_set_timeout_callback` only handled the 2-arg form; 3+ arg call sites
/// fell through and emitted a bare `setTimeout` symbol the linker couldn't
/// resolve.
#[no_mangle]
pub unsafe extern "C" fn js_set_timeout_callback_args(
    callback: i64,
    delay_ms: f64,
    args_ptr: *const f64,
    n_args: i32,
) -> i64 {
    let args: Vec<f64> = if args_ptr.is_null() || n_args <= 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, n_args as usize).to_vec()
    };
    schedule_callback_timer(
        callback,
        delay_ms,
        args,
        "Timeout",
        CallbackTimerKind::Timeout,
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_set_immediate_callback_args(
    callback: i64,
    args_ptr: *const f64,
    n_args: i32,
) -> i64 {
    let args: Vec<f64> = if args_ptr.is_null() || n_args <= 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, n_args as usize).to_vec()
    };
    schedule_callback_timer(
        callback,
        0.0,
        args,
        "Immediate",
        CallbackTimerKind::Immediate,
    )
}

/// Process any expired callback timers
/// Returns the number of callbacks that were called
#[no_mangle]
pub extern "C" fn js_callback_timer_tick() -> i32 {
    use crate::closure::{
        js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3, js_closure_call4,
        js_closure_call5, js_closure_call6, js_closure_call7, js_closure_call8, js_closure_call9,
    };

    let now = Instant::now();

    // Collect expired, non-cleared timers
    let expired: Vec<CallbackTimer> = {
        let mut queue = CALLBACK_TIMERS.lock().unwrap();
        let mut expired = Vec::new();
        let mut i = 0;
        while i < queue.len() {
            if queue[i].cleared {
                queue.remove(i);
            } else if queue[i].deadline <= now {
                expired.push(queue.remove(i));
            } else {
                i += 1;
            }
        }
        expired
    };

    let mut fired = 0;
    // Call the callbacks, forwarding any trailing args captured at
    // `setTimeout(fn, delay, ...args)` time. Refs #665.
    for timer in expired {
        if !timer.cleared {
            let scope = crate::gc::RuntimeHandleScope::new();
            let cb_handle =
                scope.root_raw_const_ptr(timer.callback as *const crate::closure::ClosureHeader);
            let arg_handles = scope.root_nanbox_f64_slice(&timer.args);
            let previous = crate::async_context::enter_context(&timer.context);
            let mut previous = previous;
            let previous_roots = crate::async_context::root_snapshot(&scope, &previous);
            crate::async_hooks::before(timer.async_id, timer.trigger_async_id);
            let a = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
            let cb = cb_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
            let prev_this = crate::object::js_implicit_this_set(timer_handle_value(timer.id));
            with_timer_uncaught_trap(|| {
                match a.len() {
                    0 => {
                        js_closure_call0(cb);
                    }
                    1 => {
                        js_closure_call1(cb, a[0]);
                    }
                    2 => {
                        js_closure_call2(cb, a[0], a[1]);
                    }
                    3 => {
                        js_closure_call3(cb, a[0], a[1], a[2]);
                    }
                    4 => {
                        js_closure_call4(cb, a[0], a[1], a[2], a[3]);
                    }
                    5 => {
                        js_closure_call5(cb, a[0], a[1], a[2], a[3], a[4]);
                    }
                    6 => {
                        js_closure_call6(cb, a[0], a[1], a[2], a[3], a[4], a[5]);
                    }
                    7 => {
                        js_closure_call7(cb, a[0], a[1], a[2], a[3], a[4], a[5], a[6]);
                    }
                    8 => {
                        js_closure_call8(cb, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]);
                    }
                    _ => {
                        // >= 9 args: clamp to 9. Real-world setTimeout
                        // rarely exceeds 1-2 trailing args; this is a
                        // conservative safety net rather than spec coverage.
                        js_closure_call9(cb, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]);
                    }
                }
            });
            crate::object::js_implicit_this_set(prev_this);
            crate::async_hooks::after(timer.async_id);
            crate::async_hooks::destroy(timer.async_id);
            crate::async_context::refresh_snapshot_from_roots(&mut previous, &previous_roots);
            crate::async_context::restore_context(previous);
            fired += 1;
        }
    }

    // NOTE: Do NOT call gc_check_trigger() here — same reason as interval
    // tick: register-held values get swept by conservative scanner.

    fired
}

/// Check if there are any pending callback timers
#[no_mangle]
pub extern "C" fn js_callback_timer_has_pending() -> i32 {
    let q = CALLBACK_TIMERS.lock().unwrap();
    if q.iter().any(|t| !t.cleared) {
        1
    } else {
        0
    }
}

/// Get the time until the next callback timer fires (in ms), or -1 if
/// none pending. Mirrors `js_timer_next_deadline` / `js_interval_timer_next_deadline`
/// — needed so `js_wait_for_event` can size its wait budget correctly
/// when the only pending work is a `setTimeout(cb, N)` callback timer
/// (the most common `setTimeout(r, N)` used inside `new Promise(...)`).
#[no_mangle]
pub extern "C" fn js_callback_timer_next_deadline() -> f64 {
    let now = Instant::now();

    CALLBACK_TIMERS
        .lock()
        .unwrap()
        .iter()
        .filter(|t| !t.cleared)
        .map(|t| {
            if t.deadline <= now {
                0.0
            } else {
                (t.deadline - now).as_millis() as f64
            }
        })
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(-1.0)
}

/// Clear a Timeout by ID. Also clears the interval queue so Node's
/// interchangeable `clearTimeout(intervalHandle)` shape works. Immediate
/// handles are distinct and are only canceled by `clearImmediate`.
#[no_mangle]
pub extern "C" fn clearTimeout(timer_id: i64) {
    {
        let mut timers = CALLBACK_TIMERS.lock().unwrap();
        for timer in timers.iter_mut() {
            if timer.id == timer_id && timer.kind == CallbackTimerKind::Timeout {
                timer.cleared = true;
                break;
            }
        }
        timers.retain(|t| !t.cleared);
    }
    let mut intervals = INTERVAL_TIMERS.lock().unwrap();
    for timer in intervals.iter_mut() {
        if timer.id == timer_id {
            timer.cleared = true;
            break;
        }
    }
    intervals.retain(|t| !t.cleared);
}

/// Clear an Immediate by ID. Timeout/Interval handles are distinct and are not
/// canceled by `clearImmediate`.
#[no_mangle]
pub extern "C" fn clearImmediate(timer_id: i64) {
    let mut timers = CALLBACK_TIMERS.lock().unwrap();
    for timer in timers.iter_mut() {
        if timer.id == timer_id && timer.kind == CallbackTimerKind::Immediate {
            timer.cleared = true;
            break;
        }
    }
    timers.retain(|t| !t.cleared);
}

/// Resolve a `clearTimeout`/`clearInterval` argument to a timer id. Accepts
/// both the Timeout/Immediate handle (POINTER_TAG, lower 48 bits = id) and the
/// primitive numeric id (`+timeout`), so `clearTimeout(+t)` works (#1213).
/// Returns `None` for nullish/other values (a no-op clear, matching Node).
fn arg_to_timer_id(arg: f64) -> Option<i64> {
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

// ============================================================================
// setInterval / clearInterval support
// ============================================================================

/// An interval timer that fires repeatedly
struct IntervalTimer {
    /// Unique ID for this interval
    id: i64,
    /// The closure pointer to call
    callback: i64,
    /// Interval duration in milliseconds
    interval_ms: u64,
    /// When this interval should next fire
    next_deadline: Instant,
    /// Trailing arguments to forward to the interval callback.
    args: Vec<f64>,
    /// AsyncLocalStorage context captured when the interval was scheduled.
    context: crate::async_context::AsyncContextSnapshot,
    /// Whether this interval has been cleared
    cleared: bool,
}

// SAFETY: closure pointers point to global compiled code data
unsafe impl Send for IntervalTimer {}

static INTERVAL_TIMERS: Mutex<Vec<IntervalTimer>> = Mutex::new(Vec::new());

/// JS-style setInterval that takes a callback function and interval
/// The callback is a closure pointer that will be called repeatedly
/// Returns an interval ID that can be used with clearInterval
#[no_mangle]
pub extern "C" fn setInterval(callback: i64, interval_ms: f64) -> i64 {
    schedule_interval_timer(callback, interval_ms, Vec::new())
}

fn schedule_interval_timer(callback: i64, interval_ms: f64, args: Vec<f64>) -> i64 {
    ensure_initialized();

    let interval = normalize_timer_delay(interval_ms);
    let next_deadline = Instant::now() + Duration::from_millis(interval);

    let id = next_timer_id();

    INTERVAL_TIMERS.lock().unwrap().push(IntervalTimer {
        id,
        callback,
        interval_ms: interval,
        next_deadline,
        args,
        context: crate::async_context::capture_context(),
        cleared: false,
    });
    set_timer_ref_state(id, true);

    id
}

#[no_mangle]
pub unsafe extern "C" fn js_set_interval_callback_args(
    callback: i64,
    interval_ms: f64,
    args_ptr: *const f64,
    n_args: i32,
) -> i64 {
    let args: Vec<f64> = if args_ptr.is_null() || n_args <= 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, n_args as usize).to_vec()
    };
    schedule_interval_timer(callback, interval_ms, args)
}

/// Clear an interval timer by ID. Also clears Timeout callback timers so
/// Node's interchangeable `clearInterval(timeoutHandle)` shape works.
/// Immediate handles are distinct and are only canceled by `clearImmediate`.
#[no_mangle]
pub extern "C" fn clearInterval(interval_id: i64) {
    {
        let mut timers = INTERVAL_TIMERS.lock().unwrap();
        for timer in timers.iter_mut() {
            if timer.id == interval_id {
                timer.cleared = true;
                break;
            }
        }
        timers.retain(|t| !t.cleared);
    }
    let mut callbacks = CALLBACK_TIMERS.lock().unwrap();
    for timer in callbacks.iter_mut() {
        if timer.id == interval_id && timer.kind == CallbackTimerKind::Timeout {
            timer.cleared = true;
            break;
        }
    }
    callbacks.retain(|t| !t.cleared);
}

/// Process any expired interval timers
/// Returns the number of callbacks that were called
#[no_mangle]
pub extern "C" fn js_interval_timer_tick() -> i32 {
    use crate::closure::{
        js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call3, js_closure_call4,
        js_closure_call5, js_closure_call6, js_closure_call7, js_closure_call8, js_closure_call9,
    };

    let now = Instant::now();

    // Collect callbacks to call and update deadlines
    let callbacks_to_call: Vec<(
        i64,
        i64,
        Vec<f64>,
        crate::async_context::AsyncContextSnapshot,
    )> = {
        let mut timers = INTERVAL_TIMERS.lock().unwrap();
        let mut callbacks = Vec::new();

        for timer in timers.iter_mut() {
            if !timer.cleared && timer.next_deadline <= now {
                callbacks.push((
                    timer.id,
                    timer.callback,
                    timer.args.clone(),
                    timer.context.clone(),
                ));
                timer.next_deadline = now + Duration::from_millis(timer.interval_ms);
            }
        }

        timers.retain(|t| !t.cleared);

        callbacks
    };

    let mut fired = 0;
    // Call the callbacks outside of the lock
    for (id, callback, args, context) in callbacks_to_call {
        let scope = crate::gc::RuntimeHandleScope::new();
        let callback_handle =
            scope.root_raw_const_ptr(callback as *const crate::closure::ClosureHeader);
        let arg_handles = scope.root_nanbox_f64_slice(&args);
        let previous = crate::async_context::enter_context(&context);
        let mut previous = previous;
        let previous_roots = crate::async_context::root_snapshot(&scope, &previous);
        let a = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
        let cb = callback_handle.get_raw_const_ptr();
        let prev_this = crate::object::js_implicit_this_set(timer_handle_value(id));
        with_timer_uncaught_trap(|| {
            match a.len() {
                0 => js_closure_call0(cb),
                1 => js_closure_call1(cb, a[0]),
                2 => js_closure_call2(cb, a[0], a[1]),
                3 => js_closure_call3(cb, a[0], a[1], a[2]),
                4 => js_closure_call4(cb, a[0], a[1], a[2], a[3]),
                5 => js_closure_call5(cb, a[0], a[1], a[2], a[3], a[4]),
                6 => js_closure_call6(cb, a[0], a[1], a[2], a[3], a[4], a[5]),
                7 => js_closure_call7(cb, a[0], a[1], a[2], a[3], a[4], a[5], a[6]),
                8 => js_closure_call8(cb, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]),
                _ => js_closure_call9(cb, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]),
            };
        });
        crate::object::js_implicit_this_set(prev_this);
        crate::async_context::refresh_snapshot_from_roots(&mut previous, &previous_roots);
        crate::async_context::restore_context(previous);
        fired += 1;
    }

    // NOTE: Do NOT call gc_check_trigger() here. Timer callbacks may leave
    // live values in registers (not yet stored to stack/globals). The
    // conservative GC scanner only scans the stack, so register-held
    // pointers get missed → use-after-free → SIGSEGV. GC is triggered
    // safely from arena_alloc (on block creation) and from the malloc
    // count threshold check, which fire during allocation when values are
    // guaranteed to be stored.

    fired
}

/// Check if there are any pending interval timers
#[no_mangle]
pub extern "C" fn js_interval_timer_has_pending() -> i32 {
    let timers = INTERVAL_TIMERS.lock().unwrap();
    if timers.iter().any(|t| !t.cleared) {
        1
    } else {
        0
    }
}

/// Get the time until the next interval timer fires (in ms), or -1 if no timers
#[no_mangle]
pub extern "C" fn js_interval_timer_next_deadline() -> f64 {
    let now = Instant::now();

    INTERVAL_TIMERS
        .lock()
        .unwrap()
        .iter()
        .filter(|t| !t.cleared)
        .map(|t| {
            if t.next_deadline <= now {
                0.0
            } else {
                (t.next_deadline - now).as_millis() as f64
            }
        })
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(-1.0)
}

/// GC root scanner: mark all values reachable from timer queues
pub fn scan_timer_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_timer_roots_mut(&mut visitor);
}

pub fn scan_timer_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // Scan promise-based timers
    {
        let mut q = TIMER_QUEUE.lock().unwrap();
        for timer in q.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut timer.promise);
            visitor.visit_nanbox_f64_slot(&mut timer.value);
        }
    }

    // Scan callback timers (closure pointers stored as i64)
    {
        let mut q = CALLBACK_TIMERS.lock().unwrap();
        for timer in q.iter_mut() {
            if !timer.cleared && timer.callback != 0 {
                visitor.visit_i64_slot(&mut timer.callback);
            }
            for arg in &mut timer.args {
                visitor.visit_nanbox_f64_slot(arg);
            }
            crate::async_context::scan_snapshot_roots_mut(&mut timer.context, visitor);
            for arg in &mut timer.args {
                visitor.visit_nanbox_f64_slot(arg);
            }
        }
    }

    // Scan interval timers
    {
        let mut q = INTERVAL_TIMERS.lock().unwrap();
        for timer in q.iter_mut() {
            if !timer.cleared && timer.callback != 0 {
                visitor.visit_i64_slot(&mut timer.callback);
            }
            crate::async_context::scan_snapshot_roots_mut(&mut timer.context, visitor);
        }
    }
}

#[cfg(test)]
const TEST_CALLBACK_TIMER_ID: i64 = i64::MIN + 101;
#[cfg(test)]
const TEST_INTERVAL_TIMER_ID: i64 = i64::MIN + 102;

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct TestTimerScannerSnapshot {
    pub timeout_promise_ptr: usize,
    pub timeout_value_bits: u64,
    pub callback_ptr: usize,
    pub callback_arg_bits: u64,
    pub callback_context_store_bits: u64,
    pub interval_callback_ptr: usize,
    pub interval_context_store_bits: u64,
}

#[cfg(test)]
pub(crate) fn test_seed_timer_scanner_roots(
    promise: *mut Promise,
    value: f64,
    callback: i64,
    arg: f64,
    context_store: f64,
) {
    let context = crate::async_context::test_snapshot_with_store(context_store);
    let deadline = Instant::now() + Duration::from_secs(86_400);
    TIMER_QUEUE.lock().unwrap().push(Timer {
        deadline,
        promise,
        value,
    });
    CALLBACK_TIMERS.lock().unwrap().push(CallbackTimer {
        id: TEST_CALLBACK_TIMER_ID,
        kind: CallbackTimerKind::Timeout,
        deadline,
        delay_ms: 86_400_000,
        callback,
        args: vec![arg],
        context: context.clone(),
        async_id: 0,
        trigger_async_id: 0,
        cleared: false,
    });
    INTERVAL_TIMERS.lock().unwrap().push(IntervalTimer {
        id: TEST_INTERVAL_TIMER_ID,
        callback,
        interval_ms: 86_400_000,
        next_deadline: deadline,
        args: Vec::new(),
        context,
        cleared: false,
    });
}

#[cfg(test)]
pub(crate) fn test_timer_scanner_snapshot() -> TestTimerScannerSnapshot {
    let mut snapshot = TestTimerScannerSnapshot::default();
    if let Some(timer) = TIMER_QUEUE.lock().unwrap().last() {
        snapshot.timeout_promise_ptr = timer.promise as usize;
        snapshot.timeout_value_bits = timer.value.to_bits();
    }
    if let Some(timer) = CALLBACK_TIMERS
        .lock()
        .unwrap()
        .iter()
        .find(|timer| timer.id == TEST_CALLBACK_TIMER_ID)
    {
        snapshot.callback_ptr = timer.callback as usize;
        snapshot.callback_arg_bits = timer.args.first().copied().map(f64::to_bits).unwrap_or(0);
        snapshot.callback_context_store_bits =
            crate::async_context::test_snapshot_first_store(&timer.context)
                .map(f64::to_bits)
                .unwrap_or(0);
    }
    if let Some(timer) = INTERVAL_TIMERS
        .lock()
        .unwrap()
        .iter()
        .find(|timer| timer.id == TEST_INTERVAL_TIMER_ID)
    {
        snapshot.interval_callback_ptr = timer.callback as usize;
        snapshot.interval_context_store_bits =
            crate::async_context::test_snapshot_first_store(&timer.context)
                .map(f64::to_bits)
                .unwrap_or(0);
    }
    snapshot
}

#[cfg(test)]
pub(crate) fn test_callback_timer_snapshot(timer_id: i64) -> Option<(usize, u64)> {
    CALLBACK_TIMERS
        .lock()
        .unwrap()
        .iter()
        .find(|timer| timer.id == timer_id)
        .map(|timer| {
            (
                timer.callback as usize,
                timer.args.first().copied().map(f64::to_bits).unwrap_or(0),
            )
        })
}

#[cfg(test)]
pub(crate) fn test_clear_timer_scanner_roots(promise_before: usize, promise_after: usize) {
    TIMER_QUEUE.lock().unwrap().retain(|timer| {
        let promise = timer.promise as usize;
        promise != promise_before && promise != promise_after
    });
    CALLBACK_TIMERS
        .lock()
        .unwrap()
        .retain(|timer| timer.id != TEST_CALLBACK_TIMER_ID);
    INTERVAL_TIMERS
        .lock()
        .unwrap()
        .retain(|timer| timer.id != TEST_INTERVAL_TIMER_ID);
}
