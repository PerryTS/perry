//! BigInt constructors, coercion, string parsing, and outward conversions.

use super::*;

/// Build a 1024-bit two's-complement limb array from a finite-integer f64.
/// Node converts any finite integer Number to a BigInt of the same value,
/// not just those that fit in i64. Caller must have already verified the
/// value is finite and has no fractional part.
fn limbs_from_integer_f64(value: f64) -> [u64; BIGINT_LIMBS] {
    if value == 0.0 {
        return ZERO_LIMBS;
    }
    let negative = value < 0.0;
    let mut mag = value.abs();
    // Decompose the magnitude into base-2^64 limbs, low limb first.
    let mut limbs = ZERO_LIMBS;
    let two_pow_64 = 18446744073709551616.0f64; // 2^64
    let mut i = 0;
    while mag >= 1.0 && i < BIGINT_LIMBS {
        let limb = mag % two_pow_64;
        limbs[i] = limb as u64;
        mag = (mag / two_pow_64).floor();
        i += 1;
    }
    // Overflow (#6073): either the magnitude needed more than 16 limbs (`mag`
    // is still >= 1 after the loop) or it landed in [2^1023, 2^1024) with a
    // sign that cannot represent it. `BigInt(1e309)` and friends throw instead
    // of silently truncating to the low 1024 bits.
    if mag >= 1.0 || !magnitude_fits_1024(&limbs, negative) {
        throw_bigint_overflow();
    }
    if negative {
        negate_limbs(&limbs)
    } else {
        limbs
    }
}

/// Create a BigInt from a u64 value
#[no_mangle]
pub extern "C" fn js_bigint_from_u64(value: u64) -> *mut BigIntHeader {
    let mut limbs = ZERO_LIMBS;
    limbs[0] = value;
    bigint_alloc_with_limbs(limbs)
}

/// Create a BigInt from a signed i64 value
#[no_mangle]
pub extern "C" fn js_bigint_from_i64(value: i64) -> *mut BigIntHeader {
    let fill = if value < 0 { u64::MAX } else { 0u64 };
    let mut limbs = [fill; BIGINT_LIMBS];
    limbs[0] = value as u64;
    bigint_alloc_with_limbs(limbs)
}

/// Create a BigInt from a compiler-owned signed 128-bit temporary, passed as
/// raw low/high 64-bit words so generated LLVM can keep small BigInt literal
/// arithmetic native until the JS-visible BigInt object boundary.
#[no_mangle]
pub extern "C" fn js_bigint_from_i128_parts(lo: u64, hi: i64) -> *mut BigIntHeader {
    let bits = ((hi as u64 as u128) << 64) | (lo as u128);
    let value = bits as i128;
    let mut limbs = ZERO_LIMBS;
    write_i128(value, &mut limbs);
    bigint_alloc_with_limbs(limbs)
}

#[used]
static KEEP_JS_BIGINT_FROM_I128_PARTS: extern "C" fn(u64, i64) -> *mut BigIntHeader =
    js_bigint_from_i128_parts;

/// Create a BigInt from a JS value (the `BigInt(value)` coercion).
///
/// Matches Node/ECMAScript `ToBigInt` semantics (#2754, #2907):
///   - `undefined` / `null`  → `TypeError`
///   - `true` / `false`      → `1n` / `0n`
///   - existing BigInt       → pass-through
///   - Number (incl. int32)  → must be a finite integer, else `RangeError`;
///                             the full integer value is preserved (not
///                             truncated/saturated to i64)
///   - string                → parsed; invalid syntax → `SyntaxError`
///
/// The argument arrives NaN-boxed, so a real Number is a plain f64 while
/// booleans/null/undefined/strings/bigints carry Perry tag bits.
#[no_mangle]
pub extern "C" fn js_bigint_from_f64(value: f64) -> *mut BigIntHeader {
    use crate::value::JSValue;
    let jsval = JSValue::from_bits(value.to_bits());

    // If already a BigInt (NaN-boxed), just return the pointer
    if jsval.is_bigint() {
        return jsval.as_bigint_ptr() as *mut BigIntHeader;
    }

    // Boolean: BigInt(true) === 1n, BigInt(false) === 0n.
    if jsval.is_bool() {
        return js_bigint_from_i64(if jsval.as_bool() { 1 } else { 0 });
    }

    // If it's an INT32 (NaN-boxed i32), extract and convert
    if jsval.is_int32() {
        let int_value = jsval.as_int32() as i64;
        return js_bigint_from_i64(int_value);
    }

    // If it's a string, parse as BigInt (e.g., BigInt("1000000")).
    // #1781: accept inline SSO short strings too — `BigInt("123")` is a
    // 3-byte SSO value that `is_string()` (STRING_TAG-only) would reject,
    // dropping it to the `value as i64` fallback (NaN → 0n). Route through
    // the unified decoder, which materializes SSO bytes onto the heap.
    if jsval.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value)
            as *const crate::string::StringHeader;
        if !ptr.is_null() {
            unsafe {
                let len = (*ptr).byte_len;
                let data =
                    (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let result = js_bigint_from_string(data, len);
                return result;
            }
        }
        // Empty / unmaterializable string → 0n, matching `BigInt("")`.
        return js_bigint_from_i64(0);
    }

    // undefined / null are not convertible — Node throws a TypeError.
    if jsval.is_undefined() {
        throw_bigint_type_error("Cannot convert undefined to a BigInt");
    }
    if jsval.is_null() {
        throw_bigint_type_error("Cannot convert null to a BigInt");
    }

    // Object / Symbol pointer. ECMAScript ToBigInt step 1 is
    // `ToPrimitive(value, number)`, so `valueOf` / `toString` /
    // `@@toPrimitive` must run (and propagate their exceptions) *before* the
    // integer check — `BigInt({valueOf(){throw}})` rethrows, and
    // `BigInt({valueOf(){return 2n}})` is 2n. Previously a non-string,
    // non-bigint pointer fell through to the Number branch below, where its
    // NaN-boxed bits read as NaN and threw a (premature) RangeError. A Symbol
    // has no primitive conversion → TypeError. Mirrors `js_number_coerce`.
    if jsval.is_pointer() {
        let ptr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
        if crate::symbol::is_registered_symbol(ptr) {
            throw_bigint_type_error("Cannot convert a Symbol value to a BigInt");
        }
        // `@@toPrimitive("number")` first.
        let primitive = unsafe { crate::symbol::js_to_primitive(value, 1) };
        if primitive.to_bits() != value.to_bits() {
            return js_bigint_from_f64(primitive);
        }
        // OrdinaryToPrimitive(O, "number"): valueOf then toString.
        match unsafe { crate::value::ordinary_to_primitive_number_for_add(value) } {
            crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) => {
                if p.to_bits() != value.to_bits() {
                    return js_bigint_from_f64(p);
                }
            }
            crate::value::OrdinaryToPrimitiveOutcome::TypeError
            | crate::value::OrdinaryToPrimitiveOutcome::DefaultString => {}
        }
        // Fall back to string coercion (e.g. an array → join → parse:
        // `BigInt([5])` === 5n, `BigInt([])` === 0n).
        let str_ptr = crate::value::js_jsvalue_to_string(value);
        if !str_ptr.is_null() {
            return js_bigint_from_f64(crate::value::js_nanbox_string(str_ptr as i64));
        }
    }

    // Remaining case: a real Number. Node only converts finite integers;
    // NaN, ±Infinity, and any value with a fractional part throw RangeError.
    if !value.is_finite() || value.fract() != 0.0 {
        let label = if value.is_nan() {
            "NaN".to_string()
        } else if value.is_infinite() {
            if value > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            }
        } else {
            // Only finite non-integers reach here (e.g. 1.5). ECMAScript
            // NumberToString switches to scientific notation outside
            // [1e-6, 1e21); for the common fractional inputs Rust's `{}`
            // already matches Node.
            let abs = value.abs();
            if !(1e-6..1e21).contains(&abs) {
                format!("{:e}", value)
            } else {
                format!("{}", value)
            }
        };
        throw_bigint_range_error(&format!(
            "The number {label} cannot be converted to a BigInt because it is not an integer"
        ));
    }
    bigint_alloc_with_limbs(limbs_from_integer_f64(value))
}

/// Create a BigInt from a string (the `BigInt("…")` coercion path).
///
/// Matches ECMAScript `StringToBigInt` (#2907): leading/trailing whitespace
/// is trimmed; an empty (or all-whitespace) string is `0n`; a decimal string
/// may carry an optional `+`/`-` sign; the radix prefixes `0x`/`0X`, `0o`/`0O`,
/// `0b`/`0B` are accepted (without a sign). Any other content — stray
/// characters, a lone sign, a sign on a prefixed literal — throws a
/// `SyntaxError` instead of silently dropping the invalid characters.
#[no_mangle]
pub extern "C" fn js_bigint_from_string(data: *const u8, len: u32) -> *mut BigIntHeader {
    unsafe {
        let bytes = std::slice::from_raw_parts(data, len as usize);
        let raw = std::str::from_utf8_unchecked(bytes);
        match parse_bigint_string(raw) {
            Ok(limbs) => bigint_alloc_with_limbs(limbs),
            Err(()) => throw_bigint_syntax_error(&format!("Cannot convert {raw} to a BigInt")),
        }
    }
}

/// Parse a string per ECMAScript `StringToBigInt`. Returns the limb array on
/// success, or `Err(())` for invalid BigInt syntax. The original (untrimmed)
/// string is used by the caller to build Node's error message.
pub(crate) fn parse_bigint_string(raw: &str) -> Result<[u64; BIGINT_LIMBS], ()> {
    // ECMAScript trims StrWhiteSpace from both ends; the empty string is 0n.
    let s = raw.trim();
    if s.is_empty() {
        return Ok(ZERO_LIMBS);
    }

    // Radix-prefixed forms do not allow a leading sign.
    let lower_prefix = s.as_bytes().get(0..2).map(|p| {
        let mut buf = [p[0], p[1]];
        buf.make_ascii_lowercase();
        buf
    });
    if let Some([b'0', tag]) = lower_prefix {
        let radix = match tag {
            b'x' => Some(16u32),
            b'o' => Some(8u32),
            b'b' => Some(2u32),
            _ => None,
        };
        if let Some(radix) = radix {
            let digits = &s[2..];
            if digits.is_empty() {
                return Err(());
            }
            return parse_radix_digits(digits, radix, false);
        }
    }

    // Optional sign, then decimal digits only.
    let (is_negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if digits.is_empty() {
        return Err(());
    }
    parse_radix_digits(digits, 10, is_negative)
}

/// Parse `digits` in the given radix (2/8/10/16), rejecting any out-of-range
/// character. Applies two's-complement negation when `is_negative`.
fn parse_radix_digits(
    digits: &str,
    radix: u32,
    is_negative: bool,
) -> Result<[u64; BIGINT_LIMBS], ()> {
    let mut limbs = ZERO_LIMBS;
    let radix_u128 = radix as u128;
    for c in digits.chars() {
        let digit = c.to_digit(radix).ok_or(())?;
        let mut carry = digit as u64;
        for limb in limbs.iter_mut() {
            let product = (*limb as u128) * radix_u128 + carry as u128;
            *limb = product as u64;
            carry = (product >> 64) as u64;
        }
        // A carry out of the top limb means the magnitude passed 2^1024 —
        // a literal too large for the fixed representation (#6073).
        if carry != 0 {
            throw_bigint_overflow();
        }
    }
    // The magnitude fits in 16 limbs but may still exceed the signed range
    // (e.g. a positive literal in [2^1023, 2^1024) would read back negative).
    if !magnitude_fits_1024(&limbs, is_negative) {
        throw_bigint_overflow();
    }
    if is_negative && !limbs.iter().all(|&l| l == 0) {
        limbs = negate_limbs(&limbs);
    }
    Ok(limbs)
}

/// Create a BigInt from a string with a given radix (for BN.js compatibility)
/// Handles decimal (10), hex (16), and other bases.
#[no_mangle]
pub extern "C" fn js_bigint_from_string_radix(
    data: *const u8,
    len: u32,
    radix: i32,
) -> *mut BigIntHeader {
    if data.is_null() || len == 0 {
        // Null input
        return js_bigint_from_i64(0);
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(data, len as usize);
        let s = std::str::from_utf8_unchecked(bytes);
        // Debug removed

        // Handle negative
        let (is_negative, s) = if s.starts_with('-') {
            (true, &s[1..])
        } else {
            (false, s)
        };

        // Strip 0x prefix for hex
        let s = if radix == 16 && (s.starts_with("0x") || s.starts_with("0X")) {
            &s[2..]
        } else {
            s
        };

        let mut limbs = ZERO_LIMBS;
        let radix = radix as u64;

        if radix == 16 {
            // Optimized hex parsing
            let mut chars = s.chars().rev();
            for limb in limbs.iter_mut() {
                let mut value = 0u64;
                for i in 0..16 {
                    if let Some(c) = chars.next() {
                        let digit = match c {
                            '0'..='9' => c as u64 - '0' as u64,
                            'a'..='f' => c as u64 - 'a' as u64 + 10,
                            'A'..='F' => c as u64 - 'A' as u64 + 10,
                            _ => continue,
                        };
                        value |= digit << (i * 4);
                    } else {
                        break;
                    }
                }
                *limb = value;
            }
            // The reversed stream fed the low 16 limbs first, so any leftover
            // chars are the high-order digits. Only a *nonzero* hex digit there
            // sets a bit at or above 2^1024 — excess leading zeros are harmless
            // (`0x0…0<256 digits>` still fits), so ignore '0' and non-hex chars (#6073).
            if chars.any(|c| matches!(c, '1'..='9' | 'a'..='f' | 'A'..='F')) {
                throw_bigint_overflow();
            }
        } else {
            // General radix parsing using long multiplication
            for c in s.chars() {
                let digit = match c {
                    '0'..='9' => (c as u64) - ('0' as u64),
                    'a'..='z' => (c as u64) - ('a' as u64) + 10,
                    'A'..='Z' => (c as u64) - ('A' as u64) + 10,
                    _ => continue,
                };
                if digit >= radix {
                    continue;
                }
                let mut carry = digit;
                for limb in limbs.iter_mut() {
                    let product = (*limb as u128) * (radix as u128) + carry as u128;
                    *limb = product as u64;
                    carry = (product >> 64) as u64;
                }
                // Carry out of the top limb → magnitude passed 2^1024 (#6073).
                if carry != 0 {
                    throw_bigint_overflow();
                }
            }
        }

        // The magnitude fits in 16 limbs but may still exceed the signed range.
        if !magnitude_fits_1024(&limbs, is_negative) {
            throw_bigint_overflow();
        }
        if is_negative && !limbs.iter().all(|&l| l == 0) {
            limbs = negate_limbs(&limbs);
        }
        bigint_alloc_with_limbs(limbs)
    }
}

/// Convert BigInt to a byte array (big-endian, for BN.toArrayLike/toArray)
/// Returns a buffer of the specified length, zero-padded on the left.
#[no_mangle]
pub extern "C" fn js_bigint_to_buffer(
    a: *const BigIntHeader,
    length: i32,
) -> *mut crate::buffer::BufferHeader {
    let limbs = bigint_limbs_or_zero(a);
    let length = if length <= 0 { 32 } else { length as usize };

    let result = crate::buffer::buffer_alloc(length as u32);
    unsafe {
        (*result).length = length as u32;
        let data = crate::buffer::buffer_data_mut(result);

        // Extract bytes from the pre-allocation limb snapshot
        // (little-endian in memory) and write in big-endian order.
        std::ptr::write_bytes(data, 0, length);
        let significant = (BIGINT_LIMBS * 8).min(length);
        for i in 0..significant {
            let limb = limbs[i / 8];
            let byte = ((limb >> ((i % 8) * 8)) & 0xff) as u8;
            *data.add(length - 1 - i) = byte;
        }
    }
    result
}

/// Convert BigInt to f64 (may lose precision)
#[no_mangle]
pub extern "C" fn js_bigint_to_f64(a: *const BigIntHeader) -> f64 {
    unsafe {
        if a.is_null() {
            return 0.0;
        }
        let limbs = (*a).limbs;
        let neg = is_negative(&limbs);
        let abs_limbs = if neg { negate_limbs(&limbs) } else { limbs };
        let mut result = 0.0f64;
        let mut multiplier = 1.0f64;
        for limb in abs_limbs.iter() {
            result += (*limb as f64) * multiplier;
            multiplier *= 18446744073709551616.0; // 2^64
        }
        if neg {
            -result
        } else {
            result
        }
    }
}

fn limbs_to_decimal_string(limbs: &[u64; BIGINT_LIMBS]) -> String {
    let mut digits = Vec::new();

    // Check if zero
    if *limbs == ZERO_LIMBS {
        return "0".to_string();
    }

    // Check if negative (two's complement)
    let negative = is_negative(limbs);
    let mut temp = if negative {
        negate_limbs(limbs)
    } else {
        *limbs
    };

    while temp != ZERO_LIMBS {
        let mut remainder = 0u128;
        for i in (0..BIGINT_LIMBS).rev() {
            let dividend = (remainder << 64) + temp[i] as u128;
            temp[i] = (dividend / 10) as u64;
            remainder = dividend % 10;
        }
        digits.push((remainder as u8 + b'0') as char);
    }

    digits.reverse();
    let s: String = digits.into_iter().collect();
    if negative {
        format!("-{}", s)
    } else {
        s
    }
}

fn limbs_to_radix_string(limbs: &[u64; BIGINT_LIMBS], radix: u32) -> String {
    let radix = if !(2..=36).contains(&radix) {
        10
    } else {
        radix
    };
    if radix == 10 {
        return limbs_to_decimal_string(limbs);
    }

    let mut digits = Vec::new();

    if *limbs == ZERO_LIMBS {
        return "0".to_string();
    }

    let negative = is_negative(limbs);
    let mut temp = if negative {
        negate_limbs(limbs)
    } else {
        *limbs
    };

    let radix_u128 = radix as u128;
    while temp != ZERO_LIMBS {
        let mut remainder = 0u128;
        for i in (0..BIGINT_LIMBS).rev() {
            let dividend = (remainder << 64) + temp[i] as u128;
            temp[i] = (dividend / radix_u128) as u64;
            remainder = dividend % radix_u128;
        }
        let digit = remainder as u8;
        let ch = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + (digit - 10)
        };
        digits.push(ch as char);
    }

    digits.reverse();
    let s: String = digits.into_iter().collect();
    if negative {
        format!("-{}", s)
    } else {
        s
    }
}

/// Convert BigInt to string
#[no_mangle]
pub extern "C" fn js_bigint_to_string(a: *const BigIntHeader) -> *mut crate::string::StringHeader {
    unsafe {
        if a.is_null() || (a as usize) < 0x10000 || (a as u64) >> 48 != 0 {
            return std::ptr::null_mut();
        }
        let s = limbs_to_decimal_string(&(*a).limbs);
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }
}

/// Convert BigInt to string with radix
#[no_mangle]
pub extern "C" fn js_bigint_to_string_radix(
    a: *const BigIntHeader,
    radix: i32,
) -> *mut crate::string::StringHeader {
    unsafe {
        if a.is_null() || (a as usize) < 0x10000 || (a as u64) >> 48 != 0 {
            return std::ptr::null_mut();
        }
        let s = limbs_to_radix_string(&(*a).limbs, radix as u32);
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }
}

/// Print BigInt to stdout (for debugging)
#[no_mangle]
pub extern "C" fn js_bigint_print(a: *const BigIntHeader) {
    unsafe {
        let s = limbs_to_decimal_string(&(*a).limbs);
        println!("{}n", s);
    }
}

/// Print BigInt to stderr (console.error)
#[no_mangle]
pub extern "C" fn js_bigint_error(a: *const BigIntHeader) {
    unsafe {
        let s = limbs_to_decimal_string(&(*a).limbs);
        let _ = s;
    }
}

/// Print BigInt to stderr (console.warn)
#[no_mangle]
pub extern "C" fn js_bigint_warn(a: *const BigIntHeader) {
    unsafe {
        let s = limbs_to_decimal_string(&(*a).limbs);
        let _ = s;
    }
}
