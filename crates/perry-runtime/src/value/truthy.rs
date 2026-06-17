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
        return if crate::bigint::js_bigint_is_zero(ptr as *const crate::bigint::BigIntHeader) != 0 {
            0
        } else {
            1
        };
    }

    // Check for JS handle (always truthy - they represent objects)
    if (bits & TAG_MASK) == JS_HANDLE_TAG {
        return 1;
    }

    // Check for int32 tag
    if (bits & TAG_MASK) == INT32_TAG {
        let int_val = (bits & INT32_MASK) as i32;
        return if int_val == 0 { 0 } else { 1 };
    }

    // Check for SHORT_STRING_TAG (inline SSO strings): falsy iff length is 0.
    // Without this branch SSO empties would fall through to the f64 path,
    // produce a non-zero non-NaN value, and report truthy.
    if (bits & TAG_MASK) == SHORT_STRING_TAG {
        let len = (bits & SHORT_STRING_LEN_MASK) >> SHORT_STRING_LEN_SHIFT;
        return if len == 0 { 0 } else { 1 };
    }

    // Check for raw pointer bits (from bitcast of string literal). This is a
    // legacy path for compiled-in string literals that were emitted as raw
    // pointer bitcasts rather than NaN-boxed STRING_TAG values.
    //
    // CAUTION: a plain f64 value (e.g., a denormal like `f64::from_bits(0x646e)`
    // ≈ 1.27e-319, or any other number whose bit pattern happens to fall in
    // the userspace pointer range) must NOT be misidentified as a string
    // pointer — dereferencing it as `*StringHeader` will SIGSEGV. Surfaced by
    // dayjs which passes `bits=0x646e` through here on a utility-object call.
    //
    // Defense in depth:
    //   1. Reject everything below the smallest realistic heap address. On
    //      macOS/Linux userspace heaps live well above 0x10_0000 (1 MiB); the
    //      previous 0x1000 (4 KiB) threshold let small integers (including
    //      0x646e) through.
    //   2. Require 8-byte alignment. `StringHeader` is `repr(C)` with at
    //      least one usize-aligned field, so a valid pointer must have its
    //      low 3 bits clear. Most small denormal/integer bit patterns fail
    //      this check.
    // Both filters together make a false-positive astronomically unlikely
    // while still preserving the legacy bitcast path for real pointers.
    if crate::value::addr_class::is_above_handle_band(bits as usize)
        && bits < 0x0001_0000_0000_0000
        && (bits & 0x7) == 0
    {
        // This could be a raw string pointer - check if it's a valid string
        let str_ptr = bits as *const crate::string::StringHeader;
        // Try to read the string length - empty string is falsy
        let len = crate::string::js_string_length(str_ptr);
        if len == 0 {
            return 0;
        }
        return 1;
    }

    // Regular f64 number: 0.0, -0.0, and NaN are falsy
    if value == 0.0 || value.is_nan() {
        return 0;
    }

    // Everything else is truthy
    1
}
