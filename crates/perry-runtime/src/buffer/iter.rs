//! Issue #1206: Node's `Buffer` exposes real iterator-protocol objects
//! from `buf.values()`, `buf.keys()`, `buf.entries()`, and
//! `buf[Symbol.iterator]()`.  Each returned value must support `.next()`
//! returning `{ value, done }`.
//!
//! Perry's `for...of` over a Buffer already works via the indexed
//! length/[i] path, but exposing the explicit iterator surface lets
//! ecosystem code call `it.next().value` (the shape the parity fixture
//! and Node's docs document).
//!
//! Representation: a regular `ObjectHeader` with a dedicated
//! `BUFFER_ITERATOR_CLASS_ID`. Three fields hold the backing buffer,
//! the current cursor index, and the iterator kind (0 = values,
//! 1 = keys, 2 = entries). Dispatch lives in
//! `object/native_call_method.rs` — see the class-id check that routes
//! to `dispatch_buffer_iterator_method`.

use super::*;

use crate::object::{js_object_alloc, js_object_get_field, js_object_set_field, ObjectHeader};
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, JSValue, TAG_UNDEFINED};

/// Class id reserved for Buffer iterators. Sits adjacent to
/// `BUFFER_TYPE_ID` (0xFFFF0004) in the 0xFFFF prefix reserved for
/// runtime-defined classes.
pub const BUFFER_ITERATOR_CLASS_ID: u32 = 0xFFFF_0005;

/// Iterator kind tags — matches the i32 stored in field 2 of every
/// BufferIterator object.
const KIND_VALUES: i32 = 0;
const KIND_KEYS: i32 = 1;
const KIND_ENTRIES: i32 = 2;

fn unbox_buffer_ptr(value: f64) -> *mut BufferHeader {
    let bits = value.to_bits();
    let addr = if bits >> 48 >= 0x7FF8 {
        bits & 0x0000_FFFF_FFFF_FFFF
    } else {
        bits
    };
    if addr < 0x1000 {
        return std::ptr::null_mut();
    }
    addr as *mut BufferHeader
}

unsafe fn alloc_iterator(buf_ptr: *mut BufferHeader, kind: i32) -> f64 {
    let obj = js_object_alloc(BUFFER_ITERATOR_CLASS_ID, 3);
    // Field 0: backing buffer (NaN-boxed pointer).
    let buf_nan = js_nanbox_pointer(buf_ptr as i64);
    js_object_set_field(obj, 0, JSValue::from_bits(buf_nan.to_bits()));
    // Field 1: cursor index, starts at 0.
    js_object_set_field(obj, 1, JSValue::number(0.0));
    // Field 2: iterator kind.
    js_object_set_field(obj, 2, JSValue::number(kind as f64));
    js_nanbox_pointer(obj as i64)
}

/// `buf.values()` — iterator yielding each byte value.
#[no_mangle]
pub extern "C" fn js_buffer_values(buf_f64: f64) -> f64 {
    let buf_ptr = unbox_buffer_ptr(buf_f64);
    if buf_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(buf_ptr, KIND_VALUES) }
}

/// `buf.keys()` — iterator yielding each index `0..length`.
#[no_mangle]
pub extern "C" fn js_buffer_keys(buf_f64: f64) -> f64 {
    let buf_ptr = unbox_buffer_ptr(buf_f64);
    if buf_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(buf_ptr, KIND_KEYS) }
}

/// `buf.entries()` — iterator yielding `[index, value]` pairs.
#[no_mangle]
pub extern "C" fn js_buffer_entries(buf_f64: f64) -> f64 {
    let buf_ptr = unbox_buffer_ptr(buf_f64);
    if buf_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe { alloc_iterator(buf_ptr, KIND_ENTRIES) }
}

/// Build the `{ value, done }` iterator-result object.  `value`
/// arrives as a NaN-boxed JSValue; `done` is a JS boolean.
unsafe fn make_iter_result(value: JSValue, done: bool) -> f64 {
    let obj = js_object_alloc(0, 2);

    // keys array so destructuring + for-of can find named slots.
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

unsafe fn make_pair_array(idx: u32, byte: u8) -> f64 {
    let pair = crate::array::js_array_alloc(2);
    (*pair).length = 2;
    let elems = (pair as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut f64;
    *elems.add(0) = idx as f64;
    *elems.add(1) = byte as f64;
    crate::array::note_array_slot(pair, 0, (idx as f64).to_bits());
    crate::array::note_array_slot(pair, 1, (byte as f64).to_bits());
    js_nanbox_pointer(pair as i64)
}

/// Dispatch `.next()` / `[Symbol.iterator]()` on a Buffer iterator
/// object. Routed from `js_native_call_method`'s class-id check.
pub unsafe fn dispatch_buffer_iterator_method(
    iter_obj: *mut ObjectHeader,
    method_name: &str,
) -> f64 {
    match method_name {
        "next" => {
            // Field 0: backing buffer pointer (NaN-boxed).
            let backing_field = js_object_get_field(iter_obj, 0);
            let backing_f64 = f64::from_bits(backing_field.bits());
            let buf_ptr = js_nanbox_get_pointer(backing_f64) as *mut BufferHeader;
            // Field 1: current index.
            let idx_field = js_object_get_field(iter_obj, 1);
            let idx_f64 = f64::from_bits(idx_field.bits());
            let idx = idx_f64 as u32;
            // Field 2: iterator kind.
            let kind_field = js_object_get_field(iter_obj, 2);
            let kind = f64::from_bits(kind_field.bits()) as i32;

            let len = if buf_ptr.is_null() {
                0u32
            } else {
                (*buf_ptr).length
            };

            if idx >= len {
                return make_iter_result(JSValue::undefined(), true);
            }

            // Advance the stored cursor before computing the value so
            // a subsequent `.next()` call sees the bumped index.
            js_object_set_field(iter_obj, 1, JSValue::number((idx + 1) as f64));

            let byte = if buf_ptr.is_null() {
                0u8
            } else {
                let data = buffer_data(buf_ptr);
                *data.add(idx as usize)
            };

            let value = match kind {
                KIND_VALUES => JSValue::number(byte as f64),
                KIND_KEYS => JSValue::number(idx as f64),
                KIND_ENTRIES => {
                    let pair = make_pair_array(idx, byte);
                    JSValue::from_bits(pair.to_bits())
                }
                _ => JSValue::undefined(),
            };
            make_iter_result(value, false)
        }
        // Iterators are themselves iterable — calling `[Symbol.iterator]()`
        // on one returns the same iterator. This is what Node does and
        // lets `for (const v of buf.values())` re-enter without an extra
        // wrapper. Without it the for-of fallback path would attempt to
        // index the iterator as an array and get nonsense.
        "Symbol.iterator" | "@@iterator" => js_nanbox_pointer(iter_obj as i64),
        // `return`/`throw` are part of the iterator spec but most
        // for-of paths don't need them; Node's BufferIterator inherits
        // them from %IteratorPrototype%. We return a `{ value: undefined,
        // done: true }` shape — enough for early-exit code that checks
        // `it.return?.()` semantics.
        "return" | "throw" => make_iter_result(JSValue::undefined(), true),
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}
