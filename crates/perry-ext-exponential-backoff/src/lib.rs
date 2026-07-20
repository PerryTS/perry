//! Native bindings for the npm `exponential-backoff` package.
//!
//! Port of `crates/perry-stdlib/src/exponential_backoff.rs` (#4917) onto
//! the perry-ffi surface plus a handful of C-ABI runtime symbols
//! (declared below; resolved at final link — same pattern as
//! perry-ext-events' by-name field reads).
//!
//! `backOff(task, options?)` honors the package's real option surface:
//! `numOfAttempts` (default 10), `startingDelay` (100 ms),
//! `timeMultiple` (x2), `maxDelay` (uncapped), `delayFirstAttempt`
//! (false), `jitter: 'full'` (default `'none'`), and the
//! `retry(e, attemptNumber)` predicate. Promise-returning tasks retry
//! on **rejection** via promise reactions chained through the timer
//! queue — no blocking `thread::sleep` on the main thread.
//!
//! The previous version of this wrapper parsed nothing and hardcoded
//! 3 attempts / 100 ms / x2 / 10 s, retried only on raw-NaN results,
//! and returned a promise-returning task's first promise directly
//! (no retry at all) — none of which matches the npm package.

use perry_ffi::{JsValue, ObjectHeader, Promise, RawClosureHeader, StringHeader};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, Once};

const NANBOX_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

extern "C" {
    // perry-runtime promise surface (C ABI).
    fn js_promise_new() -> *mut Promise;
    fn js_promise_resolve(promise: *mut Promise, value: f64);
    fn js_promise_reject(promise: *mut Promise, reason: f64);
    fn js_promise_then(
        promise: *mut Promise,
        on_fulfilled: *const RawClosureHeader,
        on_rejected: *const RawClosureHeader,
    ) -> *mut Promise;
    fn js_is_promise(ptr: *mut Promise) -> i32;
    // perry-runtime closure surface.
    fn js_closure_call0(closure: *const RawClosureHeader) -> f64;
    fn js_closure_call2(closure: *const RawClosureHeader, arg0: f64, arg1: f64) -> f64;
    fn js_closure_alloc(func_ptr: *const u8, capture_count: u32) -> *mut RawClosureHeader;
    fn js_closure_set_capture_f64(closure: *mut RawClosureHeader, index: u32, value: f64);
    fn js_closure_get_capture_f64(closure: *const RawClosureHeader, index: u32) -> f64;
    fn js_register_closure_arity(func_ptr: *const u8, arity: u32);
    // perry-runtime timer queue (promise resolved after `delay_ms`).
    fn js_set_timeout_value_ref(delay_ms: f64, value: f64, has_ref: i32) -> *mut Promise;
    // perry-runtime object / value probes.
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    fn js_is_truthy(value: f64) -> i32;
    fn js_get_string_pointer_unified(value: f64) -> i64;
}

/// Check if an f64 value represents a "real" success value. NaN-boxed
/// tagged values (pointers, strings, int32, booleans, null, …) are valid
/// results; only raw IEEE NaN signals "treat as failure, retry" for
/// synchronous tasks.
#[inline]
fn is_valid_result(result: f64) -> bool {
    if !result.is_nan() {
        return true;
    }
    let tag = result.to_bits() >> 48;
    tag >= 0x7FFA
}

fn js_undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

/// Options mirroring the npm package's `BackoffOptions` (with its
/// defaults: 10 attempts, 100 ms starting delay, x2 multiple, uncapped
/// maxDelay, no jitter, first attempt not delayed).
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
/// bits and are GC-rooted by `scan_backoff_roots` for the life of the
/// entry.
struct BackoffState {
    /// NaN-box bits of the task closure.
    task: u64,
    /// NaN-box bits (POINTER_TAG) of the outer promise returned to JS.
    outer: u64,
    /// Attempts completed (started and settled/failed).
    attempts_done: u32,
    opts: BackoffOptions,
}

static STATES: LazyLock<Mutex<HashMap<u64, BackoffState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static GC_REGISTERED: Once = Once::new();

fn ensure_backoff_gc_scanner() {
    GC_REGISTERED.call_once(|| {
        perry_ffi::gc_register_mutable_root_scanner_named(
            "perry-ext-exponential-backoff",
            scan_backoff_roots,
        );
    });
}

fn scan_backoff_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
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

unsafe fn option_field(ptr: *const ObjectHeader, name: &str) -> f64 {
    let key = perry_ffi::alloc_string(name);
    js_object_get_field_by_name_f64(ptr, key.as_raw())
}

fn option_number(value: f64) -> Option<f64> {
    let jv = JsValue::from_bits(value.to_bits());
    if jv.is_int32() {
        Some(jv.to_int32() as f64)
    } else if jv.is_number() && !value.is_nan() {
        Some(value)
    } else {
        None
    }
}

unsafe fn parse_options(options: f64) -> BackoffOptions {
    let mut opts = BackoffOptions::default();
    let jv = JsValue::from_bits(options.to_bits());
    if !jv.is_pointer() {
        return opts;
    }
    let ptr = jv.as_pointer::<ObjectHeader>();
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return opts;
    }

    if let Some(n) = option_number(option_field(ptr, "numOfAttempts")) {
        opts.num_of_attempts = n.max(1.0) as u32;
    }
    if let Some(n) = option_number(option_field(ptr, "startingDelay")) {
        opts.starting_delay = n.max(0.0);
    }
    if let Some(n) = option_number(option_field(ptr, "timeMultiple")) {
        opts.time_multiple = n.max(1.0);
    }
    if let Some(n) = option_number(option_field(ptr, "maxDelay")) {
        opts.max_delay = n.max(0.0);
    }
    if js_is_truthy(option_field(ptr, "delayFirstAttempt")) != 0 {
        opts.delay_first_attempt = true;
    }
    // `jitter` is the string 'full' (anything else, including the
    // default 'none', means no jitter).
    let jitter = option_field(ptr, "jitter");
    if JsValue::from_bits(jitter.to_bits()).is_any_string() {
        let s = js_get_string_pointer_unified(jitter) as *const StringHeader;
        if !s.is_null() {
            let handle = perry_ffi::JsString::from_raw(s as *mut StringHeader);
            if perry_ffi::read_string(handle) == Some("full") {
                opts.jitter_full = true;
            }
        }
    }
    let retry = option_field(ptr, "retry");
    let retry_bits = retry.to_bits();
    // npm's `retry` option is a function; a POINTER_TAG value here is a
    // closure by contract.
    if JsValue::from_bits(retry_bits).is_pointer() {
        opts.retry_cb = retry_bits;
    }
    opts
}

/// Build a 1-arg promise-reaction closure capturing the backoff state id.
unsafe fn bound_reaction(func_ptr: *const u8, state_id: u64) -> *const RawClosureHeader {
    js_register_closure_arity(func_ptr, 1);
    let closure = js_closure_alloc(func_ptr, 1);
    js_closure_set_capture_f64(closure, 0, f64::from_bits(state_id));
    closure as *const RawClosureHeader
}

unsafe fn state_id_from_closure(closure: *const RawClosureHeader) -> u64 {
    js_closure_get_capture_f64(closure, 0).to_bits()
}

fn settle(id: u64, resolve: bool, value: f64) {
    let Some(st) = STATES.lock().unwrap().remove(&id) else {
        return;
    };
    let outer = (st.outer & NANBOX_MASK) as *mut Promise;
    unsafe {
        if resolve {
            js_promise_resolve(outer, value);
        } else {
            js_promise_reject(outer, value);
        }
    }
}

/// Delay before attempt `attempts_done + 1`, mirroring the package's
/// `SkipFirstDelay` (power `attempts_done - 1`) vs `AlwaysDelay`
/// (power `attempts_done`) factories, capped at `maxDelay`, with
/// optional full jitter.
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
    unsafe {
        let timer = js_set_timeout_value_ref(delay, js_undefined(), 1);
        js_promise_then(
            timer,
            bound_reaction(backoff_on_timer as *const u8, id),
            std::ptr::null(),
        );
    }
}

/// A completed attempt failed with `error`. Either retry (after
/// consulting the `retry` predicate and scheduling the backoff delay)
/// or reject.
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
        // npm: `const shouldRetry = await retry(e, attemptNumber)`;
        // falsy stops and rethrows. (A promise-returning predicate is
        // treated as truthy — not awaited.)
        let cb = ((retry_cb & NANBOX_MASK) as usize) as *const RawClosureHeader;
        let should_retry = unsafe { js_closure_call2(cb, error, attempts_done as f64) };
        if unsafe { js_is_truthy(should_retry) } == 0 {
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
    let task_ptr = ((task & NANBOX_MASK) as usize) as *const RawClosureHeader;
    let result = unsafe { js_closure_call0(task_ptr) };

    let bits = result.to_bits();
    if JsValue::from_bits(bits).is_pointer() {
        let raw = (bits & NANBOX_MASK) as *mut Promise;
        if !raw.is_null() && unsafe { js_is_promise(raw) } != 0 {
            unsafe {
                js_promise_then(
                    raw,
                    bound_reaction(backoff_on_fulfilled as *const u8, id),
                    bound_reaction(backoff_on_rejected as *const u8, id),
                );
            }
            return;
        }
    }
    if is_valid_result(result) {
        settle(id, true, result);
    } else {
        handle_failure(id, result);
    }
}

extern "C" fn backoff_on_fulfilled(closure: *const RawClosureHeader, value: f64) -> f64 {
    settle(unsafe { state_id_from_closure(closure) }, true, value);
    js_undefined()
}

extern "C" fn backoff_on_rejected(closure: *const RawClosureHeader, error: f64) -> f64 {
    handle_failure(unsafe { state_id_from_closure(closure) }, error);
    js_undefined()
}

extern "C" fn backoff_on_timer(closure: *const RawClosureHeader, _value: f64) -> f64 {
    run_attempt(unsafe { state_id_from_closure(closure) });
    js_undefined()
}

/// `backOff(task, options?)` — execute a task with exponential-backoff
/// retry. `fn_ptr` is the task closure (codegen `NA_PTR`: raw extracted
/// pointer); `options` is the NaN-boxed options object (codegen
/// `NA_F64`). Returns a Promise that resolves with the first successful
/// result or rejects with the last error once attempts are exhausted or
/// the `retry` predicate says stop.
#[no_mangle]
pub extern "C" fn backOff(fn_ptr: *const RawClosureHeader, options: f64) -> *mut Promise {
    let promise = unsafe { js_promise_new() };
    if fn_ptr.is_null() {
        unsafe { js_promise_reject(promise, f64::NAN) };
        return promise;
    }
    ensure_backoff_gc_scanner();

    let opts = unsafe { parse_options(options) };
    let delay_first = opts.delay_first_attempt;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    STATES.lock().unwrap().insert(
        id,
        BackoffState {
            task: POINTER_TAG | (fn_ptr as u64 & NANBOX_MASK),
            outer: POINTER_TAG | (promise as u64 & NANBOX_MASK),
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

/// `backoffSimple(fn, attempts, delayMs)` — synchronous variant that
/// returns the result f64 directly. Kept for symbol-surface stability.
#[no_mangle]
pub extern "C" fn js_backoff_simple(
    fn_ptr: *const RawClosureHeader,
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
        let result = unsafe { js_closure_call0(fn_ptr) };
        if is_valid_result(result) {
            return result;
        }
        if attempt >= num_attempts {
            return f64::NAN;
        }
        std::thread::sleep(std::time::Duration::from_millis(current_delay));
        current_delay = (current_delay * 2).min(10_000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_result_handles_tagged_values() {
        // Real numbers are valid.
        assert!(is_valid_result(0.0));
        assert!(is_valid_result(-1.5));
        assert!(is_valid_result(1e300));
        // POINTER_TAG (Promise / object pointer).
        let pointer_bits: u64 = 0x7FFD_0000_0000_1234;
        assert!(is_valid_result(f64::from_bits(pointer_bits)));
        // STRING_TAG.
        let string_bits: u64 = 0x7FFF_0000_0000_5678;
        assert!(is_valid_result(f64::from_bits(string_bits)));
        // INT32_TAG.
        let int_bits: u64 = 0x7FFE_0000_0000_002A;
        assert!(is_valid_result(f64::from_bits(int_bits)));
        // Raw IEEE NaN — invalid.
        let raw_nan: u64 = 0x7FF8_0000_0000_0000;
        assert!(!is_valid_result(f64::from_bits(raw_nan)));
    }

    #[test]
    fn defaults_match_npm_package() {
        let opts = BackoffOptions::default();
        assert_eq!(opts.num_of_attempts, 10);
        assert_eq!(opts.starting_delay, 100.0);
        assert_eq!(opts.time_multiple, 2.0);
        assert_eq!(opts.max_delay, f64::INFINITY);
        assert!(!opts.delay_first_attempt);
        assert!(!opts.jitter_full);
    }

    #[test]
    fn delay_progression_honors_options() {
        let mk = |attempts_done: u32, opts: BackoffOptions| BackoffState {
            task: 0,
            outer: 0,
            attempts_done,
            opts,
        };
        // startingDelay 50, x3, cap 200: delays 50, 150, 200, 200…
        let opts = || BackoffOptions {
            starting_delay: 50.0,
            time_multiple: 3.0,
            max_delay: 200.0,
            ..BackoffOptions::default()
        };
        assert_eq!(next_delay_ms(&mk(1, opts())), 50.0);
        assert_eq!(next_delay_ms(&mk(2, opts())), 150.0);
        assert_eq!(next_delay_ms(&mk(3, opts())), 200.0);
        // delayFirstAttempt shifts the power by one.
        let dfa = BackoffOptions {
            delay_first_attempt: true,
            ..opts()
        };
        assert_eq!(
            next_delay_ms(&BackoffState {
                task: 0,
                outer: 0,
                attempts_done: 0,
                opts: dfa
            }),
            50.0
        );
    }
}
