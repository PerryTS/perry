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
//! - `max` — capacity; entries past it evict LRU-first. `0`/absent means
//!   unbounded, which npm only permits together with a `ttl`.
//! - `ttl` — per-entry time-to-live in ms. `get`/`has`/`peek` on an
//!   expired entry behave as if it were absent; `get` also evicts it.
//! - `updateAgeOnGet` — on a live `get`, reset the entry's TTL clock so
//!   its age restarts from the access (npm semantics).
//!
//! The clock is the runtime's `performance.now()` (`js_performance_now`)
//! — the same monotonic source npm lru-cache uses (`perf_now`), and it
//! honors Perry's mock-timer facility.
//!
//! ## Option validation is npm's, measured — not invented
//!
//! npm `lru-cache` rejects bad `max`/`ttl` loudly, and the exact errors
//! are the contract a caller writes `try`/`catch` against. Every case
//! below was measured against `lru-cache@11.5.2` on the pinned oracle
//! (Node 26.5.1) and is reproduced here, message for message:
//!
//! | `new LRUCache(…)` | throws |
//! |---|---|
//! | `()` | `TypeError: Cannot read properties of undefined (reading 'max')` |
//! | `(null)` | `TypeError: Cannot read properties of null (reading 'max')` |
//! | `(5)`, `("x")`, `({})`, `({ max: 0 })` | `TypeError: At least one of max, maxSize, or ttl is required` |
//! | `({ max: -1 \| 1.5 \| Infinity \| NaN \| "3" \| true \| null })` | `TypeError: max option must be a nonnegative integer` |
//! | `({ max: 2**32 })` … up to `MAX_SAFE_INTEGER` | `RangeError: Invalid array length` |
//! | `({ max: 2**53 })`, `({ max: 1e300 })` | `Error: invalid max value: <n>` |
//! | `({ max: 3, ttl: -5 \| 1.5 \| Infinity \| "5" })` | `TypeError: ttl must be a positive integer if specified` |
//!
//! The two upper bounds are not arbitrary: npm builds its index arrays
//! with `Array.from({ length: max })` (so `max` past the JS array-length
//! limit is a `RangeError`) after an `getUintArray(max)` lookup that
//! returns `null` past `Number.MAX_SAFE_INTEGER` (a plain `Error`).
//! Reproducing them is what keeps `new LRUCache({ max: 1e12 })` from
//! reaching an allocator with a 10^12-entry reservation.
//!
//! ## Not (yet) implemented vs npm lru-cache
//!
//! `maxSize`/`sizeCalculation`, `dispose`/`disposeAfter`, `fetch`,
//! `allowStale`, per-call `set`/`get` option objects, and the
//! iterator/`forEach`/`entries` surface are out of scope — the ABI only
//! carries `(key, value)`. Because `maxSize` is unimplemented it also does
//! not satisfy npm's "at least one of max, maxSize, or ttl" requirement:
//! a `maxSize`-only cache constructs on npm but throws here, which is the
//! loud failure rather than a silently unbounded cache. npm's
//! `UnboundedCacheWarning` (`ttl`-only caches) is likewise not emitted.
//! **Object-identity keys** (using an object as a key) are supported by
//! pointer identity but are NOT tracked across a GC relocation; primitive
//! keys (string/number/bool) are the faithful, GC-safe path and cover all
//! real usage.

use lru::LruCache;
use perry_ffi::{
    gc_register_mutable_root_scanner_named, iter_handles_of_mut, read_bytes, register_handle,
    throw_with_code, with_handle_mut, ErrorKind, GcRootVisitor, Handle, JsString, JsValue,
    ObjectHeader, StringHeader,
};
use std::num::NonZeroUsize;
use std::sync::Once;

const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

/// Largest `max` npm can build its index arrays for
/// (`Array.from({ length: max })`, i.e. the JS array-length limit).
const MAX_ARRAY_LENGTH: f64 = 4_294_967_295.0;
/// Above this npm's `getUintArray(max)` returns `null` and the constructor
/// raises a plain `Error` instead of a `RangeError`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
/// Below this a "pointer" is a small integer handle, not a real object.
const MIN_OBJECT_ADDRESS: usize = 0x1000;

extern "C" {
    // Monotonic ms clock — same source npm lru-cache uses (`perf_now`);
    // honors Perry's mock timers.
    fn js_performance_now() -> f64;
    // Materialize any string repr (heap `STRING_TAG` or inline SSO
    // `SHORT_STRING_TAG`) into a real `*StringHeader` so we can read bytes.
    fn js_get_string_pointer_unified(value: f64) -> i64;
    // Read an option field off the NaN-boxed options object.
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    fn js_is_truthy(value: f64) -> i32;
    // JS `String(n)` — npm interpolates the offending `max` into its
    // "invalid max value" message, and JS renders `1e300` as `"1e+300"`
    // where Rust's `{}` would print 301 digits.
    fn js_number_to_string(value: f64) -> *mut StringHeader;
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
    /// A string key whose bytes could not be materialized. Kept distinct
    /// from `Str(b"")` so a failed read never aliases the empty-string key
    /// — and keyed on the value's own bits so two *different* unresolvable
    /// strings do not alias each other either. Such a key can only ever be
    /// hit again by the identical value, which is the safe direction: a
    /// miss, never someone else's entry.
    UnresolvedStr(u64),
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
        CacheKey::UnresolvedStr(key.to_bits())
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
    /// `max_size == 0` is npm's unbounded (`ttl`-only) cache.
    ///
    /// `LruCache::new(cap)` eagerly reserves a `HashMap` of `cap` buckets.
    /// npm accepts any `max` up to the JS array-length limit, so an eager
    /// reservation turns a legal `new LRUCache({ max: 1e9 })` into a
    /// multi-gigabyte allocation before the first insert — the same shape
    /// of failure npm itself hits (it OOMs Node there). `unbounded()` plus
    /// `resize()` yields an identical eviction bound over a lazily grown
    /// map, so Perry survives a range where npm dies. That is the only
    /// deliberate divergence in this constructor and it is one-directional:
    /// no program can observe it except by not running out of memory.
    fn new(max_size: usize, ttl_ms: Option<f64>, update_age_on_get: bool) -> Self {
        let mut cache = LruCache::unbounded();
        if let Some(cap) = NonZeroUsize::new(max_size) {
            cache.resize(cap);
        }
        LruCacheHandle {
            cache,
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

/// Read `options.<name>` as a raw NaN-boxed value.
///
/// `None` means `options` was not an object. npm destructures its options
/// argument, and destructuring a primitive yields `undefined` for every
/// field rather than throwing, so that is what a non-object reads as here.
fn option_value(fields: Option<*const ObjectHeader>, name: &str) -> JsValue {
    let Some(ptr) = fields else {
        return JsValue::UNDEFINED;
    };
    let key = perry_ffi::alloc_string(name);
    // SAFETY: `fields` is a validated non-null object pointer (see
    // `js_lru_cache_new`), and `key` owns the freshly allocated header.
    let raw = unsafe { js_object_get_field_by_name_f64(ptr, key.as_raw()) };
    JsValue::from_bits(raw.to_bits())
}

/// npm's `isPosInt`: `!!n && n === Math.floor(n) && n > 0 && isFinite(n)`.
///
/// The `===` is a *strict* compare against `Math.floor(n)`, so a non-number
/// can never be a positive integer — `"3"`, `true` and `null` all fail it,
/// which is why they raise the same `TypeError` as `-1` does.
fn is_pos_int(v: JsValue) -> bool {
    if !v.is_number() {
        return false;
    }
    let n = v.to_number();
    n.is_finite() && n > 0.0 && n == n.trunc()
}

/// JS `String(n)`, for interpolating a number into an npm error message.
fn js_number_string(n: f64) -> String {
    // SAFETY: the runtime returns either null or a live `StringHeader`.
    let ptr = unsafe { js_number_to_string(n) };
    if ptr.is_null() {
        return n.to_string();
    }
    let handle = unsafe { JsString::from_raw(ptr) };
    read_bytes(handle).map_or_else(
        || n.to_string(),
        |b| String::from_utf8_lossy(b).into_owned(),
    )
}

/// npm: `const { max = 0 } = options; if (max !== 0 && !isPosInt(max)) throw …`
/// followed by the `getUintArray` / `Array.from({ length: max })` bounds.
/// Returns the validated `max` (`0` = unbounded); diverges on a bad value.
fn parse_max(fields: Option<*const ObjectHeader>) -> f64 {
    let raw = option_value(fields, "max");
    if raw.is_undefined() {
        return 0.0; // npm's `max = 0` destructuring default
    }
    // npm's `max !== 0` is strict, so only the *numbers* +0/-0 skip the
    // validation below. `null`/`false`/`""` are all `!== 0` and throw.
    if raw.is_number() && raw.to_number() == 0.0 {
        return 0.0;
    }
    if !is_pos_int(raw) {
        throw_with_code(
            "max option must be a nonnegative integer",
            "",
            ErrorKind::TypeError,
        );
    }
    let max = raw.to_number();
    if max > MAX_SAFE_INTEGER {
        // npm: `if (!UintArray) throw new Error('invalid max value: ' + max)`.
        let msg = format!("invalid max value: {}", js_number_string(max));
        throw_with_code(&msg, "", ErrorKind::Error);
    }
    if max > MAX_ARRAY_LENGTH {
        // npm: `Array.from({ length: max })` — V8's array-length check.
        throw_with_code("Invalid array length", "", ErrorKind::RangeError);
    }
    max
}

/// npm: `this.ttl = ttl || 0; if (this.ttl && !isPosInt(this.ttl)) throw …`.
/// Returns the validated ttl in ms (`0` = none); diverges on a bad value.
fn parse_ttl(fields: Option<*const ObjectHeader>) -> f64 {
    let raw = option_value(fields, "ttl");
    // `ttl || 0` — undefined, null, `0`, `NaN` and `""` all collapse to 0
    // *without* tripping the validation (npm only checks a truthy ttl).
    // SAFETY: `js_is_truthy` reads a NaN-boxed value by value.
    if unsafe { js_is_truthy(f64::from_bits(raw.bits())) } == 0 {
        return 0.0;
    }
    if !is_pos_int(raw) {
        throw_with_code(
            "ttl must be a positive integer if specified",
            "",
            ErrorKind::TypeError,
        );
    }
    raw.to_number()
}

/// `new LRUCache(options)` — register a fresh cache and return its handle.
///
/// `options` is the NaN-boxed options argument. Validation mirrors npm
/// `lru-cache` exactly (see the crate-level table); a rejected option
/// throws the JS error npm throws rather than being silently clamped.
#[no_mangle]
pub extern "C" fn js_lru_cache_new(options: f64) -> Handle {
    ensure_gc_scanner();

    let opts = JsValue::from_bits(options.to_bits());
    // npm destructures `options` in the constructor *signature*, so a
    // missing or null argument is a property read on undefined/null. The
    // message a caller sees on Node is V8's, so that is the message here.
    if opts.is_undefined() {
        throw_with_code(
            "Cannot read properties of undefined (reading 'max')",
            "",
            ErrorKind::TypeError,
        );
    }
    if opts.is_null() {
        throw_with_code(
            "Cannot read properties of null (reading 'max')",
            "",
            ErrorKind::TypeError,
        );
    }
    // Any other primitive destructures cleanly into all-undefined fields.
    let fields = if opts.is_pointer() {
        let ptr = opts.as_pointer::<ObjectHeader>();
        (!ptr.is_null() && (ptr as usize) >= MIN_OBJECT_ADDRESS).then_some(ptr as *const _)
    } else {
        None
    };

    let max = parse_max(fields);
    let ttl = parse_ttl(fields);
    if max == 0.0 && ttl == 0.0 {
        // npm: "do not allow completely unbounded caches". `maxSize` would
        // also satisfy this on npm, but it is unimplemented here (see the
        // crate-level scope note), so it cannot.
        throw_with_code(
            "At least one of max, maxSize, or ttl is required",
            "",
            ErrorKind::TypeError,
        );
    }
    // SAFETY: `js_is_truthy` reads a NaN-boxed value by value.
    let update_age_on_get = unsafe {
        js_is_truthy(f64::from_bits(
            option_value(fields, "updateAgeOnGet").bits(),
        )) != 0
    };

    register_handle(LruCacheHandle::new(
        max as usize,
        (ttl > 0.0).then_some(ttl),
        update_age_on_get,
    ))
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
        with_handle_mut::<LruCacheHandle, _, _>(
            handle,
            |h| matches!(h.cache.peek(&k), Some(entry) if !entry.is_expired(now)),
        )
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
