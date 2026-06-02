//! Decimal/Big.js implementation
//!
//! Native implementation of Big.js and Decimal.js for arbitrary precision math.
//! Uses Rust's rust_decimal crate for precise decimal arithmetic.

use perry_runtime::{js_string_from_bytes, StringHeader};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use crate::common::{get_handle_mut, register_handle, Handle};

/// DecimalHandle stores a Decimal value
pub struct DecimalHandle {
    value: Decimal,
}

impl DecimalHandle {
    pub fn new(value: Decimal) -> Self {
        DecimalHandle { value }
    }
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Create a new Decimal from a number
#[no_mangle]
pub extern "C" fn js_decimal_from_number(value: f64) -> Handle {
    let decimal = Decimal::from_f64(value).unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(decimal))
}

/// Create a new Decimal from a string
#[no_mangle]
pub unsafe extern "C" fn js_decimal_from_string(value_ptr: *const StringHeader) -> Handle {
    let value_str = match string_from_header(value_ptr) {
        Some(s) => s,
        None => return register_handle(DecimalHandle::new(Decimal::ZERO)),
    };

    let decimal = Decimal::from_str(&value_str).unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(decimal))
}

/// Decimal.plus(other) - Addition
#[no_mangle]
pub extern "C" fn js_decimal_plus(handle: Handle, other: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a + b))
}

/// Decimal.plus with number
#[no_mangle]
pub extern "C" fn js_decimal_plus_number(handle: Handle, other: f64) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = Decimal::from_f64(other).unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a + b))
}

/// Decimal.minus(other) - Subtraction
#[no_mangle]
pub extern "C" fn js_decimal_minus(handle: Handle, other: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a - b))
}

/// Decimal.minus with number
#[no_mangle]
pub extern "C" fn js_decimal_minus_number(handle: Handle, other: f64) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = Decimal::from_f64(other).unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a - b))
}

/// Decimal.times(other) - Multiplication
#[no_mangle]
pub extern "C" fn js_decimal_times(handle: Handle, other: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a * b))
}

/// Decimal.times with number
#[no_mangle]
pub extern "C" fn js_decimal_times_number(handle: Handle, other: f64) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = Decimal::from_f64(other).unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a * b))
}

/// Decimal.div(other) - Division
#[no_mangle]
pub extern "C" fn js_decimal_div(handle: Handle, other: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ONE);

    if b.is_zero() {
        return register_handle(DecimalHandle::new(Decimal::ZERO));
    }

    register_handle(DecimalHandle::new(a / b))
}

/// Decimal.div with number
#[no_mangle]
pub extern "C" fn js_decimal_div_number(handle: Handle, other: f64) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = Decimal::from_f64(other).unwrap_or(Decimal::ONE);

    if b.is_zero() {
        return register_handle(DecimalHandle::new(Decimal::ZERO));
    }

    register_handle(DecimalHandle::new(a / b))
}

/// Decimal.mod(other) - Modulo
#[no_mangle]
pub extern "C" fn js_decimal_mod(handle: Handle, other: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ONE);

    if b.is_zero() {
        return register_handle(DecimalHandle::new(Decimal::ZERO));
    }

    register_handle(DecimalHandle::new(a % b))
}

/// Decimal.pow(n) - Power
#[no_mangle]
pub extern "C" fn js_decimal_pow(handle: Handle, n: f64) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let exp = Decimal::from_f64(n).unwrap_or(Decimal::ZERO);

    // Use checked_powd with Decimal exponent
    let result = a.checked_powd(exp).unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(result))
}

/// Decimal.sqrt() - Square root
#[no_mangle]
pub extern "C" fn js_decimal_sqrt(handle: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);

    // Use the sqrt method from rust_decimal with maths feature
    let result = a.sqrt().unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(result))
}

/// Decimal.abs() - Absolute value
#[no_mangle]
pub extern "C" fn js_decimal_abs(handle: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a.abs()))
}

/// Decimal.neg() - Negation
#[no_mangle]
pub extern "C" fn js_decimal_neg(handle: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(-a))
}

/// Decimal.round() - Round to nearest integer
#[no_mangle]
pub extern "C" fn js_decimal_round(handle: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a.round()))
}

/// Decimal.floor() - Round down
#[no_mangle]
pub extern "C" fn js_decimal_floor(handle: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a.floor()))
}

/// Decimal.ceil() - Round up
#[no_mangle]
pub extern "C" fn js_decimal_ceil(handle: Handle) -> Handle {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    register_handle(DecimalHandle::new(a.ceil()))
}

/// Decimal.toFixed(decimals) - Format with fixed decimal places
#[no_mangle]
pub extern "C" fn js_decimal_to_fixed(handle: Handle, decimals: f64) -> *const StringHeader {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let dp = decimals as u32;

    let rounded = a.round_dp(dp);
    let result = format!("{:.1$}", rounded, dp as usize);

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// Decimal.toString() - Convert to string
#[no_mangle]
pub extern "C" fn js_decimal_to_string(handle: Handle) -> *const StringHeader {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let result = a.to_string();

    js_string_from_bytes(result.as_ptr(), result.len() as u32)
}

/// Decimal.toNumber() - Convert to number
#[no_mangle]
pub extern "C" fn js_decimal_to_number(handle: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    a.to_f64().unwrap_or(0.0)
}

// JS boolean encoding: NaN-boxed sentinel bits. Without these the predicate
// runtime fns return raw 1.0 / 0.0, which user TS code prints as "1" / "0"
// instead of "true" / "false" — a regression vs npm decimal.js. The bits
// match perry-runtime/src/value.rs's TAG_TRUE / TAG_FALSE.
const JSBOOL_TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;
const JSBOOL_FALSE_BITS: u64 = 0x7FFC_0000_0000_0003;
#[inline(always)]
fn js_bool(b: bool) -> f64 {
    f64::from_bits(if b {
        JSBOOL_TRUE_BITS
    } else {
        JSBOOL_FALSE_BITS
    })
}

/// Decimal.eq(other) - Equality comparison
#[no_mangle]
pub extern "C" fn js_decimal_eq(handle: Handle, other: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    js_bool(a == b)
}

/// Decimal.lt(other) - Less than
#[no_mangle]
pub extern "C" fn js_decimal_lt(handle: Handle, other: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    js_bool(a < b)
}

/// Decimal.lte(other) - Less than or equal
#[no_mangle]
pub extern "C" fn js_decimal_lte(handle: Handle, other: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    js_bool(a <= b)
}

/// Decimal.gt(other) - Greater than
#[no_mangle]
pub extern "C" fn js_decimal_gt(handle: Handle, other: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    js_bool(a > b)
}

/// Decimal.gte(other) - Greater than or equal
#[no_mangle]
pub extern "C" fn js_decimal_gte(handle: Handle, other: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    js_bool(a >= b)
}

/// Decimal.isZero() - Check if zero
#[no_mangle]
pub extern "C" fn js_decimal_is_zero(handle: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    js_bool(a.is_zero())
}

/// Decimal.isPositive() - Check if positive
#[no_mangle]
pub extern "C" fn js_decimal_is_positive(handle: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    js_bool(a.is_sign_positive() && !a.is_zero())
}

/// Decimal.isNegative() - Check if negative
#[no_mangle]
pub extern "C" fn js_decimal_is_negative(handle: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    js_bool(a.is_sign_negative())
}

/// Decimal.cmp(other) - Compare: -1, 0, or 1
#[no_mangle]
pub extern "C" fn js_decimal_cmp(handle: Handle, other: Handle) -> f64 {
    let a = get_handle_mut::<DecimalHandle>(handle)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);
    let b = get_handle_mut::<DecimalHandle>(other)
        .map(|h| h.value)
        .unwrap_or(Decimal::ZERO);

    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

// ---------------------------------------------------------------------------
// Arg coercion + binary-op wrappers — npm decimal.js accepts EITHER a Decimal
// instance OR a number/string for the right-hand side of plus/minus/times/
// div/mod. Codegen passes the second arg as a NaN-boxed JSValue f64; the
// helpers below let us write a single dispatch table row per op (NA_F64) that
// works regardless of what the user actually passed.

const POINTER_TAG_HI16: u64 = 0x7FFD;
const STRING_TAG_HI16: u64 = 0x7FFF;
const INT32_TAG_HI16: u64 = 0x7FFE;
const SHORT_STRING_HI16: u64 = 0x7FF9;

/// Decode a NaN-boxed JSValue (f64) into a Decimal handle. Always returns a
/// valid handle; falls back to ZERO on unrecognized inputs (matches the rest
/// of the decimal.rs surface — every fn already silently coerces failure to
/// `Decimal::ZERO` rather than panic).
#[no_mangle]
pub unsafe extern "C" fn js_decimal_coerce_to_handle(value: f64) -> Handle {
    let bits = value.to_bits();
    let tag = bits >> 48;
    if tag == POINTER_TAG_HI16 {
        // Already a Decimal handle — extract the lower 48 bits as the i64
        // handle id. (Existing handles are small positive integers, so the
        // pointer-tag mask round-trip is lossless.)
        return (bits & 0x0000_FFFF_FFFF_FFFF) as Handle;
    }
    if tag == STRING_TAG_HI16 {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        return js_decimal_from_string(ptr);
    }
    if tag == SHORT_STRING_HI16 {
        // SSO inline strings — decode into a temp buffer and parse. Length
        // byte at bits 40..47, up to 5 bytes at 0..39 (LSB-first).
        let len = ((bits >> 40) & 0xFF) as usize;
        let mut buf = [0u8; 5];
        for i in 0..len.min(5) {
            buf[i] = ((bits >> (i * 8)) & 0xFF) as u8;
        }
        let s = std::str::from_utf8(&buf[..len]).unwrap_or("0");
        let d = Decimal::from_str(s).unwrap_or(Decimal::ZERO);
        return register_handle(DecimalHandle::new(d));
    }
    if tag == INT32_TAG_HI16 {
        let n = ((bits & 0xFFFF_FFFF) as i32) as f64;
        return js_decimal_from_number(n);
    }
    // Plain double: top16 < 0x7FF8 (positive numerics) or >= 0x8000 (negative
    // — sign-extended). Matches the value.rs canonicalization tag-band check.
    if !(0x7FF8..0x8000).contains(&tag) {
        return js_decimal_from_number(value);
    }
    // undefined / null / true / false / etc. — coerce to zero.
    register_handle(DecimalHandle::new(Decimal::ZERO))
}

/// `a.plus(value)` where value is a Decimal handle, number, or string — used
/// by codegen as a single dispatch entry that accepts the NaN-boxed JSValue.
#[no_mangle]
pub unsafe extern "C" fn js_decimal_plus_value(handle: Handle, value: f64) -> Handle {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_plus(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_minus_value(handle: Handle, value: f64) -> Handle {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_minus(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_times_value(handle: Handle, value: f64) -> Handle {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_times(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_div_value(handle: Handle, value: f64) -> Handle {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_div(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_mod_value(handle: Handle, value: f64) -> Handle {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_mod(handle, other)
}

/// `a.eq(value)` / `a.lt` / etc. — coerce-then-compare versions of the
/// existing handle-handle predicates, so user code can write `a.eq(0)` or
/// `a.eq("0.1")` without pre-wrapping the rhs.
#[no_mangle]
pub unsafe extern "C" fn js_decimal_eq_value(handle: Handle, value: f64) -> f64 {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_eq(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_lt_value(handle: Handle, value: f64) -> f64 {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_lt(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_lte_value(handle: Handle, value: f64) -> f64 {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_lte(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_gt_value(handle: Handle, value: f64) -> f64 {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_gt(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_gte_value(handle: Handle, value: f64) -> f64 {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_gte(handle, other)
}

#[no_mangle]
pub unsafe extern "C" fn js_decimal_cmp_value(handle: Handle, value: f64) -> f64 {
    let other = js_decimal_coerce_to_handle(value);
    js_decimal_cmp(handle, other)
}
