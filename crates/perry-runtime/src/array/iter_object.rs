//! #321 (effect `ManagedRuntime` / `Exit.all`): a real array iterator
//! object so a value-level `arr[Symbol.iterator]()` / `arr.values()` /
//! `arr.keys()` / `arr.entries()` call returns a `.next()`-bearing
//! iterator (matching Node), not an eager array clone.
//!
//! Background: Perry's `for...of` over an array is special-cased to an
//! indexed length/[i] loop, and codegen's `Expr::ArrayValues` fast path
//! materializes `arr.values()` as a plain array clone. That covers the
//! common cases. But when an array's `[Symbol.iterator]` is invoked
//! dynamically through the runtime dispatch tower (`js_native_call_method`)
//! — e.g. effect's `Chunk[Symbol.iterator]()` delegates to
//! `backing.array[Symbol.iterator]()`, and `Array.from(chunk)` /
//! `Arr.reduce` then drive `.next()` on the result — the pre-fix tower had
//! no `values`/`keys`/`entries`/`@@iterator` arm, so the call fell through
//! to the object-field scan and returned `undefined`. `Array.from(undefined)`
//! yields nothing (or undefined elements), which surfaced downstream as
//! `Cannot read properties of undefined (reading '_tag')` in effect's
//! `exitZipWith`.
//!
//! Representation mirrors `buffer/iter.rs`: a regular `ObjectHeader` with a
//! dedicated `ARRAY_ITERATOR_CLASS_ID`. Field 0 holds the backing array
//! (NaN-boxed pointer, so the object scanner keeps it alive), field 1 the
//! cursor index, field 2 the iterator kind. Dispatch lives in
//! `object/native_call_method.rs` via the class-id check next to the
//! Buffer iterator one.

use super::*;
use crate::object::{js_object_alloc, js_object_get_field, js_object_set_field, ObjectHeader};
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_UNDEFINED};

/// Class id reserved for array iterators. Sits adjacent to the Buffer
/// iterator id (0xFFFF0005) in the 0xFFFF prefix reserved for
/// runtime-defined classes.
pub const ARRAY_ITERATOR_CLASS_ID: u32 = 0xFFFF_0006;

/// Iterator kind tags — matches the i32 stored in field 2.
const KIND_VALUES: i32 = 0;
const KIND_KEYS: i32 = 1;
const KIND_ENTRIES: i32 = 2;

/// Clean a NaN-boxed array pointer to a raw `*mut ArrayHeader`, or null.
fn unbox_array_ptr(value: f64) -> *mut ArrayHeader {
    let raw = js_nanbox_get_pointer(value);
    if raw < (crate::gc::GC_HEADER_SIZE as i64 + 0x1000) {
        return std::ptr::null_mut();
    }
    raw as *mut ArrayHeader
}

unsafe fn alloc_iterator(arr_ptr: *mut ArrayHeader, kind: i32) -> f64 {
    let obj = js_object_alloc(ARRAY_ITERATOR_CLASS_ID, 3);
    // Field 0: backing array (NaN-boxed pointer so the GC scanner keeps it).
    let arr_nan = js_nanbox_pointer(arr_ptr as i64);
    js_object_set_field(obj, 0, JSValue::from_bits(arr_nan.to_bits()));
    // Field 1: cursor index, starts at 0.
    js_object_set_field(obj, 1, JSValue::number(0.0));
    // Field 2: iterator kind.
    js_object_set_field(obj, 2, JSValue::number(kind as f64));
    js_nanbox_pointer(obj as i64)
}

/// `arr.values()` iterator — yields each element value.
pub fn array_values_iter(arr_f64: f64) -> f64 {
    let arr_ptr = unbox_array_ptr(arr_f64);
    if arr_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(arr_ptr, KIND_VALUES) }
}

/// `arr.keys()` iterator — yields each index `0..length`.
pub fn array_keys_iter(arr_f64: f64) -> f64 {
    let arr_ptr = unbox_array_ptr(arr_f64);
    if arr_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(arr_ptr, KIND_KEYS) }
}

/// `arr.entries()` iterator — yields `[index, value]` pairs.
pub fn array_entries_iter(arr_f64: f64) -> f64 {
    let arr_ptr = unbox_array_ptr(arr_f64);
    if arr_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(arr_ptr, KIND_ENTRIES) }
}

/// Build the `{ value, done }` iterator-result object. `value` arrives as
/// a NaN-boxed JSValue; `done` is a JS boolean.
unsafe fn make_iter_result(value: JSValue, done: bool) -> f64 {
    let obj = js_object_alloc(0, 2);

    // keys array so destructuring + property reads find named slots.
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

unsafe fn make_pair_array(idx: u32, value: f64) -> f64 {
    let pair = crate::array::js_array_alloc(2);
    (*pair).length = 2;
    let elems = (pair as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    *elems.add(0) = idx as f64;
    *elems.add(1) = value;
    crate::array::note_array_slot(pair, 0, (idx as f64).to_bits());
    crate::array::note_array_slot(pair, 1, value.to_bits());
    js_nanbox_pointer(pair as i64)
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on an array iterator object.
/// Routed from `js_native_call_method`'s class-id check.
pub unsafe fn dispatch_array_iterator_method(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    match method_name {
        "next" => {
            // Field 0: backing array pointer (NaN-boxed).
            let backing_field = js_object_get_field(iter_obj, 0);
            let backing_f64 = f64::from_bits(backing_field.bits());
            let arr_ptr = js_nanbox_get_pointer(backing_f64) as *const ArrayHeader;
            // Field 1: current index.
            let idx_field = js_object_get_field(iter_obj, 1);
            let idx = f64::from_bits(idx_field.bits()) as u32;
            // Field 2: iterator kind.
            let kind_field = js_object_get_field(iter_obj, 2);
            let kind = f64::from_bits(kind_field.bits()) as i32;

            let len = if arr_ptr.is_null() {
                0u32
            } else {
                crate::array::js_array_length(arr_ptr)
            };

            if idx >= len {
                return make_iter_result(JSValue::undefined(), true);
            }

            // Advance the stored cursor before computing the value so a
            // subsequent `.next()` call sees the bumped index.
            js_object_set_field(iter_obj, 1, JSValue::number((idx + 1) as f64));

            let elem = if arr_ptr.is_null() {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                crate::array::js_array_get_f64(arr_ptr, idx)
            };

            let value = match kind {
                KIND_VALUES => JSValue::from_bits(elem.to_bits()),
                KIND_KEYS => JSValue::number(idx as f64),
                KIND_ENTRIES => {
                    let pair = make_pair_array(idx, elem);
                    JSValue::from_bits(pair.to_bits())
                }
                _ => JSValue::undefined(),
            };
            make_iter_result(value, false)
        }
        // Iterators are themselves iterable — `[Symbol.iterator]()` on one
        // returns the same iterator (matches Node, and lets `js_get_iterator`
        // / `for (const v of arr.values())` re-enter without a wrapper).
        "Symbol.iterator" | "@@iterator" | "values" => js_nanbox_pointer(iter_obj as i64),
        // `return`/`throw` are part of the iterator spec; Node's array
        // iterator inherits them from %IteratorPrototype%. Return a
        // `{ value: undefined, done: true }` shape for early-exit code.
        "return" | "throw" => make_iter_result(JSValue::undefined(), true),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
