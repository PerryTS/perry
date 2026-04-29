//! LRUCache implementation
//!
//! Native implementation of the lru-cache npm package.
//! Provides a Least Recently Used cache with configurable max size.

use lru::LruCache;
use std::num::NonZeroUsize;

use crate::common::{get_handle_mut, register_handle, Handle};

/// LRUCache handle storing the cache
pub struct LruCacheHandle {
    cache: LruCache<i64, f64>,
}

impl LruCacheHandle {
    pub fn new(max_size: usize) -> Self {
        let size = NonZeroUsize::new(max_size.max(1)).unwrap();
        LruCacheHandle {
            cache: LruCache::new(size),
        }
    }
}

/// Create a new LRUCache with the specified max size
/// new LRUCache({ max: number })
#[no_mangle]
pub extern "C" fn js_lru_cache_new(max_size: f64) -> Handle {
    let max = if max_size.is_nan() || max_size < 1.0 {
        100 // default
    } else {
        max_size as usize
    };
    register_handle(LruCacheHandle::new(max))
}

/// LRUCache.get(key)
/// Returns the value for the key, or NaN if not found
#[no_mangle]
pub extern "C" fn js_lru_cache_get(handle: Handle, key: f64) -> f64 {
    let key_bits = key.to_bits() as i64;

    if let Some(cache) = get_handle_mut::<LruCacheHandle>(handle) {
        if let Some(&value) = cache.cache.get(&key_bits) {
            return value;
        }
    }

    f64::NAN // undefined
}

/// LRUCache.set(key, value)
/// Sets the value for the key
#[no_mangle]
pub extern "C" fn js_lru_cache_set(handle: Handle, key: f64, value: f64) -> Handle {
    let key_bits = key.to_bits() as i64;

    if let Some(cache) = get_handle_mut::<LruCacheHandle>(handle) {
        cache.cache.put(key_bits, value);
    }

    handle // return self for chaining
}

/// LRUCache.has(key)
/// Returns 1.0 if the key exists, 0.0 otherwise
#[no_mangle]
pub extern "C" fn js_lru_cache_has(handle: Handle, key: f64) -> f64 {
    let key_bits = key.to_bits() as i64;

    if let Some(cache) = get_handle_mut::<LruCacheHandle>(handle) {
        return if cache.cache.contains(&key_bits) {
            1.0
        } else {
            0.0
        };
    }

    0.0
}

/// LRUCache.delete(key)
/// Deletes the key from the cache, returns 1.0 if it existed, 0.0 otherwise
#[no_mangle]
pub extern "C" fn js_lru_cache_delete(handle: Handle, key: f64) -> f64 {
    let key_bits = key.to_bits() as i64;

    if let Some(cache) = get_handle_mut::<LruCacheHandle>(handle) {
        return if cache.cache.pop(&key_bits).is_some() {
            1.0
        } else {
            0.0
        };
    }

    0.0
}

/// LRUCache.clear()
/// Clears all entries from the cache
#[no_mangle]
pub extern "C" fn js_lru_cache_clear(handle: Handle) {
    if let Some(cache) = get_handle_mut::<LruCacheHandle>(handle) {
        cache.cache.clear();
    }
}

/// LRUCache.size
/// Returns the current number of entries
#[no_mangle]
pub extern "C" fn js_lru_cache_size(handle: Handle) -> f64 {
    if let Some(cache) = get_handle_mut::<LruCacheHandle>(handle) {
        return cache.cache.len() as f64;
    }

    0.0
}

/// LRUCache.peek(key)
/// Returns the value without updating recency
#[no_mangle]
pub extern "C" fn js_lru_cache_peek(handle: Handle, key: f64) -> f64 {
    let key_bits = key.to_bits() as i64;

    if let Some(cache) = get_handle_mut::<LruCacheHandle>(handle) {
        if let Some(&value) = cache.cache.peek(&key_bits) {
            return value;
        }
    }

    f64::NAN // undefined
}
