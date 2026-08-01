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

/// Serializes the tests that touch global GC state.
static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

/// NaN-boxed `f64` for a freshly allocated JS string with `text`.
fn string_value(text: &str) -> f64 {
    let s = alloc_string(text);
    assert!(!s.is_null(), "alloc_string returned null");
    f64::from_bits(nanbox_string_bits(s.as_raw()))
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
    assert_eq!(cache_key(f64::from_bits(JsValue::TRUE.bits())), CacheKey::Bool(true));
    assert_eq!(cache_key(f64::from_bits(JsValue::NULL.bits())), CacheKey::Null);
    assert_eq!(
        cache_key(f64::from_bits(JsValue::UNDEFINED.bits())),
        CacheKey::Undefined
    );
}

// ── numeric-key behaviour (parity with the old surface) ──────────────

#[test]
fn basic_set_get_round_trip() {
    let h = js_lru_cache_new(f64::from_bits(TAG_UNDEFINED));
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
    let h = js_lru_cache_new(f64::from_bits(TAG_UNDEFINED));
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
    let h = js_lru_cache_new(f64::from_bits(TAG_UNDEFINED));
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
    let h = js_lru_cache_new(f64::from_bits(TAG_UNDEFINED));
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
    let h = js_lru_cache_new(f64::from_bits(TAG_UNDEFINED));
    js_lru_cache_set(h, string_value("payload"), string_value("hello-world-value"));
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

#[test]
fn update_age_on_get_refreshes_ttl() {
    let h = perry_ffi::register_handle(LruCacheHandle::new(10, Some(80.0), true));
    js_lru_cache_set(h, 1.0, 111.0);
    // Halfway through the TTL, a get refreshes the clock.
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(js_lru_cache_get(h, 1.0), 111.0);
    // Another half-TTL later the entry is still live *because* it was
    // refreshed (without updateAgeOnGet it would have expired at ~80ms).
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(js_lru_cache_get(h, 1.0), 111.0, "get refreshed the TTL");
    perry_ffi::drop_handle(h);
}

#[test]
fn no_update_age_on_get_lets_ttl_expire() {
    let h = perry_ffi::register_handle(LruCacheHandle::new(10, Some(80.0), false));
    js_lru_cache_set(h, 1.0, 111.0);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(js_lru_cache_get(h, 1.0), 111.0); // still live, no refresh
    std::thread::sleep(std::time::Duration::from_millis(50));
    // ~100ms since set, TTL was 80ms and never refreshed → expired.
    assert_eq!(js_lru_cache_get(h, 1.0).to_bits(), TAG_UNDEFINED);
    perry_ffi::drop_handle(h);
}

// ── options-object parsing ───────────────────────────────────────────

#[test]
fn new_without_options_defaults_to_100() {
    let h = js_lru_cache_new(f64::from_bits(TAG_UNDEFINED));
    // TTL/updateAgeOnGet default off…
    let opts = with_handle_mut::<LruCacheHandle, _, _>(h, |c| (c.ttl_ms, c.update_age_on_get))
        .unwrap();
    assert_eq!(opts, (None, false));
    // …and the capacity defaults to 100 (the 101st insert evicts).
    for i in 0..101 {
        js_lru_cache_set(h, i as f64, i as f64);
    }
    assert_eq!(js_lru_cache_size(h), 100.0);
    assert!(!is_true(js_lru_cache_has(h, 0.0)), "oldest entry evicted at cap 100");
    perry_ffi::drop_handle(h);
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
    ensure_gc_scanner();

    let h = js_lru_cache_new(f64::from_bits(TAG_UNDEFINED));
    // A >5-byte string forces the heap `StringHeader` repr (not inline SSO),
    // so it is a real collectable allocation. Its ONLY root is the cache.
    js_lru_cache_set(h, 1.0, string_value("value-object-1234567890"));

    // Reclaims unrooted nursery allocations and evacuates rooted survivors.
    let _ = perry_runtime::gc::gc_collect_minor();

    let got = js_lru_cache_get(h, 1.0);
    assert_ne!(
        got.to_bits(),
        TAG_UNDEFINED,
        "cached value was collected — root scanner did not keep it alive"
    );
    assert_eq!(
        read_string_value(got).as_deref(),
        Some("value-object-1234567890"),
        "cached value corrupted across GC — slot was not rewritten to the forwarded address"
    );

    perry_ffi::drop_handle(h);
    perry_runtime::gc::js_shadow_frame_pop(frame);
    perry_runtime::gc::js_gc_write_barriers_emitted(0);
}
