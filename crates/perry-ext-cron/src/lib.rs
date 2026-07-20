//! Native bindings for the npm `node-cron` package — cron-expression
//! scheduling. Sync setup; callbacks fire on the main thread via
//! `js_cron_timer_tick`, called from the codegen-emitted event-loop
//! pump in `module_init.rs`.
//!
//! Architecture parallels perry-stdlib's existing copy:
//!   - Global `CRON_TIMERS` queue holds every scheduled job.
//!   - `js_cron_timer_tick()` walks the queue, invokes any expired
//!     callbacks via `JsClosure::call0`, and re-arms next deadlines.
//!   - `js_cron_timer_has_pending()` lets the event loop know whether
//!     to keep the process alive.
//!   - GC root scanner exposes each job's callback closure pointer so a
//!     malloc-triggered sweep between scheduling and tick can't free it
//!     and copied-minor GC can rewrite moved callbacks in place (issue
//!     #35 pattern).
//!
//! Codegen calls `js_cron_timer_tick` and `js_cron_timer_has_pending`
//! by symbol name, so as long as the `.a` perry-ext-cron produces
//! exports them, the well-known flip can swap perry-stdlib's copy
//! out without touching codegen.

use chrono::Utc;
use cron::Schedule;
use perry_ffi::{
    alloc_string, gc_register_mutable_root_scanner_named, get_handle, iter_handles_of_mut,
    js_array_alloc, js_array_push, register_handle, GcRootVisitor, Handle, JsClosure, JsString,
    JsValue, RawClosureHeader, StringHeader,
};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Once};
use std::time::Instant;

const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

pub struct CronJobHandle {
    pub schedule: Schedule,
    pub running: Arc<AtomicBool>,
    pub callback: i64,
    pub timer_id: i64,
}

struct CronTimer {
    id: i64,
    schedule: Schedule,
    callback: i64,
    next_deadline: Instant,
    running: Arc<AtomicBool>,
    cleared: bool,
}

// SAFETY: closure pointers point into the program's globally-mapped
// code/data and remain valid for the lifetime of the program. The
// `Schedule` itself is `Send`. Sharing across threads only happens
// via the `StdMutex` below.
unsafe impl Send for CronTimer {}

static CRON_TIMERS: StdMutex<Vec<CronTimer>> = StdMutex::new(Vec::new());
static CRON_NEXT_TIMER_ID: StdMutex<i64> = StdMutex::new(1);
static CRON_GC_REGISTERED: Once = Once::new();

fn next_cron_instant(schedule: &Schedule) -> Option<Instant> {
    let now_utc = Utc::now();
    let next_utc = schedule.upcoming(Utc).next()?;
    let delta = next_utc.signed_duration_since(now_utc);
    let ms = delta.num_milliseconds().max(0) as u64;
    Some(Instant::now() + std::time::Duration::from_millis(ms))
}

fn ensure_gc_scanner_registered() {
    CRON_GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-cron", scan_cron_roots);
    });
}

fn scan_cron_roots(visitor: &mut GcRootVisitor<'_>) {
    if let Ok(mut q) = CRON_TIMERS.lock() {
        for timer in q.iter_mut() {
            if !timer.cleared {
                visitor.visit_i64_slot(&mut timer.callback);
            }
        }
    }
    iter_handles_of_mut::<CronJobHandle, _>(|job| {
        visitor.visit_i64_slot(&mut job.callback);
    });
}

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

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let h = JsString::from_raw(ptr as *mut StringHeader);
    perry_ffi::read_string(h).map(String::from)
}

/// Process expired cron timers and fire callbacks on the calling
/// thread (the main thread, since this is called from
/// `module_init.rs`'s event-loop tick). Returns the number of
/// callbacks fired.
#[no_mangle]
pub extern "C" fn js_cron_timer_tick() -> i32 {
    let now = Instant::now();

    let callbacks: Vec<i64> = {
        let mut q = match CRON_TIMERS.lock() {
            Ok(g) => g,
            Err(_) => return 0,
        };
        let mut to_call = Vec::new();
        for timer in q.iter_mut() {
            if timer.cleared || !timer.running.load(Ordering::SeqCst) {
                continue;
            }
            if timer.next_deadline > now {
                continue;
            }
            to_call.push(timer.callback);
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
            // SAFETY: closure pointers come from compiled Perry code
            // that owns the closure for the program lifetime, and
            // they're GC-rooted via `scan_cron_roots`.
            let closure = unsafe { JsClosure::from_raw(callback as *const RawClosureHeader) };
            let _ = unsafe { closure.call0() };
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

/// `cron.validate(expression)` — accepts both 5-field and 6-field
/// expressions. Returns NaN-boxed `TAG_TRUE` / `TAG_FALSE`.
///
/// # Safety
/// `expr_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cron_validate(expr_ptr: *const StringHeader) -> f64 {
    let expr = match read_str(expr_ptr) {
        Some(e) => e,
        None => return f64::from_bits(TAG_FALSE),
    };
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

/// `cron.schedule(expression, callback)` — schedule a cron job.
/// `callback` is the raw closure pointer (i64) — codegen passes it
/// as i64 instead of NaN-boxed f64 since closures are GC-rooted.
///
/// # Safety
/// `expr_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cron_schedule(expr_ptr: *const StringHeader, callback: i64) -> Handle {
    ensure_gc_scanner_registered();

    let expr = match read_str(expr_ptr) {
        Some(e) => e,
        None => return -1,
    };
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

extern "C" {
    /// perry-runtime: JS truthiness probe for a NaN-boxed value.
    fn js_is_truthy(value: f64) -> i32;
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

    let expr = match read_str(expr_ptr) {
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

    let start_now = js_is_truthy(start) != 0;

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

#[no_mangle]
pub extern "C" fn js_cron_job_start(handle: Handle) {
    let job = match get_handle::<CronJobHandle>(handle) {
        Some(j) => j,
        None => return,
    };
    if job.running.load(Ordering::SeqCst) {
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

#[no_mangle]
pub extern "C" fn js_cron_job_stop(handle: Handle) {
    if let Some(job) = get_handle::<CronJobHandle>(handle) {
        job.running.store(false, Ordering::SeqCst);
        remove_cron_timer(job.timer_id);
    }
}

#[no_mangle]
pub extern "C" fn js_cron_job_is_running(handle: Handle) -> f64 {
    if let Some(job) = get_handle::<CronJobHandle>(handle) {
        if job.running.load(Ordering::SeqCst) {
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_cron_next_date(handle: Handle) -> *mut StringHeader {
    if let Some(job) = get_handle::<CronJobHandle>(handle) {
        if let Some(next) = job.schedule.upcoming(chrono::Utc).next() {
            return alloc_string(&next.to_rfc3339()).as_raw();
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn js_cron_next_dates(handle: Handle, count: f64) -> *mut perry_ffi::ArrayHeader {
    let mut result = unsafe { js_array_alloc(0) };
    let count = count as usize;
    if let Some(job) = get_handle::<CronJobHandle>(handle) {
        for next in job.schedule.upcoming(chrono::Utc).take(count) {
            let s = alloc_string(&next.to_rfc3339());
            result = unsafe { js_array_push(result, JsValue::from_string_ptr(s.as_raw())) };
        }
    }
    result
}

/// # Safety
/// `expr_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cron_describe(expr_ptr: *const StringHeader) -> *mut StringHeader {
    let expr = match read_str(expr_ptr) {
        Some(e) => e,
        None => return std::ptr::null_mut(),
    };
    let parts: Vec<&str> = expr.split_whitespace().collect();
    let description = match parts.len() {
        5 => format!(
            "At minute {} of hour {}, on day {} of month {}, on weekday {}",
            parts[0], parts[1], parts[2], parts[3], parts[4]
        ),
        6 => format!(
            "At second {} minute {} of hour {}, on day {} of month {}, on weekday {}",
            parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]
        ),
        _ => "Invalid cron expression".to_string(),
    };
    alloc_string(&description).as_raw()
}

// ── setInterval / setTimeout placeholders ─────────────────────────
//
// These export the same symbols perry-stdlib's cron module did but
// are intentionally non-firing — perry-stdlib's existing implementation
// has the same gap (the `// Invoke callback (in real impl)` comment is
// load-bearing). They keep the symbol surface stable; wiring the
// callback path through perry-ffi's spawn_blocking + JsClosure is a
// separate followup once a wrapper actually needs them.

struct IntervalHandle {
    running: Arc<AtomicBool>,
}

struct TimeoutHandle {
    cancelled: Arc<AtomicBool>,
}

#[no_mangle]
pub extern "C" fn js_cron_set_interval(_callback_id: f64, _interval_ms: f64) -> Handle {
    let running = Arc::new(AtomicBool::new(true));
    register_handle(IntervalHandle { running })
}

#[no_mangle]
pub extern "C" fn js_cron_clear_interval(handle: Handle) {
    if let Some(interval) = get_handle::<IntervalHandle>(handle) {
        interval.running.store(false, Ordering::SeqCst);
    }
}

#[no_mangle]
pub extern "C" fn js_cron_set_timeout(_callback_id: f64, _timeout_ms: f64) -> Handle {
    let cancelled = Arc::new(AtomicBool::new(false));
    register_handle(TimeoutHandle { cancelled })
}

#[no_mangle]
pub extern "C" fn js_cron_clear_timeout(handle: Handle) {
    if let Some(timeout) = get_handle::<TimeoutHandle>(handle) {
        timeout.cancelled.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{drop_handle, get_handle};
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn assert_rewritten(before: i64, after: i64) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after as usize));
    }

    #[test]
    fn gc_mutable_scanner_rewrites_timer_and_stopped_job_roots() {
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-cron", scan_cron_roots);

        let schedule = Schedule::from_str("0 0 * * * *").expect("valid schedule");
        let running = Arc::new(AtomicBool::new(true));
        let timer_callback = young_gc_root();
        let timer_id = -9_001;
        CRON_TIMERS.lock().unwrap().push(CronTimer {
            id: timer_id,
            schedule: schedule.clone(),
            callback: timer_callback,
            next_deadline: Instant::now(),
            running: running.clone(),
            cleared: false,
        });

        let stopped_callback = young_gc_root();
        let stopped = Arc::new(AtomicBool::new(false));
        let handle = register_handle(CronJobHandle {
            schedule,
            running: stopped,
            callback: stopped_callback,
            timer_id: -9_002,
        });

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let timers = CRON_TIMERS.lock().unwrap();
            let timer = timers
                .iter()
                .find(|timer| timer.id == timer_id)
                .expect("timer should remain live");
            assert_rewritten(timer_callback, timer.callback);
            let job = get_handle::<CronJobHandle>(handle).expect("job handle should remain live");
            assert_rewritten(stopped_callback, job.callback);
        }
        CRON_TIMERS
            .lock()
            .unwrap()
            .retain(|timer| timer.id != timer_id);
        drop_handle(handle);
    }

    #[test]
    fn validate_5_field_cron() {
        let s = alloc_string("* * * * *");
        let v = unsafe { js_cron_validate(s.as_raw()) };
        assert_eq!(v.to_bits(), TAG_TRUE);
    }

    #[test]
    fn validate_6_field_cron() {
        let s = alloc_string("0 * * * * *");
        let v = unsafe { js_cron_validate(s.as_raw()) };
        assert_eq!(v.to_bits(), TAG_TRUE);
    }

    #[test]
    fn validate_garbage_rejects() {
        let s = alloc_string("not-a-cron-expression");
        let v = unsafe { js_cron_validate(s.as_raw()) };
        assert_eq!(v.to_bits(), TAG_FALSE);
    }

    #[test]
    fn schedule_then_stop() {
        let _lock = GC_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let s = alloc_string("0 0 * * * *"); // top of every hour
        let h = unsafe { js_cron_schedule(s.as_raw(), 0xDEADBEEF_i64) };
        assert!(h >= 0);
        assert_eq!(js_cron_job_is_running(h).to_bits(), TAG_TRUE);
        assert_eq!(js_cron_timer_has_pending(), 1);
        js_cron_job_stop(h);
        assert_eq!(js_cron_job_is_running(h).to_bits(), TAG_FALSE);
        drop_handle(h);
    }

    #[test]
    fn describe_renders() {
        let s = alloc_string("0 0 * * *");
        let p = unsafe { js_cron_describe(s.as_raw()) };
        let out = perry_ffi::read_string(unsafe { JsString::from_raw(p) }).expect("non-null");
        assert!(out.contains("At minute"));
    }
}
