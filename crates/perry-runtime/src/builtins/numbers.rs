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
            let trimmed = s.trim_start();
            if trimmed.is_empty() {
                return f64::NAN;
            }

            let radix = parse_int_to_int32(js_number_coerce(radix));

            // Handle sign
            let (is_negative, trimmed) = if trimmed.starts_with('-') {
                (true, &trimmed[1..])
            } else if trimmed.starts_with('+') {
                (false, &trimmed[1..])
            } else {
                (false, trimmed)
            };

            let (actual_radix, trimmed) = if radix == 0 {
                if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                    (16_u32, &trimmed[2..])
                } else {
                    (10_u32, trimmed)
                }
            } else {
                if !(2..=36).contains(&radix) {
                    return f64::NAN;
                }
                let actual_radix = radix as u32;
                if actual_radix == 16 && (trimmed.starts_with("0x") || trimmed.starts_with("0X")) {
                    (16_u32, &trimmed[2..])
                } else {
                    (actual_radix, trimmed)
                }
            };

            let mut value = 0.0;
            let mut saw_digit = false;
            for &byte in trimmed.as_bytes() {
                let Some(digit) = parse_int_digit(byte) else {
                    break;
                };
                if digit >= actual_radix {
                    break;
                }
                saw_digit = true;
                value = value * actual_radix as f64 + digit as f64;
            }

            if !saw_digit {
                return f64::NAN;
            }

            if is_negative {
                -value
            } else {
                value
            }
        } else {
            f64::NAN
        }
    }
}

fn parse_int_to_int32(number: f64) -> i32 {
    if !number.is_finite() || number == 0.0 {
        return 0;
    }
    let two32 = 4_294_967_296.0;
    let int = number.trunc().rem_euclid(two32);
    if int >= 2_147_483_648.0 {
        (int - two32) as i32
    } else {
        int as i32
    }
}

fn parse_int_digit(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u32),
        b'a'..=b'z' => Some((byte - b'a') as u32 + 10),
        b'A'..=b'Z' => Some((byte - b'A') as u32 + 10),
        _ => None,
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
    fn number_coerce_handles_nondecimal_integer_literals() {
        fn nc(s: &str) -> f64 {
            let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            super::js_number_coerce(crate::value::js_nanbox_string(ptr as i64))
        }
        // 0x / 0o / 0b, case-insensitive, with/without surrounding whitespace.
        assert_eq!(nc("0xff"), 255.0);
        assert_eq!(nc("0o17"), 15.0);
        assert_eq!(nc("0b11"), 3.0);
        assert_eq!(nc("0XaB"), 171.0);
        assert_eq!(nc("  0b1010  "), 10.0);
        // No leading sign allowed on a NonDecimalIntegerLiteral → NaN.
        assert!(nc("-0xff").is_nan());
        assert!(nc("+0b11").is_nan());
        // Empty / out-of-radix digits → NaN.
        assert!(nc("0b").is_nan());
        assert!(nc("0b12").is_nan());
        // Plain decimals still parse.
        assert_eq!(nc("42"), 42.0);
        assert_eq!(nc("-3.5"), -3.5);
    }

    #[test]
    fn number_coerce_arrays_via_tostring() {
        // Number(array) = ToNumber(array.join(",")): [] -> "" -> 0,
        // [5] -> "5" -> 5, [1,2] -> "1,2" -> NaN.
        fn nc_arr(vals: &[f64]) -> f64 {
            let arr = crate::array::js_array_alloc(vals.len().max(1) as u32);
            for &v in vals {
                crate::array::js_array_push_f64(arr, v);
            }
            let boxed = crate::value::js_nanbox_pointer(arr as i64);
            super::js_number_coerce(boxed)
        }
        assert_eq!(nc_arr(&[]), 0.0);
        assert_eq!(nc_arr(&[5.0]), 5.0);
        assert_eq!(nc_arr(&[42.0]), 42.0);
        assert!(nc_arr(&[1.0, 2.0]).is_nan()); // "1,2"
        assert_eq!(nc_arr(&[0.0]), 0.0);
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
                    // Non-decimal integer literals (ECMA-262 StrNumericLiteral
                    // → NonDecimalIntegerLiteral): `0x`/`0o`/`0b`,
                    // case-insensitive, with NO leading sign. A signed form
                    // like "-0xff" is not a NonDecimalIntegerLiteral and is
                    // not a StrDecimalLiteral either, so it must parse to NaN
                    // (Node agrees) — we therefore do NOT special-case "-0x".
                    let radix = match trimmed.as_bytes() {
                        [b'0', b'x' | b'X', ..] => Some(16),
                        [b'0', b'o' | b'O', ..] => Some(8),
                        [b'0', b'b' | b'B', ..] => Some(2),
                        _ => None,
                    };
                    if let Some(radix) = radix {
                        // Empty digits ("0x") or out-of-radix digits ("0b12")
                        // are errors → NaN, matching Node.
                        return match u64::from_str_radix(&trimmed[2..], radix) {
                            Ok(n) => n as f64,
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
        // #2089: a Date is a NaN-boxed pointer to a `DateCell`. `Number(date)`
        // / `+date` / `date - other` coerce to the millisecond timestamp.
        if crate::date::is_date_cell_addr(id as usize) {
            return crate::date::date_cell_timestamp(value);
        }
        // Timer handles coerce numerically to their internal id (matches
        // Node's `+timeout` shape — Node returns `_idleTimeout`, Perry
        // returns the handle id; both are numbers and both are stable
        // identifiers, so test assertions like `typeof x === "number"`
        // hold). Gate on the timer registry so unrelated small handles
        // (UI widgets, drizzle, etc.) still fall through to toPrimitive.
        if id > 0 && id < 0x100000 && crate::timer::is_known_timer_id(id) {
            return id as f64;
        }
        // Array → ToPrimitive(number) finds no `valueOf` override, so it
        // falls to `Array.prototype.toString` = `join(",")`, then ToNumber on
        // that string: `Number([]) === 0`, `Number([5]) === 5`,
        // `Number([1,2]) === NaN` ("1,2"). `js_to_primitive` doesn't apply
        // this, so handle arrays explicitly before the generic path. #2378.
        const TAG_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
        if crate::array::js_array_is_array(value).to_bits() == TAG_TRUE_BITS {
            let arr_ptr = jsval.as_pointer::<crate::array::ArrayHeader>();
            let comma = crate::string::js_string_from_bytes(b",".as_ptr(), 1);
            let joined = unsafe { crate::array::js_array_join(arr_ptr, comma) };
            return js_number_coerce(crate::value::js_nanbox_string(joined as i64));
        }
        // Object → consult [Symbol.toPrimitive]("number") first; if the
        // object has a custom toPrimitive method, recurse with the result.
        let primitive = unsafe { crate::symbol::js_to_primitive(value, 1) };
        if primitive.to_bits() != value.to_bits() {
            // toPrimitive returned something different — re-coerce.
            return js_number_coerce(primitive);
        }
        // No custom [Symbol.toPrimitive]: complete OrdinaryToPrimitive by
        // stringifying the object and re-running ToNumber on the result
        // (#2378). `js_jsvalue_to_string` handles arrays via join(",") and
        // objects via their own/inherited toString, so `Number([])` → "" → 0,
        // `Number([5])` → "5" → 5, and `Number({})` → "[object Object]" → NaN
        // all match Node. The result is a string, so the recursive call lands
        // in the string branch — no infinite pointer-branch recursion.
        let str_ptr = crate::value::js_jsvalue_to_string(value);
        if str_ptr.is_null() {
            return f64::NAN;
        }
        js_number_coerce(crate::value::js_nanbox_string(str_ptr as i64))
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
