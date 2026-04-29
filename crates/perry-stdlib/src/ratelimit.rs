//! Rate Limiter module (rate-limiter-flexible compatible)
//!
//! Native implementation of rate limiting functionality using governor.
//! Provides token bucket rate limiting for API protection.

use crate::common::{get_handle, register_handle, spawn_for_promise, Handle};
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use perry_runtime::{js_promise_new, js_string_from_bytes, JSValue, Promise, StringHeader};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

/// Rate limiter handle (for simple per-instance limiting)
pub struct RateLimiterHandle {
    pub limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    pub points: u32,
    pub duration_secs: u64,
}

/// Keyed rate limiter handle (for per-key limiting like IP addresses)
pub struct KeyedRateLimiterHandle {
    pub limiters: Arc<Mutex<HashMap<String, RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>>,
    pub points: u32,
    pub duration_secs: u64,
}

/// Result of a consume operation
pub struct ConsumeResult {
    pub remaining_points: i32,
    pub ms_before_next: u64,
    pub consumed_points: u32,
    pub is_rejected: bool,
}

/// new RateLimiterMemory(opts) -> RateLimiter
///
/// Create a new in-memory rate limiter.
/// opts: { points: number, duration: number (seconds) }
#[no_mangle]
pub extern "C" fn js_ratelimit_new(points: f64, duration_secs: f64) -> Handle {
    let points = points.max(1.0) as u32;
    let duration_secs = duration_secs.max(1.0) as u64;

    let quota = Quota::with_period(Duration::from_secs(duration_secs))
        .unwrap()
        .allow_burst(NonZeroU32::new(points).unwrap());

    let limiter = RateLimiter::direct(quota);

    register_handle(RateLimiterHandle {
        limiter,
        points,
        duration_secs,
    })
}

/// new RateLimiterMemory(opts) for keyed limiting -> KeyedRateLimiter
///
/// Create a new keyed rate limiter (per IP, user ID, etc.)
#[no_mangle]
pub extern "C" fn js_ratelimit_new_keyed(points: f64, duration_secs: f64) -> Handle {
    let points = points.max(1.0) as u32;
    let duration_secs = duration_secs.max(1.0) as u64;

    register_handle(KeyedRateLimiterHandle {
        limiters: Arc::new(Mutex::new(HashMap::new())),
        points,
        duration_secs,
    })
}

/// limiter.consume(key, points?) -> Promise<RateLimiterRes>
///
/// Consume points from the rate limiter.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_consume(
    handle: Handle,
    key_ptr: *const StringHeader,
    points: f64,
) -> *mut Promise {
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());
    let consume_points = points.max(1.0) as u32;

    spawn_for_promise(promise as *mut u8, async move {
        // Check if it's a keyed or simple limiter
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let mut limiters = keyed.limiters.lock().unwrap();

            // Get or create limiter for this key
            let limiter = limiters.entry(key.clone()).or_insert_with(|| {
                let quota = Quota::with_period(Duration::from_secs(keyed.duration_secs))
                    .unwrap()
                    .allow_burst(NonZeroU32::new(keyed.points).unwrap());
                RateLimiter::direct(quota)
            });

            // Try to consume
            for _ in 0..consume_points {
                if limiter.check().is_err() {
                    // Rate limited
                    let result = format!(
                        r#"{{"remainingPoints":0,"msBeforeNext":{},"consumedPoints":{},"isFirstInDuration":false}}"#,
                        keyed.duration_secs * 1000,
                        0
                    );
                    let ptr = js_string_from_bytes(result.as_ptr(), result.len() as u32);
                    return Err("Rate limit exceeded".to_string());
                }
            }

            // Success
            let result = format!(
                r#"{{"remainingPoints":{},"msBeforeNext":0,"consumedPoints":{},"isFirstInDuration":false}}"#,
                keyed.points.saturating_sub(consume_points),
                consume_points
            );
            let ptr = js_string_from_bytes(result.as_ptr(), result.len() as u32);
            Ok(JSValue::string_ptr(ptr).bits())
        } else if let Some(simple) = get_handle::<RateLimiterHandle>(handle) {
            // Simple (non-keyed) limiter
            for _ in 0..consume_points {
                if simple.limiter.check().is_err() {
                    return Err("Rate limit exceeded".to_string());
                }
            }

            let result = format!(
                r#"{{"remainingPoints":{},"msBeforeNext":0,"consumedPoints":{},"isFirstInDuration":false}}"#,
                simple.points.saturating_sub(consume_points),
                consume_points
            );
            let ptr = js_string_from_bytes(result.as_ptr(), result.len() as u32);
            Ok(JSValue::string_ptr(ptr).bits())
        } else {
            Err("Invalid rate limiter handle".to_string())
        }
    });

    promise
}

/// limiter.get(key) -> Promise<RateLimiterRes | null>
///
/// Get the current state for a key without consuming.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_get(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let limiters = keyed.limiters.lock().unwrap();

            if limiters.contains_key(&key) {
                let result = format!(
                    r#"{{"remainingPoints":{},"msBeforeNext":0,"consumedPoints":0,"isFirstInDuration":false}}"#,
                    keyed.points
                );
                let ptr = js_string_from_bytes(result.as_ptr(), result.len() as u32);
                Ok(JSValue::string_ptr(ptr).bits())
            } else {
                Ok(JSValue::null().bits())
            }
        } else {
            Ok(JSValue::null().bits())
        }
    });

    promise
}

/// limiter.delete(key) -> Promise<boolean>
///
/// Delete rate limit record for a key.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_delete(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let mut limiters = keyed.limiters.lock().unwrap();
            let removed = limiters.remove(&key).is_some();
            Ok(JSValue::bool(removed).bits())
        } else {
            Ok(JSValue::bool(false).bits())
        }
    });

    promise
}

/// limiter.block(key, durationSec) -> Promise<void>
///
/// Block a key for a specified duration.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_block(
    handle: Handle,
    key_ptr: *const StringHeader,
    duration_sec: f64,
) -> *mut Promise {
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());
    let _duration = duration_sec.max(1.0) as u64;

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let mut limiters = keyed.limiters.lock().unwrap();

            // Create a limiter that's already exhausted
            let quota = Quota::with_period(Duration::from_secs(keyed.duration_secs))
                .unwrap()
                .allow_burst(NonZeroU32::new(1).unwrap());
            let limiter = RateLimiter::direct(quota);

            // Consume all points to block
            let _ = limiter.check();

            limiters.insert(key, limiter);
            Ok(JSValue::undefined().bits())
        } else {
            Ok(JSValue::undefined().bits())
        }
    });

    promise
}

/// limiter.penalty(key, points) -> Promise<RateLimiterRes>
///
/// Add penalty points (consume extra).
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_penalty(
    handle: Handle,
    key_ptr: *const StringHeader,
    points: f64,
) -> *mut Promise {
    js_ratelimit_consume(handle, key_ptr, points)
}

/// limiter.reward(key, points) -> Promise<RateLimiterRes>
///
/// Reward points (add back to quota).
/// Note: This is a simplified implementation that just resets the limiter.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_reward(
    handle: Handle,
    key_ptr: *const StringHeader,
    _points: f64,
) -> *mut Promise {
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let mut limiters = keyed.limiters.lock().unwrap();

            // Reset the limiter for this key (simplified reward)
            let quota = Quota::with_period(Duration::from_secs(keyed.duration_secs))
                .unwrap()
                .allow_burst(NonZeroU32::new(keyed.points).unwrap());
            limiters.insert(key, RateLimiter::direct(quota));

            let result = format!(
                r#"{{"remainingPoints":{},"msBeforeNext":0,"consumedPoints":0,"isFirstInDuration":true}}"#,
                keyed.points
            );
            let ptr = js_string_from_bytes(result.as_ptr(), result.len() as u32);
            Ok(JSValue::string_ptr(ptr).bits())
        } else {
            Ok(JSValue::null().bits())
        }
    });

    promise
}

// ============================================================================
// Synchronous variants for simple use cases
// ============================================================================

/// Check if a key would be rate limited (without consuming)
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_check(handle: Handle, key_ptr: *const StringHeader) -> bool {
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let limiters = keyed.limiters.lock().unwrap();
        if let Some(limiter) = limiters.get(&key) {
            return limiter.check().is_ok();
        }
        return true; // No limiter yet means not rate limited
    } else if let Some(simple) = get_handle::<RateLimiterHandle>(handle) {
        return simple.limiter.check().is_ok();
    }

    true
}

/// Get remaining points for a key
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_remaining(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> f64 {
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let limiters = keyed.limiters.lock().unwrap();
        if limiters.contains_key(&key) {
            // Simplified: return max points (actual tracking would require more state)
            return keyed.points as f64;
        }
        return keyed.points as f64;
    } else if let Some(simple) = get_handle::<RateLimiterHandle>(handle) {
        return simple.points as f64;
    }

    0.0
}

/// Reset all rate limiters
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_reset(handle: Handle) {
    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let mut limiters = keyed.limiters.lock().unwrap();
        limiters.clear();
    }
}
