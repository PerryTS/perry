//! BigInt construction and conversion share the operator precision.
use super::*;

#[no_mangle]
pub extern "C" fn js_bigint_from_u64(value: u64) -> *mut BigIntHeader {
    bigint_alloc_integer(BigInt::from(value))
}
#[no_mangle]
pub extern "C" fn js_bigint_from_i64(value: i64) -> *mut BigIntHeader {
    bigint_alloc_integer(BigInt::from(value))
}
#[no_mangle]
pub extern "C" fn js_bigint_from_i128_parts(lo: u64, hi: i64) -> *mut BigIntHeader {
    bigint_alloc_integer(BigInt::from(
        (((hi as u64 as u128) << 64) | lo as u128) as i128,
    ))
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
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
    bigint_alloc_integer(BigInt::from_f64(value).expect("finite integer"))
}

#[no_mangle]
pub extern "C" fn js_bigint_from_string(data: *const u8, len: u32) -> *mut BigIntHeader {
    unsafe {
        let raw = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len as usize));
        match parse_bigint_string(raw) {
            Ok(limbs) => bigint_alloc_with_limbs(limbs),
            Err(()) => throw_bigint_syntax_error(&format!("Cannot convert {raw} to a BigInt")),
        }
    }
}

/// ECMAScript StringToBigInt syntax; relational string comparisons use the
/// same parser without throwing on invalid syntax.
pub(crate) fn parse_bigint_string(raw: &str) -> Result<Vec<u64>, ()> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(ZERO_LIMBS.to_vec());
    }
    let (negative, digits, radix) =
        if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            (false, rest, 16)
        } else if let Some(rest) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
            (false, rest, 8)
        } else if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
            (false, rest, 2)
        } else if let Some(rest) = s.strip_prefix('-') {
            (true, rest, 10)
        } else {
            (false, s.strip_prefix('+').unwrap_or(s), 10)
        };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return Err(());
    }
    let value = BigInt::parse_bytes(digits.as_bytes(), radix).ok_or(())?;
    Ok(integer_limbs(&if negative { -value } else { value }))
}

/// BN.js compatibility parser retains its permissive digit filtering.
#[no_mangle]
pub extern "C" fn js_bigint_from_string_radix(
    data: *const u8,
    len: u32,
    radix: i32,
) -> *mut BigIntHeader {
    if data.is_null() || len == 0 {
        return js_bigint_from_i64(0);
    }
    if !(2..=36).contains(&radix) {
        return js_bigint_from_i64(0);
    }
    let raw =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len as usize)) };
    let negative = raw.starts_with('-');
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    let digits = if radix == 16 {
        digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
            .unwrap_or(digits)
    } else {
        digits
    };
    let digits: String = digits
        .chars()
        .filter(|c| c.is_digit(radix as u32))
        .collect();
    let value = BigInt::parse_bytes(digits.as_bytes(), radix as u32).unwrap_or_default();
    bigint_alloc_integer(if negative { -value } else { value })
}

#[no_mangle]
pub extern "C" fn js_bigint_to_buffer(
    a: *const BigIntHeader,
    length: i32,
) -> *mut crate::buffer::BufferHeader {
    let limbs = bigint_limbs_or_zero(a);
    let length = if length <= 0 { 32 } else { length as usize };
    let (value, pin) = crate::buffer::bytes::new_bytes(
        crate::buffer::bytes::Brand::Buffer,
        length,
        crate::buffer::bytes::Init::Uninit,
    );
    unsafe {
        let data = pin.as_mut_ptr();
        std::ptr::write_bytes(data, if is_negative(&limbs) { 0xff } else { 0 }, length);
        for i in 0..(limbs.len() * 8).min(length) {
            *data.add(length - 1 - i) = (limbs[i / 8] >> ((i % 8) * 8)) as u8;
        }
    }
    crate::value::JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::buffer::BufferHeader>()
        .cast_mut()
}

#[no_mangle]
pub extern "C" fn js_bigint_to_f64(a: *const BigIntHeader) -> f64 {
    let value = bigint_integer(a);
    value.to_f64().unwrap_or(if value.is_negative() {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    })
}

#[no_mangle]
pub extern "C" fn js_bigint_to_string(a: *const BigIntHeader) -> *mut crate::string::StringHeader {
    js_bigint_to_string_radix(a, 10)
}
#[no_mangle]
pub extern "C" fn js_bigint_to_string_radix(
    a: *const BigIntHeader,
    radix: i32,
) -> *mut crate::string::StringHeader {
    if a.is_null() || (a as usize) < 0x10000 || (a as u64) >> 48 != 0 {
        return std::ptr::null_mut();
    }
    let radix = if (2..=36).contains(&radix) {
        radix as u32
    } else {
        10
    };
    let text = bigint_integer(a).to_str_radix(radix);
    crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
}
#[no_mangle]
pub extern "C" fn js_bigint_print(a: *const BigIntHeader) {
    println!("{}n", bigint_integer(a));
}
#[no_mangle]
pub extern "C" fn js_bigint_error(_a: *const BigIntHeader) {}
#[no_mangle]
pub extern "C" fn js_bigint_warn(_a: *const BigIntHeader) {}
