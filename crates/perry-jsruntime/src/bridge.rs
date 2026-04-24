//! Value bridge between NaN-boxed JSValue and V8 values
//!
//! This module handles conversion between the Perry runtime's NaN-boxed
//! representation and V8's value system.
//!
//! ## V8 Object Handle Table
//!
//! V8 objects (objects, arrays, functions) returned to native code are stored
//! in a thread-local handle table. The native code receives a handle ID that
//! can be used to retrieve the V8 object for subsequent operations.

use perry_runtime::JSValue;
use deno_core::v8;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

// NaN-boxing constants (must match perry-runtime/src/value.rs)
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;

/// Tag for V8 object handles - these are opaque references to V8 objects
/// stored in the handle table, NOT native Perry objects
const JS_HANDLE_TAG: u64 = 0x7FFB_0000_0000_0000;

const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

// Thread-local storage for V8 object handles
thread_local! {
    /// Maps handle IDs to V8 Global handles
    static JS_OBJECT_HANDLES: RefCell<HashMap<u64, v8::Global<v8::Value>>> = RefCell::new(HashMap::new());
    /// Counter for generating unique handle IDs
    static NEXT_HANDLE_ID: Cell<u64> = const { Cell::new(1) };
}

/// Store a V8 value in the handle table and return a handle ID
pub fn store_js_handle(scope: &mut v8::HandleScope, value: v8::Local<v8::Value>) -> u64 {
    let handle_id = NEXT_HANDLE_ID.with(|id| {
        let current = id.get();
        id.set(current + 1);
        current
    });
    let global = v8::Global::new(scope, value);
    JS_OBJECT_HANDLES.with(|handles| {
        handles.borrow_mut().insert(handle_id, global);
    });
    handle_id
}

/// Retrieve a V8 value from the handle table
pub fn get_js_handle<'s>(scope: &mut v8::HandleScope<'s>, handle: u64) -> Option<v8::Local<'s, v8::Value>> {
    JS_OBJECT_HANDLES.with(|handles| {
        handles.borrow().get(&handle).map(|g| v8::Local::new(scope, g))
    })
}

/// Release a V8 handle from the table
pub fn release_js_handle(handle: u64) -> bool {
    JS_OBJECT_HANDLES.with(|handles| {
        handles.borrow_mut().remove(&handle).is_some()
    })
}

/// Check if a NaN-boxed value is a JS handle
pub fn is_js_handle(value: f64) -> bool {
    let bits = value.to_bits();
    (bits & TAG_MASK) == JS_HANDLE_TAG
}

/// Extract handle ID from a NaN-boxed JS handle value
pub fn get_handle_id(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    if (bits & TAG_MASK) == JS_HANDLE_TAG {
        Some(bits & POINTER_MASK)
    } else {
        None
    }
}

/// Create a NaN-boxed value representing a JS handle
pub fn make_js_handle_value(handle_id: u64) -> f64 {
    f64::from_bits(JS_HANDLE_TAG | (handle_id & POINTER_MASK))
}

/// Fix up a native value for JS interop boundary.
/// Raw pointers (non-NaN-boxed I64 values bitcast to F64) need POINTER_TAG
/// so that native_to_v8 can properly convert them to V8 arrays/objects.
pub fn fixup_native_for_v8(value: f64) -> f64 {
    let bits = value.to_bits();
    // Raw heap pointers on arm64 are typically 0x0000_0001_xxxx_xxxx to 0x0000_000F_xxxx_xxxx
    // These appear as subnormal f64 values (exponent = 0, mantissa != 0)
    // No legitimate JS number would have bits in this range
    if bits > 0x0000_0001_0000_0000 && bits < 0x0001_0000_0000_0000 {
        // Raw pointer - add POINTER_TAG so native_to_v8 can convert it
        f64::from_bits(POINTER_TAG | (bits & POINTER_MASK))
    } else {
        value
    }
}

/// Convert a native NaN-boxed value to a V8 value
pub fn native_to_v8<'s>(
    scope: &mut v8::HandleScope<'s>,
    value: f64,
) -> v8::Local<'s, v8::Value> {
    let bits = value.to_bits();

    // Check special values
    if bits == TAG_UNDEFINED {
        return v8::undefined(scope).into();
    }
    if bits == TAG_NULL {
        return v8::null(scope).into();
    }
    if bits == TAG_FALSE {
        return v8::Boolean::new(scope, false).into();
    }
    if bits == TAG_TRUE {
        return v8::Boolean::new(scope, true).into();
    }

    let tag = bits & TAG_MASK;

    // Check for JS handle (V8 object reference)
    if tag == JS_HANDLE_TAG {
        let handle_id = bits & POINTER_MASK;
        if let Some(v8_val) = get_js_handle(scope, handle_id) {
            return v8_val;
        }
        return v8::undefined(scope).into();
    }

    // Check for int32
    if tag == INT32_TAG {
        let int_val = (bits & 0xFFFF_FFFF) as i32;
        return v8::Integer::new(scope, int_val).into();
    }

    // Check for string pointer
    if tag == STRING_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            let rust_str = unsafe { native_string_to_rust(ptr) };
            if let Some(v8_str) = v8::String::new(scope, &rust_str) {
                return v8_str.into();
            }
        }
        return v8::String::empty(scope).into();
    }

    // Check for BigInt pointer
    if tag == BIGINT_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            return native_bigint_to_v8(scope, ptr);
        }
        return v8::BigInt::new_from_i64(scope, 0).into();
    }

    // Check for object/array pointer
    if tag == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if !ptr.is_null() {
            return native_object_to_v8(scope, ptr);
        }
        return v8::null(scope).into();
    }

    // Otherwise it's a regular f64 number
    // Check if it's a valid IEEE 754 number (not NaN with our special tags)
    if (bits & 0x7FF0_0000_0000_0000) != 0x7FF0_0000_0000_0000
        || (bits & 0x000F_FFFF_FFFF_FFFF) == 0
    {
        return v8::Number::new(scope, value).into();
    }

    // Fallback to undefined for unrecognized values
    v8::undefined(scope).into()
}

/// Convert a V8 value to a native NaN-boxed value
///
/// For simple values (undefined, null, boolean, number, string), this converts
/// them to Perry's native NaN-boxed representation.
///
/// For complex values (objects, arrays, functions), this stores them in the
/// handle table and returns a JS handle. This preserves V8 objects for
/// subsequent method calls.
pub fn v8_to_native(scope: &mut v8::HandleScope<'_>, value: v8::Local<v8::Value>) -> f64 {
    if value.is_undefined() {
        return f64::from_bits(TAG_UNDEFINED);
    }

    if value.is_null() {
        return f64::from_bits(TAG_NULL);
    }

    if value.is_boolean() {
        let b = value.is_true();
        return f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE });
    }

    // Check number before int32 as numbers can also be int32
    if value.is_number() && !value.is_int32() {
        let num = value.number_value(scope).unwrap_or(f64::NAN);
        return num;
    }

    if value.is_int32() {
        let int_val = value.int32_value(scope).unwrap_or(0);
        return f64::from_bits(INT32_TAG | (int_val as u32 as u64));
    }

    if value.is_string() {
        let v8_str = value.to_string(scope).unwrap();
        let rust_str = v8_str.to_rust_string_lossy(scope);
        let ptr = rust_string_to_native(&rust_str);
        return f64::from_bits(STRING_TAG | (ptr as u64 & POINTER_MASK));
    }

    // Check for BigInt (used by ethers.js and other blockchain libraries)
    if value.is_big_int() {
        let bigint = v8::Local::<v8::BigInt>::try_from(value).unwrap();
        let ptr = v8_bigint_to_native(scope, bigint);
        return f64::from_bits(BIGINT_TAG | (ptr as u64 & POINTER_MASK));
    }

    // For functions, always store as JS handle to preserve callability
    if value.is_function() {
        let handle_id = store_js_handle(scope, value);
        return make_js_handle_value(handle_id);
    }

    // For arrays and objects, store as JS handle to preserve V8 methods and prototype chain
    // This is critical for objects returned from JS function calls (e.g., express())
    // which may have methods we need to call later (e.g., app.use(), app.get())
    if value.is_array() || value.is_object() {
        let handle_id = store_js_handle(scope, value);
        return make_js_handle_value(handle_id);
    }

    // Fallback to undefined
    f64::from_bits(TAG_UNDEFINED)
}

/// Convert a V8 value to a native NaN-boxed value, converting arrays to native arrays
///
/// This variant converts arrays to native Perry arrays instead of JS handles.
/// Use this when you know the result should be a native array (e.g., for Array operations).
#[allow(dead_code)]
pub fn v8_to_native_array(scope: &mut v8::HandleScope<'_>, value: v8::Local<v8::Value>) -> f64 {
    // For arrays, convert to native Perry array
    if value.is_array() {
        let array = v8::Local::<v8::Array>::try_from(value).unwrap();
        let ptr = v8_array_to_native(scope, array);
        return f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK));
    }

    // For everything else, use the standard conversion
    v8_to_native(scope, value)
}

/// Convert a native string pointer to a Rust String
unsafe fn native_string_to_rust(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }

    // StringHeader layout: { utf16_len: u32, byte_len: u32, capacity: u32, refcount: u32, flags: u32, data: [u8] }
    #[repr(C)]
    struct StringHeader {
        _utf16_len: u32,
        byte_len: u32,
        _capacity: u32,
        _refcount: u32,
        _flags: u32,
    }

    let header = ptr as *const StringHeader;
    let length = (*header).byte_len as usize;
    let data_ptr = ptr.add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, length);

    String::from_utf8_lossy(bytes).to_string()
}

/// Convert a Rust string to a native string pointer
fn rust_string_to_native(s: &str) -> *const u8 {
    use perry_runtime::js_string_from_bytes;

    let bytes = s.as_bytes();
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as *const u8 }
}

/// Convert a native object pointer to a V8 object
fn native_object_to_v8<'s>(scope: &mut v8::HandleScope<'s>, ptr: *const u8) -> v8::Local<'s, v8::Value> {
    if ptr.is_null() {
        return v8::null(scope).into();
    }

    // Use GcHeader (8 bytes before user pointer) to reliably determine type.
    // All Perry arrays and objects are arena-allocated with GcHeader via arena_alloc_gc.
    let gc_header_ptr = (ptr as usize).wrapping_sub(perry_runtime::gc::GC_HEADER_SIZE);
    if gc_header_ptr > 0x1000 {
        let gc_header = unsafe { &*(gc_header_ptr as *const perry_runtime::gc::GcHeader) };
        let is_arena = (gc_header.gc_flags & perry_runtime::gc::GC_FLAG_ARENA) != 0;

        if is_arena && gc_header.obj_type == perry_runtime::gc::GC_TYPE_ARRAY {
            // GC-tracked array: ArrayHeader { length: u32, capacity: u32 } + f64 elements
            let header = ptr as *const perry_runtime::array::ArrayHeader;
            let length = unsafe { (*header).length };
            let elements_ptr = unsafe {
                ptr.add(std::mem::size_of::<perry_runtime::array::ArrayHeader>()) as *const f64
            };
            let v8_array = v8::Array::new(scope, length as i32);
            for i in 0..length {
                let elem_f64 = unsafe { *elements_ptr.add(i as usize) };
                let v8_elem = native_to_v8(scope, elem_f64);
                v8_array.set_index(scope, i, v8_elem);
            }
            return v8_array.into();
        }

        if is_arena && gc_header.obj_type == perry_runtime::gc::GC_TYPE_OBJECT {
            // GC-tracked object: ObjectHeader (24 bytes) + field values
            let obj_header = ptr as *const perry_runtime::object::ObjectHeader;
            let field_count = unsafe { (*obj_header).field_count };
            let keys_array = unsafe { (*obj_header).keys_array };

            let v8_obj = v8::Object::new(scope);

            if !keys_array.is_null() && field_count > 0 {
                // Object has named keys - iterate and set each field
                let keys_length = unsafe { (*keys_array).length };
                let keys_elements_ptr = unsafe {
                    (keys_array as *const u8)
                        .add(std::mem::size_of::<perry_runtime::array::ArrayHeader>())
                        as *const f64
                };
                // Fields are stored as f64 (NaN-boxed JSValues) right after ObjectHeader
                let fields_ptr = unsafe {
                    ptr.add(std::mem::size_of::<perry_runtime::object::ObjectHeader>())
                        as *const f64
                };

                let count = std::cmp::min(field_count, keys_length);
                for i in 0..count {
                    // Get key string from keys_array (NaN-boxed with STRING_TAG)
                    let key_f64 = unsafe { *keys_elements_ptr.add(i as usize) };
                    let key_bits = key_f64.to_bits();
                    let key_ptr = (key_bits & POINTER_MASK) as *const u8;
                    if key_ptr.is_null() || (key_ptr as usize) < 0x1000 {
                        continue;
                    }
                    let key_str = unsafe { native_string_to_rust(key_ptr) };
                    if key_str.is_empty() {
                        continue;
                    }
                    let v8_key = match v8::String::new(scope, &key_str) {
                        Some(k) => k,
                        None => continue,
                    };

                    // Get field value (NaN-boxed f64)
                    let field_f64 = unsafe { *fields_ptr.add(i as usize) };
                    let v8_val = native_to_v8(scope, field_f64);

                    v8_obj.set(scope, v8_key.into(), v8_val);
                }
            }

            return v8_obj.into();
        }
    }

    // Safety check: If the pointer looks like a StringHeader (length + capacity match,
    // and data after header is valid UTF-8), convert it as a string instead of an array.
    // This handles the case where a string pointer accidentally gets POINTER_TAG instead of STRING_TAG.
    {
        let str_header = ptr as *const perry_runtime::string::StringHeader;
        let str_len = unsafe { (*str_header).byte_len } as usize;
        let str_cap = unsafe { (*str_header).capacity } as usize;
        if str_len > 0 && str_len <= 100_000 && str_cap >= str_len && str_cap <= str_len + 64 {
            // Capacity is close to length — looks like a string, not an array
            // (Arrays typically have capacity much larger than needed due to growth)
            let data = unsafe { ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>()) };
            let bytes = unsafe { std::slice::from_raw_parts(data, str_len) };
            if let Ok(s) = std::str::from_utf8(bytes) {
                if let Some(v8_str) = v8::String::new(scope, s) {
                    return v8_str.into();
                }
            }
        }
    }

    // Fallback: heuristic array detection for non-arena allocations (Maps, etc.)
    let header = ptr as *const perry_runtime::array::ArrayHeader;
    let length = unsafe { (*header).length };
    let capacity = unsafe { (*header).capacity };
    if length <= 100_000 && capacity >= length && capacity <= 200_000 {
        let elements_ptr = unsafe {
            ptr.add(std::mem::size_of::<perry_runtime::array::ArrayHeader>()) as *const f64
        };
        let v8_array = v8::Array::new(scope, length as i32);
        for i in 0..length {
            let elem_f64 = unsafe { *elements_ptr.add(i as usize) };
            let v8_elem = native_to_v8(scope, elem_f64);
            v8_array.set_index(scope, i, v8_elem);
        }
        return v8_array.into();
    }

    // Unknown type - wrap native pointer for opaque access
    let obj = v8::Object::new(scope);
    let external = v8::External::new(scope, ptr as *mut std::ffi::c_void);
    let key = v8::String::new(scope, "__native_ptr__").unwrap();
    obj.set(scope, key.into(), external.into());

    obj.into()
}

/// Convert a native BigInt pointer to a V8 BigInt
fn native_bigint_to_v8<'s>(scope: &mut v8::HandleScope<'s>, ptr: *const u8) -> v8::Local<'s, v8::Value> {
    use perry_runtime::bigint::BigIntHeader;

    if ptr.is_null() {
        return v8::BigInt::new_from_i64(scope, 0).into();
    }

    let header = ptr as *const BigIntHeader;
    let limbs = unsafe { (*header).limbs };

    // Check if the value fits in i64 (most common case)
    if limbs[1] == 0 && limbs[2] == 0 && limbs[3] == 0 {
        // Fits in a single limb - check sign
        let val = limbs[0];
        if val <= i64::MAX as u64 {
            return v8::BigInt::new_from_i64(scope, val as i64).into();
        }
        // Value is positive but too large for i64, use u64
        return v8::BigInt::new_from_u64(scope, val).into();
    }

    // Check if it's a negative number (two's complement: high bit set in top limb)
    let is_negative = (limbs[3] >> 63) == 1;

    if is_negative {
        // Convert from two's complement to magnitude
        let mut magnitude = limbs;
        // Subtract 1 and invert
        let mut borrow = 1u64;
        for limb in magnitude.iter_mut() {
            let (result, underflow) = limb.overflowing_sub(borrow);
            *limb = !result;
            borrow = if underflow { 1 } else { 0 };
        }
        // Find the actual word count (trim trailing zeros)
        let word_count = magnitude.iter().rposition(|&x| x != 0).map(|i| i + 1).unwrap_or(1);
        v8::BigInt::new_from_words(scope, true, &magnitude[..word_count])
            .map(|bi| bi.into())
            .unwrap_or_else(|| v8::BigInt::new_from_i64(scope, 0).into())
    } else {
        // Positive number with multiple limbs
        // Find the actual word count (trim trailing zeros)
        let word_count = limbs.iter().rposition(|&x| x != 0).map(|i| i + 1).unwrap_or(1);
        v8::BigInt::new_from_words(scope, false, &limbs[..word_count])
            .map(|bi| bi.into())
            .unwrap_or_else(|| v8::BigInt::new_from_i64(scope, 0).into())
    }
}

/// Convert a V8 object to a native object pointer
fn v8_object_to_native(scope: &mut v8::HandleScope<'_>, obj: v8::Local<v8::Object>) -> *mut u8 {
    use perry_runtime::{js_object_alloc, js_object_set_field};

    // Check if this object has a native pointer already
    let key = v8::String::new(scope, "__native_ptr__").unwrap();
    if let Some(val) = obj.get(scope, key.into()) {
        if val.is_external() {
            let external = v8::Local::<v8::External>::try_from(val).unwrap();
            return external.value() as *mut u8;
        }
    }

    // Get all own property names
    let names = obj
        .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
        .unwrap_or_else(|| v8::Array::new(scope, 0));

    let field_count = names.length() as u32;

    // Allocate native object
    let native_obj = unsafe { js_object_alloc(0, field_count) };

    // Set fields (keys handling is simplified for now)
    for i in 0..field_count {
        let key_val = names.get_index(scope, i).unwrap();

        // Get and convert the value
        if let Some(val) = obj.get(scope, key_val) {
            let native_val = v8_to_native(scope, val);
            // Convert f64 bits to JSValue
            let jsval = JSValue::from_bits(native_val.to_bits());
            unsafe {
                js_object_set_field(native_obj, i, jsval);
            }
        }
    }

    native_obj as *mut u8
}

/// Convert a V8 array to a native array pointer
fn v8_array_to_native(scope: &mut v8::HandleScope<'_>, array: v8::Local<v8::Array>) -> *mut u8 {
    use perry_runtime::js_array_alloc;

    let length = array.length();

    // Allocate native array
    let native_array = js_array_alloc(length);

    // Convert each element
    // We use js_array_set_f64 which takes the raw f64 bits
    for i in 0..length {
        if let Some(val) = array.get_index(scope, i) {
            let native_val = v8_to_native(scope, val);
            unsafe {
                // Set the value directly using pointer arithmetic
                // ArrayHeader is { length: u32, capacity: u32 } = 8 bytes
                // Followed by array of f64 values
                let data_ptr = (native_array as *mut u8).add(8) as *mut f64;
                *data_ptr.add(i as usize) = native_val;
            }
        }
    }

    native_array as *mut u8
}

/// Convert a V8 BigInt to a native BigInt pointer
fn v8_bigint_to_native(_scope: &mut v8::HandleScope<'_>, bigint: v8::Local<v8::BigInt>) -> *mut u8 {
    use perry_runtime::bigint::BigIntHeader;
    use std::alloc::{alloc, Layout};

    // Get the word count to determine the size needed
    let word_count = bigint.word_count();

    // Allocate a BigIntHeader (4 x u64 = 256 bits)
    let layout = Layout::new::<BigIntHeader>();
    let ptr = unsafe { alloc(layout) as *mut BigIntHeader };
    if ptr.is_null() {
        panic!("Failed to allocate BigInt");
    }

    use perry_runtime::bigint::BIGINT_LIMBS;

    if word_count == 0 {
        // Zero value
        unsafe {
            (*ptr).limbs = [0; BIGINT_LIMBS];
        }
        return ptr as *mut u8;
    }

    // Get the words from V8 BigInt
    let mut words = vec![0u64; word_count];
    let (sign_bit, _) = bigint.to_words_array(&mut words);

    // Copy words to our BigIntHeader (up to BIGINT_LIMBS limbs)
    unsafe {
        let mut limbs = [0u64; BIGINT_LIMBS];
        for (i, &word) in words.iter().enumerate().take(BIGINT_LIMBS) {
            limbs[i] = word;
        }

        // Handle negative numbers (two's complement)
        if sign_bit {
            // Negate: invert all bits and add 1
            for limb in limbs.iter_mut() {
                *limb = !*limb;
            }
            // Add 1
            let mut carry = 1u64;
            for limb in limbs.iter_mut() {
                let (result, overflow) = limb.overflowing_add(carry);
                *limb = result;
                carry = if overflow { 1 } else { 0 };
            }
        }

        (*ptr).limbs = limbs;
    }

    ptr as *mut u8
}

/// Convert a native array pointer to a V8 array
pub fn native_array_to_v8<'s>(
    scope: &mut v8::HandleScope<'s>,
    ptr: *const u8,
) -> v8::Local<'s, v8::Array> {
    if ptr.is_null() {
        return v8::Array::new(scope, 0);
    }

    // ArrayHeader layout: { length: u32, capacity: u32 }
    #[repr(C)]
    struct ArrayHeader {
        length: u32,
        _capacity: u32,
    }

    let header = ptr as *const ArrayHeader;
    let length = unsafe { (*header).length };

    let array = v8::Array::new(scope, length as i32);

    for i in 0..length {
        // Read the f64 value directly from the array data
        let native_val = unsafe {
            let data_ptr = (ptr as *const u8).add(8) as *const f64;
            *data_ptr.add(i as usize)
        };
        let v8_val = native_to_v8(scope, native_val);
        array.set_index(scope, i, v8_val);
    }

    array
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_constants() {
        // Verify our tag constants match expected values
        assert_eq!(TAG_UNDEFINED, 0x7FFC_0000_0000_0001);
        assert_eq!(TAG_NULL, 0x7FFC_0000_0000_0002);
        assert_eq!(TAG_FALSE, 0x7FFC_0000_0000_0003);
        assert_eq!(TAG_TRUE, 0x7FFC_0000_0000_0004);
    }
}
