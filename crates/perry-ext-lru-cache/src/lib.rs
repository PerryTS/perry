//! Native bindings for the npm `lru-cache` package.
//!
//! Handle-based port under #466 Phase 5 — exercises the
//! `Handle` / `register_handle` / `with_handle_mut` surface plus the
//! perry-ffi GC-root-scanner surface (`gc_register_mutable_root_scanner_named`).
//!
//! # Values and keys are real JS values, not raw `f64` numbers
//!
//! Perry NaN-boxes every JS value into an `f64`, so the FFI ABI stays
//! homogeneous (every method takes/returns `f64`). But the *contents*
//! are arbitrary JS values, and this wrapper treats them as such:
//!
//! - **String keys hash/compare by CONTENT.** The previous version used
//!   `key.to_bits() as i64` as the map key, so two different string
//!   allocations holding the same text (or an SSO short string vs a heap
//!   string) were treated as different keys and a `cache.get("k")` after
//!   `cache.set("k", …)` missed. We materialize the key via
//!   `js_get_string_pointer_unified` and key the map on the UTF-8 bytes.
//!
//! - **Stored heap values are GC roots for as long as they are cached.**
//!   A cached object/string is otherwise unreachable from the JS shadow
//!   stack, so the collector would free it out from under the cache — a
//!   use-after-free that surfaces later as "value is not a function" /
//!   corrupted reads. We register a mutable root scanner that visits
//!   every cached value slot on each GC cycle, so live values are marked
//!   AND rewritten to their forwarded address after copying evacuation.
//!
//! # Options
//!
//! `new LRUCache({ max, ttl, updateAgeOnGet })` is parsed from the
//! NaN-boxed options object (mirrors npm's option surface for the parts
//! typical callers use):
//!
//! - `max` — capacity; entries past it evict LRU-first (default 100 when
//!   absent, so an unconfigured cache still has a bound).
//! - `ttl` — per-entry time-to-live in ms. `get`/`has`/`peek` on an
//!   expired entry behave as if it were absent; `get` also evicts it.
//! - `updateAgeOnGet` — on a live `get`, reset the entry's TTL clock so
//!   its age restarts from the access (npm semantics).
//!
//! The clock is the runtime's `performance.now()` (`js_performance_now`)
//! — the same monotonic source npm lru-cache uses (`perf_now`), and it
//! honors Perry's mock-timer facility.
//!
//! ## Not (yet) implemented vs npm lru-cache
//!
//! `maxSize`/`sizeCalculation`, `dispose`/`disposeAfter`, `fetch`,
//! `allowStale`, per-call `set`/`get` option objects, and the
//! iterator/`forEach`/`entries` surface are out of scope — the ABI only
//! carries `(key, value)`. **Object-identity keys** (using an object as a
//! key) are supported by pointer identity but are NOT tracked across a
//! GC relocation; primitive keys (string/number/bool) are the faithful,
//! GC-safe path and cover all real usage.

use lru::LruCache;
use perry_ffi::{
    gc_register_mutable_root_scanner_named, iter_handles_of_mut, read_bytes, register_handle,
    with_handle_mut, GcRootVisitor, Handle, JsString, JsValue, ObjectHeader, StringHeader,
};
use std::num::NonZeroUsize;
use std::sync::Once;

const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

extern "C" {
    // Monotonic ms clock — same source npm lru-cache uses (`perf_now`);
    // honors Perry's mock timers.
    fn js_performance_now() -> f64;
    // Materialize any string repr (heap `STRING_TAG` or inline SSO
    // `SHORT_STRING_TAG`) into a real `*StringHeader` so we can read bytes.
    fn js_get_string_pointer_unified(value: f64) -> i64;
    // Read a numeric/boolean option field off the NaN-boxed options object.
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    fn js_is_truthy(value: f64) -> i32;
}

#[inline]
fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

/// A NaN-boxed JS boolean. npm `has`/`delete` return real booleans, and a
/// NaN-tagged bool round-trips through the `f64`-wide ABI unchanged (same as
/// the object pointers `get` already returns).
#[inline]
fn js_bool(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE })
}

/// An owned, GC-independent map key.
///
/// Primitives are stored by value/content so a relocation of the caller's
/// JS value never invalidates a stored key. Object keys fall back to
/// pointer identity (see the crate-level "Not implemented" note).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum CacheKey {
    /// Canonicalized `f64` bits (+0/-0 unified, all NaNs unified — matching
    /// JS `Map` SameValueZero key semantics).
    Num(u64),
    /// UTF-8 (or raw byte) content of a string key.
    Str(Box<[u8]>),
    Bool(bool),
    Null,
    Undefined,
    /// Heap pointer identity (lower 48 bits) for object/array/function keys.
    Obj(u64),
}

#[inline]
fn canonical_num_bits(n: f64) -> u64 {
    if n == 0.0 {
        0.0f64.to_bits() // unify +0.0 / -0.0
    } else if n.is_nan() {
        f64::NAN.to_bits() // unify all NaN payloads
    } else {
        n.to_bits()
    }
}

/// Derive an owned [`CacheKey`] from a NaN-boxed key value.
fn cache_key(key: f64) -> CacheKey {
    let jv = JsValue::from_bits(key.to_bits());
    if jv.is_any_string() {
        // Materialize either string repr into a heap header, then copy bytes.
        let ptr = unsafe { js_get_string_pointer_unified(key) } as *mut StringHeader;
        if !ptr.is_null() {
            let handle = unsafe { JsString::from_raw(ptr) };
            if let Some(bytes) = read_bytes(handle) {
                return CacheKey::Str(bytes.to_vec().into_boxed_slice());
            }
        }
        CacheKey::Str(Box::default())
    } else if jv.is_int32() {
        CacheKey::Num(canonical_num_bits(jv.to_int32() as f64))
    } else if jv.is_undefined() {
        CacheKey::Undefined
    } else if jv.is_null() {
        CacheKey::Null
    } else if jv.is_bool() {
        CacheKey::Bool(jv.to_bool())
    } else if jv.is_pointer() {
        CacheKey::Obj(key.to_bits() & POINTER_MASK)
    } else {
        // Real numbers (and anything else numeric) key by canonical bits.
        CacheKey::Num(canonical_num_bits(f64::from_bits(key.to_bits())))
    }
}

/// A cached value plus its optional TTL expiry (ms on the `performance.now`
/// clock). `value_bits` are NaN-boxed JS value bits, GC-rooted by
/// [`scan_lru_roots`] while the entry is live.
struct Entry {
    value_bits: u64,
    expires_at: Option<f64>,
}

impl Entry {
    #[inline]
    fn is_expired(&self, now: f64) -> bool {
        matches!(self.expires_at, Some(t) if now >= t)
    }
}

/// Wrapper struct so the registry's downcast resolves uniquely.
pub struct LruCacheHandle {
    cache: LruCache<CacheKey, Entry>,
    /// Default per-entry TTL in ms, or `None` when `ttl` was not set.
    ttl_ms: Option<f64>,
    /// npm `updateAgeOnGet` — refresh an entry's TTL clock on `get`.
    update_age_on_get: bool,
}

impl LruCacheHandle {
    fn new(max_size: usize, ttl_ms: Option<f64>, update_age_on_get: bool) -> Self {
        let size = NonZeroUsize::new(max_size.max(1)).expect("max_size at least 1");
        LruCacheHandle {
            cache: LruCache::new(size),
            ttl_ms,
            update_age_on_get,
        }
    }

    #[inline]
    fn expiry_from_now(&self, now: f64) -> Option<f64> {
        self.ttl_ms.and_then(|ttl| (ttl > 0.0).then_some(now + ttl))
    }
}

static GC_REGISTERED: Once = Once::new();

fn ensure_gc_scanner() {
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-lru-cache", scan_lru_roots);
    });
}

/// GC root scanner: visit every cached value slot across every live cache
/// handle so the collector marks the referent and, under copying
/// evacuation, rewrites the stored bits to the forwarded address.
fn scan_lru_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<LruCacheHandle, _>(|h| {
        for (_key, entry) in h.cache.iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut entry.value_bits);
        }
    });
}

#[inline]
fn now_ms() -> f64 {
    unsafe { js_performance_now() }
}

/// Read a numeric option field; `None` when absent or non-numeric.
unsafe fn option_number(ptr: *const ObjectHeader, name: &str) -> Option<f64> {
    let key = perry_ffi::alloc_string(name);
    let raw = js_object_get_field_by_name_f64(ptr, key.as_raw());
    let jv = JsValue::from_bits(raw.to_bits());
    if jv.is_int32() {
        Some(jv.to_int32() as f64)
    } else if jv.is_number() && !raw.is_nan() {
        Some(raw)
    } else {
        None
    }
}

/// `new LRUCache(options)` — register a fresh cache and return its handle.
///
/// `options` is the NaN-boxed options object. `max < 1` / absent falls back
/// to 100 (so an unconfigured cache is still bounded). `ttl` and
/// `updateAgeOnGet` are honored when present.
#[no_mangle]
pub extern "C" fn js_lru_cache_new(options: f64) -> Handle {
    ensure_gc_scanner();

    let mut max = 100usize;
    let mut ttl_ms = None;
    let mut update_age_on_get = false;

    let jv = JsValue::from_bits(options.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<ObjectHeader>();
        if !ptr.is_null() && (ptr as usize) >= 0x1000 {
            unsafe {
                if let Some(n) = option_number(ptr, "max") {
                    if n >= 1.0 {
                        max = n as usize;
                    }
                }
                if let Some(n) = option_number(ptr, "ttl") {
                    if n > 0.0 {
                        ttl_ms = Some(n);
                    }
                }
                let key = perry_ffi::alloc_string("updateAgeOnGet");
                let uaog = js_object_get_field_by_name_f64(ptr, key.as_raw());
                if js_is_truthy(uaog) != 0 {
                    update_age_on_get = true;
                }
            }
        }
    }

    register_handle(LruCacheHandle::new(max, ttl_ms, update_age_on_get))
}

/// `cache.get(key)` — returns `undefined` when the key is absent or its
/// entry has expired (an expired entry is evicted). Bumps LRU recency; when
/// `updateAgeOnGet` is set, also resets the entry's TTL clock.
#[no_mangle]
pub extern "C" fn js_lru_cache_get(handle: Handle, key: f64) -> f64 {
    let k = cache_key(key);
    let now = now_ms();
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| {
        let refresh = h.update_age_on_get;
        let new_expiry = h.expiry_from_now(now);
        let outcome = match h.cache.get_mut(&k) {
            Some(entry) => {
                if entry.is_expired(now) {
                    None // expired → evict below
                } else {
                    if refresh {
                        entry.expires_at = new_expiry;
                    }
                    Some(entry.value_bits)
                }
            }
            None => return undefined(),
        };
        match outcome {
            Some(bits) => f64::from_bits(bits),
            None => {
                h.cache.pop(&k);
                undefined()
            }
        }
    })
    .unwrap_or_else(undefined)
}

/// `cache.set(key, value)` — returns the handle for chaining.
#[no_mangle]
pub extern "C" fn js_lru_cache_set(handle: Handle, key: f64, value: f64) -> Handle {
    let k = cache_key(key);
    let now = now_ms();
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| {
        let expires_at = h.expiry_from_now(now);
        h.cache.put(
            k,
            Entry {
                value_bits: value.to_bits(),
                expires_at,
            },
        );
    });
    handle
}

/// `cache.has(key)` → `true` / `false`. Does not bump recency and does not
/// refresh age; an expired entry reads as absent (but is not evicted here,
/// matching npm's lazy purge).
#[no_mangle]
pub extern "C" fn js_lru_cache_has(handle: Handle, key: f64) -> f64 {
    let k = cache_key(key);
    let now = now_ms();
    js_bool(
        with_handle_mut::<LruCacheHandle, _, _>(handle, |h| {
            matches!(h.cache.peek(&k), Some(entry) if !entry.is_expired(now))
        })
        .unwrap_or(false),
    )
}

/// `cache.delete(key)` → `true` if removed, `false` if absent.
#[no_mangle]
pub extern "C" fn js_lru_cache_delete(handle: Handle, key: f64) -> f64 {
    let k = cache_key(key);
    js_bool(
        with_handle_mut::<LruCacheHandle, _, _>(handle, |h| h.cache.pop(&k).is_some())
            .unwrap_or(false),
    )
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

/// `cache.peek(key)` — like `get` but doesn't bump recency and doesn't
/// refresh age. Returns `undefined` for an absent or expired entry (and
/// leaves an expired entry in place, matching npm's lazy purge).
#[no_mangle]
pub extern "C" fn js_lru_cache_peek(handle: Handle, key: f64) -> f64 {
    let k = cache_key(key);
    let now = now_ms();
    with_handle_mut::<LruCacheHandle, _, _>(handle, |h| match h.cache.peek(&k) {
        Some(entry) if !entry.is_expired(now) => f64::from_bits(entry.value_bits),
        _ => undefined(),
    })
    .unwrap_or_else(undefined)
}

#[cfg(test)]
mod tests;
