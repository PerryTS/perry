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
