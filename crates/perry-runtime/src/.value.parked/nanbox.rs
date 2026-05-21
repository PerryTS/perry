//! NaN-box pack / unpack FFI helpers.
//!
//! These are the smallest building blocks called from generated LLVM IR
//! and from other native code: pointer/string/bigint boxing constructors,
//! the inverse `is_*` / `get_*` predicates, plus debug printers and the
//! unified string-pointer extractor.

use super::*;

// FFI functions for creating NaN-boxed values from raw pointers

/// Create a NaN-boxed pointer value from an i64 raw pointer.
/// Returns the value as f64 for storage in union-typed variables.
/// If the value already has a NaN-box tag (JS_HANDLE, STRING, POINTER, etc.),
/// it is preserved as-is to prevent tag corruption.
#[no_mangle]
pub extern "C" fn js_nanbox_pointer(ptr: i64) -> f64 {
    // Guard: null pointer (ptr == 0) must NOT produce null POINTER_TAG (0x7FFD_0000_0000_0000).
    // Null POINTER_TAG causes crashes when code tries to dereference it as a real object pointer.
    if ptr == 0 {
        return f64::from_bits(TAG_NULL);
    }
    let bits = ptr as u64;
    // If value already has a NaN-box tag (top bits in NaN range), preserve it
    if bits & 0xFFF0_0000_0000_0000 >= 0x7FF0_0000_0000_0000 {
        return f64::from_bits(bits);
    }
    let jsval = JSValue::pointer(ptr as *const u8);
    f64::from_bits(jsval.bits())
}

/// Create a NaN-boxed string pointer value from an i64 raw pointer.
/// Returns the value as f64 for storage in union-typed variables.
/// This uses STRING_TAG (0x7FFF) to distinguish from object pointers.
/// If ptr is null, returns a NaN-boxed empty string to prevent null
/// dereference when callers access .length on the result.
#[no_mangle]
pub extern "C" fn js_nanbox_string(ptr: i64) -> f64 {
    let actual_ptr = if ptr == 0 {
        // Allocate an empty string instead of boxing null
        crate::string::js_string_from_bytes(b"".as_ptr(), 0) as i64
    } else {
        ptr
    };
    let jsval = JSValue::string_ptr(actual_ptr as *mut crate::string::StringHeader);
    f64::from_bits(jsval.bits())
}

/// Debug checkpoint function: prints checkpoint number to stderr.
/// Used to narrow down crash locations in generated code.
#[no_mangle]
pub extern "C" fn js_checkpoint(n: i32) {
    use std::io::Write;
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "[CHECKPOINT] {}", n);
    let _ = stderr.flush();
}

/// Debug: print a value's raw bits to stderr (for diagnosing NaN-boxing issues)
#[no_mangle]
pub extern "C" fn js_debug_val(label: i32, val: f64) {
    use std::io::Write;
    let bits = val.to_bits();
    let _ = writeln!(
        std::io::stderr(),
        "[DEBUG_VAL] label={} bits=0x{:016X} f64={}",
        label,
        bits,
        val
    );
    let _ = std::io::stderr().flush();
}

/// Create a NaN-boxed BigInt pointer value from an i64 raw pointer.
/// Returns the value as f64 for storage in union-typed variables.
/// This uses BIGINT_TAG (0x7FFA) to distinguish from other pointer types.
#[no_mangle]
pub extern "C" fn js_nanbox_bigint(ptr: i64) -> f64 {
    let jsval = JSValue::bigint_ptr(ptr as *mut crate::bigint::BigIntHeader);
    f64::from_bits(jsval.bits())
}

/// Check if an f64 value (interpreted as NaN-boxed) represents a BigInt.
#[no_mangle]
pub extern "C" fn js_nanbox_is_bigint(value: f64) -> i32 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_bigint() {
        1
    } else {
        0
    }
}

/// Extract a BigInt pointer from a NaN-boxed f64 value.
/// Returns the pointer as i64.
#[no_mangle]
pub extern "C" fn js_nanbox_get_bigint(value: f64) -> i64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_bigint() {
        return jsval.as_bigint_ptr() as i64;
    }
    if value.is_nan() {
        return 0;
    }
    bits as i64
}

/// Check if an f64 value (interpreted as NaN-boxed) represents a pointer.
#[no_mangle]
pub extern "C" fn js_nanbox_is_pointer(value: f64) -> i32 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_pointer() {
        1
    } else {
        0
    }
}

/// Extract a pointer from a NaN-boxed f64 value.
/// Also handles raw pointer bits (bitcast from i64) for backward compatibility.
/// Handles POINTER_TAG, STRING_TAG, BIGINT_TAG, and JS_HANDLE_TAG.
/// Returns the pointer as i64.
#[no_mangle]
pub extern "C" fn js_nanbox_get_pointer(value: f64) -> i64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    if jsval.is_pointer() {
        return jsval.as_pointer::<u8>() as i64;
    }

    if jsval.is_string() {
        return jsval.as_string_ptr() as i64;
    }

    if jsval.is_bigint() {
        return jsval.as_bigint_ptr() as i64;
    }

    // JS_HANDLE_TAG (0x7FFB): used for V8 handles and Perry UI widget handles
    // when values pass through inline_nanbox_pointer's "already tagged" path.
    if (bits & TAG_MASK) == JS_HANDLE_TAG {
        return (bits & POINTER_MASK) as i64;
    }

    if bits != 0 && bits <= POINTER_MASK {
        let upper = bits >> 48;
        if upper == 0 || (upper > 0 && upper < 0x7FF0) {
            return bits as i64;
        }
    }

    0
}

/// Returns the pointer as i64.
#[no_mangle]
pub extern "C" fn js_nanbox_get_string_pointer(value: f64) -> i64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_string() {
        jsval.as_string_ptr() as i64
    } else {
        0
    }
}

/// Extract a string pointer from an f64 value that may be either:
/// 1. A properly NaN-boxed string (with STRING_TAG)
/// 2. A raw pointer bitcast to f64 (for locally-created strings)
/// This unified function handles both cases for function parameters.
#[no_mangle]
pub extern "C" fn js_get_string_pointer_unified(value: f64) -> i64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    // Check if it's a properly NaN-boxed string (STRING_TAG = 0x7FFF)
    if jsval.is_string() {
        return jsval.as_string_ptr() as i64;
    }

    // SSO inline value (SHORT_STRING_TAG = 0x7FF9) — caller wants a
    // `*const StringHeader`, so materialize the inline bytes onto the
    // heap. Pre-fix this fell through every branch (SSO bits are NaN
    // so the raw-pointer / number-to-string fallbacks rejected it),
    // returned 0, and any consumer that did
    // `js_string_equals(handle_a, handle_b)` saw "one side is null
    // → not equal" — which is why `JSON.parse(...).foo === "perry"`
    // returned false (SSO === heap string mixed compare). Materialize
    // here defeats the SSO win for the comparison path but is the
    // smallest-blast-radius correctness fix; future codegen sites can
    // avoid the alloc by routing through `js_jsvalue_equals` directly.
    if jsval.is_short_string() {
        return crate::string::js_string_materialize_to_heap(value) as i64;
    }

    // Check if it's a POINTER_TAG (0x7FFD) NaN-boxed pointer (used for cross-module returns)
    if jsval.is_pointer() {
        return (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
    }

    // Raw pointer fallback: only accept values that look like valid heap pointers.
    // Must be non-NaN, non-zero, within 48-bit address space, AND at least 4-byte aligned.
    // The alignment check prevents subnormal f64 numbers like 2.16e-314 (bits=0x1100000003)
    // from being misidentified as pointers.
    if !value.is_nan() && bits != 0 && bits < 0x0001_0000_0000_0000 {
        // Must be at least 4-byte aligned (StringHeader starts with u32 length)
        // and above minimum heap address
        if (bits & 0x3) == 0 && bits >= 0x10000 {
            return bits as i64;
        }
    }

    // For numeric values used as property keys (e.g., obj[pool.id], obj[Direction.Up]),
    // convert the number to a string representation.
    // Note: 0.0 (bits == 0) is a valid number that should produce "0", so we must
    // NOT skip it. The bits != 0 guard above is only for the raw-pointer fallback.
    if !value.is_nan() {
        let s = crate::string::js_number_to_string(value);
        if !s.is_null() {
            return s as i64;
        }
    }

    0
}

/// Check if a NaN-boxed f64 value represents a string.
#[no_mangle]
pub extern "C" fn js_nanbox_is_string(value: f64) -> i32 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_string() {
        1
    } else {
        0
    }
}
