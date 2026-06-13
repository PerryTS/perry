//! Exponential Backoff implementation
//!
//! Native implementation of the `exponential-backoff` npm package.
//!
//! `backOff(task, options?)` honors the package's real option surface
//! (#4917): `numOfAttempts`, `startingDelay`, `timeMultiple`, `maxDelay`,
//! `delayFirstAttempt`, `jitter: 'full'`, and the `retry(e, attemptNumber)`
//! predicate. Promise-returning tasks are retried on **rejection** via
//! promise reactions chained through the timer queue (no blocking
//! `thread::sleep` on the main thread); each retry waits
//! `startingDelay * timeMultiple^n` ms, capped at `maxDelay`.

use perry_runtime::promise::js_promise_then;
use perry_runtime::{
    js_closure_call0, js_closure_call2, js_is_promise, js_promise_new, js_promise_reject,
    js_promise_resolve, ClosureHeader, JSValue, Promise,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, Once};

/// Check if an f64 value represents a "real" success value.
/// NaN-boxed tagged values (pointers, strings, int32, booleans, etc.) are valid results.
/// Only raw IEEE NaN (0x7FF8_0000_0000_0000) or undefined should be treated as potential errors.
#[inline]
fn is_valid_result(result: f64) -> bool {
    let bits = result.to_bits();
    // Any non-NaN value is valid (regular numbers)
    if !result.is_nan() {
        return true;
    }
    // NaN-boxed tagged values are valid results:
    // POINTER_TAG (0x7FFD) - objects, arrays, Promises
    // INT32_TAG (0x7FFE) - integers
    // STRING_TAG (0x7FFF) - strings
    // BIGINT_TAG (0x7FFA) - bigints
    // JS_HANDLE_TAG (0x7FFB) - V8 handles
    // TAG_NULL (0x7FFC_0000_0000_0002) - null is also a valid result
    // TAG_TRUE/TAG_FALSE - booleans
    let tag = bits >> 48;
    // 0x7FFC..0x7FFF are all our custom tags (booleans, pointers, ints, strings)
    // 0x7FFA, 0x7FFB are bigint and JS handle tags
    // Only IEEE quiet NaN (0x7FF8) with no tag is a "real" NaN
    tag >= 0x7FFA
}

fn js_undefined() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

/// Options mirroring the npm package's `BackoffOptions` (with its defaults:
/// 10 attempts, 100ms starting delay, x2 multiple, uncapped maxDelay,
/// no jitter, first attempt not delayed).
struct BackoffOptions {
    num_of_attempts: u32,
    starting_delay: f64,
    time_multiple: f64,
    max_delay: f64,
    delay_first_attempt: bool,
    jitter_full: bool,
    /// NaN-box bits of the `retry` predicate closure, or 0 when absent.
    retry_cb: u64,
}

impl Default for BackoffOptions {
    fn default() -> Self {
        BackoffOptions {
            num_of_attempts: 10,
            starting_delay: 100.0,
            time_multiple: 2.0,
            max_delay: f64::INFINITY,
            delay_first_attempt: false,
            jitter_full: false,
            retry_cb: 0,
        }
    }
}

/// One in-flight `backOff()` call. `task`/`outer`/`retry_cb` hold NaN-box
/// bits and are GC-rooted by `scan_backoff_roots` for the life of the entry.
struct BackoffState {
    /// NaN-box bits of the task closure.
    task: u64,
    /// NaN-box bits (POINTER_TAG) of the outer promise returned to JS.
    outer: u64,
    /// Attempts completed (successfully started and settled/failed).
    attempts_done: u32,
    opts: BackoffOptions,
}

static STATES: LazyLock<Mutex<HashMap<u64, BackoffState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static GC_REGISTERED: Once = Once::new();

fn ensure_backoff_gc_scanner() {
    GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:exponential-backoff",
            scan_backoff_roots,
        );
    });
}

fn scan_backoff_roots(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut states) = STATES.lock() {
        for st in states.values_mut() {
            let mut task = st.task as i64;
            visitor.visit_i64_slot(&mut task);
            st.task = task as u64;
            let mut outer = st.outer as i64;
            visitor.visit_i64_slot(&mut outer);
            st.outer = outer as u64;
            if st.opts.retry_cb != 0 {
                let mut cb = st.opts.retry_cb as i64;
                visitor.visit_i64_slot(&mut cb);
                st.opts.retry_cb = cb as u64;
            }
        }
    }
}

unsafe fn option_field(ptr: *const perry_runtime::ObjectHeader, name: &[u8]) -> f64 {
    let key = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::object::js_object_get_field_by_name_f64(ptr, key)
}

fn option_number(value: f64) -> Option<f64> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_int32() {
        Some(jv.as_int32() as f64)
    } else if jv.is_number() && !value.is_nan() {
        Some(value)
    } else {
        None
    }
}

unsafe fn parse_options(options: f64) -> BackoffOptions {
    let mut opts = BackoffOptions::default();
    let jv = JSValue::from_bits(options.to_bits());
    if !jv.is_pointer() {
        return opts;
    }
    let ptr = jv.as_pointer::<perry_runtime::ObjectHeader>();
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return opts;
    }

    if let Some(n) = option_number(option_field(ptr, b"numOfAttempts")) {
        opts.num_of_attempts = n.max(1.0) as u32;
    }
    if let Some(n) = option_number(option_field(ptr, b"startingDelay")) {
        opts.starting_delay = n.max(0.0);
    }
    if let Some(n) = option_number(option_field(ptr, b"timeMultiple")) {
        opts.time_multiple = n.max(1.0);
    }
    if let Some(n) = option_number(option_field(ptr, b"maxDelay")) {
        opts.max_delay = n.max(0.0);
    }
    let dfa = option_field(ptr, b"delayFirstAttempt");
    if perry_runtime::value::js_is_truthy(dfa) != 0 {
        opts.delay_first_attempt = true;
    }
    // `jitter` is the string 'full' (anything else, including the default
    // 'none', means no jitter).
    let jitter = option_field(ptr, b"jitter");
    if JSValue::from_bits(jitter.to_bits()).is_any_string() {
        let s = perry_runtime::js_get_string_pointer_unified(jitter)
            as *const perry_runtime::StringHeader;
        if !s.is_null() && (*s).byte_len == 4 {
            let data = (s as *const u8).add(std::mem::size_of::<perry_runtime::StringHeader>());
            if std::slice::from_raw_parts(data, 4) == b"full" {
                opts.jitter_full = true;
            }
        }
    }
    let retry = option_field(ptr, b"retry");
    let retry_bits = retry.to_bits();
    if JSValue::from_bits(retry_bits).is_pointer() {
        let raw = (retry_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if perry_runtime::closure::is_closure_ptr(raw) {
            opts.retry_cb = retry_bits;
        }
    }
    opts
}

/// Build a 1-arg promise-reaction closure capturing the backoff state id.
fn bound_reaction(func_ptr: *const u8, state_id: u64) -> *const ClosureHeader {
    perry_runtime::closure::js_register_closure_arity(func_ptr, 1);
    let closure = perry_runtime::closure::js_closure_alloc(func_ptr, 1);
    perry_runtime::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(state_id));
    closure as *const ClosureHeader
}

fn state_id_from_closure(closure: *const ClosureHeader) -> u64 {
    perry_runtime::closure::js_closure_get_capture_f64(closure, 0).to_bits()
}

fn settle(id: u64, resolve: bool, value: f64) {
    let Some(st) = STATES.lock().unwrap().remove(&id) else {
        return;
    };
    let outer = (st.outer & 0x0000_FFFF_FFFF_FFFF) as *mut Promise;
    if resolve {
        js_promise_resolve(outer, value);
    } else {
        js_promise_reject(outer, value);
    }
}

/// Delay before attempt `attempts_done + 1`, mirroring the package's
/// `SkipFirstDelay` (power `attempts_done - 1`) vs `AlwaysDelay`
/// (power `attempts_done`) factories, capped at `maxDelay`, with optional
/// full jitter.
fn next_delay_ms(st: &BackoffState) -> f64 {
    let power = if st.opts.delay_first_attempt {
        st.attempts_done as f64
    } else {
        (st.attempts_done as f64 - 1.0).max(0.0)
    };
    let mut delay = st.opts.starting_delay * st.opts.time_multiple.powf(power);
    if !delay.is_finite() {
        delay = st.opts.max_delay;
    }
    delay = delay.min(st.opts.max_delay);
    if st.opts.jitter_full {
        // Cheap jitter source — the npm package only needs uniform-ish
        // `random() * delay`, not crypto-grade randomness.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        delay *= (nanos % 1_000_000) as f64 / 1_000_000.0;
    }
    if delay.is_finite() {
        delay.max(0.0)
    } else {
        0.0
    }
}

fn schedule_next_attempt(id: u64) {
    let delay = {
        let states = STATES.lock().unwrap();
        let Some(st) = states.get(&id) else { return };
        next_delay_ms(st)
    };
    let timer = perry_runtime::timer::js_set_timeout_value_ref(delay, js_undefined(), 1);
    js_promise_then(
        timer,
        bound_reaction(backoff_on_timer as *const u8, id),
        std::ptr::null(),
    );
}

/// A completed attempt failed with `error`. Either retry (after consulting
/// the `retry` predicate and scheduling the backoff delay) or reject.
fn handle_failure(id: u64, error: f64) {
    let (attempts_done, exhausted, retry_cb) = {
        let mut states = STATES.lock().unwrap();
        let Some(st) = states.get_mut(&id) else {
            return;
        };
        st.attempts_done += 1;
        (
            st.attempts_done,
            st.attempts_done >= st.opts.num_of_attempts,
            st.opts.retry_cb,
        )
    };
    if exhausted {
        settle(id, false, error);
        return;
    }
    if retry_cb != 0 {
        // npm: `const shouldRetry = await retry(e, attemptNumber)`; falsy stops
        // and rethrows. (A promise-returning predicate is treated as truthy —
        // Perry does not await it.)
        let cb = ((retry_cb & 0x0000_FFFF_FFFF_FFFF) as usize) as *const ClosureHeader;
        let should_retry = js_closure_call2(cb, error, attempts_done as f64);
        if perry_runtime::value::js_is_truthy(should_retry) == 0 {
            settle(id, false, error);
            return;
        }
    }
    schedule_next_attempt(id);
}

fn run_attempt(id: u64) {
    let task = {
        let states = STATES.lock().unwrap();
        let Some(st) = states.get(&id) else { return };
        st.task
    };
    let task_ptr = ((task & 0x0000_FFFF_FFFF_FFFF) as usize) as *const ClosureHeader;
    let result = js_closure_call0(task_ptr);

    let bits = result.to_bits();
    if JSValue::from_bits(bits).is_pointer() {
        let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut Promise;
        if !raw.is_null() && js_is_promise(raw) != 0 {
            js_promise_then(
                raw,
                bound_reaction(backoff_on_fulfilled as *const u8, id),
                bound_reaction(backoff_on_rejected as *const u8, id),
            );
            return;
        }
    }
    if is_valid_result(result) {
        settle(id, true, result);
    } else {
        handle_failure(id, result);
    }
}

extern "C" fn backoff_on_fulfilled(closure: *const ClosureHeader, value: f64) -> f64 {
    settle(state_id_from_closure(closure), true, value);
    js_undefined()
}

extern "C" fn backoff_on_rejected(closure: *const ClosureHeader, error: f64) -> f64 {
    handle_failure(state_id_from_closure(closure), error);
    js_undefined()
}

extern "C" fn backoff_on_timer(closure: *const ClosureHeader, _value: f64) -> f64 {
    run_attempt(state_id_from_closure(closure));
    js_undefined()
}

/// Execute a task with exponential-backoff retry.
///
/// `fn_ptr` is the task closure (codegen `NA_PTR`: raw extracted pointer);
/// `options` is the NaN-boxed options object (codegen `NA_F64`). Returns a
/// Promise that resolves with the first successful result or rejects with
/// the last error once attempts are exhausted or the `retry` predicate says
/// stop.
#[no_mangle]
pub extern "C" fn backOff(fn_ptr: *const ClosureHeader, options: f64) -> *mut Promise {
    let promise = js_promise_new();
    if fn_ptr.is_null() {
        js_promise_reject(promise, f64::NAN);
        return promise;
    }
    ensure_backoff_gc_scanner();

    let opts = unsafe { parse_options(options) };
    let delay_first = opts.delay_first_attempt;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    STATES.lock().unwrap().insert(
        id,
        BackoffState {
            task: JSValue::pointer(fn_ptr as *const u8).bits(),
            outer: JSValue::pointer(promise as *const u8).bits(),
            attempts_done: 0,
            opts,
        },
    );

    if delay_first {
        schedule_next_attempt(id);
    } else {
        run_attempt(id);
    }
    promise
}

/// Simplified backOff that takes just the function and retry count
#[no_mangle]
pub extern "C" fn js_backoff_simple(
    fn_ptr: *const ClosureHeader,
    num_attempts: i32,
    delay_ms: i32,
) -> f64 {
    if fn_ptr.is_null() {
        return f64::NAN;
    }

    let mut attempt = 0;
    let mut current_delay = delay_ms.max(10) as u64;

    loop {
        attempt += 1;

        // Call the function
        let result = js_closure_call0(fn_ptr);

        // Success if valid result
        if is_valid_result(result) {
            return result;
        }

        // Check if we've exhausted retries
        if attempt >= num_attempts {
            return f64::NAN;
        }

        // Wait before retrying
        std::thread::sleep(std::time::Duration::from_millis(current_delay));

        // Increase delay exponentially
        current_delay = (current_delay * 2).min(10000);
    }
}
