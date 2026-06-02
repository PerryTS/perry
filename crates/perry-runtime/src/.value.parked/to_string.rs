//! NaN-boxed value to-string conversion helpers.

use super::*;
use std::sync::atomic::Ordering;

/// Convert a NaN-boxed f64 value to a string pointer.
/// Handles all value types: strings (extract pointer), numbers (convert), JS handles, etc.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string(value: f64) -> *mut crate::string::StringHeader {
    // Check for JS handle first - these come from the JS runtime (e.g., process.env values)
    if is_js_handle(value) {
        let func_ptr = JS_HANDLE_TO_STRING.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleToStringFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value);
        }
        // Fallback if no handler registered
        return crate::string::js_string_from_bytes(b"[JS Handle]".as_ptr(), 11);
    }

    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_string() {
        // Already a heap string — return the pointer directly.
        jsval.as_string_ptr() as *mut crate::string::StringHeader
    } else if jsval.is_short_string() {
        // Inline SSO — materialize into a heap StringHeader so the
        // caller gets a uniform `*mut StringHeader`. This defeats
        // the SSO benefit for this particular conversion, but it's
        // a correctness-preserving compatibility shim for the many
        // call sites that currently expect a heap pointer.
        crate::string::js_string_materialize_to_heap(value)
    } else if jsval.is_undefined() {
        crate::string::js_string_from_bytes(b"undefined".as_ptr(), 9)
    } else if jsval.is_null() {
        crate::string::js_string_from_bytes(b"null".as_ptr(), 4)
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            crate::string::js_string_from_bytes(b"true".as_ptr(), 4)
        } else {
            crate::string::js_string_from_bytes(b"false".as_ptr(), 5)
        }
    } else if jsval.is_int32() {
        // Convert int32 to string
        let n = jsval.as_int32();
        let s = n.to_string();
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else if jsval.is_bigint() {
        // BigInt - convert to decimal string
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string(ptr)
    } else if jsval.is_pointer() {
        // Pointer: could be an array, object, or other heap type. Arrays
        // stringify via `Array.prototype.join(",")` per JS semantics; other
        // objects fall back to "[object Object]".
        let ptr: *const u8 = jsval.as_pointer();
        if !ptr.is_null() && (ptr as usize) >= 0x10000 {
            // Symbols: detect via the side-table before any GC header read.
            if crate::symbol::is_registered_symbol(ptr as usize) {
                return unsafe {
                    crate::symbol::js_symbol_to_string(value) as *mut crate::string::StringHeader
                };
            }
            // Consult `[Symbol.toPrimitive]("string")` if the object has a
            // custom toPrimitive method registered in the symbol side-table.
            // A changed result means the user-defined method produced a
            // string-hint primitive — recurse so strings pass through as-is
            // and numbers get js_number_to_string.
            let primitive = unsafe { crate::symbol::js_to_primitive(value, 2) };
            if primitive.to_bits() != value.to_bits() {
                return js_jsvalue_to_string(primitive);
            }
            // Buffers: BufferHeader has no GC header, so we must detect via
            // BUFFER_REGISTRY before computing gc_header (which would read
            // garbage one word before the buffer). `Buffer.toString()` with
            // no arg defaults to UTF-8 — Node prints the raw bytes.
            if crate::buffer::is_registered_buffer(ptr as usize) {
                return crate::buffer::js_buffer_to_string(
                    ptr as *const crate::buffer::BufferHeader,
                    0,
                );
            }
            unsafe {
                let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                    // Use js_array_join with a "," separator to match Array.prototype.toString.
                    let sep = crate::string::js_string_from_bytes(b",".as_ptr(), 1);
                    return crate::array::js_array_join(
                        ptr as *const crate::array::ArrayHeader,
                        sep as *const crate::string::StringHeader,
                    );
                }
            }
        }
        crate::string::js_string_from_bytes(b"[object Object]".as_ptr(), 15)
    } else {
        // Regular number - use js_number_to_string
        crate::string::js_number_to_string(value)
    }
}

/// Convert a NaN-boxed f64 value to a string with the given radix.
/// Handles BigInt (uses bigint_to_string_radix), numbers, strings, etc.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string_radix(
    value: f64,
    radix: i32,
) -> *mut crate::string::StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_bigint() {
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string_radix(ptr, radix)
    } else if jsval.is_string() {
        jsval.as_string_ptr() as *mut crate::string::StringHeader
    } else if jsval.is_int32() {
        let n = jsval.as_int32();
        let s = if radix == 16 {
            format!("{:x}", n)
        } else if radix == 10 || radix == 0 {
            n.to_string()
        } else {
            // General radix conversion
            let mut result = String::new();
            let mut val = if n < 0 { -(n as i64) as u64 } else { n as u64 };
            let r = radix as u64;
            if val == 0 {
                return crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
            }
            while val > 0 {
                let digit = (val % r) as u8;
                result.push(if digit < 10 {
                    (b'0' + digit) as char
                } else {
                    (b'a' + digit - 10) as char
                });
                val /= r;
            }
            if n < 0 {
                result.push('-');
            }
            let s: String = result.chars().rev().collect();
            return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        };
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else {
        // Regular f64 number
        let n = value;
        if n.is_nan() {
            return crate::string::js_string_from_bytes(b"NaN".as_ptr(), 3);
        }
        if n.is_infinite() {
            if n > 0.0 {
                return crate::string::js_string_from_bytes(b"Infinity".as_ptr(), 8);
            } else {
                return crate::string::js_string_from_bytes(b"-Infinity".as_ptr(), 9);
            }
        }
        if radix == 10 || radix == 0 {
            return crate::string::js_number_to_string(value);
        }
        // For hex and other radixes, convert via integer
        let n_i64 = n as i64;
        let s = if radix == 16 {
            if n_i64 < 0 {
                format!("-{:x}", -n_i64)
            } else {
                format!("{:x}", n_i64)
            }
        } else {
            let mut result = String::new();
            let mut val = if n_i64 < 0 {
                (-n_i64) as u64
            } else {
                n_i64 as u64
            };
            let r = radix as u64;
            if val == 0 {
                return crate::string::js_string_from_bytes(b"0".as_ptr(), 1);
            }
            while val > 0 {
                let digit = (val % r) as u8;
                result.push(if digit < 10 {
                    (b'0' + digit) as char
                } else {
                    (b'a' + digit - 10) as char
                });
                val /= r;
            }
            if n_i64 < 0 {
                result.push('-');
            }
            result.chars().rev().collect()
        };
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }
}

/// Ensure a value is a native string pointer.
/// This is specifically for fetch headers where we need to handle:
/// 1. Raw string pointers (literal strings - f64 bits ARE the pointer)
/// 2. NaN-boxed strings (STRING_TAG)
/// 3. JS handle strings (from process.env)
/// Returns the string pointer as i64.
#[no_mangle]
pub extern "C" fn js_ensure_string_ptr(value: f64) -> i64 {
    let bits = value.to_bits();

    // Check for JS handle first - these need conversion
    if is_js_handle(value) {
        let func_ptr = JS_HANDLE_TO_STRING.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleToStringFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value) as i64;
        }
        // Fallback - create a placeholder string
        return crate::string::js_string_from_bytes(b"[JS Handle]".as_ptr(), 11) as i64;
    }

    // Check for NaN-boxed string (STRING_TAG)
    if (bits & TAG_MASK) == STRING_TAG {
        let ptr = (bits & POINTER_MASK) as i64;
        if ptr != 0 {
            let str_header = ptr as *const crate::string::StringHeader;
            unsafe {
                let length = (*str_header).byte_len;
                // Make a copy of the string to ensure we have a Perry-allocated string
                let data_ptr = (str_header as *const u8)
                    .add(std::mem::size_of::<crate::string::StringHeader>());
                let copy = crate::string::js_string_from_bytes(data_ptr, length);
                return copy as i64;
            }
        }
        return ptr;
    }

    // Otherwise, treat the f64 bits directly as a pointer (raw string literal)
    bits as i64
}
