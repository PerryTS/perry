//! `ArraySpeciesCreate` helpers: the allocation-free default test, the
//! default arm of the full resolution, and the define / `length` steps a
//! species constructor's result is filled with.

use super::species::{
    array_species_create, default_species_is_certain, species_result_define,
    species_result_set_length,
};
use super::*;
use crate::value::{JSValue, TAG_HOLE, TAG_UNDEFINED};

fn boxed(arr: *const ArrayHeader) -> f64 {
    f64::from_bits(JSValue::pointer(arr as *const u8).bits())
}

fn pushed(values: &[f64]) -> *mut ArrayHeader {
    let mut arr = js_array_alloc(values.len() as u32);
    for &v in values {
        arr = js_array_push_f64(arr, v);
    }
    arr
}

unsafe fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

#[test]
fn a_plain_array_has_the_default_species_without_a_lookup() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0]);
        assert!(default_species_is_certain(boxed(arr)));
        // Not an Array at all: the caller asks the full resolution.
        assert!(!default_species_is_certain(42.0));
        assert!(!default_species_is_certain(f64::from_bits(TAG_UNDEFINED)));

        let (result, is_default) = array_species_create(boxed(arr), 2);
        assert!(is_default);
        let out = crate::value::js_nanbox_get_pointer(result) as *const ArrayHeader;
        assert_ne!(out, arr as *const ArrayHeader);
        assert_eq!(js_array_length(out), 2);
        assert_eq!(js_array_get_f64(out, 0).to_bits(), TAG_HOLE);
    }
}

#[test]
fn an_own_constructor_takes_the_full_resolution() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = pushed(&[1.0, 2.0, 3.0, 4.0]);
        // `a.constructor = undefined`: an own `constructor`, so no shortcut;
        // the resolution reads it and falls back to `%Array%`.
        let arr = super::named_props::array_named_property_set(
            arr,
            key("constructor"),
            f64::from_bits(TAG_UNDEFINED),
        );
        assert!(!default_species_is_certain(boxed(arr)));
        let (_, is_default) = array_species_create(boxed(arr), 0);
        assert!(is_default);

        // `slice` through the rooted path: a plain copy of [1, 3).
        let copy = js_array_slice(arr, 1, 3);
        assert_ne!(copy as *const ArrayHeader, arr as *const ArrayHeader);
        assert_eq!(js_array_length(copy), 2);
        assert_eq!(js_array_get_f64(copy, 0), 2.0);
        assert_eq!(js_array_get_f64(copy, 1), 3.0);

        // `concat` through the rooted path: receiver, a spread array, a value.
        let more = pushed(&[8.0, 9.0]);
        let args = [boxed(more), 7.0];
        let joined = js_array_concat_variadic(arr, args.as_ptr(), 2);
        assert_eq!(js_array_length(joined), 7);
        assert_eq!(js_array_get_f64(joined, 3), 4.0);
        assert_eq!(js_array_get_f64(joined, 4), 8.0);
        assert_eq!(js_array_get_f64(joined, 6), 7.0);
        assert_eq!(js_array_length(arr), 4);
    }
}

#[test]
fn a_species_result_is_defined_into_and_gets_its_length() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        // What a species constructor may return: an Array shorter than the
        // count. Defining past its end grows it; no slot is written blind.
        let short = js_array_alloc(0);
        let scope = crate::gc::RuntimeHandleScope::new();
        let short_h = scope.root_nanbox_f64(boxed(short));
        species_result_define(short_h.get_nanbox_f64(), 0, 10.0);
        species_result_define(short_h.get_nanbox_f64(), 2, 30.0);
        let live =
            || crate::value::js_nanbox_get_pointer(short_h.get_nanbox_f64()) as *const ArrayHeader;
        assert_eq!(js_array_length(live()), 3);
        assert_eq!(js_array_get_f64(live(), 0), 10.0);
        assert_eq!(js_array_get_f64(live(), 1).to_bits(), TAG_HOLE);
        assert_eq!(js_array_get_f64(live(), 2), 30.0);

        // ... or longer: Set(A, "length", n, true) cuts it.
        let long = pushed(&[7.0, 7.0, 7.0, 7.0, 7.0]);
        let long_h = scope.root_nanbox_f64(boxed(long));
        species_result_define(long_h.get_nanbox_f64(), 1, 5.0);
        species_result_set_length(long_h.get_nanbox_f64(), 2);
        let live =
            crate::value::js_nanbox_get_pointer(long_h.get_nanbox_f64()) as *const ArrayHeader;
        assert_eq!(js_array_length(live), 2);
        assert_eq!(js_array_get_f64(live, 0), 7.0);
        assert_eq!(js_array_get_f64(live, 1), 5.0);

        // ... or an ordinary object: own index properties and a `length`.
        let obj = crate::object::js_object_alloc(0, 0);
        let obj_h =
            scope.root_nanbox_f64(f64::from_bits(JSValue::pointer(obj as *const u8).bits()));
        species_result_define(obj_h.get_nanbox_f64(), 0, 4.0);
        species_result_define(obj_h.get_nanbox_f64(), 1, 5.0);
        species_result_set_length(obj_h.get_nanbox_f64(), 2);
        let read = |name: &str| {
            let k = key(name);
            crate::object::js_object_get_property_key(
                obj_h.get_nanbox_f64(),
                f64::from_bits(JSValue::string_ptr(k as *mut crate::StringHeader).bits()),
            )
        };
        assert_eq!(read("0"), 4.0);
        assert_eq!(read("1"), 5.0);
        assert_eq!(crate::builtins::js_number_coerce(read("length")), 2.0);
    }
}
