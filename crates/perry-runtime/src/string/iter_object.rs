//! Real String iterator objects.
//!
//! Node's `''[Symbol.iterator]()` returns a String Iterator OBJECT exposing a
//! `.next()` returning `{ value, done }`, iterable via `Symbol.iterator`, whose
//! `[[Prototype]]` is `%StringIteratorPrototype%` (chaining to the shared
//! `%IteratorPrototype%`). test262's built-ins/StringIteratorPrototype suite
//! checks this shape.
//!
//! Representation mirrors `array/iter_object.rs`: a regular `ObjectHeader` with
//! a dedicated class id. Iteration is over Unicode CODE POINTS (surrogate pairs
//! collapse to one element, per ECMA-262 §22.1.5), so we materialize the source
//! string into a codepoint array once (field 0, NaN-boxed pointer so the GC
//! scanner keeps it alive) and advance a cursor (field 1).
//!
//! Dispatch lives in `object/native_call_method.rs` via the class-id check next
//! to the array iterator one.

use crate::array::ArrayHeader;
use crate::object::{js_object_alloc, js_object_get_field, js_object_set_field, ObjectHeader};
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_UNDEFINED};
use crate::StringHeader;

/// Class id reserved for String iterators. Sits just past the Set iterator id
/// (0xFFFF0008) in the 0xFFFF prefix reserved for runtime-defined classes.
pub const STRING_ITERATOR_CLASS_ID: u32 = 0xFFFF_0009;

unsafe fn alloc_iterator(cp_array: *mut ArrayHeader) -> f64 {
    let obj = js_object_alloc(STRING_ITERATOR_CLASS_ID, 2);
    // Field 0: backing codepoint array (NaN-boxed pointer for the GC scanner).
    js_object_set_field(
        obj,
        0,
        JSValue::from_bits(js_nanbox_pointer(cp_array as i64).to_bits()),
    );
    // Field 1: cursor index, starts at 0.
    js_object_set_field(obj, 1, JSValue::number(0.0));
    crate::object::attach_iterator_prototype(obj, STRING_ITERATOR_CLASS_ID);
    js_nanbox_pointer(obj as i64)
}

/// `''[Symbol.iterator]()` — build a String iterator over `s`'s code points.
/// Returns a NaN-boxed pointer to the iterator object (or undefined on null).
pub fn string_values_iter(s: *const StringHeader) -> f64 {
    if s.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let cp_array = crate::array::js_array_from_string_codepoints(s);
        alloc_iterator(cp_array)
    }
}

/// Build the `{ value, done }` iterator-result object. Mirrors
/// `array/iter_object.rs::make_iter_result`.
unsafe fn make_iter_result(value: JSValue, done: bool) -> f64 {
    let obj = js_object_alloc(0, 2);
    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    let done_key = crate::string::js_string_from_bytes(b"done".as_ptr(), 4);
    let keys = crate::array::js_array_alloc(2);
    crate::array::js_array_push(keys, JSValue::string_ptr(value_key));
    crate::array::js_array_push(keys, JSValue::string_ptr(done_key));
    crate::object::js_object_set_keys(obj, keys);
    js_object_set_field(obj, 0, value);
    js_object_set_field(obj, 1, JSValue::bool(done));
    js_nanbox_pointer(obj as i64)
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on a String iterator object.
pub unsafe fn dispatch_string_iterator_method(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    match method_name {
        "next" => {
            let backing = f64::from_bits(js_object_get_field(iter_obj, 0).bits());
            let arr = js_nanbox_get_pointer(backing) as *const ArrayHeader;
            let idx = f64::from_bits(js_object_get_field(iter_obj, 1).bits()) as u32;
            let len = if arr.is_null() {
                0
            } else {
                crate::array::js_array_length(arr)
            };
            if idx >= len {
                return make_iter_result(JSValue::undefined(), true);
            }
            js_object_set_field(iter_obj, 1, JSValue::number((idx + 1) as f64));
            let elem = crate::array::js_array_get_f64(arr, idx);
            make_iter_result(JSValue::from_bits(elem.to_bits()), false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj as i64),
        "return" | "throw" => make_iter_result(JSValue::undefined(), true),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
