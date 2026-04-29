//! Exponential Backoff implementation
//!
//! Native implementation of the `exponential-backoff` npm package.
//! Provides retry functionality with exponential delays.

use perry_runtime::{
    js_closure_call0, js_promise_new, js_promise_reject, js_promise_resolve, ClosureHeader, Promise,
};
use std::thread;
use std::time::Duration;

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

/// Execute a function with exponential backoff retry logic
/// fn_ptr: Closure to execute (should return a Promise)
/// options_ptr: Object containing options (numOfAttempts, startingDelay, etc.)
/// Returns: Promise that resolves with the result or rejects after all retries fail
#[no_mangle]
pub extern "C" fn backOff(
    fn_ptr: *const ClosureHeader,
    _options_ptr: *const perry_runtime::ObjectHeader,
) -> *mut Promise {
    if fn_ptr.is_null() {
        let promise = unsafe { js_promise_new() };
        unsafe {
            js_promise_reject(promise, f64::NAN);
        }
        return promise;
    }

    // Call the function once - for async callbacks (which return Promises),
    // we just pass through the Promise. The caller will await it.
    let result = unsafe { js_closure_call0(fn_ptr) };

    // Check if the result is a NaN-boxed pointer (Promise, object, etc.)
    let bits = result.to_bits();
    let tag = bits >> 48;

    if tag == 0x7FFD {
        // POINTER_TAG - the callback returned a Promise or object pointer.
        // Extract the raw pointer and return it directly as a Promise.
        // This avoids wrapping Promise-in-Promise.
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut Promise;
        if !ptr.is_null() {
            return ptr;
        }
    }

    // For non-Promise results, wrap in a new Promise
    let promise = unsafe { js_promise_new() };

    if is_valid_result(result) {
        unsafe {
            js_promise_resolve(promise, result);
        }
        return promise;
    }

    // Result looks like an error (raw NaN). Retry with backoff.
    // Default backoff options
    let num_of_attempts: u32 = 3;
    let starting_delay: u64 = 100;
    let max_delay: u64 = 10000;
    let time_multiple: f64 = 2.0;

    // TODO: Parse options from options_ptr if provided

    let mut attempt = 1; // Already did attempt 1 above
    let mut current_delay = starting_delay;

    loop {
        attempt += 1;

        if attempt > num_of_attempts {
            unsafe {
                js_promise_reject(promise, f64::NAN);
            }
            return promise;
        }

        // Wait before retrying
        thread::sleep(Duration::from_millis(current_delay));

        // Call the function again
        let result = unsafe { js_closure_call0(fn_ptr) };

        let bits = result.to_bits();
        let tag = bits >> 48;

        if tag == 0x7FFD {
            // Promise returned - extract and return directly
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut Promise;
            if !ptr.is_null() {
                return ptr;
            }
        }

        if is_valid_result(result) {
            unsafe {
                js_promise_resolve(promise, result);
            }
            return promise;
        }

        // Increase delay exponentially
        current_delay = ((current_delay as f64) * time_multiple).min(max_delay as f64) as u64;
    }
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
        let result = unsafe { js_closure_call0(fn_ptr) };

        // Success if valid result
        if is_valid_result(result) {
            return result;
        }

        // Check if we've exhausted retries
        if attempt >= num_attempts {
            return f64::NAN;
        }

        // Wait before retrying
        thread::sleep(Duration::from_millis(current_delay));

        // Increase delay exponentially
        current_delay = (current_delay * 2).min(10000);
    }
}
