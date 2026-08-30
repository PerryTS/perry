//! `error.rs` to-string / throw-bridge regressions.
//!
//! Split out of `error.rs` to keep that file under the 2,000-line CI
//! cap (`scripts/check_file_size.sh`). Included from there with
//! `#[cfg(test)] #[path = "error_tostring_tests.rs"] mod tostring_tests;`,
//! so `use super::*` still resolves against `error.rs`.

use super::*;

#[test]
fn not_a_function_throw_bridge_is_unwind_capable() {
    let _: extern "C-unwind" fn(*const u8, usize, *const u8, usize) -> ! =
        js_throw_type_error_not_a_function;
}

#[test]
fn unresolved_global_name_survives_collection_during_global_this_init() {
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();

    // Mirror a generated module string slot: the collector rewrites this
    // registered root, but it cannot rewrite the by-value f64 copied into
    // `js_global_get_optional`. That callee must establish its own handle
    // before lazy global initialization reaches a collection point.
    let key_ptr = s(b"navigator");
    let key_before = key_ptr as usize;
    let mut key_value =
        f64::from_bits(crate::value::STRING_TAG | (key_before as u64 & crate::value::POINTER_MASK));
    crate::gc::js_gc_register_global_root((&mut key_value as *mut f64) as i64);
    crate::object::collect_before_global_this_alloc_for_test();

    let navigator = js_global_get_optional(key_value);
    let key_after = (key_value.to_bits() & crate::value::POINTER_MASK) as usize;
    assert_ne!(
        key_after, key_before,
        "the forced collection must relocate the caller's rooted key"
    );
    assert!(
        crate::value::JSValue::from_bits(navigator.to_bits()).is_pointer(),
        "the refreshed key must still resolve globalThis.navigator"
    );
}

fn s(bytes: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

#[test]
fn error_to_string_name_and_message() {
    let e = js_error_new_with_message(s(b"boom"));
    let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
    assert_eq!(out, "Error: boom");
}

#[test]
fn error_to_string_no_message_is_just_name() {
    let e = js_error_new_with_message(s(b""));
    let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
    assert_eq!(out, "Error");
}

#[test]
fn typed_error_to_string_uses_subclass_name() {
    let e = js_error_new_with_name_message(b"TypeError", s(b"bad"));
    let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
    assert_eq!(out, "TypeError: bad");
}

#[test]
fn get_errors_on_regular_object_reads_real_property_not_fixed_slot() {
    // Codegen lowers EVERY `obj.errors` read to `js_error_get_errors` and
    // then OR-s POINTER_TAG onto the result. For a *regular* object (not a
    // native error), the `ErrorHeader.errors` byte offset (+48) is an
    // unrelated slot — historically this returned NaN-boxed garbage that
    // the caller's re-tag turned into a handle-band id (e.g.
    // `0x7FFD_0000_0000_0001`), crashing `for…of`. The fix resolves the
    // `errors` property generically for non-errors.
    let arr = crate::array::js_array_alloc(2);
    crate::array::js_array_push_f64(arr, 11.0);
    crate::array::js_array_push_f64(arr, 22.0);
    let arr_boxed = crate::value::js_nanbox_pointer(arr as i64);

    // Plain object with an own `errors` property pointing at `arr`.
    let obj = crate::object::js_object_alloc(0, 2);
    let key = s(b"errors");
    crate::object::js_object_set_field_by_name(obj, key, arr_boxed);

    // The accessor receives the *cleaned* (untagged) pointer, as codegen
    // strips the tag before the call.
    let got = js_error_get_errors(obj as *mut ErrorHeader);
    assert_eq!(
        got as usize, arr as usize,
        "regular object's .errors must resolve to its real array property, \
         not the +48 ErrorHeader slot"
    );

    // An object with no `errors` property yields null (→ caller re-tag is a
    // null receiver that `for…of` rejects as not iterable, matching the
    // generic property read).
    let empty = crate::object::js_object_alloc(0, 1);
    assert!(js_error_get_errors(empty as *mut ErrorHeader).is_null());

    // A small-handle-band "pointer" must never be dereferenced.
    assert!(js_error_get_errors(1usize as *mut ErrorHeader).is_null());
}

#[test]
fn get_errors_on_native_aggregate_error_uses_fixed_slot() {
    let arr = crate::array::js_array_alloc(1);
    crate::array::js_array_push_f64(arr, 7.0);
    let agg = js_aggregateerror_new(arr, s(b"agg"));
    let got = js_error_get_errors(agg);
    assert_eq!(
        got as usize, arr as usize,
        "native AggregateError must read its fixed errors slot"
    );
}

#[test]
fn eval_and_uri_errors_have_distinct_kinds_and_names() {
    let eval = js_evalerror_new(s(b"eval"));
    assert_eq!(js_error_get_kind(eval), ERROR_KIND_EVAL_ERROR);
    assert_eq!(
        unsafe { read_string_header_owned(js_error_get_name(eval)) },
        "EvalError"
    );

    let uri = js_urierror_new(s(b"uri"));
    assert_eq!(js_error_get_kind(uri), ERROR_KIND_URI_ERROR);
    assert_eq!(
        unsafe { read_string_header_owned(js_error_get_name(uri)) },
        "URIError"
    );
}
