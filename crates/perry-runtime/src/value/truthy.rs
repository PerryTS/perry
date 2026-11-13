//! Truthiness check for NaN-boxed values.

use super::*;

/// Check if a JavaScript value is truthy.
/// In JavaScript, the following values are falsy:
/// - false
/// - 0 (and -0)
/// - NaN
/// - "" (empty string)
/// - null
/// - undefined
/// Everything else is truthy.
/// Returns 1 if truthy, 0 if falsy.
#[no_mangle]
pub extern "C" fn js_is_truthy(value: f64) -> i32 {
    let bits = value.to_bits();

    // Check for special tagged values first
    if bits == TAG_UNDEFINED || bits == TAG_NULL || bits == TAG_FALSE {
        return 0;
    }

    // TAG_TRUE is truthy
    if bits == TAG_TRUE {
        return 1;
    }

    // Check for NaN-boxed string (empty string is falsy)
    if (bits & TAG_MASK) == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
        if str_ptr.is_null() {
            return 0;
        }
        // Empty string is falsy
        let len = crate::string::js_string_length(str_ptr);
        if len == 0 {
            return 0;
        }
        return 1;
    }

    // Check for NaN-boxed pointer (objects/arrays are always truthy)
    if (bits & TAG_MASK) == POINTER_TAG {
        // Null pointer (0x7FFD_0000_0000_0000) is falsy — like null in JS
        if (bits & POINTER_MASK) == 0 {
            return 0;
        }
        return 1;
    }

    // Check for BigInt (0n is falsy, non-zero is truthy)
    if (bits & !POINTER_MASK) == BIGINT_TAG {
        let ptr = (bits & POINTER_MASK) as *const u8;
        if ptr.is_null() {
            return 0;
        }
        return if crate::bigint_hooks::js_bigint_is_zero(ptr as *const crate::bigint::BigIntHeader)
            != 0
        {
            0
        } else {
            1
        };
    }

    // Check for JS handle (always truthy - they represent objects)
    if (bits & TAG_MASK) == JS_HANDLE_TAG || (bits & TAG_MASK) == HANDLE_TAG {
        return 1;
    }

    // Check for int32 tag
    if is_int32_bits(bits) {
        let int_val = (bits & INT32_MASK) as i32;
        return if int_val == 0 { 0 } else { 1 };
    }
    // Any other `0x7FFE` word (a `C.prototype` ref) is an object: truthy.
    if (bits & TAG_MASK) == INT32_TAG {
        return 1;
    }

    // Check for SHORT_STRING_TAG (inline SSO strings): falsy iff length is 0.
    // Without this branch SSO empties would fall through to the f64 path,
    // produce a non-zero non-NaN value, and report truthy.
    if (bits & TAG_MASK) == SHORT_STRING_TAG {
        let len = (bits & SHORT_STRING_LEN_MASK) >> SHORT_STRING_LEN_SHIFT;
        return if len == 0 { 0 } else { 1 };
    }

    // A word with clear top 16 bits is a Number (a denormal double, or +0),
    // never a string: a string is a STRING_TAG or SHORT_STRING_TAG value. (A
    // legacy arm read an aligned denormal above the handle band as a
    // `StringHeader` and dereferenced it.) Regular f64 number: 0.0, -0.0
    // and NaN are falsy.
    if value == 0.0 || value.is_nan() {
        return 0;
    }

    // Everything else is truthy
    1
}
