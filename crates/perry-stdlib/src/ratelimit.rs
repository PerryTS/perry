//! Rate Limiter module (rate-limiter-flexible compatible)
//!
//! `RateLimiterMemory`-compatible fixed-window rate limiting. Semantics
//! mirror the npm package's in-memory limiter: `points` consumable per
//! `duration`-second fixed window (defaults 4 / 1s, like
//! `RateLimiterAbstract`). `consume` resolves a `RateLimiterRes`-shaped
//! object `{ remainingPoints, msBeforeNext, consumedPoints,
//! isFirstInDuration }` and **rejects with the same shape** when the
//! quota is exceeded. All state math is synchronous and runs on the
//! calling (main) thread, so result objects are built inline and
//! settled immediately.
//!
//! Kept in lock-step with `crates/perry-ext-ratelimit` (the well-known
//! flip's copy); this bundled copy links when the ext staticlib is
//! unavailable or `PERRY_DISABLE_WELL_KNOWN` is set.

use crate::common::{get_handle, register_handle, Handle};
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use perry_runtime::{
    js_object_alloc_with_shape, js_object_set_field, js_promise_new, js_promise_reject,
    js_promise_resolve, js_string_from_bytes, JSValue, ObjectHeader, Promise, StringHeader,
};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

impl KeyedRateLimiterHandle {
    fn window(&self) -> Duration {
        Duration::from_secs(self.duration_secs)
    }
}

const RATELIMIT_RES_SHAPE_ID: u32 = 0x7FFF_F31A;

/// Build a `RateLimiterRes`-shaped result object (main thread only).
fn ratelimiter_res(
    remaining_points: f64,
    ms_before_next: f64,
    consumed_points: f64,
    is_first_in_duration: bool,
) -> f64 {
    let packed = b"remainingPoints\0msBeforeNext\0consumedPoints\0isFirstInDuration\0";
    let obj = js_object_alloc_with_shape(
        RATELIMIT_RES_SHAPE_ID,
        4,
        packed.as_ptr(),
        packed.len() as u32,
    );
    js_object_set_field(obj, 0, JSValue::number(remaining_points));
    js_object_set_field(obj, 1, JSValue::number(ms_before_next));
    js_object_set_field(obj, 2, JSValue::number(consumed_points));
    js_object_set_field(obj, 3, JSValue::bool(is_first_in_duration));
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

fn reject_str(promise: *mut Promise, msg: &str) {
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    js_promise_reject(promise, f64::from_bits(JSValue::string_ptr(s).bits()));
}

/// new RateLimiter (legacy, non-keyed).
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
    let jv = JSValue::from_bits(options_bits as u64);
    if jv.is_pointer() {
        let obj = jv.as_pointer::<ObjectHeader>();
        if !obj.is_null() && (obj as usize) >= 0x100000 {
            let field = |name: &[u8]| -> f64 {
                let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
                perry_runtime::object::js_object_get_field_by_name_f64(obj, key)
            };
            let read_num = |v: f64| -> Option<f64> {
                let jv = JSValue::from_bits(v.to_bits());
                if jv.is_int32() {
                    Some(jv.as_int32() as f64)
                } else if jv.is_number() && !v.is_nan() {
                    Some(v)
                } else {
                    None
                }
            };
            if let Some(n) = read_num(field(b"points")) {
                points = n;
            }
            if let Some(n) = read_num(field(b"duration")) {
                duration = n;
            }
        }
    }
    js_ratelimit_new_keyed(points, duration)
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
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());
    let consume_points = points.max(1.0) as u32;

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        reject_str(promise, "Invalid rate limiter handle");
        return promise;
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
            js_promise_reject(promise, res);
            return promise;
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
        js_promise_reject(promise, res);
    } else {
        js_promise_resolve(promise, res);
    }
    promise
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
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());
    let null = f64::from_bits(JSValue::null().bits());

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        js_promise_resolve(promise, null);
        return promise;
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
            js_promise_resolve(promise, res);
        }
        _ => {
            drop(states);
            js_promise_resolve(promise, null);
        }
    }
    promise
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
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

    let removed = if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        keyed.states.lock().unwrap().remove(&key).is_some()
    } else {
        false
    };
    js_promise_resolve(promise, f64::from_bits(JSValue::bool(removed).bits()));
    promise
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
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        js_promise_resolve(promise, f64::from_bits(JSValue::undefined().bits()));
        return promise;
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

    js_promise_resolve(
        promise,
        ratelimiter_res(0.0, secs * 1000.0, consumed, false),
    );
    promise
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
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());
    let n = points.max(1.0) as u32;

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        js_promise_resolve(promise, f64::from_bits(JSValue::null().bits()));
        return promise;
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

    js_promise_resolve(promise, ratelimiter_res(remaining, ms, consumed, false));
    promise
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
    let promise = js_promise_new();
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());
    let n = points.max(1.0) as u32;

    let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) else {
        js_promise_resolve(promise, f64::from_bits(JSValue::null().bits()));
        return promise;
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

    js_promise_resolve(promise, ratelimiter_res(remaining, ms, consumed, false));
    promise
}

// ============================================================================
// Synchronous variants for simple use cases
// ============================================================================

/// Check if a key would be rate limited (without consuming)
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_check(handle: Handle, key_ptr: *const StringHeader) -> bool {
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

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
        return true; // No live window means not rate limited
    } else if let Some(simple) = get_handle::<RateLimiterHandle>(handle) {
        return simple.limiter.check().is_ok();
    }

    true
}

/// Get remaining points for a key
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ratelimit_remaining(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> f64 {
    let key = string_from_header(key_ptr).unwrap_or_else(|| "default".to_string());

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

/// Reset all rate limiters
#[no_mangle]
pub extern "C" fn js_ratelimit_reset(handle: Handle) {
    if let Some(keyed) = get_handle::<KeyedRateLimiterHandle>(handle) {
        keyed.states.lock().unwrap().clear();
    }
}
