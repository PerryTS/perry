//! #6084: Map pointer-key index (`MAP_PTR_INDEX`) behaviour under the moving
//! GC, split out of `copying.rs` to keep it under the 2 000-line file gate
//! (`scripts/check_file_size.sh`).

use super::*;

// #6084: an object key lives in MAP_PTR_INDEX keyed by its NaN-box bits, so
// a copying minor that evacuates the key (and the MapHeader itself) leaves
// every stored key bit stale. GcRewriteHookKind::MapIndex must rebuild the
// index from the rewritten entries buffer, exactly as SetIndex does — the
// Map analog of test_copying_minor_relocates_managed_set below. Without the
// hook, the post-GC lookup silently misses (the ptr index treats a miss as
// definitive, so it never falls back to a linear scan).
#[test]
fn test_copying_minor_rebuilds_map_pointer_key_index() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let (child_obj, _child_fields) = unsafe { alloc_nursery_test_object(0) };
    let child = child_obj as usize;
    let child_bits = ptr_bits(child);
    let map = crate::map::js_map_alloc(16);
    for i in 0..9 {
        crate::map::js_map_set(map, i as f64, (i * 10) as f64);
    }
    crate::map::js_map_set(map, f64::from_bits(child_bits), 777.0);
    assert!(crate::map::test_map_ptr_index_contains(
        map,
        f64::from_bits(child_bits)
    ));
    assert_eq!(
        crate::map::js_map_get(map, f64::from_bits(child_bits)),
        777.0
    );

    js_shadow_slot_set(0, ptr_bits(map as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let map_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::map::MapHeader;
    let rewritten_key = crate::map::js_map_entry_key_at(map_after, 9);
    let rewritten = (rewritten_key.to_bits() & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(map_after as usize, map as usize, "MapHeader must move");
    assert_ne!(rewritten, child, "object key must be evacuated");
    assert!(crate::arena::pointer_in_nursery(rewritten));

    // The rebuilt index must find the key at its NEW address...
    assert!(crate::map::test_map_ptr_index_contains(
        map_after,
        rewritten_key
    ));
    assert_eq!(crate::map::js_map_get(map_after, rewritten_key), 777.0);
    // ...and the numeric keys must survive the rebuild untouched.
    assert_eq!(crate::map::js_map_get(map_after, 8.0), 80.0);
    // The stale pre-GC bits are a different identity and must NOT hit.
    // Compare BITS: undefined is a NaN payload, and NaN != NaN as an f64.
    assert_eq!(
        crate::map::js_map_get(map_after, f64::from_bits(child_bits)).to_bits(),
        crate::value::JSValue::undefined().bits(),
        "the stale pre-evacuation key bits must not resolve"
    );
}

// #6084: bigint Map keys compare by mathematical value (SameValueZero), not
// by allocation identity — two distinct `1n` allocations are the SAME key.
// Pre-fix they were bits-indexed, so this lookup missed entirely.
#[test]
fn test_map_bigint_keys_match_by_content_not_identity() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let map = crate::map::js_map_alloc(8);
    let boxed =
        |v: i64| crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_i64(v) as i64);
    let a = boxed(1234);
    let b = boxed(1234);
    let other = boxed(9999);
    assert_ne!(
        a.to_bits(),
        b.to_bits(),
        "two 1234n allocations must be distinct pointers"
    );
    // undefined is a NaN payload, so compare BITS — NaN != NaN as an f64.
    let undefined_bits = crate::value::JSValue::undefined().bits();

    crate::map::js_map_set(map, a, 42.0);
    assert_eq!(
        crate::map::js_map_get(map, b),
        42.0,
        "content-equal key hits"
    );
    assert_eq!(crate::map::js_map_has(map, b), 1);
    assert_eq!(
        crate::map::js_map_get(map, other).to_bits(),
        undefined_bits,
        "a different bigint value must not hit"
    );
    // Re-setting via the content-equal allocation overwrites, not appends.
    crate::map::js_map_set(map, b, 43.0);
    assert_eq!(
        unsafe { (*map).size },
        1,
        "content-equal key must not append"
    );
    assert_eq!(crate::map::js_map_get(map, a), 43.0);
}
