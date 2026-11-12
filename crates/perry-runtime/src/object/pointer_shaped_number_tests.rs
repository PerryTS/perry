//! A Number whose bits equal a live object's start address is a Number.
//!
//! In value context an object carries POINTER_TAG. A word whose top 16 bits
//! are clear is a denormal double (or +0); the legacy raw-pointer decodes read
//! it as an object address, so `f64::from_bits(object address)` was read AND
//! written as that object by the value entries. No allocator lookup can tell
//! the two apart (the address is an owned object start), so each value entry
//! must classify by the tag alone. Every test here forges exactly that Number
//! and expects Number semantics and an untouched object.

use crate::value::{TAG_FALSE, TAG_UNDEFINED};

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn key_value(name: &str) -> f64 {
    crate::value::js_nanbox_string(key(name) as i64)
}

/// An object `{ x: 42 }` and the Number with its address as bits.
fn object_and_forgery() -> (*mut crate::ObjectHeader, f64) {
    let obj = super::js_object_alloc(0, 0);
    super::js_object_set_field_by_name(obj, key("x"), 42.0);
    let forged = f64::from_bits(obj as u64);
    assert_eq!(forged.to_bits() >> 48, 0, "an address is a denormal");
    assert!(forged.is_finite() && forged > 0.0);
    (obj, forged)
}

fn own(obj: *mut crate::ObjectHeader, name: &str) -> u64 {
    super::js_object_get_field_by_name_f64(obj, key(name)).to_bits()
}

#[test]
fn a_named_read_answers_as_a_number() {
    let (obj, forged) = object_and_forgery();
    let read =
        super::field_get_set::js_object_get_field_value(forged.to_bits() as i64, key("x"), 0);
    assert_eq!(read.to_bits(), TAG_UNDEFINED, "(number).x is undefined");
    assert_eq!(own(obj, "x"), 42.0f64.to_bits());
}

#[test]
fn a_named_store_does_not_write_the_object() {
    let (obj, forged) = object_and_forgery();
    super::property_ops::put_value::js_put_value_set(forged, key_value("x"), 7.0, forged, 0);
    super::property_ops::put_value::js_put_value_set(forged, key_value("y"), 7.0, forged, 0);
    assert_eq!(own(obj, "x"), 42.0f64.to_bits(), "x was overwritten");
    assert_eq!(own(obj, "y"), TAG_UNDEFINED, "y was created");
}

#[test]
fn an_indexed_read_and_store_do_not_reach_the_array() {
    let arr = crate::array::js_array_alloc(4);
    let arr = crate::array::js_array_push_f64(arr, 9.0);
    let forged = f64::from_bits(arr as u64);
    let tagged = crate::value::js_nanbox_pointer(arr as i64);
    assert_eq!(
        crate::value::js_dyn_index_get(forged, 0.0).to_bits(),
        TAG_UNDEFINED,
        "(number)[0] is undefined"
    );
    crate::value::js_dyn_index_set(forged, 0.0, 5.0);
    assert_eq!(
        crate::value::js_dyn_index_get(tagged, 0.0).to_bits(),
        9.0f64.to_bits(),
        "the array element was overwritten"
    );
    assert_eq!(
        crate::array::js_array_is_array(forged).to_bits(),
        TAG_FALSE,
        "Array.isArray(number)"
    );
}

#[test]
fn a_map_and_a_set_key_it_as_a_number() {
    let (obj, forged) = object_and_forgery();
    let tagged = crate::value::js_nanbox_pointer(obj as i64);
    let map = crate::map::js_map_alloc(4);
    let map = crate::map::js_map_set(map, forged, 1.0);
    assert_eq!(crate::map::js_map_size(map), 1);
    assert_eq!(
        crate::map::js_map_get(map, forged).to_bits(),
        1.0f64.to_bits()
    );
    assert_eq!(
        crate::map::js_map_get(map, tagged).to_bits(),
        TAG_UNDEFINED,
        "the object found the Number's entry"
    );
    let set = crate::set::js_set_alloc(4);
    let set = crate::set::js_set_add(set, forged);
    assert_eq!(crate::set::js_set_has(set, forged), 1);
    assert_eq!(
        crate::set::js_set_has(set, tagged),
        0,
        "the object is the Number"
    );
}

#[test]
fn it_is_not_identical_to_the_object() {
    let (obj, forged) = object_and_forgery();
    let tagged = crate::value::js_nanbox_pointer(obj as i64);
    assert_eq!(crate::value::js_jsvalue_equals(forged, tagged), 0);
    assert_eq!(crate::value::js_jsvalue_equals(forged, forged), 1);
}

#[test]
fn the_object_decodes_name_no_address() {
    let (obj, forged) = object_and_forgery();
    assert_eq!(crate::value::addr_class::object_ref_addr(forged), 0);
    assert!(unsafe { super::object_ops::extract_obj_ptr(forged) }.is_null());
    assert_eq!(
        super::property_ops::set::extract_pointer(forged.to_bits()),
        0
    );
    // The tagged object still decodes.
    let tagged = crate::value::js_nanbox_pointer(obj as i64);
    assert_eq!(
        crate::value::addr_class::object_ref_addr(tagged),
        obj as usize
    );
    assert_eq!(unsafe { super::object_ops::extract_obj_ptr(tagged) }, obj);
}
