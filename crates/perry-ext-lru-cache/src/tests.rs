//! Unit tests for the lru-cache wrapper.
//!
//! Every test links `perry-runtime` (the `runtime-link` dev-dep feature)
//! because the wrapper reaches the runtime for its clock
//! (`js_performance_now`) and string materialization
//! (`js_get_string_pointer_unified`), and reaches `perry-runtime`
//! directly for `js_call_catching` so a rejected constructor option can be
//! asserted on instead of exiting the process. The GC-survival test lives
//! in `tests/gc_survival.rs` — see that file for why it needs its own
//! process.

use super::*;
use perry_ffi::{alloc_string, nanbox_string_bits, JsValue, ObjectHeader};
extern "C" {
    fn js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader;
    fn js_object_set_field_by_name(obj: *mut ObjectHeader, key: *const StringHeader, value: f64);
}

/// NaN-boxed `f64` for a freshly allocated JS string with `text`.
fn string_value(text: &str) -> f64 {
    let s = alloc_string(text);
    assert!(!s.is_null(), "alloc_string returned null");
    f64::from_bits(nanbox_string_bits(s.as_raw()))
}

/// Build a real JS options object carrying `fields`, the way a compiled
/// `new LRUCache({ … })` object literal reaches the constructor.
///
/// The slot count is declared up front rather than using
/// `perry_ffi::alloc_object()` (which allocates zero inline slots and
/// grows on the first `js_object_set_field_by_name`). That is not a
/// stylistic preference: an object built the zero-slot way makes the
/// copying minor collector walk a bogus slot address and SIGSEGV in
/// `gc::copying::scan_slot` on the *next* collection in the process
/// (reproduced with `--test-threads=1`; see the PR discussion). Declaring
/// the slots avoids the grow path entirely. A compiled object literal
/// declares its slots the same way, so this is also the more faithful
/// model of what reaches the constructor.
fn options(fields: &[(&str, f64)]) -> f64 {
    let ptr = unsafe { js_object_alloc(0, fields.len() as u32) };
    assert!(!ptr.is_null(), "js_object_alloc returned null");
    for (name, value) in fields {
        let key = alloc_string(name);
        unsafe { js_object_set_field_by_name(ptr, key.as_raw(), *value) };
    }
    f64::from_bits(JsValue::from_object_ptr(ptr).bits())
}

/// `new LRUCache({ max })` through the real constructor.
fn new_cache(max: f64) -> Handle {
    js_lru_cache_new(options(&[("max", max)]))
}

/// Call `js_lru_cache_new` inside a JS `try` so a rejected option can be
/// asserted on. Without the `try`, a throw at depth 0 prints the uncaught
/// error and exits the process, taking the test binary with it.
fn new_catching(opts: f64) -> Result<Handle, (String, String)> {
    match perry_runtime::exception::js_call_catching(|| js_lru_cache_new(opts) as f64) {
        Ok(handle) => Ok(handle as Handle),
        Err(bits) => {
            let field = |name: &str| {
                let key = alloc_string(name);
                let v = unsafe { js_object_get_field_by_name_boxed(bits, key.as_raw()) };
                read_string_value(v).unwrap_or_default()
            };
            Err((field("name"), field("message")))
        }
    }
}

/// Assert `new LRUCache(opts)` throws npm's `name` + `message`.
#[track_caller]
fn assert_throws(opts: f64, name: &str, message: &str) {
    match new_catching(opts) {
        Ok(h) => {
            perry_ffi::drop_handle(h);
            panic!("expected {name}: {message}, but the constructor returned a cache");
        }
        Err((got_name, got_message)) => {
            assert_eq!(
                (got_name.as_str(), got_message.as_str()),
                (name, message),
                "constructor error must match npm lru-cache byte for byte"
            );
        }
    }
}

/// npm `has`/`delete` return NaN-boxed JS booleans — decode one.
fn is_true(v: f64) -> bool {
    v.to_bits() == TAG_TRUE
}

/// Read the string content behind a NaN-boxed value produced by the cache.
fn read_string_value(value: f64) -> Option<String> {
    let ptr = unsafe { js_get_string_pointer_unified(value) } as *mut StringHeader;
    if ptr.is_null() {
        return None;
    }
    let handle = unsafe { JsString::from_raw(ptr) };
    read_bytes(handle).map(|b| String::from_utf8_lossy(b).into_owned())
}

// ── pure key-derivation logic (no runtime clock) ─────────────────────

#[test]
fn canonical_num_unifies_zero_and_nan() {
    assert_eq!(canonical_num_bits(0.0), canonical_num_bits(-0.0));
    assert_eq!(
        canonical_num_bits(f64::NAN),
        canonical_num_bits(f64::from_bits(0x7FF8_0000_0000_0001))
    );
    assert_ne!(canonical_num_bits(1.0), canonical_num_bits(2.0));
}

#[test]
fn cache_key_primitive_variants() {
    assert_eq!(cache_key(3.5), CacheKey::Num(canonical_num_bits(3.5)));
    assert_eq!(
        cache_key(f64::from_bits(JsValue::from_int32(7).bits())),
        CacheKey::Num(canonical_num_bits(7.0))
    );
    assert_eq!(
        cache_key(f64::from_bits(JsValue::TRUE.bits())),
        CacheKey::Bool(true)
    );
    assert_eq!(
        cache_key(f64::from_bits(JsValue::NULL.bits())),
        CacheKey::Null
    );
    assert_eq!(
        cache_key(f64::from_bits(JsValue::UNDEFINED.bits())),
        CacheKey::Undefined
    );
}

// ── numeric-key behaviour (parity with the old surface) ──────────────

#[test]
fn basic_set_get_round_trip() {
    let h = new_cache(10.0);
    assert_ne!(h, perry_ffi::INVALID_HANDLE);
    js_lru_cache_set(h, 1.0, 100.0);
    assert_eq!(js_lru_cache_get(h, 1.0), 100.0);
    assert!(is_true(js_lru_cache_has(h, 1.0)));
    assert_eq!(js_lru_cache_size(h), 1.0);
    perry_ffi::drop_handle(h);
}

#[test]
fn lru_eviction_at_max_size() {
    // max:3 via a directly-built handle (options-object parsing is covered
    // end-to-end by the compiled smoke program, not reachable from a unit
    // test without allocating a real JS object).
    let h = perry_ffi::register_handle(LruCacheHandle::new(3, None, false));
    for i in 0..3 {
        js_lru_cache_set(h, i as f64, (i * 10) as f64);
    }
    assert_eq!(js_lru_cache_size(h), 3.0);
    // Adding a 4th evicts the LRU (key=0).
    js_lru_cache_set(h, 99.0, 990.0);
    assert_eq!(js_lru_cache_size(h), 3.0);
    assert!(!is_true(js_lru_cache_has(h, 0.0)));
    assert!(is_true(js_lru_cache_has(h, 99.0)));
    perry_ffi::drop_handle(h);
}

#[test]
fn delete_and_clear() {
    let h = new_cache(10.0);
    js_lru_cache_set(h, 1.0, 100.0);
    js_lru_cache_set(h, 2.0, 200.0);
    assert!(is_true(js_lru_cache_delete(h, 1.0)));
    assert!(!is_true(js_lru_cache_delete(h, 1.0))); // already gone
    assert_eq!(js_lru_cache_size(h), 1.0);
    js_lru_cache_clear(h);
    assert_eq!(js_lru_cache_size(h), 0.0);
    perry_ffi::drop_handle(h);
}

#[test]
fn peek_does_not_bump_recency() {
    let h = perry_ffi::register_handle(LruCacheHandle::new(2, None, false));
    js_lru_cache_set(h, 1.0, 100.0);
    js_lru_cache_set(h, 2.0, 200.0);
    // peek(1) reads but does not bump recency, so adding key 3 evicts key 1.
    let _ = js_lru_cache_peek(h, 1.0);
    js_lru_cache_set(h, 3.0, 300.0);
    assert!(!is_true(js_lru_cache_has(h, 1.0)));
    assert!(is_true(js_lru_cache_has(h, 2.0)));
    assert!(is_true(js_lru_cache_has(h, 3.0)));
    perry_ffi::drop_handle(h);
}

#[test]
fn missing_key_returns_undefined() {
    let h = new_cache(10.0);
    let v = js_lru_cache_get(h, 42.0);
    assert_eq!(v.to_bits(), TAG_UNDEFINED, "missing key must be undefined");
    perry_ffi::drop_handle(h);
}

#[test]
fn invalid_handle_is_no_op() {
    assert_eq!(js_lru_cache_get(99_999, 0.0).to_bits(), TAG_UNDEFINED);
    assert!(!is_true(js_lru_cache_has(99_999, 0.0)));
    assert_eq!(js_lru_cache_size(99_999), 0.0);
    js_lru_cache_clear(99_999); // no panic
}

// ── string keys hash/compare by content (the core fix) ───────────────

#[test]
fn string_key_round_trip_by_content() {
    let h = new_cache(10.0);
    // Store under one string allocation…
    js_lru_cache_set(h, string_value("cache-key"), 4242.0);
    // …read back through a *different* allocation of the same text. The old
    // pointer-bits keying missed here; content keying hits.
    assert_eq!(js_lru_cache_get(h, string_value("cache-key")), 4242.0);
    assert!(is_true(js_lru_cache_has(h, string_value("cache-key"))));
    assert!(!is_true(js_lru_cache_has(h, string_value("other"))));
    assert_eq!(js_lru_cache_size(h), 1.0);
    perry_ffi::drop_handle(h);
}

#[test]
fn string_key_object_value_round_trip() {
    let h = new_cache(10.0);
    js_lru_cache_set(
        h,
        string_value("payload"),
        string_value("hello-world-value"),
    );
    let got = js_lru_cache_get(h, string_value("payload"));
    assert_eq!(read_string_value(got).as_deref(), Some("hello-world-value"));
    perry_ffi::drop_handle(h);
}

// ── TTL + updateAgeOnGet ─────────────────────────────────────────────

#[test]
fn ttl_expiry_evicts_on_get() {
    let h = perry_ffi::register_handle(LruCacheHandle::new(10, Some(20.0), false));
    js_lru_cache_set(h, 1.0, 111.0);
    assert_eq!(js_lru_cache_get(h, 1.0), 111.0);
    std::thread::sleep(std::time::Duration::from_millis(60));
    // Expired: get returns undefined AND evicts.
    assert_eq!(js_lru_cache_get(h, 1.0).to_bits(), TAG_UNDEFINED);
    assert_eq!(js_lru_cache_size(h), 0.0, "expired entry evicted by get");
    perry_ffi::drop_handle(h);
}

#[test]
fn has_and_peek_report_expired_as_absent() {
    let h = perry_ffi::register_handle(LruCacheHandle::new(10, Some(20.0), false));
    js_lru_cache_set(h, 1.0, 111.0);
    std::thread::sleep(std::time::Duration::from_millis(60));
    assert!(!is_true(js_lru_cache_has(h, 1.0)));
    assert_eq!(js_lru_cache_peek(h, 1.0).to_bits(), TAG_UNDEFINED);
    perry_ffi::drop_handle(h);
}

// The TTL/sleep ratio below is deliberately wide. A 3:1 TTL-to-sleep
// margin means a loaded CI runner has to overshoot a 150 ms sleep by
// 150 ms before the refresh test can misread a live entry as expired.
const REFRESH_TTL_MS: f64 = 450.0;
const REFRESH_STEP: std::time::Duration = std::time::Duration::from_millis(150);

#[test]
fn update_age_on_get_refreshes_ttl() {
    let h = perry_ffi::register_handle(LruCacheHandle::new(10, Some(REFRESH_TTL_MS), true));
    js_lru_cache_set(h, 1.0, 111.0);
    // A third of the way through the TTL, a get refreshes the clock.
    std::thread::sleep(REFRESH_STEP);
    assert_eq!(js_lru_cache_get(h, 1.0), 111.0);
    // Two more steps put us past the original expiry; the entry is still
    // live *because* the get above restarted its clock.
    std::thread::sleep(REFRESH_STEP);
    std::thread::sleep(REFRESH_STEP);
    assert_eq!(js_lru_cache_get(h, 1.0), 111.0, "get refreshed the TTL");
    perry_ffi::drop_handle(h);
}

#[test]
fn no_update_age_on_get_lets_ttl_expire() {
    let h = perry_ffi::register_handle(LruCacheHandle::new(10, Some(REFRESH_TTL_MS), false));
    js_lru_cache_set(h, 1.0, 111.0);
    std::thread::sleep(REFRESH_STEP);
    assert_eq!(js_lru_cache_get(h, 1.0), 111.0); // still live, no refresh
    std::thread::sleep(REFRESH_STEP);
    std::thread::sleep(REFRESH_STEP);
    // Past the TTL measured from `set`, and never refreshed → expired.
    assert_eq!(js_lru_cache_get(h, 1.0).to_bits(), TAG_UNDEFINED);
    perry_ffi::drop_handle(h);
}

// ── options-object parsing ───────────────────────────────────────────
//
// Every expectation below was measured against `lru-cache@11.5.2` under
// the repo's pinned Node oracle (26.5.1). They are npm's errors, not
// Perry's invention — see the table in the crate docs.

#[test]
fn options_object_is_honored() {
    let h = js_lru_cache_new(options(&[
        ("max", 3.0),
        ("ttl", 5_000.0),
        ("updateAgeOnGet", f64::from_bits(TAG_TRUE)),
    ]));
    let parsed =
        with_handle_mut::<LruCacheHandle, _, _>(h, |c| (c.ttl_ms, c.update_age_on_get)).unwrap();
    assert_eq!(parsed, (Some(5_000.0), true));
    for i in 0..4 {
        js_lru_cache_set(h, i as f64, i as f64);
    }
    assert_eq!(js_lru_cache_size(h), 3.0, "max:3 bounds the cache");
    perry_ffi::drop_handle(h);
}

#[test]
fn huge_max_is_a_range_error_not_an_allocation() {
    // The regression this pins: `max: 1e12` used to saturate through
    // `n as usize` into the backing map's reserve and abort the process.
    // npm raises `Array.from({ length: 1e12 })`'s RangeError instead.
    assert_throws(
        options(&[("max", 1e12)]),
        "RangeError",
        "Invalid array length",
    );
    assert_throws(
        options(&[("max", 4_294_967_296.0)]),
        "RangeError",
        "Invalid array length",
    );
    // Past MAX_SAFE_INTEGER npm reports its own `getUintArray` failure,
    // and renders the number the way JS does.
    assert_throws(
        options(&[("max", 9_007_199_254_740_992.0)]),
        "Error",
        "invalid max value: 9007199254740992",
    );
    assert_throws(
        options(&[("max", 1e300)]),
        "Error",
        "invalid max value: 1e+300",
    );
}

#[test]
fn max_below_the_range_error_still_constructs_lazily() {
    // Just under the array-length limit: npm OOMs Node here, Perry does
    // not, because the backing map grows lazily instead of reserving
    // `max` buckets up front. Constructing must be instant and cheap.
    let h = new_cache(MAX_ARRAY_LENGTH);
    js_lru_cache_set(h, 1.0, 111.0);
    assert_eq!(js_lru_cache_get(h, 1.0), 111.0);
    assert_eq!(js_lru_cache_size(h), 1.0);
    perry_ffi::drop_handle(h);
}

#[test]
fn non_integer_max_is_a_type_error() {
    const MSG: &str = "max option must be a nonnegative integer";
    for bad in [-1.0, 1.5, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert_throws(options(&[("max", bad)]), "TypeError", MSG);
    }
    // Non-numbers fail npm's strict `n === Math.floor(n)` too.
    assert_throws(options(&[("max", string_value("3"))]), "TypeError", MSG);
    assert_throws(
        options(&[("max", f64::from_bits(JsValue::TRUE.bits()))]),
        "TypeError",
        MSG,
    );
    assert_throws(
        options(&[("max", f64::from_bits(JsValue::NULL.bits()))]),
        "TypeError",
        MSG,
    );
}

#[test]
fn unbounded_cache_is_rejected() {
    const MSG: &str = "At least one of max, maxSize, or ttl is required";
    // `max: 0` and `max: -0` pass npm's `max !== 0` guard, then fall into
    // its "do not allow completely unbounded caches" check.
    assert_throws(options(&[("max", 0.0)]), "TypeError", MSG);
    assert_throws(options(&[("max", -0.0)]), "TypeError", MSG);
    assert_throws(options(&[]), "TypeError", MSG);
    // A primitive `options` destructures into all-undefined fields.
    assert_throws(5.0, "TypeError", MSG);
    assert_throws(string_value("x"), "TypeError", MSG);
}

#[test]
fn missing_options_matches_npm_destructuring() {
    assert_throws(
        f64::from_bits(TAG_UNDEFINED),
        "TypeError",
        "Cannot read properties of undefined (reading 'max')",
    );
    assert_throws(
        f64::from_bits(JsValue::NULL.bits()),
        "TypeError",
        "Cannot read properties of null (reading 'max')",
    );
}

#[test]
fn ttl_alone_makes_an_unbounded_cache() {
    let h = js_lru_cache_new(options(&[("ttl", 5_000.0)]));
    for i in 0..1_000 {
        js_lru_cache_set(h, i as f64, i as f64);
    }
    assert_eq!(js_lru_cache_size(h), 1_000.0, "no max ⇒ no eviction");
    perry_ffi::drop_handle(h);
}

#[test]
fn non_integer_ttl_is_a_type_error() {
    const MSG: &str = "ttl must be a positive integer if specified";
    for bad in [-5.0, 1.5, f64::INFINITY] {
        assert_throws(options(&[("max", 3.0), ("ttl", bad)]), "TypeError", MSG);
    }
    assert_throws(
        options(&[("max", 3.0), ("ttl", string_value("5"))]),
        "TypeError",
        MSG,
    );
    // npm's `ttl || 0` swallows every falsy ttl before the check runs.
    for falsy in [0.0, f64::NAN] {
        let h = js_lru_cache_new(options(&[("max", 3.0), ("ttl", falsy)]));
        let ttl = with_handle_mut::<LruCacheHandle, _, _>(h, |c| c.ttl_ms).unwrap();
        assert_eq!(ttl, None, "falsy ttl is dropped, not rejected");
        perry_ffi::drop_handle(h);
    }
}

// ── GC rooting ───────────────────────────────────────────────────────
//
// The cached-value-survives-a-copying-minor test lives in its own test
// binary (`tests/gc_survival.rs`). It needs a pristine heap to assert
// that the collector actually *relocated* the value, and it cannot get
// one here: any earlier test's stack leftovers conservatively pin the
// string (minor then reports `copied_objects=0`), and driving
// `gc_collect_minor()` in a binary that has also allocated JS objects
// SIGSEGVs the copying collector. See that file's module docs.

// ── heap-typed non-object options ────────────────────────────────────

extern "C" {
    fn js_array_alloc(capacity: u32) -> *mut std::ffi::c_void;
    fn js_array_set_f64_extend(arr: *mut std::ffi::c_void, index: u32, value: f64);
    fn js_nanbox_pointer(ptr: i64) -> f64;
}

/// A heap value that is *not* a plain object must read as all-undefined
/// fields, never as a dereference of a forged object pointer.
///
/// npm reaches its options by destructuring, so a string, an array, a
/// function, a `Map` — anything without a `max` own property — lands on
/// the same "At least one of max, maxSize, or ttl is required" TypeError.
/// Measured against `lru-cache@11.5.2` under Node 26.5.1 for strings,
/// arrays, `Map`, `Set`, functions, `Date`, `RegExp` and boxed numbers;
/// `Object.assign([], { max: 3 })` constructs fine there, because the own
/// property is what matters, not the exotic-ness of the receiver.
///
/// The point of the test is the *receiver classification*, not the error:
/// `options` arrives as an untrusted NaN-boxed value, and the wrapper must
/// not unbox it to a `*const ObjectHeader` on faith. Arrays and handle-band
/// ids are the two shapes that pass a naive `is_pointer()` check.
#[test]
fn heap_typed_non_object_options_read_as_undefined_fields() {
    const MSG: &str = "At least one of max, maxSize, or ttl is required";

    assert_throws(string_value("longer-than-inline-string"), "TypeError", MSG);

    let empty = unsafe { js_array_alloc(4) };
    assert_throws(unsafe { js_nanbox_pointer(empty as i64) }, "TypeError", MSG);

    // A populated array has real element bytes behind the cast, so a
    // forged-object read would find *something* rather than a zeroed slot.
    let populated = unsafe { js_array_alloc(8) };
    for i in 0..8u32 {
        unsafe { js_array_set_f64_extend(populated, i, 42.0 + f64::from(i)) };
    }
    assert_throws(
        unsafe { js_nanbox_pointer(populated as i64) },
        "TypeError",
        MSG,
    );

    // Native handle ids: pointer-tagged, above the old hand-rolled 0x1000
    // floor, but small integers rather than heap addresses.
    for raw in [0x1001_i64, 0x2000, 0x8000, 0xF_FFFF] {
        assert_throws(unsafe { js_nanbox_pointer(raw) }, "TypeError", MSG);
    }
}
