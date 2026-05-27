//! Iterator-protocol → array converter.
use super::*;

/// Materialize an arbitrary iterable into a plain Array, used by the
/// `for...of` desugar when the receiver's static type can NOT be proven
/// (an `any`-typed property, an untyped JS-source value, etc.). The HIR
/// loop iterates the returned array by index (`for (i=0; i<arr.length;
/// i++) item = arr[i]`), so this helper must hand back an Array whose
/// elements are exactly what `for...of` would yield in JS:
///
///   * Array / lazy-array  → returned unchanged (no copy; the index
///                           loop reads it directly).
///   * Map                 → array of `[key, value]` pair arrays
///                           (matches `map[Symbol.iterator]()` ===
///                           `map.entries()`), so `for (const [k,v] of
///                           m)` destructures correctly.
///   * Set                 → array of values.
///   * String              → array of code-point substrings (JS spreads
///                           a string by code point, not UTF-16 unit).
///   * anything else        → drive the iterator protocol: obtain the
///                           default iterator via `js_get_iterator`
///                           (custom `[Symbol.iterator]`, perry
///                           generator objects, …) and collect `.value`s
///                           with [`js_iterator_to_array`].
///
/// Returns a NaN-boxed (POINTER_TAG) Array JSValue. Returning the boxed
/// f64 (rather than a raw pointer) keeps the HIR `Stmt::Let` holder typed
/// as a normal array value so `.length` / `arr[i]` lower through the
/// usual array fast paths.
///
/// Refs #321 (effect Context/Layer iterate `for (const [tag, s] of
/// self.unsafeMap)` over an untyped Map).
#[no_mangle]
pub extern "C" fn js_for_of_to_array(val_f64: f64) -> f64 {
    use crate::gc::{
        GcHeader, GC_HEADER_SIZE, GC_TYPE_ARRAY, GC_TYPE_LAZY_ARRAY, GC_TYPE_MAP, GC_TYPE_SET,
    };
    use crate::value::{js_nanbox_pointer, JSValue};

    let jsv = JSValue::from_bits(val_f64.to_bits());

    // Strings: iterate by code point. `is_any_string` covers both heap
    // STRING_TAG and inline SSO short strings. `js_get_string_pointer_unified`
    // returns a real `*const StringHeader` for either representation
    // (materializing SSO onto the heap); re-box with STRING_TAG so
    // `js_string_to_char_array` (which masks POINTER_MASK off the bits)
    // reads it correctly. The resulting array yields single-char
    // substrings exactly like `for (const c of "abc")`.
    if jsv.is_any_string() {
        let str_ptr = crate::value::js_get_string_pointer_unified(val_f64);
        let str_bits = crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK);
        let arr_i64 = crate::string::js_string_to_char_array(str_bits as i64);
        return js_nanbox_pointer(arr_i64);
    }

    // Non-pointer scalars (number/bool/null/undefined) are not iterable;
    // hand back an empty array so the loop runs zero times rather than
    // dereferencing a non-pointer.
    let raw_ptr = crate::value::js_nanbox_get_pointer(val_f64);
    if raw_ptr == 0 {
        return js_nanbox_pointer(js_array_alloc(0) as i64);
    }

    // Inspect the GC header's object kind to dispatch Array / Map / Set
    // without consulting any static type.
    let obj_type = unsafe {
        let gc_header = (raw_ptr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
        (*gc_header).obj_type
    };

    match obj_type {
        // Already an array: return unchanged — the index loop reads it in
        // place, no allocation. Lazy arrays are arrays from the iterator's
        // perspective and `js_array_length` / indexing materialize lazily.
        t if t == GC_TYPE_ARRAY || t == GC_TYPE_LAZY_ARRAY => val_f64,
        // Map → `[k, v]` pair array (=== `map.entries()` spread).
        GC_TYPE_MAP => {
            let arr = js_map_entries_for_for_of(raw_ptr);
            js_nanbox_pointer(arr as i64)
        }
        // Set → values array.
        GC_TYPE_SET => {
            let arr = js_set_to_array_for_for_of(raw_ptr);
            js_nanbox_pointer(arr as i64)
        }
        // Generic objects / generator objects / anything carrying a
        // custom `[Symbol.iterator]` or a `.next()`: walk the iterator
        // protocol. `js_get_iterator` returns the operand's
        // `Symbol.iterator()` result when iterable, or the operand
        // unchanged when it already is an iterator (perry generators).
        _ => {
            let iter = crate::symbol::js_get_iterator(val_f64);
            let arr = js_iterator_to_array(iter);
            js_nanbox_pointer(arr as i64)
        }
    }
}

/// Thin wrappers so this module can reach the Map/Set materializers
/// without importing their concrete header types (they live in sibling
/// runtime modules and take typed pointers). `raw_ptr` is the cleaned
/// payload pointer already extracted by `js_nanbox_get_pointer`.
#[inline]
fn js_map_entries_for_for_of(raw_ptr: i64) -> *mut ArrayHeader {
    crate::map::js_map_entries(raw_ptr as *const crate::map::MapHeader)
}

#[inline]
fn js_set_to_array_for_for_of(raw_ptr: i64) -> *mut ArrayHeader {
    crate::set::js_set_to_array(raw_ptr as *const crate::set::SetHeader)
}

/// Convert any iterator-protocol object (has `.next()` method) to an array.
/// Used by spread on generators, Array.from on generators, etc.
/// Calls `.next()` in a loop until `.done` is true, collecting `.value` entries.
#[no_mangle]
pub extern "C" fn js_iterator_to_array(iter_f64: f64) -> *mut ArrayHeader {
    use crate::closure;
    use crate::object::{js_object_get_field_by_name, ObjectHeader};
    use crate::string::js_string_from_bytes;
    use crate::value::{js_nanbox_get_pointer, TAG_UNDEFINED};

    let arr = js_array_alloc(8); // start with capacity 8

    // Get the iterator object pointer
    let _iter_bits = iter_f64.to_bits();
    let iter_ptr = js_nanbox_get_pointer(iter_f64);
    if iter_ptr == 0 {
        return arr;
    }
    let iter_obj = iter_ptr as *const ObjectHeader;

    // Look up the "next" method on the iterator object
    let next_key = js_string_from_bytes(b"next".as_ptr(), 4);
    let next_val = js_object_get_field_by_name(iter_obj, next_key);
    if next_val.is_undefined() {
        return arr;
    }

    // next_val should be a closure pointer
    let next_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(next_val)) };
    let next_ptr = js_nanbox_get_pointer(next_f64) as *const closure::ClosureHeader;
    if next_ptr.is_null() {
        return arr;
    }

    // Iterate: call next() until done
    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let mut result = arr;

    for _ in 0..100_000 {
        // safety limit
        // Call next()
        let result_f64 = closure::js_closure_call1(next_ptr, f64::from_bits(TAG_UNDEFINED));
        let result_ptr = js_nanbox_get_pointer(result_f64);
        if result_ptr == 0 {
            break;
        }
        let result_obj = result_ptr as *const ObjectHeader;

        // Check .done
        let done_val = js_object_get_field_by_name(result_obj, done_key);
        let done_bits = unsafe { std::mem::transmute::<_, u64>(done_val) };
        // done is true when it's TAG_TRUE (0x7FFC_0000_0000_0004) or truthy number
        if done_bits == 0x7FFC_0000_0000_0004 {
            break;
        } // TAG_TRUE

        // Get .value and push to array
        let val = js_object_get_field_by_name(result_obj, value_key);
        let val_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(val)) };
        result = js_array_push_f64(result, val_f64);
    }

    result
}
