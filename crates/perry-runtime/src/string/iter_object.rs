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

/// Class id reserved for String iterators, in the 0xFFFF prefix reserved for
/// runtime-defined classes.
///
/// #7576: "sits just past the Set iterator id (0xFFFF0008)" was this comment's
/// original wording, and it was copied verbatim onto
/// [`ITERATOR_HELPER_CLASS_ID`], which then claimed the same value. Every
/// dispatch tower matches these ids in a fixed order, so the later arm went
/// unreachable and the whole iterator-helper surface died silently. **The next
/// free id is not "one past the id in the comment above" — check the family**:
/// `iterator_helpers::tests::iterator_class_ids_are_pairwise_distinct`
/// enumerates every one of them and fails on a duplicate.
///
/// [`ITERATOR_HELPER_CLASS_ID`]: crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID
pub const STRING_ITERATOR_CLASS_ID: u32 = 0xFFFF_0009;

unsafe fn alloc_iterator(cp_array: *mut ArrayHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp_h = scope.root_raw_mut_ptr(cp_array);
    let obj_h = scope.root_raw_mut_ptr(js_object_alloc(STRING_ITERATOR_CLASS_ID, 2));
    // Field 0: backing codepoint array (NaN-boxed pointer for the GC scanner).
    obj_h.with_mut_ptr(|obj| {
        cp_h.with_mut_ptr::<ArrayHeader, _>(|cp| {
            js_object_set_field(
                obj,
                0,
                JSValue::from_bits(js_nanbox_pointer(cp as i64).to_bits()),
            )
        })
    });
    // Field 1: cursor index, starts at 0.
    obj_h.with_mut_ptr(|obj| js_object_set_field(obj, 1, JSValue::number(0.0)));
    obj_h.with_mut_ptr(|obj| {
        crate::object::attach_iterator_prototype(obj, STRING_ITERATOR_CLASS_ID)
    });
    let (_, obj) = obj_h.across_mut::<ObjectHeader, _>(|| ());
    js_nanbox_pointer(obj as i64)
}

/// `''[Symbol.iterator]()` — build a String iterator over `s`'s code points.
/// Returns a NaN-boxed pointer to the iterator object (or undefined on null).
pub fn string_values_iter(s: *const StringHeader) -> f64 {
    if s.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        // Use the bounded WTF-8 iterator shared with string spread. A JS
        // string may contain lone surrogates, so `str::chars()`-based
        // materialization would reject the entire payload and produce an
        // empty iterator.
        let cp_array = crate::string::js_string_to_char_array(s as i64) as *mut ArrayHeader;
        alloc_iterator(cp_array)
    }
}

/// Build the `{ value, done }` iterator-result object. Mirrors
/// `array/iter_object.rs::make_iter_result`.
// #7564: was a local five-allocation copy with unrooted intermediates.
use crate::iter_result::make_iter_result;

/// Dispatch `.next()` / `[Symbol.iterator]()` on a String iterator object.
pub unsafe fn dispatch_string_iterator_method(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let iter_h = scope.root_nanbox_f64(js_nanbox_pointer(iter_obj as i64));
    let iter_obj = || js_nanbox_get_pointer(iter_h.get_nanbox_f64()) as *mut ObjectHeader;
    match method_name {
        "next" => {
            if let Some(result) =
                crate::object::call_overridden_iterator_next(iter_obj(), STRING_ITERATOR_CLASS_ID)
            {
                return result;
            }
            let backing = f64::from_bits(js_object_get_field(iter_obj(), 0).bits());
            let arr_h = scope.root_nanbox_f64(backing);
            let arr = || js_nanbox_get_pointer(arr_h.get_nanbox_f64()) as *const ArrayHeader;
            let idx = f64::from_bits(js_object_get_field(iter_obj(), 1).bits()) as u32;
            let len = if arr().is_null() {
                0
            } else {
                crate::array::js_array_length(arr())
            };
            if idx >= len {
                return make_iter_result(JSValue::undefined(), true);
            }
            js_object_set_field(iter_obj(), 1, JSValue::number((idx + 1) as f64));
            let elem = crate::array::js_array_get_f64(arr(), idx);
            make_iter_result(JSValue::from_bits(elem.to_bits()), false)
        }
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj() as i64),
        "return" | "throw" => make_iter_result(JSValue::undefined(), true),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
