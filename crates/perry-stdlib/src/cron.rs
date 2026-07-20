//! Cron module (node-cron compatible)
//!
//! Native implementation of the 'node-cron' npm package.
//! Provides cron-based job scheduling.
//!
//! ## Architecture
//!
//! Cron jobs are dispatched the same way `setInterval` is in
//! `perry-runtime/src/timer.rs`: each scheduled job lives in a global
//! `Mutex<Vec<CronTimer>>`, gets a `next_deadline: Instant` computed from the
//! cron expression, and the main-thread event loop in
//! `perry-codegen/src/module_init.rs` calls `js_cron_timer_tick` /
//! `js_cron_timer_has_pending` alongside the interval/callback timer ticks.
//! Tick fires expired callbacks **on the main thread** via `js_closure_call0`,
//! then re-arms the deadline from the schedule's next upcoming time.
//!
//! Closure pointers stored in the timer queue are registered as mutable GC
//! roots on first schedule, mirroring how `INTERVAL_TIMERS` is scanned in
//! `perry-runtime/src/timer.rs`.
//!
//! The previous implementation spawned a tokio task per schedule and only
//! noted "we'd invoke js_callback_invoke(callback_id) here" — callbacks
//! never fired in user code.

use crate::common::{get_handle, register_handle, Handle, RUNTIME};
use cron::Schedule;
use perry_runtime::closure::{js_closure_call0, ClosureHeader};
use perry_runtime::gc::{gc_register_mutable_root_scanner_named, RuntimeRootVisitor};
use perry_runtime::{js_string_from_bytes, StringHeader};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Once};
use std::time::Instant;

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Cron job handle.
///
/// `running` is shared with the global `CRON_TIMERS` queue so `start()` /
/// `stop()` toggles whether the corresponding timer entry actually fires its
/// callback on each tick.
pub struct CronJobHandle {
    pub schedule: Schedule,
    pub running: Arc<AtomicBool>,
    /// Closure pointer (i64) — kept on the handle so `start()` after `stop()`
    /// can re-locate the entry in `CRON_TIMERS` if it was removed.
    pub callback: i64,
    /// Stable ID linking this handle to its `CRON_TIMERS` entry.
    pub timer_id: i64,
}

// Global callback counter
// #854: reserved for cron callback-id allocation; not wired into the current
// CRON_TIMERS path (callbacks are keyed by timer_id) but kept for that use.
#[allow(dead_code)]
static CALLBACK_COUNTER: AtomicU64 = AtomicU64::new(1);

// ============================================================================
// Cron timer queue (parallel to INTERVAL_TIMERS in perry-runtime/src/timer.rs)
// ============================================================================

/// One entry in the global cron tick queue.
struct CronTimer {
    /// Stable ID matching `CronJobHandle::timer_id`.
    id: i64,
    /// Cron expression — re-evaluated to compute the next deadline after each fire.
    schedule: Schedule,
    /// Closure pointer (NaN-strip'd to a raw `*const ClosureHeader`).
    /// Stored as `i64` to keep `Send` simple; reinterpreted on tick.
    callback: i64,
    /// When this job should next run. Recomputed after every fire.
    next_deadline: Instant,
    /// Whether the job is enabled (toggled by `start` / `stop`).
    /// `cleared` entries are removed from the queue at the next tick.
    running: Arc<AtomicBool>,
    /// Permanently removed entries (no future tick).
    cleared: bool,
}

// SAFETY: closure pointers point into the binary's globally-mapped code/data
// segments and remain valid for the entire program lifetime. The `Schedule`
// itself is `Send`. We never share `CronTimer` across threads except via the
// `StdMutex` below, which is `Send` as long as the inner type is.
unsafe impl Send for CronTimer {}

static CRON_TIMERS: StdMutex<Vec<CronTimer>> = StdMutex::new(Vec::new());
static CRON_NEXT_TIMER_ID: StdMutex<i64> = StdMutex::new(1);
static CRON_GC_REGISTERED: Once = Once::new();

/// Compute the next firing time for a cron schedule, or `None` if the
/// schedule has no future occurrences (which would terminate the job).
fn next_cron_instant(schedule: &Schedule) -> Option<Instant> {
    use chrono::Utc;
    let now_utc = Utc::now();
    let next_utc = schedule.upcoming(Utc).next()?;
    let delta = next_utc.signed_duration_since(now_utc);
    let ms = delta.num_milliseconds().max(0) as u64;
    Some(Instant::now() + std::time::Duration::from_millis(ms))
}

/// Register the cron GC root scanner exactly once. Safe to call from any
/// `js_cron_*` entry point on the main thread.
fn ensure_gc_scanner_registered() {
    CRON_GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("stdlib:cron", scan_cron_roots_mut);
    });
}

/// GC root scanner for cron callback closures.
///
/// Called from `gc.rs` during the mark phase, mirroring
/// `timer::scan_timer_roots`. Each non-cleared cron timer's callback pointer
/// must be marked or the closure could be freed between cron ticks.
#[allow(dead_code)]
fn scan_cron_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = RuntimeRootVisitor::for_copy(mark);
    scan_cron_roots_mut(&mut visitor);
}

fn scan_cron_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    if let Ok(mut q) = CRON_TIMERS.lock() {
        for timer in q.iter_mut() {
            if !timer.cleared {
                visitor.visit_i64_slot(&mut timer.callback);
            }
        }
    }
}

/// Remove a cron timer entry by id. Used by `js_cron_job_stop`.
fn remove_cron_timer(id: i64) {
    if let Ok(mut q) = CRON_TIMERS.lock() {
        for timer in q.iter_mut() {
            if timer.id == id {
                timer.cleared = true;
                timer.running.store(false, Ordering::SeqCst);
            }
        }
        q.retain(|t| !t.cleared);
    }
}

/// Process expired cron timers and fire their callbacks on the calling
/// thread (the main thread, since this is called from the event loop in
/// `module_init.rs`). Returns the number of callbacks fired.
#[no_mangle]
pub extern "C" fn js_cron_timer_tick() -> i32 {
    let now = Instant::now();

    // Collect callbacks to call AND update deadlines while holding the lock.
    // Calling closures while holding the lock would deadlock if a callback
    // re-entrantly calls cron.schedule.
    let callbacks: Vec<i64> = {
        let mut q = match CRON_TIMERS.lock() {
            Ok(g) => g,
            Err(_) => return 0,
        };
        let mut to_call = Vec::new();
        for timer in q.iter_mut() {
            if timer.cleared {
                continue;
            }
            if !timer.running.load(Ordering::SeqCst) {
                continue;
            }
            if timer.next_deadline > now {
                continue;
            }
            to_call.push(timer.callback);
            // Re-arm. If the schedule has no future occurrences, mark cleared.
            match next_cron_instant(&timer.schedule) {
                Some(next) => timer.next_deadline = next,
                None => timer.cleared = true,
            }
        }
        q.retain(|t| !t.cleared);
        to_call
    };

    let mut fired = 0;
    for callback in callbacks {
        if callback != 0 {
            // SAFETY: closure pointers come from compiled Perry code that
            // owns the closure for the entire program lifetime, and callbacks
            // are GC-rooted via `scan_cron_roots`.
            js_closure_call0(callback as *const ClosureHeader);
            fired += 1;
        }
    }
    fired
}

/// Returns 1 if any cron timer is currently scheduled and running, else 0.
/// Called from the CLI event loop in `module_init.rs` to keep the process
/// alive while cron jobs are pending.
#[no_mangle]
pub extern "C" fn js_cron_timer_has_pending() -> i32 {
    if let Ok(q) = CRON_TIMERS.lock() {
        if q.iter()
            .any(|t| !t.cleared && t.running.load(Ordering::SeqCst))
        {
            return 1;
        }
    }
    0
}

/// cron.validate(expression) -> boolean
///
/// Validate a cron expression.
#[no_mangle]
pub unsafe extern "C" fn js_cron_validate(expr_ptr: *const StringHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let expr = match string_from_header(expr_ptr) {
        Some(e) => e,
        None => return f64::from_bits(TAG_FALSE),
    };

    // Convert 5-field cron to 6-field (add seconds)
    let expr = if expr.split_whitespace().count() == 5 {
        format!("0 {}", expr)
    } else {
        expr
    };

    if Schedule::from_str(&expr).is_ok() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// cron.schedule(expression, callback) -> CronJob
///
/// Schedule a job with a cron expression. Per node-cron defaults, the job is
/// scheduled in the *running* state immediately — the user does not need to
/// call `.start()` (matching `cron.schedule(expr, cb).start()` being the
/// same as `cron.schedule(expr, cb)` in node-cron when `scheduled !== false`).
///
/// `callback` is the raw closure pointer (i64) — the matching codegen
/// branch in `expr.rs` ensures the second argument is passed as an `i64`
/// closure pointer rather than a NaN-boxed `f64`.
#[no_mangle]
pub unsafe extern "C" fn js_cron_schedule(expr_ptr: *const StringHeader, callback: i64) -> Handle {
    ensure_gc_scanner_registered();

    let expr = match string_from_header(expr_ptr) {
        Some(e) => e,
        None => return -1,
    };

    // Convert 5-field cron to 6-field (add seconds at position 0)
    let expr = if expr.split_whitespace().count() == 5 {
        format!("0 {}", expr)
    } else {
        expr
    };

    let schedule = match Schedule::from_str(&expr) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let next_deadline = match next_cron_instant(&schedule) {
        Some(d) => d,
        // No future occurrences — return -1 so the user knows the schedule
        // never fired (matches the `Schedule::from_str` failure path).
        None => return -1,
    };

    let timer_id = {
        let mut next = CRON_NEXT_TIMER_ID.lock().unwrap();
        let id = *next;
        *next += 1;
        id
    };

    let running = Arc::new(AtomicBool::new(true));

    if let Ok(mut q) = CRON_TIMERS.lock() {
        q.push(CronTimer {
            id: timer_id,
            schedule: schedule.clone(),
            callback,
            next_deadline,
            running: running.clone(),
            cleared: false,
        });
    }

    register_handle(CronJobHandle {
        schedule,
        running,
        callback,
        timer_id,
    })
}

/// `new CronJob(cronTime, onTick, onComplete?, start?)` — the npm `cron`
/// package's constructor. Unlike node-cron's `schedule()` factory, a
/// CronJob does **not** start automatically: it only begins firing when
/// the 4th constructor argument is truthy or `job.start()` is called.
///
/// `callback` is the raw closure pointer (i64), same convention as
/// `js_cron_schedule`. `start` is the NaN-boxed 4th argument
/// (TAG_UNDEFINED when absent → not started).
///
/// # Safety
/// `expr_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cron_job_new(
    expr_ptr: *const StringHeader,
    callback: i64,
    start: f64,
) -> Handle {
    ensure_gc_scanner_registered();

    let expr = match string_from_header(expr_ptr) {
        Some(e) => e,
        None => return -1,
    };
    // The `cron` package accepts 5-field (no seconds) and 6-field forms.
    let expr = if expr.split_whitespace().count() == 5 {
        format!("0 {}", expr)
    } else {
        expr
    };
    let schedule = match Schedule::from_str(&expr) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let start_now = perry_runtime::value::js_is_truthy(start) != 0;

    let timer_id = {
        let mut next = CRON_NEXT_TIMER_ID.lock().unwrap();
        let id = *next;
        *next += 1;
        id
    };
    let running = Arc::new(AtomicBool::new(start_now));

    if start_now {
        if let Some(next_deadline) = next_cron_instant(&schedule) {
            if let Ok(mut q) = CRON_TIMERS.lock() {
                q.push(CronTimer {
                    id: timer_id,
                    schedule: schedule.clone(),
                    callback,
                    next_deadline,
                    running: running.clone(),
                    cleared: false,
                });
            }
        }
    }

    register_handle(CronJobHandle {
        schedule,
        running,
        callback,
        timer_id,
    })
}

/// job.start() -> void
///
/// Start (or re-start) the scheduled job. After `stop()` removed it, `start()`
/// re-inserts the timer entry with a freshly-computed next deadline so it
/// fires again.
#[no_mangle]
pub unsafe extern "C" fn js_cron_job_start(handle: Handle) {
    let job = match get_handle::<CronJobHandle>(handle) {
        Some(j) => j,
        None => return,
    };
    if job.running.load(Ordering::SeqCst) {
        // Already running — make sure the timer entry exists in case it was
        // pruned by a previous stop(). This is a no-op if it's still there.
        let exists = CRON_TIMERS
            .lock()
            .map(|q| q.iter().any(|t| t.id == job.timer_id && !t.cleared))
            .unwrap_or(false);
        if exists {
            return;
        }
    }

    job.running.store(true, Ordering::SeqCst);

    let next_deadline = match next_cron_instant(&job.schedule) {
        Some(d) => d,
        None => return,
    };

    if let Ok(mut q) = CRON_TIMERS.lock() {
        // Don't double-insert if a live entry already exists.
        if q.iter().any(|t| t.id == job.timer_id && !t.cleared) {
            return;
        }
        q.push(CronTimer {
            id: job.timer_id,
            schedule: job.schedule.clone(),
            callback: job.callback,
            next_deadline,
            running: job.running.clone(),
            cleared: false,
        });
    }
}

/// job.stop() -> void
///
/// Stop the scheduled job. Removes its entry from the global tick queue so
/// `js_cron_timer_has_pending` returns 0 once no other jobs remain (which
/// lets the CLI event loop exit cleanly).
#[no_mangle]
pub unsafe extern "C" fn js_cron_job_stop(handle: Handle) {
    if let Some(job) = get_handle::<CronJobHandle>(handle) {
        job.running.store(false, Ordering::SeqCst);
        remove_cron_timer(job.timer_id);
    }
}

/// job.isRunning() -> boolean
///
/// Check if the job is currently running.
#[no_mangle]
pub unsafe extern "C" fn js_cron_job_is_running(handle: Handle) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    if let Some(job) = get_handle::<CronJobHandle>(handle) {
        if job.running.load(Ordering::SeqCst) {
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// Get the next scheduled execution time as ISO string
#[no_mangle]
pub unsafe extern "C" fn js_cron_next_date(handle: Handle) -> *mut StringHeader {
    if let Some(job) = get_handle::<CronJobHandle>(handle) {
        if let Some(next) = job.schedule.upcoming(chrono::Utc).next() {
            let iso = next.to_rfc3339();
            return js_string_from_bytes(iso.as_ptr(), iso.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get the next N scheduled execution times
#[no_mangle]
pub unsafe extern "C" fn js_cron_next_dates(
    handle: Handle,
    count: f64,
) -> *mut perry_runtime::ArrayHeader {
    use perry_runtime::{js_array_alloc, js_array_push, JSValue};

    let result = js_array_alloc(0);
    let count = count as usize;

    if let Some(job) = get_handle::<CronJobHandle>(handle) {
        for next in job.schedule.upcoming(chrono::Utc).take(count) {
            let iso = next.to_rfc3339();
            let ptr = js_string_from_bytes(iso.as_ptr(), iso.len() as u32);
            js_array_push(result, JSValue::string_ptr(ptr));
        }
    }

    result
}

/// Parse cron expression and get human-readable description
#[no_mangle]
pub unsafe extern "C" fn js_cron_describe(expr_ptr: *const StringHeader) -> *mut StringHeader {
    let expr = match string_from_header(expr_ptr) {
        Some(e) => e,
        None => return std::ptr::null_mut(),
    };

    let parts: Vec<&str> = expr.split_whitespace().collect();
    let description = match parts.len() {
        5 => {
            // minute hour day month weekday
            format!(
                "At minute {} of hour {}, on day {} of month {}, on weekday {}",
                parts[0], parts[1], parts[2], parts[3], parts[4]
            )
        }
        6 => {
            // second minute hour day month weekday
            format!(
                "At second {} minute {} of hour {}, on day {} of month {}, on weekday {}",
                parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]
            )
        }
        _ => "Invalid cron expression".to_string(),
    };

    js_string_from_bytes(description.as_ptr(), description.len() as u32)
}

// ============================================================================
// Interval/Timeout helpers (not strictly cron, but commonly used together)
// ============================================================================

/// Set an interval (simplified - returns handle)
#[no_mangle]
pub extern "C" fn js_cron_set_interval(_callback_id: f64, interval_ms: f64) -> Handle {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let interval = interval_ms as u64;

    RUNTIME.spawn(async move {
        while running_clone.load(Ordering::SeqCst) {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
            if running_clone.load(Ordering::SeqCst) {
                // Invoke callback (in real impl: js_callback_invoke(callback_id))
            }
        }
    });

    // Store running flag in a handle
    struct IntervalHandle {
        // #854: the spawned task owns `running_clone`; this handle copy keeps
        // the Arc alive for the handle's lifetime but isn't read back.
        #[allow(dead_code)]
        running: Arc<AtomicBool>,
    }

    register_handle(IntervalHandle { running })
}

/// Clear an interval
#[no_mangle]
pub unsafe extern "C" fn js_cron_clear_interval(handle: Handle) {
    struct IntervalHandle {
        running: Arc<AtomicBool>,
    }

    if let Some(interval) = get_handle::<IntervalHandle>(handle) {
        interval.running.store(false, Ordering::SeqCst);
    }
}

/// Set a timeout (simplified - returns handle)
#[no_mangle]
pub extern "C" fn js_cron_set_timeout(_callback_id: f64, timeout_ms: f64) -> Handle {
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_clone = cancelled.clone();
    let timeout = timeout_ms as u64;

    RUNTIME.spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(timeout)).await;
        if !cancelled_clone.load(Ordering::SeqCst) {
            // Invoke callback (in real impl: js_callback_invoke(callback_id))
        }
    });

    struct TimeoutHandle {
        // #854: the spawned task owns `cancelled_clone`; this handle copy keeps
        // the Arc alive for the handle's lifetime but isn't read back.
        #[allow(dead_code)]
        cancelled: Arc<AtomicBool>,
    }

    register_handle(TimeoutHandle { cancelled })
}

/// Clear a timeout
#[no_mangle]
pub unsafe extern "C" fn js_cron_clear_timeout(handle: Handle) {
    struct TimeoutHandle {
        cancelled: Arc<AtomicBool>,
    }

    if let Some(timeout) = get_handle::<TimeoutHandle>(handle) {
        timeout.cancelled.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scanner_emits_active_timer_callback() {
        let schedule = Schedule::from_str("0 * * * * *").unwrap();
        let running = Arc::new(AtomicBool::new(true));
        {
            let mut timers = CRON_TIMERS.lock().unwrap();
            timers.clear();
            timers.push(CronTimer {
                id: 1,
                schedule,
                callback: 0x1234_5678,
                next_deadline: Instant::now(),
                running,
                cleared: false,
            });
        }

        let mut emitted = Vec::new();
        scan_cron_roots(&mut |value| emitted.push(value.to_bits()));

        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x1234_5678)));
        CRON_TIMERS.lock().unwrap().clear();
    }
}
