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
            // StrWhiteSpace per spec (NBSP/BOM in, NEL out) — not Rust's
            // `trim_start`, whose White_Space set diverges from JS's.
            let trimmed = s.trim_start_matches(crate::string::is_js_whitespace);
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
    // JS spec: strip leading StrWhiteSpace — the full set (NBSP, VT, LS/PS, BOM,
    // Unicode space separators), not just ASCII. Trim by char so multi-byte
    // whitespace (U+00A0, U+2028, …) is consumed (test262 parseFloat A2_T3/5/8/9),
    // but operate byte-wise so a *trailing* lone surrogate elsewhere in the WTF-8
    // input doesn't poison the whole parse (A6: `parseFloat("0.1e1" + cu)`).
    let bytes = trim_leading_js_whitespace(bytes);
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

/// Strip leading StrWhiteSpace, decoding one UTF-8 scalar at a time so that
/// invalid bytes *after* the leading run (e.g. a lone surrogate in WTF-8 input)
/// don't abort the whole trim. ASCII whitespace is the common case and stays a
/// single-byte check.
fn trim_leading_js_whitespace(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < bytes.len() {
        let b = bytes[start];
        if b < 0x80 {
            if crate::string::is_js_whitespace(b as char) {
                start += 1;
                continue;
            }
            break;
        }
        // Multi-byte lead: decode just the next scalar value from the valid
        // UTF-8 prefix; stop at the first byte that isn't valid UTF-8.
        let rest = &bytes[start..];
        let valid = match std::str::from_utf8(rest) {
            Ok(s) => s,
            Err(e) if e.valid_up_to() > 0 => unsafe {
                std::str::from_utf8_unchecked(&rest[..e.valid_up_to()])
            },
            Err(_) => break,
        };
        match valid.chars().next() {
            Some(c) if crate::string::is_js_whitespace(c) => start += c.len_utf8(),
            _ => break,
        }
    }
    &bytes[start..]
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
/// ECMA-262 ToIntegerOrInfinity (§7.1.5): ToNumber, then map NaN→0, truncate
/// toward zero, preserving ±Infinity. Exposed for codegen so Array index
/// arguments (`fill`/`copyWithin` start/end/target, etc.) fire
/// `valueOf` / `Symbol.toPrimitive` and propagate their throws (and the
/// `Symbol`→TypeError) at the spec-mandated point instead of being silently
/// swallowed by a downstream `is_nan()` shortcut.
pub extern "C" fn js_to_integer_or_infinity(value: f64) -> f64 {
    let n = js_number_coerce(value);
    if n.is_nan() {
        0.0
    } else {
        n.trunc()
    }
}

#[no_mangle]
pub extern "C" fn js_number_coerce(value: f64) -> f64 {
    // #5525 hot fast path: a plain IEEE-754 double (top 16 bits, sign stripped,
    // below the 0x7FF9 Perry NaN-box tag band) is already a Number — ToNumber is
    // identity. Returns before the undefined/null/bool/string/int32/bigint/
    // pointer tag ladder below. bcryptjs's Blowfish core funnels every `any`-
    // typed `S[i]` element read (which `js_dyn_index_get` returns as a plain f64)
    // through this coercion before the Feistel arithmetic, so it was the #1 frame
    // after the typed-array element-access + dynamic-add fast paths landed.
    // Canonical / payload NaNs (top16 0x7FF8) and all finite/inf doubles stay on
    // this path; tagged values (0x7FF9..=0x7FFF) fall through unchanged.
    if (value.to_bits() & 0x7FFF_0000_0000_0000) < 0x7FF9_0000_0000_0000 {
        return value;
    }
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
                    // Rust's f64::from_str accepts `inf`/`infinity`/`INFINITY`
                    // case-insensitively, but ECMAScript StrNumericLiteral only
                    // accepts exactly `Infinity` (optionally signed). Reject any
                    // alphabetic body that isn't exactly `Infinity`.
                    let body = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
                    if body.eq_ignore_ascii_case("inf")
                        || (body.eq_ignore_ascii_case("infinity") && body != "Infinity")
                        || body.eq_ignore_ascii_case("nan")
                    {
                        return f64::NAN;
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
        // ECMA-262 ToNumber(Symbol) is a TypeError (§7.1.4). Symbols are
        // NaN-boxed with POINTER_TAG, so they would otherwise fall through to
        // the object/toPrimitive path below and stringify to "Symbol(...)" or
        // produce NaN. Throw before any of the pointer-shape handling. This
        // covers both explicit `Number(sym)` and the implicit arithmetic
        // ToNumber the codegen routes here (`sym * 2`, `-sym`, etc.). BigInt is
        // intentionally NOT thrown here: `Number(1n)` converts (handled in the
        // is_bigint arm above); the arithmetic-throws-on-BigInt rule lives at
        // the arithmetic call sites (see dynamic_arith.rs both_bigint_or_throw).
        if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
            crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
        }
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
        if crate::value::addr_class::is_small_handle(id as usize)
            && crate::timer::is_known_timer_id(id)
        {
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
        // TypedArray → OrdinaryToPrimitive(number): a *patched own*
        // `valueOf`/`toString` expando (stored in the typed-array own-props
        // side table, invisible to the generic object helpers below) runs
        // first, with `this` = the typed array and abrupt completions
        // propagating (test262 ctors/object-arg/throws-setting-obj-*). With
        // no patch, fall through to the generic path (join + ToNumber).
        if crate::typedarray::lookup_typed_array_kind(id as usize).is_some() {
            if let Some(p) = unsafe {
                crate::typedarray_props::typed_array_own_to_primitive_number(id as usize, value)
            } {
                return js_number_coerce(p);
            }
        }
        // Object → consult [Symbol.toPrimitive]("number") first; if the
        // object has a custom toPrimitive method, recurse with the result.
        let primitive = unsafe { crate::symbol::js_to_primitive(value, 1) };
        if primitive.to_bits() != value.to_bits() {
            // toPrimitive returned something different — re-coerce.
            return js_number_coerce(primitive);
        }
        // No custom [Symbol.toPrimitive]: OrdinaryToPrimitive(O, "number") =
        // try `valueOf` THEN `toString` (ES2024 §7.1.1.1). Previously this
        // jumped straight to stringifying, so `Number({valueOf(){return 42}})`,
        // `+obj`, `obj * 2` etc. wrongly gave NaN. Reuse the shared
        // valueOf-first helper (the same one `+`/addition uses): a primitive
        // result re-coerces; DefaultString/TypeError fall through to the
        // stringify path (so `Number([5])` → "5" → 5, `Number({})` →
        // "[object Object]" → NaN, `Number([])` → "" → 0 still match Node).
        match unsafe { crate::value::ordinary_to_primitive_number_for_add(value) } {
            crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) => {
                if p.to_bits() != value.to_bits() {
                    return js_number_coerce(p);
                }
                return f64::NAN;
            }
            crate::value::OrdinaryToPrimitiveOutcome::TypeError => {
                // ECMA-262 7.1.1 OrdinaryToPrimitive: if both `valueOf` and
                // `toString` return non-primitive objects, ToPrimitive throws a
                // TypeError — `Number(obj)` must propagate it rather than fall
                // through and stringify to NaN (test262 built-ins/Number/
                // S8.12.8_A4.js).
                crate::collection_iter::throw_type_error(
                    "Cannot convert object to primitive value",
                );
            }
            crate::value::OrdinaryToPrimitiveOutcome::DefaultString => {}
        }
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
        // Regular number — ECMAScript NumberToString. #3987: route through the
        // shared `js_format_f64` so `String(1e21)` → "1e+21" / `String(1e-7)`
        // → "1e-7" (scientific notation for |n| >= 1e21 / < 1e-6) instead of
        // Rust's full-decimal `to_string()`, matching `.toString()` and Node.
        crate::string::js_format_f64(value)
    };

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// Spec `ToString` (ECMA-262 §7.1.17) rejects a Symbol with a `TypeError`.
/// The lenient `js_string_coerce` / `js_jsvalue_to_string` paths instead
/// produce a `"Symbol(desc)"` descriptive string — correct for `String(sym)`,
/// `console.log`, etc., but NOT for the abstract ToString operation invoked by
/// string built-ins on their arguments (`"".padEnd(1, sym)`,
/// `"".startsWith(sym)`, `String.raw({raw:[sym]})`, `"".normalize(sym)`). Those
/// call sites invoke this guard *before* coercing so a Symbol argument throws as
/// the spec requires.
#[inline]
pub fn reject_symbol_to_string(value: f64) {
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
    }
}

/// isNaN(value) -> boolean
/// Returns true if value is NaN.
#[no_mangle]
pub extern "C" fn js_is_nan(value: f64) -> f64 {
    // `isNaN(x)` is `Number.isNaN(ToNumber(x))`. Delegate to the canonical
    // ToNumber (`js_number_coerce`), which correctly handles SSO inline short
    // strings (e.g. `isNaN("16")` → false), heap strings, int32/bigint, Date,
    // arrays, and object toPrimitive. The previous hand-rolled coercion used
    // `is_string()` (heap STRING_TAG only), so a 2-char SSO numeric string like
    // a `content-length: "16"` header fell through to the `else` arm and
    // returned the raw NaN-boxed bits → `isNaN("16")` wrongly reported `true`,
    // which made express body-parser's `type-is.hasBody` skip JSON parsing.
    let num = js_number_coerce(value);
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
    // `isFinite(x)` is `Number.isFinite(ToNumber(x))`. Delegate to the canonical
    // ToNumber (`js_number_coerce`) so SSO inline short strings (`isFinite("16")`
    // → true) and all other types coerce correctly; the old `is_string()`-only
    // path mishandled SSO strings (same root cause as `js_is_nan`).
    let num = js_number_coerce(value);
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
