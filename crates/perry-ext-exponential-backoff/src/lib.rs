//! Native bindings for the npm `exponential-backoff` package.
//!
//! Acceptance test for perry-ffi's closure invocation surface
//! (`JsClosure::call0`). Functionally identical to
//! `crates/perry-stdlib/src/exponential_backoff.rs`.

use perry_ffi::{JsClosure, JsPromise, JsValue, ObjectHeader, Promise, RawClosureHeader};
use std::thread;
use std::time::Duration;

const POINTER_TAG_HIGH: u64 = 0x7FFD;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// True when the result the user closure returned is "real" — a
/// regular number, a NaN-boxed pointer / int32 / string / bool /
/// null. Only raw IEEE NaN (no perry tag bits set) signals
/// "treat as failure, retry".
fn is_valid_result(result: f64) -> bool {
    if !result.is_nan() {
        return true;
    }
    // Tag check: 0x7FFA..0x7FFF are perry's NaN-box tag values.
    let tag = result.to_bits() >> 48;
    tag >= 0x7FFA
}

/// `backOff(fn, options?)` — call `fn`, retry on failure with
/// exponentially-increasing delays. Returns a Promise that
/// resolves with the success value or rejects after exhausting
/// retries.
///
/// Options parsing matches perry-stdlib's existing wrapper:
/// `options_ptr` is currently unused (TODO carried over from
/// the original — `numOfAttempts`, `startingDelay`,
/// `timeMultiple`, `maxDelay` could be parsed via
/// `JsValue::is_pointer` + object-field reads in a followup).
#[no_mangle]
pub extern "C" fn backOff(
    fn_ptr: *const RawClosureHeader,
    _options_ptr: *const ObjectHeader,
) -> *mut Promise {
    let closure = unsafe { JsClosure::from_raw(fn_ptr) };
    if closure.is_null() {
        let p = JsPromise::new();
        p.reject(JsValue::from_number(f64::NAN));
        return JsPromise::new().as_raw(); // Note: original does the same odd thing — kept for parity.
    }

    // First attempt — no delay.
    let result = unsafe { closure.call0() };

    // If the callback returned a Promise (POINTER_TAG-tagged
    // pointer), unwrap and return it directly to avoid
    // Promise-in-Promise. Same trick the original uses.
    let bits = result.to_bits();
    if (bits >> 48) == POINTER_TAG_HIGH {
        let ptr = (bits & POINTER_MASK) as *mut Promise;
        if !ptr.is_null() {
            return ptr;
        }
    }

    let promise = JsPromise::new();
    let raw = promise.as_raw();
    if is_valid_result(result) {
        promise.resolve(JsValue::from_bits(bits));
        return raw;
    }

    // Retry with exponential backoff. Defaults match the npm
    // `exponential-backoff` package's defaults.
    let num_of_attempts: u32 = 3;
    let starting_delay: u64 = 100;
    let max_delay: u64 = 10_000;
    let time_multiple: f64 = 2.0;

    // `promise` is consumed if the first-shot resolved above; in
    // the retry path we re-wrap `raw` inside the loop body.
    drop(promise);
    let mut attempt = 1;
    let mut current_delay = starting_delay;

    loop {
        attempt += 1;
        if attempt > num_of_attempts {
            unsafe { JsPromise::from_raw(raw) }.reject(JsValue::from_number(f64::NAN));
            return raw;
        }
        thread::sleep(Duration::from_millis(current_delay));
        let result = unsafe { closure.call0() };
        let bits = result.to_bits();
        if (bits >> 48) == POINTER_TAG_HIGH {
            let ptr = (bits & POINTER_MASK) as *mut Promise;
            if !ptr.is_null() {
                return ptr;
            }
        }
        if is_valid_result(result) {
            unsafe { JsPromise::from_raw(raw) }.resolve(JsValue::from_bits(bits));
            return raw;
        }
        current_delay = ((current_delay as f64) * time_multiple).min(max_delay as f64) as u64;
    }
}

/// `backoffSimple(fn, attempts, delayMs)` — synchronous variant
/// that returns the result f64 directly. Used by perry-stdlib's
/// callers for non-Promise-returning closures.
#[no_mangle]
pub extern "C" fn js_backoff_simple(
    fn_ptr: *const RawClosureHeader,
    num_attempts: i32,
    delay_ms: i32,
) -> f64 {
    let closure = unsafe { JsClosure::from_raw(fn_ptr) };
    if closure.is_null() {
        return f64::NAN;
    }
    let mut attempt = 0;
    let mut current_delay = delay_ms.max(10) as u64;
    loop {
        attempt += 1;
        let result = unsafe { closure.call0() };
        if is_valid_result(result) {
            return result;
        }
        if attempt >= num_attempts {
            return f64::NAN;
        }
        thread::sleep(Duration::from_millis(current_delay));
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
    fn null_closure_returns_nan() {
        let r = js_backoff_simple(std::ptr::null(), 5, 10);
        assert!(r.is_nan());
    }
}
