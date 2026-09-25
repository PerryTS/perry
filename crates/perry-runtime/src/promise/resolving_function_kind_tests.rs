//! #10521 — the promise resolving functions' spec-visible facts (`length` 1,
//! `name` "", no `[[Construct]]`) come from their function KIND, not from
//! per-closure side-table entries. These pin both halves: reflection still
//! answers correctly, and creating a pair writes nothing per closure and does
//! not bump the global property-plan epoch.

use super::combinators::make_resolving_functions;
use super::js_promise_new;
use crate::closure::ClosureHeader;
use crate::value::{js_nanbox_pointer, JSValue};

fn string_content(value: f64) -> Option<String> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let hdr = crate::builtins::js_string_coerce(value);
    // SAFETY: `js_string_coerce` returns a live string header.
    unsafe { crate::object::has_own_helpers::str_from_string_header(hdr) }.map(str::to_owned)
}

/// The property-read entry point compiled `f.name` / `f.length` reaches.
fn read_property(closure: *mut ClosureHeader, key: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_get_field_by_name_f64(
        closure as *const crate::object::ObjectHeader,
        key,
    )
}

#[test]
fn resolving_functions_answer_reflection_from_their_kind() {
    let _lock = crate::gc::global_side_table_test_lock();
    let promise = js_promise_new();
    let epoch_before = crate::object::prop_plan::prop_plan_semantic_epoch();
    let (resolve, reject) = make_resolving_functions(promise);
    assert_eq!(
        crate::object::prop_plan::prop_plan_semantic_epoch(),
        epoch_before,
        "creating a resolving pair must not invalidate every cached property plan"
    );

    for f in [resolve, reject] {
        let addr = f as usize;
        // Nothing per closure.
        assert!(!crate::closure::closure_has_own_dynamic_prop(addr, "name"));
        assert!(crate::object::get_property_attrs(addr, "name").is_none());
        assert!(crate::object::builtin_closure_length(addr).is_none());

        // The facts, from the kind.
        assert_eq!(crate::closure::closure_length(f), Some(1));
        assert_eq!(
            JSValue::from_bits(read_property(f, "length").to_bits()).as_number(),
            1.0
        );
        assert_eq!(
            string_content(read_property(f, "name")).as_deref(),
            Some("")
        );
        assert!(crate::object::builtin_closure_is_non_constructable(addr));
        assert!(!crate::object::js_value_is_constructor(js_nanbox_pointer(
            f as i64
        )));
    }
}

extern "C" fn unrelated_native_body(_closure: *const ClosureHeader, value: f64) -> f64 {
    value
}

#[test]
fn non_constructor_kind_is_keyed_by_body_not_blanket() {
    let _lock = crate::gc::global_side_table_test_lock();
    // Register the kinds, then show a closure over a different body and a
    // plain object are still not answered as non-constructors by it.
    let _ = make_resolving_functions(js_promise_new());
    let other = crate::closure::js_closure_alloc(unrelated_native_body as *const u8, 0);
    assert!(!crate::object::builtin_closure_is_non_constructable(
        other as usize
    ));
    let obj = crate::object::js_object_alloc(0, 0);
    assert!(!crate::object::builtin_closure_is_non_constructable(
        obj as usize
    ));
    assert!(!crate::object::builtin_closure_is_non_constructable(0));
}
