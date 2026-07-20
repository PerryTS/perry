//! Native bindings for the npm `rate-limiter-flexible` package —
//! `RateLimiterMemory`-compatible fixed-window rate limiting. Uses only
//! perry-ffi v0.5 strings + handles + Promise + JsValue.
//!
//! Semantics mirror the npm package's in-memory limiter:
//! `points` consumable per `duration`-second fixed window (defaults 4 /
//! 1s, like `RateLimiterAbstract`). `consume` resolves a
//! `RateLimiterRes`-shaped object `{ remainingPoints, msBeforeNext,
//! consumedPoints, isFirstInDuration }` and **rejects with the same
//! shape** when the quota is exceeded. All state math is synchronous
//! and runs on the calling (main) thread, so result objects are built
//! inline and settled immediately — no worker-arena hazards.
//!
//! The old version fed each consume through a `governor` token bucket
//! but reported `remainingPoints` as a constant (`points - n`,
//! regardless of prior consumption) and resolved a JSON *string*
//! instead of an object, so `res.remainingPoints` was `undefined` and
//! repeated consumes never counted down.

use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use perry_ffi::{
    alloc_string, build_object_shape, get_handle, js_object_alloc_with_shape, js_object_set_field,
    read_string, register_handle, Handle, JsPromise, JsString, JsValue, ObjectHeader, Promise,
    StringHeader,
};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Mutex;
use std::time::{Duration, Instant};

extern "C" {
    /// perry-runtime: read an object field by string key, returning the
    /// raw NaN-boxed JSValue bits as f64 (undefined tag when absent).
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
}

/// Legacy direct (non-keyed) limiter — kept for the pre-existing
/// `js_ratelimit_new` / `js_ratelimit_check` / `js_ratelimit_remaining`
/// symbol surface.
pub struct RateLimiterHandle {
    pub limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    pub points: u32,
    pub duration_secs: u64,
}

/// Per-key fixed-window state, npm-style.
struct KeyState {
    consumed: u32,
    window_end: Instant,
    blocked_until: Option<Instant>,
}

/// `new RateLimiterMemory({ points, duration })`.
pub struct KeyedRateLimiterHandle {
    states: Mutex<HashMap<String, KeyState>>,
    pub points: u32,
    pub duration_secs: u64,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// Build a `RateLimiterRes`-shaped result object (main thread only).
fn ratelimiter_res(
    remaining_points: f64,
    ms_before_next: f64,
    consumed_points: f64,
    is_first_in_duration: bool,
) -> JsValue {
    let (packed, shape_id) = build_object_shape(&[
        "remainingPoints",
        "msBeforeNext",
        "consumedPoints",
        "isFirstInDuration",
    ]);
    unsafe {
        let obj = js_object_alloc_with_shape(shape_id, 4, packed.as_ptr(), packed.len() as u32);
        js_object_set_field(obj, 0, JsValue::from_number(remaining_points));
        js_object_set_field(obj, 1, JsValue::from_number(ms_before_next));
        js_object_set_field(obj, 2, JsValue::from_number(consumed_points));
        js_object_set_field(obj, 3, JsValue::from_bool(is_first_in_duration));
        JsValue::from_object_ptr(obj)
    }
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
        states: Mutex::new(HashMap::new()),
        points,
        duration_secs,
    })
}

/// `new RateLimiterMemory(opts)` — parse `{ points, duration }` from the
/// NaN-boxed options object (raw bits as i64; TAG_UNDEFINED when the
/// constructor was called without arguments). Defaults mirror npm's
/// `RateLimiterAbstract`: points = 4, duration = 1 second.
///
/// # Safety
/// `options_bits` must be valid NaN-box bits.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_new_from_options(options_bits: i64) -> Handle {
    let mut points: f64 = 4.0;
    let mut duration: f64 = 1.0;
    let jv = JsValue::from_bits(options_bits as u64);
    if jv.is_pointer() {
        let obj = jv.as_pointer::<ObjectHeader>();
        if !obj.is_null() && (obj as usize) >= 0x100000 {
            let field = |name: &str| -> f64 {
                let key = alloc_string(name);
                js_object_get_field_by_name_f64(obj, key.as_raw())
            };
            let read_num = |v: f64| -> Option<f64> {
                let jv = JsValue::from_bits(v.to_bits());
                if jv.is_int32() {
                    Some(jv.to_int32() as f64)
                } else if jv.is_number() && !v.is_nan() {
                    Some(v)
                } else {
                    None
                }
            };
            if let Some(n) = read_num(field("points")) {
                points = n;
            }
            if let Some(n) = read_num(field("duration")) {
                duration = n;
            }
        }
    }
    js_ratelimit_new_keyed(points, duration)
}

impl KeyedRateLimiterHandle {
    fn window(&self) -> Duration {
        Duration::from_secs(self.duration_secs)
    }
}

/// `limiter.consume(key, points = 1)` — npm-style fixed window. Resolves
/// a `RateLimiterRes` object; rejects with the same shape when the
/// window's quota is exceeded (or the key is blocked).
///
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

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        promise.reject_string("Invalid rate limiter handle");
        return raw;
    };

    let now = Instant::now();
    let window = keyed.window();
    let mut states = keyed.states.lock().unwrap();
    let state = states.entry(key).or_insert_with(|| KeyState {
        consumed: 0,
        window_end: now + window,
        blocked_until: None,
    });

    if let Some(until) = state.blocked_until {
        if until > now {
            let ms = until.duration_since(now).as_millis() as f64;
            let res = ratelimiter_res(0.0, ms, state.consumed as f64, false);
            drop(states);
            promise.reject(res);
            return raw;
        }
        state.blocked_until = None;
        state.consumed = 0;
        state.window_end = now + window;
    }

    if now >= state.window_end {
        state.consumed = 0;
        state.window_end = now + window;
    }

    state.consumed = state.consumed.saturating_add(consume_points);
    let is_first = state.consumed == consume_points;
    let ms_before_next = state.window_end.duration_since(now).as_millis() as f64;
    let consumed = state.consumed as f64;
    let over_limit = state.consumed > keyed.points;
    let remaining = (keyed.points as f64 - consumed).max(0.0);
    drop(states);

    let res = ratelimiter_res(remaining, ms_before_next, consumed, is_first && !over_limit);
    if over_limit {
        promise.reject(res);
    } else {
        promise.resolve(res);
    }
    raw
}

/// `limiter.get(key)` — current state without consuming; `null` when the
/// key has no live window.
///
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

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        promise.resolve(JsValue::NULL);
        return raw;
    };

    let now = Instant::now();
    let states = keyed.states.lock().unwrap();
    match states.get(&key) {
        Some(state) if now < state.window_end || state.blocked_until.is_some_and(|u| u > now) => {
            let ms = state
                .blocked_until
                .filter(|u| *u > now)
                .unwrap_or(state.window_end)
                .duration_since(now)
                .as_millis() as f64;
            let consumed = state.consumed as f64;
            // A key still inside an active block reports zero remaining
            // points even after its normal window would have expired.
            let blocked = state.blocked_until.is_some_and(|u| u > now);
            let remaining = if blocked {
                0.0
            } else {
                (keyed.points as f64 - consumed).max(0.0)
            };
            let res = ratelimiter_res(remaining, ms, consumed, false);
            drop(states);
            promise.resolve(res);
        }
        _ => {
            drop(states);
            promise.resolve(JsValue::NULL);
        }
    }
    raw
}

/// `limiter.delete(key)` — drop the key's window. Resolves `true` when a
/// record existed.
///
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

    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let removed = keyed.states.lock().unwrap().remove(&key).is_some();
        promise.resolve(JsValue::from_bool(removed));
    } else {
        promise.resolve(JsValue::FALSE);
    }
    raw
}

/// `limiter.block(key, secDuration)` — block the key for `secDuration`
/// seconds (0 = forever). Resolves a `RateLimiterRes`.
///
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

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        promise.resolve(JsValue::UNDEFINED);
        return raw;
    };

    let now = Instant::now();
    let secs = duration_sec.max(0.0);
    let until = if secs == 0.0 {
        now + Duration::from_secs(u32::MAX as u64)
    } else {
        now + Duration::from_millis((secs * 1000.0) as u64)
    };
    let window = keyed.window();
    let mut states = keyed.states.lock().unwrap();
    let state = states.entry(key).or_insert_with(|| KeyState {
        consumed: 0,
        window_end: now + window,
        blocked_until: None,
    });
    state.blocked_until = Some(until);
    let consumed = state.consumed as f64;
    drop(states);

    let res = ratelimiter_res(0.0, secs * 1000.0, consumed, false);
    promise.resolve(res);
    raw
}

/// `limiter.penalty(key, points = 1)` — add consumed points without a
/// quota check. Resolves a `RateLimiterRes`.
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_penalty(
    handle: Handle,
    key_ptr: *const StringHeader,
    points: f64,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());
    let n = points.max(1.0) as u32;

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        promise.resolve(JsValue::NULL);
        return raw;
    };

    let now = Instant::now();
    let window = keyed.window();
    let mut states = keyed.states.lock().unwrap();
    let state = states.entry(key).or_insert_with(|| KeyState {
        consumed: 0,
        window_end: now + window,
        blocked_until: None,
    });
    if now >= state.window_end {
        state.consumed = 0;
        state.window_end = now + window;
    }
    state.consumed = state.consumed.saturating_add(n);
    let consumed = state.consumed as f64;
    let ms = state.window_end.duration_since(now).as_millis() as f64;
    let remaining = (keyed.points as f64 - consumed).max(0.0);
    drop(states);

    promise.resolve(ratelimiter_res(remaining, ms, consumed, false));
    raw
}

/// `limiter.reward(key, points = 1)` — give consumed points back.
/// Resolves a `RateLimiterRes`.
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_reward(
    handle: Handle,
    key_ptr: *const StringHeader,
    points: f64,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());
    let n = points.max(1.0) as u32;

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        promise.resolve(JsValue::NULL);
        return raw;
    };

    let now = Instant::now();
    let window = keyed.window();
    let mut states = keyed.states.lock().unwrap();
    let state = states.entry(key).or_insert_with(|| KeyState {
        consumed: 0,
        window_end: now + window,
        blocked_until: None,
    });
    // Reset an expired window before reward math so a delayed reward()
    // cannot underflow the (now stale) window_end in `duration_since`.
    if now >= state.window_end {
        state.consumed = 0;
        state.window_end = now + window;
    }
    state.consumed = state.consumed.saturating_sub(n);
    let consumed = state.consumed as f64;
    let ms = state.window_end.duration_since(now).as_millis() as f64;
    let remaining = (keyed.points as f64 - consumed).max(0.0);
    drop(states);

    promise.resolve(ratelimiter_res(remaining, ms, consumed, false));
    raw
}

/// Sync probe: would a consume of 1 point succeed?
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_check(handle: Handle, key_ptr: *const StringHeader) -> bool {
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());
    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let now = Instant::now();
        let states = keyed.states.lock().unwrap();
        if let Some(state) = states.get(&key) {
            if state.blocked_until.is_some_and(|u| u > now) {
                return false;
            }
            if now < state.window_end {
                return state.consumed < keyed.points;
            }
        }
        return true;
    } else if let Some(simple) = get_handle::<RateLimiterHandle>(handle) {
        return simple.limiter.check().is_ok();
    }
    true
}

/// Sync probe: remaining points for a key.
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_remaining(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> f64 {
    let key = read_str(key_ptr).unwrap_or_else(|| "default".to_string());
    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        let now = Instant::now();
        let states = keyed.states.lock().unwrap();
        if let Some(state) = states.get(&key) {
            if state.blocked_until.is_some_and(|u| u > now) {
                return 0.0;
            }
            if now < state.window_end {
                return (keyed.points as f64 - state.consumed as f64).max(0.0);
            }
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
        keyed.states.lock().unwrap().clear();
    }
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
        assert!(unsafe { js_ratelimit_check(h, key.as_raw()) });
    }

    #[test]
    fn keyed_state_counts_down_and_blocks() {
        let h = js_ratelimit_new_keyed(2.0, 60.0);
        let keyed = get_handle::<KeyedRateLimiterHandle>(h).unwrap();
        let now = Instant::now();
        {
            let mut states = keyed.states.lock().unwrap();
            states.insert(
                "k".into(),
                KeyState {
                    consumed: 2,
                    window_end: now + Duration::from_secs(60),
                    blocked_until: None,
                },
            );
        }
        let key = alloc_string("k");
        // Quota exhausted: sync probe must fail…
        assert!(!unsafe { js_ratelimit_check(h, key.as_raw()) });
        // …and remaining must be 0 (the old governor-backed version
        // reported the constant max here).
        assert_eq!(unsafe { js_ratelimit_remaining(h, key.as_raw()) }, 0.0);
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
        let key = alloc_string("anything");
        assert_eq!(unsafe { js_ratelimit_remaining(h, key.as_raw()) }, 3.0);
    }

    #[test]
    fn invalid_handle_check_returns_true() {
        // Per the perry-stdlib convention, invalid-handle returns a
        // permissive `true` for check (no rate limit applies).
        let key = alloc_string("x");
        assert!(unsafe { js_ratelimit_check(-1, key.as_raw()) });
    }
}
