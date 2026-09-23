//! Radix-based number/BigInt stringification: `Number.prototype.toString(radix)`,
//! `BigInt.prototype.toString(radix)`, and the shared `DoubleToRadixCString`
//! formatter.
//!
//! Split out of `to_string.rs`, which sits at the 2000-line cap (#8480 series).

use super::*;

/// ECMAScript `ToNumber` for a radix argument value (NaN-boxed f64). Numbers
/// pass through; strings are parsed with `Number()` semantics (trim + full
/// numeric parse, NOT `parseFloat` prefix parse — `"16px"` → NaN); booleans →
/// 0/1; null → 0; undefined → NaN (signals "use the default radix 10").
/// Returns NaN for anything that does not coerce to a finite number.
unsafe fn radix_arg_to_number(radix_value: f64) -> f64 {
    let jsval = JSValue::from_bits(radix_value.to_bits());
    // ToInteger(radix) → ToNumber(radix): a Symbol or BigInt radix throws a
    // TypeError (must precede the NaN→RangeError path). e.g.
    // `(0n).toString(Symbol())` / `(123).toString(2n)` → TypeError, not RangeError.
    if jsval.is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    if crate::symbol::js_is_symbol(radix_value) != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
    }
    if jsval.is_int32() {
        jsval.as_int32() as f64
    } else if jsval.is_bool() {
        if jsval.as_bool() {
            1.0
        } else {
            0.0
        }
    } else if jsval.is_null() {
        0.0
    } else if jsval.is_undefined() {
        // Signals the "no radix supplied" / default path.
        f64::NAN
    } else if jsval.is_any_string() {
        let s_ptr = js_jsvalue_to_string(radix_value);
        if s_ptr.is_null() {
            return f64::NAN;
        }
        let len = (*s_ptr).byte_len as usize;
        let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        let trimmed = std::str::from_utf8(bytes).unwrap_or("").trim();
        if trimmed.is_empty() {
            // `Number("")` === 0
            0.0
        } else {
            trimmed.parse::<f64>().unwrap_or(f64::NAN)
        }
    } else if jsval.is_number() {
        radix_value
    } else {
        // An object radix runs ToNumber → OrdinaryToPrimitive(number), i.e. its
        // `valueOf`/`toString`. An abrupt completion there must propagate rather
        // than be swallowed into a `RangeError` (test262 Number/prototype/
        // toString/numeric-literal-tostring-radix-poisoned:
        // `0..toString({valueOf(){throw}})` must throw the poison, not a
        // RangeError). `js_number_coerce` performs that coercion (and yields NaN
        // for a non-coercible object, still landing on the RangeError path).
        crate::builtins::js_number_coerce(radix_value)
    }
}

/// Coerce + validate a radix argument per ECMAScript `Number.prototype.toString`
/// (and `BigInt.prototype.toString`). Returns the validated integer radix in
/// `2..=36`, or `None` when the argument was `undefined` (caller uses the
/// default radix 10). Throws (diverges via `js_throw`) with a `RangeError` for
/// any other out-of-range / non-coercible value, matching Node.
pub(crate) unsafe fn coerce_validate_radix(radix_value: f64) -> Option<i32> {
    let n = radix_arg_to_number(radix_value);
    if n.is_nan() {
        // `undefined` → default radix (None); everything else NaN → RangeError.
        if JSValue::from_bits(radix_value.to_bits()).is_undefined() {
            return None;
        }
        throw_radix_range_error();
    }
    // ToInteger: truncate toward zero.
    let r = n.trunc();
    if !(2.0..=36.0).contains(&r) {
        throw_radix_range_error();
    }
    Some(r as i32)
}

fn throw_radix_range_error() -> ! {
    // Node/V8 message verbatim: includes the word "argument" (#3146).
    let message = b"toString() radix argument must be between 2 and 36";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// V8-style `DoubleToRadixCString`: render a finite f64 in
/// `radix` (2..=36) producing the shortest digit sequence that round-trips
/// back to the same double. Mirrors ECMAScript `Number::toString` for
/// non-decimal radices, including the fractional part (`(10.5).toString(2)`
/// === `"1010.1"`). Assumes `radix` is already validated.
fn double_to_radix_string(value: f64, radix: u32) -> String {
    debug_assert!((2..=36).contains(&radix));
    const CHARS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

    let negative = value < 0.0;
    let abs = value.abs();

    // Split into integer and fractional parts.
    let mut integer = abs.floor();
    let mut fraction = abs - integer;

    // `delta` is half the distance to the next representable double, the
    // tolerance used to decide when enough fractional digits have been
    // emitted to uniquely identify `value` (shortest round-trip).
    let mut delta = 0.5 * (next_double(abs) - abs);
    delta = next_double(0.0).max(delta);

    let mut frac_buf = String::new();
    if fraction >= delta {
        frac_buf.push('.');
        loop {
            // Shift up by the radix.
            fraction *= radix as f64;
            delta *= radix as f64;
            // Extract the digit.
            let digit = fraction.floor() as usize;
            frac_buf.push(CHARS[digit] as char);
            fraction -= digit as f64;
            if fraction > 0.5 || (fraction == 0.5 && digit & 1 != 0) {
                // Round up: carry into the already-emitted digits.
                if fraction + delta > 1.0 {
                    // Propagate the carry through fraction digits, possibly
                    // into the integer part.
                    loop {
                        // Pop the last char; if it was '.', carry into integer.
                        let last = frac_buf.pop();
                        match last {
                            None => {
                                integer += 1.0;
                                break;
                            }
                            Some('.') => {
                                frac_buf.push('.');
                                integer += 1.0;
                                break;
                            }
                            Some(c) => {
                                let idx = CHARS.iter().position(|&b| b as char == c).unwrap();
                                if idx + 1 < radix as usize {
                                    frac_buf.push(CHARS[idx + 1] as char);
                                    break;
                                }
                                // Was the max digit (e.g. 'f' in hex): becomes
                                // '0' and the carry continues leftward.
                            }
                        }
                    }
                    break;
                }
            }
            if fraction < delta {
                break;
            }
        }
        // A trailing '.' with no fraction digits (carry consumed all) is junk.
        if frac_buf == "." {
            frac_buf.clear();
        }
    }

    // Integer part: repeated division. `integer` may have grown via carry.
    let mut int_buf = String::new();
    // V8's Double(integer / radix).Exponent() > 0 means the quotient's
    // least significant binary digit is above the units place (>= 2^53).
    // Such radix digits are unrepresented: emit zeros until the quotient
    // fits, retaining the rounded quotient rather than flooring it.
    while integer / radix as f64 >= crate::builtins::INT_EXACT_FASTPATH_LIMIT {
        integer /= radix as f64;
        int_buf.push('0');
    }
    if integer == 0.0 {
        int_buf.push('0');
    } else {
        while integer >= 1.0 {
            let remainder = (integer % radix as f64) as usize;
            int_buf.push(CHARS[remainder] as char);
            // Subtract before dividing: a rounded quotient can otherwise
            // cross an integer boundary and invent a carry above 2^53.
            integer = (integer - remainder as f64) / radix as f64;
        }
    }
    let int_part: String = int_buf.chars().rev().collect();

    let mut result = String::new();
    if negative {
        result.push('-');
    }
    result.push_str(&int_part);
    result.push_str(&frac_buf);
    result
}

/// Smallest representable double strictly greater than `x` (for finite `x`).
fn next_double(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    let bits = x.to_bits();
    let next = if x >= 0.0 {
        bits + 1
    } else if bits == (1u64 << 63) {
        // -0.0 → smallest positive subnormal
        1
    } else {
        bits - 1
    };
    f64::from_bits(next)
}

/// Convert a NaN-boxed f64 value to a string with the given radix argument.
/// `radix_value` is the *raw* NaN-boxed radix argument (number/string/bool/
/// undefined); it is ToNumber/ToInteger-coerced and validated to `2..=36`
/// here, throwing `RangeError` for out-of-range values (#2864). Handles
/// BigInt (uses bigint_to_string_radix), numbers, strings, etc.
#[no_mangle]
pub extern "C" fn js_jsvalue_to_string_radix(
    value: f64,
    radix_value: f64,
) -> *mut crate::string::StringHeader {
    let jsval = JSValue::from_bits(value.to_bits());

    // A Temporal value's `toString` takes an *options object*, not a radix —
    // the codegen routes any single-arg `.toString(x)` here. Dispatch back to
    // the Temporal method router so the options bag flows through, instead of
    // ToNumber-coercing it as a radix (which throws a spurious RangeError).
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(value) {
        let result = crate::temporal::dispatch::call_method(value, "toString", &[radix_value]);
        let rv = JSValue::from_bits(result.to_bits());
        if rv.is_string() {
            return rv.as_string_ptr() as *mut crate::string::StringHeader;
        }
        return js_jsvalue_to_string(result);
    }

    // Numeric receivers (Number / BigInt / Int32 / boxed Number): the second
    // argument is a radix — coerce + validate it (throws on out-of-range). Other
    // object receivers (Date, user `toString(opts)` methods) reach this with a
    // non-radix argument, so we lazily validate the radix only on the numeric
    // arms and otherwise dispatch the receiver's own `toString` with the
    // argument forwarded — never ToNumber-coercing an options object as a radix.
    macro_rules! radix {
        () => {
            match unsafe { coerce_validate_radix(radix_value) } {
                Some(r) => r,
                None => 10,
            }
        };
    }

    if jsval.is_bigint() {
        let ptr = jsval.as_bigint_ptr();
        crate::bigint::js_bigint_to_string_radix(ptr, radix!())
    } else if jsval.is_string() {
        jsval.as_string_ptr() as *mut crate::string::StringHeader
    } else if jsval.is_int32() {
        let radix = radix!();
        let n = jsval.as_int32();
        if radix == 10 {
            let s = n.to_string();
            return crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        }
        let s = double_to_radix_string(n as f64, radix as u32);
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    } else if jsval.is_number() {
        number_to_radix_string(value, radix!())
    } else {
        // Pointer / object receiver. `Number.prototype.toString` brand
        // semantics (ECMA-262 21.1.3): a boxed `Number` exposes its
        // [[NumberData]]; `Number.prototype` itself has [[NumberData]] +0;
        // any other object has no number value and dispatches its own
        // `toString` with the argument forwarded (so a Temporal/Date receiver
        // honours its options bag instead of treating it as a radix).
        const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
        if let Some((cid, payload)) = crate::builtins::boxed_primitive_payload(value) {
            if cid == CLASS_ID_BOXED_NUMBER {
                return number_to_radix_string(payload, radix!());
            }
        }
        if value.to_bits() == crate::object::builtin_prototype_value("Number").to_bits() {
            return number_to_radix_string(0.0, radix!());
        }
        // Forward the argument to the receiver's own `toString`. A Temporal
        // value routes to its options-aware `toString`; a plain object falls
        // back to `Object.prototype.toString` ([object Object]).
        if jsval.is_pointer() {
            let args = [radix_value];
            let result = unsafe {
                crate::object::js_native_call_method(
                    value,
                    b"toString".as_ptr() as *const i8,
                    8,
                    args.as_ptr(),
                    1,
                )
            };
            let rjv = JSValue::from_bits(result.to_bits());
            if rjv.is_string() {
                return rjv.as_string_ptr() as *mut crate::string::StringHeader;
            }
            if rjv.is_short_string() {
                return crate::string::js_string_materialize_to_heap(result);
            }
        }
        js_jsvalue_to_string(value)
    }
}

/// Format a real f64 `n` in the given `radix` (2..=36), matching
/// `Number.prototype.toString`'s NaN/Infinity/decimal handling.
fn number_to_radix_string(n: f64, radix: i32) -> *mut crate::string::StringHeader {
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
    if radix == 10 {
        return crate::string::js_number_to_string(n);
    }
    let s = double_to_radix_string(n, radix as u32);
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

#[cfg(test)]
mod radix_tostring_tests {
    use super::*;

    #[test]
    fn integer_radix_formatting() {
        assert_eq!(double_to_radix_string(255.0, 16), "ff");
        assert_eq!(double_to_radix_string(10.0, 2), "1010");
        assert_eq!(double_to_radix_string(255.0, 2), "11111111");
        assert_eq!(double_to_radix_string(-255.0, 16), "-ff");
        assert_eq!(double_to_radix_string(0.0, 2), "0");
        assert_eq!(double_to_radix_string(35.0, 36), "z");
    }

    #[test]
    fn large_integer_radix_formatting_matches_node() {
        // Node 26.5.1: preserve represented digits at the 2^53 boundary,
        // then zero-fill digits beyond the double's precision (#9725).
        for (value, radix, expected) in [
            (255.0, 36, "73"),
            (1e15, 36, "9ugxnorjls"),
            (9_007_199_254_740_991.0, 36, "2gosa7pa2gv"),
            (9_007_199_254_740_992.0, 36, "2gosa7pa2gw"),
            (9_007_199_254_740_994.0, 36, "2gosa7pa2gy"),
            (
                9_007_199_254_740_994.0,
                3,
                "1121202011211211122211100012101111",
            ),
            (1e21, 36, "5v1j4f4ds7c000"),
            (1e21, 7, "5135235413265003022600000"),
            (1e30, 36, "2oy99wnkl1a000000000"),
            (1e30, 7, "243230604464041356220000000000000000"),
            (1e21, 16, "3635c9adc5dea00000"),
            (1e30, 16, "c9f2c9cd04675000000000000"),
        ] {
            assert_eq!(double_to_radix_string(value, radix), expected);
            assert_eq!(
                double_to_radix_string(-value, radix),
                format!("-{expected}")
            );
        }
        assert_eq!(
            double_to_radix_string(f64::MAX, 36),
            format!("1a1e4vngaiqo{}", "0".repeat(187))
        );
    }

    #[test]
    fn fractional_radix_formatting_matches_v8() {
        // Terminating fractions.
        assert_eq!(double_to_radix_string(10.5, 2), "1010.1");
        assert_eq!(double_to_radix_string(10.5, 16), "a.8");
        assert_eq!(double_to_radix_string(10.5, 36), "a.i");
        assert_eq!(double_to_radix_string(-10.5, 2), "-1010.1");
        assert_eq!(double_to_radix_string(255.5, 16), "ff.8");
        assert_eq!(double_to_radix_string(1.5, 2), "1.1");
        assert_eq!(double_to_radix_string(100.25, 2), "1100100.01");
        // Repeating fraction — shortest round-trip (matches Node v25).
        assert_eq!(
            double_to_radix_string(0.1, 2),
            "0.0001100110011001100110011001100110011001100110011001101"
        );
        // Rounding is still needed when the residual is within delta.
        assert_eq!(double_to_radix_string(0.1, 36), "0.3lllllllllm");
        assert_eq!(double_to_radix_string(10.5, 7), "13.333333333333333334");
    }

    #[test]
    fn coerce_validate_radix_semantics() {
        unsafe {
            // undefined → None (default radix path).
            assert_eq!(
                coerce_validate_radix(f64::from_bits(crate::value::TAG_UNDEFINED)),
                None
            );
            // Plain number radices.
            assert_eq!(coerce_validate_radix(16.0), Some(16));
            assert_eq!(coerce_validate_radix(2.0), Some(2));
            assert_eq!(coerce_validate_radix(36.0), Some(36));
            // ToInteger truncation.
            assert_eq!(coerce_validate_radix(2.9), Some(2));
            // int32-boxed radix.
            assert_eq!(
                coerce_validate_radix(f64::from_bits(JSValue::int32(16).bits())),
                Some(16)
            );
            // String radix coerces via ToNumber.
            let s = crate::string::js_string_from_bytes(b"16".as_ptr(), 2);
            assert_eq!(
                coerce_validate_radix(crate::value::js_nanbox_string(s as i64)),
                Some(16)
            );
        }
    }
}
