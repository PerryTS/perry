//! Native bindings for the npm `lru-cache` package.
//!
//! First handle-based port under #466 Phase 5 — exercises the
//! `Handle` / `register_handle` / `with_handle_mut` surface that
//! perry-ffi gained in v0.5.x. Functionally identical to
//! `crates/perry-stdlib/src/lru_cache.rs`.
//!
//! Keys + values are stored as f64 bit-patterns so the FFI ABI
//! stays homogeneous (every method takes/returns f64). For
//! string-keyed caches the call site NaN-boxes the string pointer
//! into the f64 — same trick perry-stdlib's copy uses.

use lru::LruCache;
use perry_ffi::{register_handle, with_handle_mut, Handle};
use std::num::NonZeroUsize;

/// Wrapper struct so the registry's downcast resolves uniquely
/// (each wrapper crate uses a private newtype to namespace its
/// handle space within the shared registry).
pub struct LruCacheHandle {
    cache: LruCache<i64, f64>,
}

impl LruCacheHandle {
    pub fn new(max_size: usize) -> Self {
        let size = NonZeroUsize::new(max_size.max(1)).expect("max_size at least 1");
        LruCacheHandle {
            cache: LruCache::new(size),
        }
    }
}

/// `new LRUCache({ max })` — register a fresh cache and return its
/// handle. `max < 1` or NaN falls back to 100 (the default in the
/// npm package's `Map`-options form).
#[no_mangle]
pub extern "C" fn js_lru_cache_new(max_size: f64) -> Handle {
    let max = if max_size.is_nan() || max_size < 1.0 {
        100
    } else {
        max_size as usize
    };
    register_handle(LruCacheHandle::new(max))
}

/// `cache.get(key)` — `NaN` if the key isn't present (matches the
/// existing perry-stdlib convention for "undefined" through f64
/// returns).
#[no_mangle]
pub extern "C" fn js_lru_cache_get(handle: Handle, key: f64) -> f64 {
    let key_bits = key.to_bits() as i64;
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| h.cache.get(&key_bits).copied())
        .flatten()
        .unwrap_or(f64::NAN)
}

/// `cache.set(key, value)` — returns the handle for chaining.
#[no_mangle]
pub extern "C" fn js_lru_cache_set(handle: Handle, key: f64, value: f64) -> Handle {
    let key_bits = key.to_bits() as i64;
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| {
        h.cache.put(key_bits, value);
    });
    handle
}

/// `cache.has(key)` → `1.0` / `0.0`.
#[no_mangle]
pub extern "C" fn js_lru_cache_has(handle: Handle, key: f64) -> f64 {
    let key_bits = key.to_bits() as i64;
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| {
        if h.cache.contains(&key_bits) {
            1.0
        } else {
            0.0
        }
    })
    .unwrap_or(0.0)
}

/// `cache.delete(key)` → `1.0` if removed, `0.0` if absent.
#[no_mangle]
pub extern "C" fn js_lru_cache_delete(handle: Handle, key: f64) -> f64 {
    let key_bits = key.to_bits() as i64;
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| {
        if h.cache.pop(&key_bits).is_some() {
            1.0
        } else {
            0.0
        }
    })
    .unwrap_or(0.0)
}

/// `cache.clear()` — drops every entry.
#[no_mangle]
pub extern "C" fn js_lru_cache_clear(handle: Handle) {
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| h.cache.clear());
}

/// `cache.size` — current entry count.
#[no_mangle]
pub extern "C" fn js_lru_cache_size(handle: Handle) -> f64 {
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| h.cache.len() as f64).unwrap_or(0.0)
}

/// `cache.peek(key)` — like `get` but doesn't bump recency.
#[no_mangle]
pub extern "C" fn js_lru_cache_peek(handle: Handle, key: f64) -> f64 {
    let key_bits = key.to_bits() as i64;
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| h.cache.peek(&key_bits).copied())
        .flatten()
        .unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_set_get_round_trip() {
        let h = js_lru_cache_new(8.0);
        assert_ne!(h, perry_ffi::INVALID_HANDLE);
        js_lru_cache_set(h, 1.0, 100.0);
        assert_eq!(js_lru_cache_get(h, 1.0), 100.0);
        assert_eq!(js_lru_cache_has(h, 1.0), 1.0);
        assert_eq!(js_lru_cache_size(h), 1.0);
    }

    #[test]
    fn lru_eviction_at_max_size() {
        let h = js_lru_cache_new(3.0);
        for i in 0..3 {
            js_lru_cache_set(h, i as f64, (i * 10) as f64);
        }
        assert_eq!(js_lru_cache_size(h), 3.0);
        // Adding a 4th evicts the oldest (key=0).
        js_lru_cache_set(h, 99.0, 990.0);
        assert_eq!(js_lru_cache_has(h, 0.0), 0.0);
        assert_eq!(js_lru_cache_has(h, 99.0), 1.0);
    }

    #[test]
    fn delete_and_clear() {
        let h = js_lru_cache_new(8.0);
        js_lru_cache_set(h, 1.0, 100.0);
        js_lru_cache_set(h, 2.0, 200.0);
        assert_eq!(js_lru_cache_delete(h, 1.0), 1.0);
        assert_eq!(js_lru_cache_delete(h, 1.0), 0.0); // already gone
        assert_eq!(js_lru_cache_size(h), 1.0);
        js_lru_cache_clear(h);
        assert_eq!(js_lru_cache_size(h), 0.0);
    }

    #[test]
    fn peek_does_not_bump_recency() {
        let h = js_lru_cache_new(2.0);
        js_lru_cache_set(h, 1.0, 100.0);
        js_lru_cache_set(h, 2.0, 200.0);
        // `peek(1)` reads but doesn't bump recency. Now adding key
        // 3 should evict key 1 (oldest), not key 2.
        let _ = js_lru_cache_peek(h, 1.0);
        js_lru_cache_set(h, 3.0, 300.0);
        assert_eq!(js_lru_cache_has(h, 1.0), 0.0);
        assert_eq!(js_lru_cache_has(h, 2.0), 1.0);
        assert_eq!(js_lru_cache_has(h, 3.0), 1.0);
    }

    #[test]
    fn missing_key_returns_nan() {
        let h = js_lru_cache_new(8.0);
        let v = js_lru_cache_get(h, 42.0);
        assert!(v.is_nan(), "expected NaN, got {}", v);
    }

    #[test]
    fn invalid_handle_is_no_op() {
        // Operating on a never-registered handle should return
        // sensible defaults, not panic.
        assert!(js_lru_cache_get(99_999, 0.0).is_nan());
        assert_eq!(js_lru_cache_has(99_999, 0.0), 0.0);
        assert_eq!(js_lru_cache_size(99_999), 0.0);
        js_lru_cache_clear(99_999); // no panic
    }
}
