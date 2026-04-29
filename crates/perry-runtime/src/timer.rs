//! Timer support for setTimeout/setInterval
//!
//! Provides a simple timer queue that integrates with the Promise runtime.
//!
//! Uses global Mutex-protected state (not thread_local) so that timers
//! registered on one thread can be pumped from another. This is critical
//! on Android where TypeScript runs on the perry-native thread but the
//! timer pump fires on the UI thread.

use crate::promise::{js_promise_new, js_promise_resolve, Promise};
use std::sync::Mutex;
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
    let delay = Duration::from_millis(delay_ms.max(0.0) as u64);
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
    let delay = Duration::from_millis(delay_ms.max(0.0) as u64);
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
        js_promise_resolve(timer.promise, timer.value);
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
struct CallbackTimer {
    /// Unique ID for this timer
    id: i64,
    /// When this timer should fire
    deadline: Instant,
    /// The closure pointer to call
    callback: i64,
    /// Whether this timer has been cleared
    cleared: bool,
}

// SAFETY: closure pointers point to global compiled code data
unsafe impl Send for CallbackTimer {}

static CALLBACK_TIMERS: Mutex<Vec<CallbackTimer>> = Mutex::new(Vec::new());
static NEXT_CALLBACK_TIMER_ID: Mutex<i64> = Mutex::new(1);

/// JS-style setTimeout that takes a callback function and delay
/// The callback is a closure pointer that will be called with no arguments
/// Returns a timer ID
#[no_mangle]
pub extern "C" fn js_set_timeout_callback(callback: i64, delay_ms: f64) -> i64 {
    ensure_initialized();

    let delay = Duration::from_millis(delay_ms.max(0.0) as u64);
    let deadline = Instant::now() + delay;

    let id = {
        let mut next = NEXT_CALLBACK_TIMER_ID.lock().unwrap();
        let current = *next;
        *next += 1;
        current
    };

    CALLBACK_TIMERS.lock().unwrap().push(CallbackTimer {
        id,
        deadline,
        callback,
        cleared: false,
    });

    id
}

/// Process any expired callback timers
/// Returns the number of callbacks that were called
#[no_mangle]
pub extern "C" fn js_callback_timer_tick() -> i32 {
    use crate::closure::js_closure_call0;

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
    // Call the callbacks
    for timer in expired {
        if !timer.cleared {
            unsafe {
                js_closure_call0(timer.callback as *const crate::closure::ClosureHeader);
            }
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

/// Clear a callback timer by ID
#[no_mangle]
pub extern "C" fn clearTimeout(timer_id: i64) {
    let mut timers = CALLBACK_TIMERS.lock().unwrap();
    for timer in timers.iter_mut() {
        if timer.id == timer_id {
            timer.cleared = true;
            break;
        }
    }
    timers.retain(|t| !t.cleared);
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
    /// Whether this interval has been cleared
    cleared: bool,
}

// SAFETY: closure pointers point to global compiled code data
unsafe impl Send for IntervalTimer {}

static INTERVAL_TIMERS: Mutex<Vec<IntervalTimer>> = Mutex::new(Vec::new());
static NEXT_INTERVAL_ID: Mutex<i64> = Mutex::new(1);

/// JS-style setInterval that takes a callback function and interval
/// The callback is a closure pointer that will be called repeatedly
/// Returns an interval ID that can be used with clearInterval
#[no_mangle]
pub extern "C" fn setInterval(callback: i64, interval_ms: f64) -> i64 {
    ensure_initialized();

    let interval = interval_ms.max(0.0) as u64;
    let next_deadline = Instant::now() + Duration::from_millis(interval);

    let id = {
        let mut next = NEXT_INTERVAL_ID.lock().unwrap();
        let current = *next;
        *next += 1;
        current
    };

    INTERVAL_TIMERS.lock().unwrap().push(IntervalTimer {
        id,
        callback,
        interval_ms: interval,
        next_deadline,
        cleared: false,
    });

    id
}

/// Clear an interval timer by ID
#[no_mangle]
pub extern "C" fn clearInterval(interval_id: i64) {
    let mut timers = INTERVAL_TIMERS.lock().unwrap();
    for timer in timers.iter_mut() {
        if timer.id == interval_id {
            timer.cleared = true;
            break;
        }
    }
    timers.retain(|t| !t.cleared);
}

/// Process any expired interval timers
/// Returns the number of callbacks that were called
#[no_mangle]
pub extern "C" fn js_interval_timer_tick() -> i32 {
    use crate::closure::js_closure_call0;

    let now = Instant::now();

    // Collect callbacks to call and update deadlines
    let callbacks_to_call: Vec<i64> = {
        let mut timers = INTERVAL_TIMERS.lock().unwrap();
        let mut callbacks = Vec::new();

        for timer in timers.iter_mut() {
            if !timer.cleared && timer.next_deadline <= now {
                callbacks.push(timer.callback);
                timer.next_deadline = now + Duration::from_millis(timer.interval_ms);
            }
        }

        timers.retain(|t| !t.cleared);

        callbacks
    };

    let mut fired = 0;
    // Call the callbacks outside of the lock
    for callback in callbacks_to_call {
        unsafe {
            js_closure_call0(callback as *const crate::closure::ClosureHeader);
        }
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
    // Scan promise-based timers
    {
        let q = TIMER_QUEUE.lock().unwrap();
        for timer in q.iter() {
            if !timer.promise.is_null() {
                let boxed = f64::from_bits(
                    0x7FFD_0000_0000_0000 | (timer.promise as u64 & 0x0000_FFFF_FFFF_FFFF),
                );
                mark(boxed);
            }
            mark(timer.value);
        }
    }

    // Scan callback timers (closure pointers stored as i64)
    {
        let q = CALLBACK_TIMERS.lock().unwrap();
        for timer in q.iter() {
            if !timer.cleared && timer.callback != 0 {
                let boxed = f64::from_bits(
                    0x7FFD_0000_0000_0000 | (timer.callback as u64 & 0x0000_FFFF_FFFF_FFFF),
                );
                mark(boxed);
            }
        }
    }

    // Scan interval timers
    {
        let q = INTERVAL_TIMERS.lock().unwrap();
        for timer in q.iter() {
            if !timer.cleared && timer.callback != 0 {
                let boxed = f64::from_bits(
                    0x7FFD_0000_0000_0000 | (timer.callback as u64 & 0x0000_FFFF_FFFF_FFFF),
                );
                mark(boxed);
            }
        }
    }
}
