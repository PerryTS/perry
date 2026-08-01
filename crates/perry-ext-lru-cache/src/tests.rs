//! Unit tests for the lru-cache wrapper.
//!
//! Every test links `perry-runtime` (the `runtime-link` dev-dep feature)
//! because the wrapper reaches the runtime for its clock
//! (`js_performance_now`) and string materialization
//! (`js_get_string_pointer_unified`). The GC-survival test additionally
//! drives `perry-runtime`'s collector directly.

use super::*;
use perry_ffi::{alloc_string, nanbox_string_bits, JsValue};
use std::sync::Mutex;

extern "C" {
    fn js_object_set_field_by_name(obj: *mut ObjectHeader, key: *const StringHeader, value: f64);
}

/// Serializes the tests that touch global GC state.
static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

/// NaN-boxed `f64` for a freshly allocated JS string with `text`.
fn string_value(text: &str) -> f64 {
    let s = alloc_string(text);
    assert!(!s.is_null(), "alloc_string returned null");
    f64::from_bits(nanbox_string_bits(s.as_raw()))
}

/// Build a real JS options object carrying `fields`, the way a compiled
/// `new LRUCache({ … })` object literal reaches the constructor.
fn options(fields: &[(&str, f64)]) -> f64 {
    let obj = perry_ffi::alloc_object();
    assert!(obj.is_pointer(), "alloc_object returned a non-object");
    let ptr = obj.as_pointer::<ObjectHeader>();
    for (name, value) in fields {
        let key = alloc_string(name);
        unsafe { js_object_set_field_by_name(ptr, key.as_raw(), *value) };
    }
    f64::from_bits(obj.bits())
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
            let err = JsValue::from_bits(bits.to_bits());
            let ptr = err.as_pointer::<ObjectHeader>();
            let field = |name: &str| {
                let key = alloc_string(name);
                let v = unsafe { js_object_get_field_by_name_f64(ptr, key.as_raw()) };
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

// ── GC rooting: a cached heap value survives a collection ────────────

#[test]
fn cached_value_survives_gc_cycle() {
    let _lock = GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Match the runtime's evacuation preconditions: write barriers active
    // and a live shadow frame (mirrors perry-ext-events' scanner test).
    perry_runtime::gc::js_gc_write_barriers_emitted(1);
    let frame = perry_runtime::gc::js_shadow_frame_push(0);
    // Both of those are process-global. An assertion below panicking must
    // not leave write barriers on and a frame pushed for every later test
    // in this binary, so tear down in `Drop` rather than on the happy path.
    struct GcStateGuard(u64);
    impl Drop for GcStateGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.0);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }
    let _gc_state = GcStateGuard(frame);
    ensure_gc_scanner();

    let h = new_cache(10.0);
    // A >5-byte string forces the heap `StringHeader` repr (not inline SSO),
    // so it is a real collectable allocation. Its ONLY root is the cache.
    let value = string_value("value-object-1234567890");
    let before = value.to_bits() & POINTER_MASK;
    js_lru_cache_set(h, 1.0, value);

    // Reclaims unrooted nursery allocations and evacuates rooted survivors.
    let _ = perry_runtime::gc::gc_collect_minor();

    let got = js_lru_cache_get(h, 1.0);
    assert_ne!(
        got.to_bits(),
        TAG_UNDEFINED,
        "cached value was collected — root scanner did not keep it alive"
    );
    // Subject-live gate (#6942/#6946). Marking alone would satisfy the
    // assertions above, and a non-moving collection satisfies them even if
    // the scanner never rewrites a slot — so this test would keep passing
    // while covering nothing. Require the evacuation to have actually
    // relocated the value: the address the cache hands back must differ
    // from the one it was given. If this fires, the minor did not copy
    // (`PERRY_GEN_GC=0` routes to non-moving mark-sweep, and barriers-off
    // falls back the same way) and the rewrite path is untested, not fixed.
    let after = got.to_bits() & POINTER_MASK;
    assert_ne!(
        before, after,
        "minor GC did not relocate the cached value (0x{before:x}), so this test \
         never exercised the scanner's forwarding-pointer rewrite"
    );
    assert_eq!(
        read_string_value(got).as_deref(),
        Some("value-object-1234567890"),
        "cached value corrupted across GC — slot was not rewritten to the forwarded address"
    );

    perry_ffi::drop_handle(h);
}
