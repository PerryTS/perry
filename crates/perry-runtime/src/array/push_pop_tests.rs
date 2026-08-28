//! `pop` / `push` unit tests — split from `array/tests.rs` for the
//! 2000-line file-size gate (extract a cohesive group into a sibling file,
//! wire it with an explicit `mod`). No logic change.

use super::*;

/// `pop()` on an empty plain array is answered from the header fast path:
/// `undefined`, length untouched — the drained pool's `pool.pop() ?? []`.
#[test]
fn pop_on_an_empty_plain_array_is_undefined_from_the_fast_path() {
    let arr = js_array_alloc(4);
    assert_eq!(
        js_array_pop_f64(arr).to_bits(),
        crate::value::TAG_UNDEFINED,
        "fresh empty array"
    );
    assert_eq!(js_array_length(arr), 0);

    let arr = js_array_push_f64(arr, 1.0);
    assert_eq!(js_array_pop_f64(arr), 1.0);
    assert_eq!(
        js_array_pop_f64(arr).to_bits(),
        crate::value::TAG_UNDEFINED,
        "emptied by a pop"
    );
    assert_eq!(js_array_length(arr), 0);
    // The slot the pop retired reads as a hole for a later length extension,
    // exactly as before: nothing on the empty arm touches the payload.
    js_array_set_length(arr, 1.0);
    assert_eq!(
        array_spec_get(arr, 0).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

/// `length = 0` through both entries takes the header-only lane on a plain
/// dense array — holes over the retired prefix, length published, the array
/// still usable — and declines it for a frozen array (the strict entry must
/// still throw) and for one carrying a named property.
#[test]
fn length_zero_takes_the_header_lane_on_a_plain_array_and_declines_otherwise() {
    let mut arr = js_array_alloc(4);
    for v in 1..=3 {
        arr = js_array_push_f64(arr, v as f64);
    }
    js_array_set_length(arr, 0.0);
    assert_eq!(js_array_length(arr), 0);
    js_array_set_length(arr, 2.0);
    for index in 0..2 {
        assert_eq!(
            array_spec_get(arr, index).to_bits(),
            crate::value::TAG_UNDEFINED
        );
    }
    js_array_set_length(arr, 0.0);
    let arr = js_array_push_f64(arr, 9.0);
    assert_eq!(js_array_get_f64(arr, 0), 9.0);
    assert_eq!(js_array_length(arr), 1);
    js_array_set_length_strict(arr, 0.0);
    assert_eq!(js_array_length(arr), 0);
    // Already empty: a no-op through both entries.
    js_array_set_length(arr, 0.0);
    js_array_set_length_strict(arr, 0.0);
    assert_eq!(js_array_length(arr), 0);

    // A named property keeps the full entry (which clears both representations).
    let mut named = js_array_alloc(4);
    named = js_array_push_f64(named, 1.0);
    let key = crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
    unsafe { array_named_property_set(named, key, 99.0) };
    js_array_set_length(named, 0.0);
    assert_eq!(js_array_length(named), 0);
    js_array_set_length(named, 1.0);
    assert_eq!(
        array_spec_get(named, 0).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

#[test]
fn test_array_pop_and_push() {
    let arr = js_array_alloc(4);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);

    let popped = js_array_pop_f64(arr);
    assert_eq!(popped, 3.0);
    assert_eq!(js_array_length(arr), 2);

    let arr = js_array_push_f64(arr, 4.0);
    assert_eq!(js_array_length(arr), 3);
    assert_eq!(js_array_get_f64(arr, 2), 4.0);
}
