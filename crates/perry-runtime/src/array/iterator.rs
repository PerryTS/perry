//! Iterator-protocol → array converter.
use super::*;

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
