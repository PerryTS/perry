//! Native bindings for the npm `decimal.js` / `bignumber.js`
//! arbitrary-precision-arithmetic packages. Sync, handle-based,
//! uses only perry-ffi v0.5 strings + handles.

use perry_ffi::{
    alloc_string, get_handle_mut, read_string, register_handle, Handle, JsString, StringHeader,
};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

pub struct DecimalHandle {
    value: Decimal,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

fn register(d: Decimal) -> Handle {
    register_handle(DecimalHandle { value: d })
}

fn val(h: Handle) -> Option<Decimal> {
    get_handle_mut::<DecimalHandle>(h).map(|d| d.value)
}

#[inline]
fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn js_decimal_from_number(value: f64) -> Handle {
    register(Decimal::from_f64(value).unwrap_or(Decimal::ZERO))
}

/// # Safety
///
/// `value_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_decimal_from_string(value_ptr: *const StringHeader) -> Handle {
    let s = read_str(value_ptr).unwrap_or_default();
    register(Decimal::from_str(&s).unwrap_or(Decimal::ZERO))
}

macro_rules! binop_handle {
    ($name:ident, $op:tt) => {
        #[no_mangle]
        pub extern "C" fn $name(handle: Handle, other: Handle) -> Handle {
            let a = val(handle).unwrap_or(Decimal::ZERO);
            let b = val(other).unwrap_or(Decimal::ZERO);
            register(a $op b)
        }
    };
}

macro_rules! binop_number {
    ($name:ident, $op:tt) => {
        #[no_mangle]
        pub extern "C" fn $name(handle: Handle, other: f64) -> Handle {
            let a = val(handle).unwrap_or(Decimal::ZERO);
            let b = Decimal::from_f64(other).unwrap_or(Decimal::ZERO);
            register(a $op b)
        }
    };
}

binop_handle!(js_decimal_plus, +);
binop_number!(js_decimal_plus_number, +);
binop_handle!(js_decimal_minus, -);
binop_number!(js_decimal_minus_number, -);
binop_handle!(js_decimal_times, *);
binop_number!(js_decimal_times_number, *);

#[no_mangle]
pub extern "C" fn js_decimal_div(handle: Handle, other: Handle) -> Handle {
    let a = val(handle).unwrap_or(Decimal::ZERO);
    let b = val(other).unwrap_or(Decimal::ZERO);
    if b.is_zero() {
        return register(Decimal::ZERO);
    }
    register(a / b)
}

#[no_mangle]
pub extern "C" fn js_decimal_div_number(handle: Handle, other: f64) -> Handle {
    let a = val(handle).unwrap_or(Decimal::ZERO);
    let b = Decimal::from_f64(other).unwrap_or(Decimal::ZERO);
    if b.is_zero() {
        return register(Decimal::ZERO);
    }
    register(a / b)
}

#[no_mangle]
pub extern "C" fn js_decimal_mod(handle: Handle, other: Handle) -> Handle {
    let a = val(handle).unwrap_or(Decimal::ZERO);
    let b = val(other).unwrap_or(Decimal::ZERO);
    if b.is_zero() {
        return register(Decimal::ZERO);
    }
    register(a % b)
}

#[no_mangle]
pub extern "C" fn js_decimal_pow(handle: Handle, n: f64) -> Handle {
    let a = val(handle).unwrap_or(Decimal::ZERO);
    // rust_decimal lacks an exponent op for arbitrary f64 exponents;
    // approximate via f64 round-trip — same trick perry-stdlib uses.
    let result = a
        .to_f64()
        .and_then(|af| Decimal::from_f64(af.powf(n)))
        .unwrap_or(Decimal::ZERO);
    register(result)
}

#[no_mangle]
pub extern "C" fn js_decimal_sqrt(handle: Handle) -> Handle {
    let a = val(handle).unwrap_or(Decimal::ZERO);
    let result = a
        .to_f64()
        .filter(|x| *x >= 0.0)
        .and_then(|x| Decimal::from_f64(x.sqrt()))
        .unwrap_or(Decimal::ZERO);
    register(result)
}

#[no_mangle]
pub extern "C" fn js_decimal_abs(handle: Handle) -> Handle {
    register(val(handle).unwrap_or(Decimal::ZERO).abs())
}

#[no_mangle]
pub extern "C" fn js_decimal_neg(handle: Handle) -> Handle {
    register(-val(handle).unwrap_or(Decimal::ZERO))
}

#[no_mangle]
pub extern "C" fn js_decimal_round(handle: Handle) -> Handle {
    register(val(handle).unwrap_or(Decimal::ZERO).round())
}

#[no_mangle]
pub extern "C" fn js_decimal_floor(handle: Handle) -> Handle {
    register(val(handle).unwrap_or(Decimal::ZERO).floor())
}

#[no_mangle]
pub extern "C" fn js_decimal_ceil(handle: Handle) -> Handle {
    register(val(handle).unwrap_or(Decimal::ZERO).ceil())
}

#[no_mangle]
pub extern "C" fn js_decimal_to_fixed(handle: Handle, decimals: f64) -> *const StringHeader {
    let v = val(handle).unwrap_or(Decimal::ZERO);
    let dp = decimals as u32;
    let rounded = v.round_dp(dp);
    let formatted = if dp == 0 {
        rounded.trunc().to_string()
    } else {
        format!("{:.*}", dp as usize, rounded)
    };
    alloc_string(&formatted).as_raw()
}

#[no_mangle]
pub extern "C" fn js_decimal_to_string(handle: Handle) -> *const StringHeader {
    let v = val(handle).unwrap_or(Decimal::ZERO);
    alloc_string(&v.to_string()).as_raw()
}

#[no_mangle]
pub extern "C" fn js_decimal_to_number(handle: Handle) -> f64 {
    val(handle).and_then(|v| v.to_f64()).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_decimal_eq(handle: Handle, other: Handle) -> f64 {
    b(val(handle).unwrap_or(Decimal::ZERO) == val(other).unwrap_or(Decimal::ZERO))
}

#[no_mangle]
pub extern "C" fn js_decimal_lt(handle: Handle, other: Handle) -> f64 {
    b(val(handle).unwrap_or(Decimal::ZERO) < val(other).unwrap_or(Decimal::ZERO))
}

#[no_mangle]
pub extern "C" fn js_decimal_lte(handle: Handle, other: Handle) -> f64 {
    b(val(handle).unwrap_or(Decimal::ZERO) <= val(other).unwrap_or(Decimal::ZERO))
}

#[no_mangle]
pub extern "C" fn js_decimal_gt(handle: Handle, other: Handle) -> f64 {
    b(val(handle).unwrap_or(Decimal::ZERO) > val(other).unwrap_or(Decimal::ZERO))
}

#[no_mangle]
pub extern "C" fn js_decimal_gte(handle: Handle, other: Handle) -> f64 {
    b(val(handle).unwrap_or(Decimal::ZERO) >= val(other).unwrap_or(Decimal::ZERO))
}

#[no_mangle]
pub extern "C" fn js_decimal_is_zero(handle: Handle) -> f64 {
    b(val(handle).unwrap_or(Decimal::ZERO).is_zero())
}

#[no_mangle]
pub extern "C" fn js_decimal_is_positive(handle: Handle) -> f64 {
    b(val(handle).unwrap_or(Decimal::ZERO) > Decimal::ZERO)
}

#[no_mangle]
pub extern "C" fn js_decimal_is_negative(handle: Handle) -> f64 {
    b(val(handle).unwrap_or(Decimal::ZERO) < Decimal::ZERO)
}

#[no_mangle]
pub extern "C" fn js_decimal_cmp(handle: Handle, other: Handle) -> f64 {
    let a = val(handle).unwrap_or(Decimal::ZERO);
    let b = val(other).unwrap_or(Decimal::ZERO);
    use std::cmp::Ordering;
    match a.cmp(&b) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

// ---------------------------------------------------------------------------
// NaN-boxed JSValue coercion + binary-op wrappers — npm `decimal.js` accepts
// EITHER a Decimal instance OR a number/string for the rhs of every arith /
// comparison op. Codegen passes the rhs as a NaN-boxed JSValue f64; these
// helpers let `native_table.rs` use a single `_value` symbol per op. Mirrors
// perry-stdlib::decimal so the well-known routing in #466 links cleanly.
// Fixes #1192 (lld-link: undefined symbol js_decimal_coerce_to_handle /
// js_decimal_plus_value when `compilePackages: ["decimal.js"]` is set).

const POINTER_TAG_HI16: u64 = 0x7FFD;
const STRING_TAG_HI16: u64 = 0x7FFF;
const INT32_TAG_HI16: u64 = 0x7FFE;
const SHORT_STRING_HI16: u64 = 0x7FF9;

/// Decode a NaN-boxed JSValue (f64) into a Decimal handle. Always returns a
/// valid handle; falls back to ZERO on unrecognized inputs (matches the rest
/// of this surface — every fn already silently coerces failure to
/// `Decimal::ZERO` rather than panic).
///
/// # Safety
///
/// `value` must be a valid NaN-boxed Perry JSValue. STRING_TAG pointers must
/// reference a live `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_decimal_coerce_to_handle(value: f64) -> Handle {
    let bits = value.to_bits();
    let tag = bits >> 48;
    if tag == POINTER_TAG_HI16 {
        // Already a Decimal handle — extract the lower 48 bits as the handle id.
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
        for (i, slot) in buf.iter_mut().enumerate().take(len.min(5)) {
            *slot = ((bits >> (i * 8)) & 0xFF) as u8;
        }
        let s = std::str::from_utf8(&buf[..len.min(5)]).unwrap_or("0");
        let d = Decimal::from_str(s).unwrap_or(Decimal::ZERO);
        return register(d);
    }
    if tag == INT32_TAG_HI16 {
        let n = ((bits & 0xFFFF_FFFF) as i32) as f64;
        return js_decimal_from_number(n);
    }
    // Plain double: top16 < 0x7FF8 (positive numerics) or >= 0x8000 (negative
    // — sign-extended). Matches value.rs canonicalization tag-band check.
    if !(0x7FF8..0x8000).contains(&tag) {
        return js_decimal_from_number(value);
    }
    // undefined / null / true / false / etc. — coerce to zero.
    register(Decimal::ZERO)
}

macro_rules! binop_value {
    ($name:ident, $inner:ident) => {
        /// # Safety
        ///
        /// `value` must be a valid NaN-boxed Perry JSValue.
        #[no_mangle]
        pub unsafe extern "C" fn $name(handle: Handle, value: f64) -> Handle {
            let other = js_decimal_coerce_to_handle(value);
            $inner(handle, other)
        }
    };
}

macro_rules! cmp_value {
    ($name:ident, $inner:ident) => {
        /// # Safety
        ///
        /// `value` must be a valid NaN-boxed Perry JSValue.
        #[no_mangle]
        pub unsafe extern "C" fn $name(handle: Handle, value: f64) -> f64 {
            let other = js_decimal_coerce_to_handle(value);
            $inner(handle, other)
        }
    };
}

binop_value!(js_decimal_plus_value, js_decimal_plus);
binop_value!(js_decimal_minus_value, js_decimal_minus);
binop_value!(js_decimal_times_value, js_decimal_times);
binop_value!(js_decimal_div_value, js_decimal_div);
binop_value!(js_decimal_mod_value, js_decimal_mod);

cmp_value!(js_decimal_eq_value, js_decimal_eq);
cmp_value!(js_decimal_lt_value, js_decimal_lt);
cmp_value!(js_decimal_lte_value, js_decimal_lte);
cmp_value!(js_decimal_gt_value, js_decimal_gt);
cmp_value!(js_decimal_gte_value, js_decimal_gte);
cmp_value!(js_decimal_cmp_value, js_decimal_cmp);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_round_trip() {
        let a = js_decimal_from_number(1.5);
        let b = js_decimal_from_number(2.5);
        let sum = js_decimal_plus(a, b);
        assert_eq!(js_decimal_to_number(sum), 4.0);

        let prod = js_decimal_times(a, b);
        assert_eq!(js_decimal_to_number(prod), 3.75);
    }

    #[test]
    fn comparison_predicates() {
        let one = js_decimal_from_number(1.0);
        let two = js_decimal_from_number(2.0);
        assert_eq!(js_decimal_lt(one, two), 1.0);
        assert_eq!(js_decimal_gt(one, two), 0.0);
        assert_eq!(js_decimal_eq(one, one), 1.0);
        assert_eq!(js_decimal_cmp(one, two), -1.0);
        assert_eq!(js_decimal_cmp(two, one), 1.0);
        assert_eq!(js_decimal_cmp(one, one), 0.0);
    }

    #[test]
    fn divide_by_zero_returns_zero() {
        let one = js_decimal_from_number(1.0);
        let zero = js_decimal_from_number(0.0);
        let q = js_decimal_div(one, zero);
        assert_eq!(js_decimal_to_number(q), 0.0);
    }

    #[test]
    fn to_fixed_rounds() {
        let pi = js_decimal_from_number(3.14159);
        let s_ptr = js_decimal_to_fixed(pi, 2.0);
        let s = read_string(unsafe { JsString::from_raw(s_ptr as *mut _) }).expect("non-null");
        assert_eq!(s, "3.14");
    }

    fn nanbox_int32(n: i32) -> f64 {
        // Matches perry-runtime value.rs: INT32_TAG = 0x7FFE_0000_0000_0000
        f64::from_bits(0x7FFE_0000_0000_0000 | ((n as u32) as u64))
    }

    fn nanbox_pointer(h: Handle) -> f64 {
        // POINTER_TAG = 0x7FFD_0000_0000_0000 | (ptr & 0xFFFF_FFFF_FFFF)
        f64::from_bits(0x7FFD_0000_0000_0000 | ((h as u64) & 0x0000_FFFF_FFFF_FFFF))
    }

    #[test]
    fn coerce_plain_double() {
        let h = unsafe { js_decimal_coerce_to_handle(2.5) };
        assert_eq!(js_decimal_to_number(h), 2.5);
    }

    #[test]
    fn coerce_int32_tag() {
        let h = unsafe { js_decimal_coerce_to_handle(nanbox_int32(7)) };
        assert_eq!(js_decimal_to_number(h), 7.0);
    }

    #[test]
    fn coerce_pointer_roundtrips() {
        let a = js_decimal_from_number(3.5);
        let boxed = nanbox_pointer(a);
        let h = unsafe { js_decimal_coerce_to_handle(boxed) };
        assert_eq!(h, a);
    }

    #[test]
    fn plus_value_with_pointer() {
        // 0.1 + 0.2 via `a.plus(b)` where b is a Decimal handle NaN-boxed
        let a = unsafe { js_decimal_from_string(alloc_string("0.1").as_raw()) };
        let b = unsafe { js_decimal_from_string(alloc_string("0.2").as_raw()) };
        let sum = unsafe { js_decimal_plus_value(a, nanbox_pointer(b)) };
        let s_ptr = js_decimal_to_string(sum);
        let s = read_string(unsafe { JsString::from_raw(s_ptr as *mut _) }).expect("non-null");
        assert_eq!(s, "0.3");
    }

    #[test]
    fn plus_value_with_number() {
        let a = js_decimal_from_number(1.5);
        let sum = unsafe { js_decimal_plus_value(a, 2.25) };
        assert_eq!(js_decimal_to_number(sum), 3.75);
    }

    #[test]
    fn eq_value_with_int32() {
        let a = js_decimal_from_number(7.0);
        assert_eq!(unsafe { js_decimal_eq_value(a, nanbox_int32(7)) }, 1.0);
        assert_eq!(unsafe { js_decimal_eq_value(a, nanbox_int32(8)) }, 0.0);
    }
}
