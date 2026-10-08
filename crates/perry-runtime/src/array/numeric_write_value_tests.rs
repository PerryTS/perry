//! `js_array_note_numeric_write_value`: the whole F64-kind cold arm of a
//! generated element store (perry-codegen `index_set_guarded.rs`) in one call.

use super::*;

/// The dense kind bit itself (`js_array_is_numeric_f64_layout` would re-verify
/// the slots and set it again); every fixture here is dense.
fn raw_f64_kind(arr: *mut ArrayHeader) -> i32 {
    i32::from(unsafe { super::header::array_has_raw_f64_layout_flag(arr) })
}

fn numeric_array() -> *mut ArrayHeader {
    let arr = js_array_alloc(4);
    for value in [1.5, 2.5, 3.5] {
        assert_eq!(js_array_push_f64(arr, value), arr);
    }
    assert_eq!(
        js_array_mark_numeric_f64_layout(arr),
        1,
        "fixture is raw-f64"
    );
    arr
}

/// A Number box keeps the raw-f64 kind and is answered as its double.
#[test]
fn a_number_box_keeps_the_kind_and_is_written_as_its_double() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = numeric_array();
    let int32_seven = f64::from_bits(0x7FFE_0000_0000_0007);
    assert_eq!(js_array_note_numeric_write_value(arr, int32_seven), 7.0);
    assert_eq!(raw_f64_kind(arr), 1, "a Number keeps the kind");
}

/// Anything else clears the kind BEFORE it is answered, and is answered
/// unchanged: its bits are what the store writes into a now-Any array.
#[test]
fn a_non_number_clears_the_kind_and_is_written_unchanged() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    for bits in [
        crate::value::TAG_TRUE,
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_NULL,
    ] {
        let arr = numeric_array();
        let value = f64::from_bits(bits);
        assert_eq!(
            js_array_note_numeric_write_value(arr, value).to_bits(),
            bits
        );
        assert_eq!(
            raw_f64_kind(arr),
            0,
            "{bits:#x} must clear the raw-f64 kind"
        );
    }
}

/// An Any array is answered the value unchanged and stays Any.
#[test]
fn an_any_array_answers_the_value_unchanged() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = js_array_alloc(2);
    js_array_clear_numeric_layout(arr);
    let int32_seven = f64::from_bits(0x7FFE_0000_0000_0007);
    assert_eq!(
        js_array_note_numeric_write_value(arr, int32_seven).to_bits(),
        int32_seven.to_bits()
    );
    assert_eq!(raw_f64_kind(arr), 0);
}
