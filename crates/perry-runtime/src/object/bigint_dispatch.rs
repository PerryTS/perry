//! BigInt binary-method dispatch (called from `js_native_call_method` when
//! the receiver is `BIGINT_TAG`).
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

/// Dispatch BigInt binary methods (add, sub, mul, div, mod, etc.)
/// Called from js_native_call_method when object is BIGINT_TAG.
pub(crate) unsafe fn dispatch_bigint_binary_method(
    a: *const crate::bigint::BigIntHeader,
    method: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Extract second operand from args (if any)
    let b = if args_len > 0 && !args_ptr.is_null() {
        let arg_f64 = *args_ptr;
        let arg_jsval = JSValue::from_bits(arg_f64.to_bits());
        if arg_jsval.is_bigint() {
            crate::bigint::clean_bigint_ptr(
                (arg_f64.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader,
            )
        } else {
            // Try to convert number to BigInt
            crate::bigint::js_bigint_from_f64(arg_f64)
        }
    } else {
        std::ptr::null()
    };

    match method {
        // Binary arithmetic → returns BigInt
        "add" => {
            let result = crate::bigint::js_bigint_add(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "sub" => {
            let result = crate::bigint::js_bigint_sub(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "mul" => {
            let result = crate::bigint::js_bigint_mul(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "div" => {
            let result = crate::bigint::js_bigint_div(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "mod" | "umod" => {
            let result = crate::bigint::js_bigint_mod(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "pow" => {
            let result = crate::bigint::js_bigint_pow(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "and" => {
            let result = crate::bigint::js_bigint_and(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "or" => {
            let result = crate::bigint::js_bigint_or(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "xor" => {
            let result = crate::bigint::js_bigint_xor(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "shln" => {
            let result = crate::bigint::js_bigint_shl(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "shrn" => {
            let result = crate::bigint::js_bigint_shr(a, b);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "maskn" => {
            // maskn(bits) — mask to lowest N bits
            let result = crate::bigint::js_bigint_and(a, b); // approximate
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        // Comparison → returns boolean/number
        "eq" => {
            let result = crate::bigint::js_bigint_eq(a, b);
            f64::from_bits(JSValue::bool(result != 0).bits())
        }
        "lt" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            f64::from_bits(JSValue::bool(result < 0).bits())
        }
        "lte" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            f64::from_bits(JSValue::bool(result <= 0).bits())
        }
        "gt" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            f64::from_bits(JSValue::bool(result > 0).bits())
        }
        "gte" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            f64::from_bits(JSValue::bool(result >= 0).bits())
        }
        "cmp" => {
            let result = crate::bigint::js_bigint_cmp(a, b);
            result as f64
        }
        "fromTwos" => {
            // bn.js: interpret `a` as the unsigned encoding of a signed
            // `width`-bit integer in two's complement. If bit (width-1) of
            // `a` is set the result is `a - 2^width`; otherwise return `a`.
            // `width` arrives in `b` (already a BigInt — see top of fn).
            let width = if b.is_null() { 0u64 } else { (*b).limbs[0] };
            let max_bits = (crate::bigint::BIGINT_LIMBS * 64) as u64;
            if width == 0 || width > max_bits {
                return f64::from_bits(
                    JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits(),
                );
            }
            let bit = (width - 1) as usize;
            let high_bit_set = ((*a).limbs[bit / 64] >> (bit % 64)) & 1 == 1;
            if !high_bit_set {
                return f64::from_bits(
                    JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits(),
                );
            }
            let one = crate::bigint::js_bigint_from_u64(1);
            let two_pow = crate::bigint::js_bigint_shl(one, b);
            let result = crate::bigint::js_bigint_sub(a, two_pow);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        "toTwos" => {
            // bn.js: convert to `width`-bit two's complement encoding. If `a`
            // is negative the result is `a + 2^width` (mod 2^width);
            // otherwise return `a` unchanged. bn.js does not mask
            // non-negative inputs to `width` bits, so neither do we.
            let width = if b.is_null() { 0u64 } else { (*b).limbs[0] };
            let max_bits = (crate::bigint::BIGINT_LIMBS * 64) as u64;
            if width == 0 || width > max_bits {
                return f64::from_bits(
                    JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits(),
                );
            }
            if crate::bigint::js_bigint_is_negative(a) == 0 {
                return f64::from_bits(
                    JSValue::bigint_ptr(a as *mut crate::bigint::BigIntHeader).bits(),
                );
            }
            let one = crate::bigint::js_bigint_from_u64(1);
            let two_pow = crate::bigint::js_bigint_shl(one, b);
            let result = crate::bigint::js_bigint_add(a, two_pow);
            f64::from_bits(JSValue::bigint_ptr(result).bits())
        }
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}
