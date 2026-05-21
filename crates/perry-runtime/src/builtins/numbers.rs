//! Number / coercion / parsing built-ins.
//!
//! Split out of the original monolithic `builtins.rs` (#topic: split-large-files).
//! Houses `parseInt` / `parseFloat`, `Number(...)` / `String(...)` coercions,
//! `isNaN` / `isFinite`, and the strict `Number.isNaN` / `isFinite` / `isInteger` /
//! `isSafeInteger` family.

use super::*;

/// parseInt(string, radix?) -> number
/// Parses a string and returns an integer.
/// If the string cannot be parsed, returns NaN.
#[no_mangle]
pub extern "C" fn js_parse_int(str_ptr: *const StringHeader, radix: f64) -> f64 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return f64::NAN;
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);

        if let Ok(s) = std::str::from_utf8(bytes) {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return f64::NAN;
            }

            // Determine radix
            let radix = if radix.is_nan() || radix == 0.0 {
                10
            } else {
                radix as u32
            };

            // Handle sign
            let (is_negative, trimmed) = if trimmed.starts_with('-') {
                (true, &trimmed[1..])
            } else if trimmed.starts_with('+') {
                (false, &trimmed[1..])
            } else {
                (false, trimmed)
            };

            // Handle hex prefix (only if radix is 16 or auto)
            let (actual_radix, trimmed) = if (radix == 16 || radix == 10)
                && (trimmed.starts_with("0x") || trimmed.starts_with("0X"))
            {
                (16, &trimmed[2..])
            } else {
                (radix, trimmed)
            };

            // Parse characters until we hit a non-digit
            let valid_chars: String = trimmed
                .chars()
                .take_while(|c| c.is_digit(actual_radix))
                .collect();

            if valid_chars.is_empty() {
                return f64::NAN;
            }

            match i64::from_str_radix(&valid_chars, actual_radix) {
                Ok(n) => {
                    let result = if is_negative { -n } else { n };
                    result as f64
                }
                Err(_) => f64::NAN,
            }
        } else {
            f64::NAN
        }
    }
}

/// parseFloat(string) -> number
/// Parses a string and returns a floating-point number.
#[no_mangle]
pub extern "C" fn js_parse_float(str_ptr: *const StringHeader) -> f64 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return f64::NAN;
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        parse_float_bytes(bytes)
    }
}

/// Core parseFloat logic operating on raw bytes — no heap allocation.
/// Exposed as `pub(crate)` so unit tests can call it directly.
pub(crate) fn parse_float_bytes(bytes: &[u8]) -> f64 {
    // JS spec: strip leading StrWhiteSpace (ASCII subset covers all common cases)
    let bytes = bytes.trim_ascii_start();
    if bytes.is_empty() {
        return f64::NAN;
    }

    // Detect optional sign, then check for "Infinity"
    let (neg, rest) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        Some(b'+') => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    if rest.starts_with(b"Infinity") {
        return if neg {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }

    // Scan for the longest valid StrDecimalLiteral prefix — zero allocations.
    let end = float_prefix_end(bytes);
    if end == 0 {
        return f64::NAN;
    }

    // bytes[..end] contains only ASCII chars (digits, sign, '.', 'e'/'E'), so
    // from_utf8_unchecked is safe.
    let s = unsafe { std::str::from_utf8_unchecked(&bytes[..end]) };
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// Returns the byte length of the leading StrDecimalLiteral prefix in `bytes`.
/// Returns 0 when no valid prefix exists (e.g. `"abc"`, `"."`, `"+"`).
fn float_prefix_end(bytes: &[u8]) -> usize {
    let mut i = 0;
    let n = bytes.len();

    // Optional sign
    if i < n && (bytes[i] == b'-' || bytes[i] == b'+') {
        i += 1;
    }

    // Integer digits
    let int_start = i;
    while i < n && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let has_int = i > int_start;

    // Optional fractional part
    let mut has_frac = false;
    if i < n && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < n && bytes[i].is_ascii_digit() {
            i += 1;
        }
        has_frac = i > frac_start;
    }

    // Need at least one digit on either side of the (optional) decimal point
    if !has_int && !has_frac {
        return 0;
    }

    // Optional exponent — only consumed when at least one exponent digit follows
    if i < n && (bytes[i] == b'e' || bytes[i] == b'E') {
        let exp_start = i;
        i += 1;
        if i < n && (bytes[i] == b'-' || bytes[i] == b'+') {
            i += 1;
        }
        let exp_digit_start = i;
        while i < n && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_digit_start {
            i = exp_start; // backtrack: bare 'e' or 'e±' with no digits
        }
    }

    i
}

#[cfg(test)]
mod parse_float_tests {
    use super::parse_float_bytes;

    fn pf(s: &str) -> f64 {
        parse_float_bytes(s.as_bytes())
    }

    #[test]
    fn well_formed_inputs() {
        assert_eq!(pf("3.14"), 3.14_f64);
        assert_eq!(pf("1e10"), 1e10_f64);
        assert_eq!(pf("-0.5"), -0.5_f64);
        assert_eq!(pf("1234567890.12345"), 1234567890.12345_f64);
        assert_eq!(pf("0"), 0.0_f64);
        assert_eq!(pf("42"), 42.0_f64);
        assert_eq!(pf(".5"), 0.5_f64);
        assert_eq!(pf("5."), 5.0_f64);
        assert_eq!(pf("+3.14"), 3.14_f64);
    }

    #[test]
    fn leading_whitespace() {
        assert_eq!(pf("  3.14"), 3.14_f64);
        assert_eq!(pf("\t3.14"), 3.14_f64);
        assert_eq!(pf("\n3.14"), 3.14_f64);
    }

    #[test]
    fn trailing_junk() {
        assert_eq!(pf("3.14abc"), 3.14_f64);
        assert_eq!(pf("1e10xyz"), 1e10_f64);
        assert_eq!(pf("42 extra"), 42.0_f64);
        // bare 'e' with no exponent digits — stop before 'e'
        assert_eq!(pf("1e"), 1.0_f64);
        assert_eq!(pf("1e+"), 1.0_f64);
    }

    #[test]
    fn invalid_inputs_return_nan() {
        assert!(pf("abc").is_nan());
        assert!(pf("").is_nan());
        assert!(pf("   ").is_nan());
        assert!(pf(".").is_nan());
        assert!(pf("+").is_nan());
        assert!(pf("-").is_nan());
    }

    #[test]
    fn infinity_variants() {
        assert_eq!(pf("Infinity"), f64::INFINITY);
        assert_eq!(pf("-Infinity"), f64::NEG_INFINITY);
        assert_eq!(pf("+Infinity"), f64::INFINITY);
        assert_eq!(pf("  Infinity"), f64::INFINITY);
    }
}

/// Number(value) -> number
/// Converts a value to a number.
///
/// Marked `#[inline]` so the bitcode-link path can inline + DCE the
/// branches when the input type is statically known.
#[no_mangle]
pub extern "C" fn js_number_coerce(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            1.0
        } else {
            0.0
        }
    } else if jsval.is_any_string() {
        // Parse string as number. Accepts both STRING_TAG heap
        // pointers and SHORT_STRING_TAG inline SSO values
        // (v0.5.216). Decode via `str_bytes_from_jsvalue` into a
        // stack scratch buffer for SSO; heap strings get a direct
        // view over the StringHeader payload.
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let view = crate::string::str_bytes_from_jsvalue(value, &mut scratch);
        if let Some((data, len)) = view {
            if data.is_null() && len == 0 {
                return 0.0;
            }
            unsafe {
                let bytes = std::slice::from_raw_parts(data, len as usize);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        return 0.0;
                    }
                    if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                        return match u64::from_str_radix(&trimmed[2..], 16) {
                            Ok(n) => n as f64,
                            Err(_) => f64::NAN,
                        };
                    }
                    if trimmed.starts_with("-0x") || trimmed.starts_with("-0X") {
                        return match u64::from_str_radix(&trimmed[3..], 16) {
                            Ok(n) => -(n as f64),
                            Err(_) => f64::NAN,
                        };
                    }
                    trimmed.parse::<f64>().unwrap_or(f64::NAN)
                } else {
                    f64::NAN
                }
            }
        } else {
            f64::NAN
        }
    } else if jsval.is_int32() {
        // INT32 NaN-boxed value → convert to f64
        jsval.as_int32() as f64
    } else if jsval.is_bigint() {
        // BigInt → number conversion
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_f64(ptr)
    } else if jsval.is_pointer() {
        let id = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
        // Timer handles coerce numerically to their internal id (matches
        // Node's `+timeout` shape — Node returns `_idleTimeout`, Perry
        // returns the handle id; both are numbers and both are stable
        // identifiers, so test assertions like `typeof x === "number"`
        // hold). Gate on the timer registry so unrelated small handles
        // (UI widgets, drizzle, etc.) still fall through to toPrimitive.
        if id > 0 && id < 0x100000 && crate::timer::is_known_timer_id(id) {
            return id as f64;
        }
        // Object → consult [Symbol.toPrimitive]("number") first; if the
        // object has a custom toPrimitive method, recurse with the result.
        // Otherwise returns NaN.
        let primitive = unsafe { crate::symbol::js_to_primitive(value, 1) };
        if primitive.to_bits() != value.to_bits() {
            // toPrimitive returned something different — re-coerce.
            return js_number_coerce(primitive);
        }
        f64::NAN
    } else {
        // Already a number
        value
    }
}

/// String(value) -> string
/// Converts a value to a string.
#[no_mangle]
pub extern "C" fn js_string_coerce(value: f64) -> *mut StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());

    let result = if jsval.is_undefined() {
        "undefined".to_string()
    } else if jsval.is_null() {
        "null".to_string()
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            "true".to_string()
        } else {
            "false".to_string()
        }
    } else if jsval.is_string() {
        // Already a heap string, return as-is
        return jsval.as_string_ptr() as *mut StringHeader;
    } else if jsval.is_short_string() {
        // SSO inline value — caller wants a `*mut StringHeader`, so
        // materialize the inline bytes onto the heap. Defeats the SSO
        // win for this value but preserves correctness on coercion
        // paths (`String(x)`, `'' + x` via the runtime fallback, etc.)
        // that pass the result downstream as a heap pointer.
        return crate::string::js_string_materialize_to_heap(value);
    } else if jsval.is_bigint() {
        let ptr = jsval.as_bigint_ptr();
        if ptr.is_null() {
            "0".to_string()
        } else {
            let str_ptr = crate::bigint::js_bigint_to_string(ptr);
            return str_ptr as *mut StringHeader;
        }
    } else if jsval.is_pointer() {
        // Pointer type — could be array or object.
        // Delegate to js_jsvalue_to_string which handles arrays via join(",") and objects.
        return crate::value::js_jsvalue_to_string(value);
    } else if jsval.is_int32() {
        jsval.as_int32().to_string()
    } else {
        // Regular number
        let n = value;
        if n.is_nan() {
            "NaN".to_string()
        } else if n.is_infinite() {
            if n > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            }
        } else if n == 0.0 {
            "0".to_string()
        } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            (n as i64).to_string()
        } else {
            n.to_string()
        }
    };

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// isNaN(value) -> boolean
/// Returns true if value is NaN.
#[no_mangle]
pub extern "C" fn js_is_nan(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    // isNaN first coerces to number, then checks for NaN
    let num = if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            1.0
        } else {
            0.0
        }
    } else if jsval.is_string() {
        // Parse string as number
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        0.0
                    } else {
                        trimmed.parse::<f64>().unwrap_or(f64::NAN)
                    }
                } else {
                    f64::NAN
                }
            }
        }
    } else {
        value
    };

    // Return NaN-boxed boolean (TAG_TRUE / TAG_FALSE)
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if num.is_nan() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// isFinite(value) -> boolean
/// Returns true if value is a finite number.
#[no_mangle]
pub extern "C" fn js_is_finite(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());

    // isFinite first coerces to number, then checks for finite
    let num = if jsval.is_undefined() {
        f64::NAN
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            1.0
        } else {
            0.0
        }
    } else if jsval.is_string() {
        // Parse string as number
        let ptr = jsval.as_string_ptr();
        if ptr.is_null() {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        0.0
                    } else {
                        trimmed.parse::<f64>().unwrap_or(f64::NAN)
                    }
                } else {
                    f64::NAN
                }
            }
        }
    } else {
        value
    };

    // Return NaN-boxed boolean (TAG_TRUE / TAG_FALSE)
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if num.is_finite() {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

const NB_TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const NB_TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

/// Number.isNaN(value) -> boolean (strict, no coercion)
/// Returns true only if value is a plain number AND that number is NaN.
#[no_mangle]
pub extern "C" fn js_number_is_nan(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    // Strict: only plain numbers can be NaN. Any NaN-boxed tag type => false.
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_nan() {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isFinite(value) -> boolean (strict, no coercion)
/// Returns true only if value is a plain finite number.
#[no_mangle]
pub extern "C" fn js_number_is_finite(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_finite() {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isInteger(value) -> boolean
/// Returns true if value is a finite number with no fractional part.
#[no_mangle]
pub extern "C" fn js_number_is_integer(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    if n.is_finite() && n.floor() == n {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}

/// Number.isSafeInteger(value) -> boolean
/// Returns true if value is an integer within ±(2^53 - 1).
#[no_mangle]
pub extern "C" fn js_number_is_safe_integer(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_number() {
        return f64::from_bits(NB_TAG_FALSE);
    }
    let n = jsval.as_number();
    const MAX_SAFE: f64 = 9007199254740991.0;
    if n.is_finite() && n.floor() == n && n.abs() <= MAX_SAFE {
        f64::from_bits(NB_TAG_TRUE)
    } else {
        f64::from_bits(NB_TAG_FALSE)
    }
}
