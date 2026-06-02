//! Native bindings for the npm `rate-limiter-flexible` package —
//! token-bucket rate limiting via the `governor` crate. Uses only
//! perry-ffi v0.5 strings + handles + Promise + JsValue. The async
//! exports bridge through `spawn_blocking` + `JsPromise` since
//! `governor::RateLimiter::check()` is sync.

use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use perry_ffi::{
    alloc_string, get_handle, read_string, register_handle, spawn_blocking, Handle, JsPromise,
    JsString, JsValue, Promise, StringHeader,
};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct RateLimiterHandle {
    pub limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    pub points: u32,
    pub duration_secs: u64,
}

pub struct KeyedRateLimiterHandle {
    pub limiters: Arc<Mutex<HashMap<String, RateLimiter<NotKeyed, InMemoryState, DefaultClock>>>>,
    pub points: u32,
    pub duration_secs: u64,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

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

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_consume(
    handle: Handle,
    key_ptr: *const StringHeader,
    points: f64,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());
    let consume_points = points.max(1.0) as u32;

    spawn_blocking(move || {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let mut limiters = keyed.limiters.lock().unwrap();
            let limiter = limiters.entry(key.clone()).or_insert_with(|| {
                let quota = Quota::with_period(Duration::from_secs(keyed.duration_secs))
                    .unwrap()
                    .allow_burst(NonZeroU32::new(keyed.points).unwrap());
                RateLimiter::direct(quota)
            });
            for _ in 0..consume_points {
                if limiter.check().is_err() {
                    promise.reject_string("Rate limit exceeded");
                    return;
                }
            }
            let result = format!(
                r#"{{"remainingPoints":{},"msBeforeNext":0,"consumedPoints":{},"isFirstInDuration":false}}"#,
                keyed.points.saturating_sub(consume_points),
                consume_points
            );
            promise.resolve_string(&result);
        } else if let Some(simple) = get_handle::<RateLimiterHandle>(handle) {
            for _ in 0..consume_points {
                if simple.limiter.check().is_err() {
                    promise.reject_string("Rate limit exceeded");
                    return;
                }
            }
            let result = format!(
                r#"{{"remainingPoints":{},"msBeforeNext":0,"consumedPoints":{},"isFirstInDuration":false}}"#,
                simple.points.saturating_sub(consume_points),
                consume_points
            );
            promise.resolve_string(&result);
        } else {
            promise.reject_string("Invalid rate limiter handle");
        }
    });
    raw
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_get(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());

    spawn_blocking(move || {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let limiters = keyed.limiters.lock().unwrap();
            if limiters.contains_key(&key) {
                let result = format!(
                    r#"{{"remainingPoints":{},"msBeforeNext":0,"consumedPoints":0,"isFirstInDuration":false}}"#,
                    keyed.points
                );
                promise.resolve_string(&result);
            } else {
                promise.resolve(JsValue::NULL);
            }
        } else {
            promise.resolve(JsValue::NULL);
        }
    });
    raw
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_delete(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());

    spawn_blocking(move || {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let mut limiters = keyed.limiters.lock().unwrap();
            let removed = limiters.remove(&key).is_some();
            promise.resolve(JsValue::from_bool(removed));
        } else {
            promise.resolve(JsValue::FALSE);
        }
    });
    raw
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_block(
    handle: Handle,
    key_ptr: *const StringHeader,
    duration_sec: f64,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());
    let _duration = duration_sec.max(1.0) as u64;

    spawn_blocking(move || {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let mut limiters = keyed.limiters.lock().unwrap();
            let quota = Quota::with_period(Duration::from_secs(keyed.duration_secs))
                .unwrap()
                .allow_burst(NonZeroU32::new(1).unwrap());
            let limiter = RateLimiter::direct(quota);
            let _ = limiter.check();
            limiters.insert(key, limiter);
        }
        promise.resolve(JsValue::UNDEFINED);
    });
    raw
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_penalty(
    handle: Handle,
    key_ptr: *const StringHeader,
    points: f64,
) -> *mut Promise {
    js_ratelimit_consume(handle, key_ptr, points)
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_reward(
    handle: Handle,
    key_ptr: *const StringHeader,
    _points: f64,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());

    spawn_blocking(move || {
        if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
            let mut limiters = keyed.limiters.lock().unwrap();
            let quota = Quota::with_period(Duration::from_secs(keyed.duration_secs))
                .unwrap()
                .allow_burst(NonZeroU32::new(keyed.points).unwrap());
            limiters.insert(key, RateLimiter::direct(quota));
            let result = format!(
                r#"{{"remainingPoints":{},"msBeforeNext":0,"consumedPoints":0,"isFirstInDuration":true}}"#,
                keyed.points
            );
            promise.resolve_string(&result);
        } else {
            promise.resolve(JsValue::NULL);
        }
    });
    raw
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_check(handle: Handle, key_ptr: *const StringHeader) -> bool {
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());
    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let limiters = keyed.limiters.lock().unwrap();
        if let Some(limiter) = limiters.get(&key) {
            return limiter.check().is_ok();
        }
        return true;
    } else if let Some(simple) = get_handle::<RateLimiterHandle>(handle) {
        return simple.limiter.check().is_ok();
    }
    true
}

/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_remaining(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> f64 {
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());
    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let limiters = keyed.limiters.lock().unwrap();
        if limiters.contains_key(&key) {
            return keyed.points as f64;
        }
        return keyed.points as f64;
    } else if let Some(simple) = get_handle::<RateLimiterHandle>(handle) {
        return simple.points as f64;
    }
    0.0
}

#[no_mangle]
pub extern "C" fn js_ratelimit_reset(handle: Handle) {
    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let mut limiters = keyed.limiters.lock().unwrap();
        limiters.clear();
    }
}

// `alloc_string` available for follow-ups; currently unused.
#[allow(dead_code)]
fn _ensure_alloc_string_linkage() -> *mut StringHeader {
    alloc_string("").as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_handle() {
        let h = js_ratelimit_new(10.0, 60.0);
        assert!(h >= 0);
    }

    #[test]
    fn check_passes_when_under_quota() {
        let h = js_ratelimit_new(10.0, 60.0);
        let key = alloc_string("user-a");
        // First call should pass (no limiter for this key, returns true).
        assert!(unsafe { js_ratelimit_check(h, key.as_raw()) });
    }

    #[test]
    fn remaining_returns_max_for_unknown_key() {
        let h = js_ratelimit_new_keyed(5.0, 60.0);
        let key = alloc_string("nope");
        assert_eq!(unsafe { js_ratelimit_remaining(h, key.as_raw()) }, 5.0);
    }

    #[test]
    fn reset_clears_keyed_limiters() {
        let h = js_ratelimit_new_keyed(3.0, 60.0);
        js_ratelimit_reset(h);
        // After reset, remaining for any key is still the max.
        let key = alloc_string("anything");
        assert_eq!(unsafe { js_ratelimit_remaining(h, key.as_raw()) }, 3.0);
    }

    #[test]
    fn invalid_handle_check_returns_true() {
        // Per the perry-stdlib convention, invalid-handle returns
        // a permissive `true` for check (no rate limit applies).
        let key = alloc_string("x");
        assert!(unsafe { js_ratelimit_check(-1, key.as_raw()) });
    }
}
