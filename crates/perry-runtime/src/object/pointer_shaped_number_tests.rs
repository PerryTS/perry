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

/// Truthiness of a Number never reads a string: the Number whose bits are the
/// empty string's address is a nonzero Number (truthy), not `""` (falsy).
#[test]
fn truthiness_of_the_number_reads_no_string() {
    let empty = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    let forged = f64::from_bits(empty as u64);
    assert_eq!(forged.to_bits() >> 48, 0, "an address is a denormal");
    assert_eq!(crate::value::js_is_truthy(forged), 1);
    assert_eq!(crate::value::js_is_truthy(f64::from_bits(0x1000_0000)), 1);
    assert_eq!(
        crate::value::js_is_truthy(crate::value::js_nanbox_string(empty as i64)),
        0
    );
    assert_eq!(crate::value::js_is_truthy(0.0), 0);
}

#[test]
fn a_named_read_answers_as_a_number() {
    let (obj, forged) = object_and_forgery();
    let read =
        super::field_get_set::js_object_get_field_value(forged.to_bits() as i64, key("x"), 0);
    assert_eq!(read.to_bits(), TAG_UNDEFINED, "(number).x is undefined");
    assert_eq!(own(obj, "x"), 42.0f64.to_bits());
}

/// The reads runtime code performs on a receiver VALUE (a method call's
/// [[Get]], `value.length`): the forged Number's `x` is not the object's.
#[test]
fn a_runtime_read_of_the_value_answers_as_a_number() {
    let (obj, forged) = object_and_forgery();
    let read = unsafe { super::method_call::spec_get_value(forged, b"x") };
    assert_eq!(read.to_bits(), TAG_UNDEFINED, "the [[Get]] read the object");
    let read = super::field_get_set::get_field_by_name_of_value(forged, key("x"));
    assert_eq!(read.to_bits(), TAG_UNDEFINED, "(number).x is undefined");
    let tagged = crate::value::js_nanbox_pointer(obj as i64);
    let read = super::field_get_set::get_field_by_name_of_value(tagged, key("x"));
    assert_eq!(
        read.to_bits(),
        42.0f64.to_bits(),
        "the object itself reads x"
    );
}

/// The Set / Map brand checks decode the receiver by its tag: the Number whose
/// bits are a live Set's / Map's address is no Set / Map receiver.
#[test]
fn collection_brand_checks_reject_the_number() {
    let set = crate::set::js_set_alloc(4);
    let map = crate::map::js_map_alloc(4);
    assert!(crate::set::set_ptr_from_receiver_bits(set as u64).is_none());
    assert!(crate::map::map_ptr_from_receiver_bits(map as u64).is_none());
    let set_value = crate::value::js_nanbox_pointer(set as i64).to_bits();
    let map_value = crate::value::js_nanbox_pointer(map as i64).to_bits();
    assert_eq!(crate::set::set_ptr_from_receiver_bits(set_value), Some(set));
    assert_eq!(crate::map::map_ptr_from_receiver_bits(map_value), Some(map));
}

#[test]
fn a_named_store_does_not_write_the_object() {
    let (obj, forged) = object_and_forgery();
    super::property_ops::put_value::js_put_value_set(forged, key_value("x"), 7.0, forged, 0);
    super::property_ops::put_value::js_put_value_set(forged, key_value("y"), 7.0, forged, 0);
    assert_eq!(own(obj, "x"), 42.0f64.to_bits(), "x was overwritten");
    assert_eq!(own(obj, "y"), TAG_UNDEFINED, "y was created");
}

/// A string parameter that is a Number is that Number's string form: the
/// Number whose bits are a live string's address is not that string.
#[test]
fn a_string_argument_that_is_the_number_is_its_string_form() {
    let s = key("perry");
    let forged = f64::from_bits(s as u64);
    assert_eq!(forged.to_bits() >> 48, 0, "an address is a denormal");
    let got = crate::value::js_get_string_pointer_unified(forged);
    assert_ne!(got, s as i64);
    let expected = crate::string::js_number_to_string(forged);
    assert_eq!(
        crate::string::js_string_equals(got as *const _, expected as *const _),
        1
    );
    assert_eq!(
        crate::value::js_get_string_pointer_unified(crate::value::js_nanbox_string(s as i64)),
        s as i64
    );
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

/// The shared value decoder (`js_nanbox_get_pointer`) classifies by the tag
/// alone: the forged Number, the smallest denormal and a word in the handle
/// band name no address; every tagged payload still decodes.
#[test]
fn the_shared_value_decoder_names_no_address() {
    use crate::value::js_nanbox_get_pointer as decode;
    let (obj, forged) = object_and_forgery();
    assert_eq!(decode(forged), 0, "a Number decoded as the object");
    assert_eq!(decode(f64::from_bits(1)), 0, "Number.MIN_VALUE");
    assert_eq!(decode(f64::from_bits(0x1234)), 0, "a handle-band word");
    assert_eq!(decode(f64::from_bits(0x000F_FFFF_FFFF_FFF8)), 0);
    assert_eq!(decode(0.0), 0);
    assert_eq!(decode(1.5), 0);
    assert_eq!(decode(f64::from_bits(TAG_UNDEFINED)), 0);

    let tagged = crate::value::js_nanbox_pointer(obj as i64);
    assert_eq!(decode(tagged), obj as i64, "the object itself decodes");
    let name = key("x");
    assert_eq!(
        decode(crate::value::js_nanbox_string(name as i64)),
        name as i64
    );
    assert_eq!(decode(crate::value::js_nanbox_handle(7)), 7);
    let big = crate::value::BIGINT_TAG | 0x1000;
    assert_eq!(decode(f64::from_bits(big)), 0x1000);
}

/// Entries that decode their operand through the shared decoder: the forged
/// Number is not the array (no length, no element), and is no iterable.
#[test]
fn entries_behind_the_shared_decoder_answer_as_a_number() {
    let arr = crate::array::js_array_alloc(4);
    let arr = crate::array::js_array_push_f64(arr, 9.0);
    let forged = f64::from_bits(arr as u64);
    let tagged = crate::value::js_nanbox_pointer(arr as i64);
    assert_eq!(crate::value::js_dynamic_array_length(tagged), 1);
    assert_eq!(
        crate::value::js_dynamic_array_length(forged),
        0,
        "the Number has the array's length"
    );
    assert_eq!(
        crate::value::js_dynamic_array_get(forged, 0).to_bits(),
        TAG_UNDEFINED,
        "the Number has the array's element"
    );
    let copy = crate::array::js_array_from_value(forged);
    assert_eq!(
        crate::array::js_array_length(copy),
        0,
        "Array.from(number) copied the array"
    );
}
