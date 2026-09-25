//! #10509: reads against an elided Arguments object.
//!
//! Codegen keeps the caller's argument bundle in a function's `arguments` slot
//! when every use is a `.length` or `[k]` read. `js_arguments_bundle_index_get`
//! answers own elements from the bundle and `TAG_HOLE` for everything else;
//! `js_arguments_bundle_get_slow` answers those from the object
//! `js_arguments_object_alloc` would have built. The two together must agree
//! with a `[[Get]]` on that object for every key, which is what these pin.

use super::arguments::{thrower_closure_value, THROWER_FROZEN_FLAGS};
use super::*;

fn bundle(values: &[f64]) -> f64 {
    let mut arr = crate::array::js_array_alloc(values.len() as u32);
    for v in values {
        arr = crate::array::js_array_push_f64(arr, *v);
    }
    crate::array::js_array_mark_arguments_object(arr);
    crate::value::js_nanbox_pointer(arr as i64)
}

fn string(s: &str) -> f64 {
    crate::value::js_nanbox_string(
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64,
    )
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn is_hole(v: f64) -> bool {
    v.to_bits() == crate::value::TAG_HOLE
}

/// What the unelided program reads: a `[[Get]]` on the materialized object.
fn materialized_get(raw_args: f64, key: f64, restricted: i32) -> f64 {
    let obj = js_arguments_object_alloc(raw_args, undefined(), restricted);
    crate::value::js_dyn_index_get(crate::value::js_nanbox_pointer(obj as i64), key)
}

#[test]
fn own_elements_read_from_the_bundle() {
    let raw = bundle(&[10.0, 20.0, 30.0]);
    assert_eq!(js_arguments_bundle_index_get(raw, 0.0), 10.0);
    assert_eq!(js_arguments_bundle_index_get(raw, 2.0), 30.0);
    // ToPropertyKey(-0) is "0".
    assert_eq!(js_arguments_bundle_index_get(raw, -0.0), 10.0);
    let s = string("own");
    let raw = bundle(&[s]);
    assert_eq!(
        js_arguments_bundle_index_get(raw, 0.0).to_bits(),
        s.to_bits()
    );
}

#[test]
fn every_non_element_key_declines() {
    let raw = bundle(&[10.0, 20.0]);
    for key in [
        2.0,
        3.0,
        -1.0,
        1.5,
        f64::NAN,
        f64::INFINITY,
        u32::MAX as f64,
        4294967296.0,
        string("0"),
        string("length"),
        string("callee"),
        undefined(),
    ] {
        assert!(
            is_hole(js_arguments_bundle_index_get(raw, key)),
            "key bits {:#x} must take the materializing path",
            key.to_bits()
        );
    }
    // A receiver that is not an array at all declines too.
    assert!(is_hole(js_arguments_bundle_index_get(undefined(), 0.0)));
}

#[test]
fn declined_keys_answer_what_the_object_answers() {
    let raw = bundle(&[10.0, 20.0]);
    for key in [
        2.0,
        -1.0,
        1.5,
        string("0"),
        string("1"),
        string("01"),
        string("length"),
        string("map"),
    ] {
        let slow = js_arguments_bundle_get_slow(raw, key, undefined(), std::ptr::null(), 1);
        let expected = materialized_get(raw, key, 1);
        assert_eq!(
            slow.to_bits(),
            expected.to_bits(),
            "key bits {:#x}",
            key.to_bits()
        );
    }
    assert_eq!(
        js_arguments_bundle_get_slow(raw, string("length"), undefined(), std::ptr::null(), 1),
        2.0
    );
    assert_eq!(
        js_arguments_bundle_get_slow(raw, string("1"), undefined(), std::ptr::null(), 1),
        20.0
    );
    // Out-of-range stays undefined: the bundle is an Array, the object is not,
    // so `Array.prototype` never participates.
    assert_eq!(
        js_arguments_bundle_get_slow(raw, 5.0, undefined(), std::ptr::null(), 1).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

#[test]
fn a_sloppy_fallback_sees_the_callee_it_was_given() {
    let raw = bundle(&[1.0]);
    let callee = string("stand-in callee");
    let got = js_arguments_bundle_get_slow(raw, string("callee"), callee, std::ptr::null(), 0);
    assert_eq!(got.to_bits(), callee.to_bits());
}

#[test]
fn the_strict_thrower_is_configured_once_per_singleton() {
    let first = thrower_closure_value();
    let second = thrower_closure_value();
    assert_eq!(
        first.to_bits(),
        second.to_bits(),
        "one singleton per thread"
    );
    let closure = crate::value::js_nanbox_get_pointer(first) as usize;
    let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(closure) }
        .expect("the thrower is a tracked closure");
    assert_eq!(
        unsafe { (*header.as_ptr())._reserved } & THROWER_FROZEN_FLAGS,
        THROWER_FROZEN_FLAGS,
        "the configure-once marker must be set by the first call"
    );
    // The early return must hand back a fully configured accessor, not just
    // the same pointer: its own-property attributes are still frozen.
    for name in ["name", "length"] {
        let attrs = get_property_attrs(closure, name).expect("configured attribute");
        assert!(!attrs.writable() && !attrs.enumerable() && !attrs.configurable());
    }
}
