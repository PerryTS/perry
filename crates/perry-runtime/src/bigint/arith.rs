//! BigInt arithmetic, using the same precision for constructors and operators.
use super::*;

#[no_mangle]
pub extern "C" fn js_bigint_is_negative(a: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    i32::from(!a.is_null() && unsafe { is_negative(crate::bigint::BigIntHeader::all_limbs(a)) })
}

#[no_mangle]
pub extern "C" fn js_bigint_is_zero(a: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    i32::from(
        a.is_null()
            || unsafe {
                crate::bigint::BigIntHeader::all_limbs(a)
                    .iter()
                    .all(|word| *word == 0)
            },
    )
}

#[no_mangle]
pub extern "C" fn js_bigint_neg(a: *const BigIntHeader) -> *mut BigIntHeader {
    bigint_alloc_integer(-bigint_integer(a))
}

#[no_mangle]
pub extern "C" fn js_bigint_not(a: *const BigIntHeader) -> *mut BigIntHeader {
    bigint_alloc_integer(!bigint_integer(a))
}

macro_rules! binary {
    ($name:ident, $op:tt) => {
        #[no_mangle]
        pub extern "C" fn $name(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
            // Own both operands before the arena allocation can collect.
            bigint_alloc_integer(bigint_integer(a) $op bigint_integer(b))
        }
    };
}
binary!(js_bigint_add, +);
binary!(js_bigint_sub, -);
binary!(js_bigint_mul, *);

#[no_mangle]
pub extern "C" fn js_bigint_div(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a = bigint_integer(a);
    let b = bigint_integer(b);
    if b.is_zero() {
        throw_bigint_division_by_zero();
    }
    bigint_alloc_integer(a / b)
}

#[no_mangle]
pub extern "C" fn js_bigint_mod(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a = bigint_integer(a);
    let b = bigint_integer(b);
    if b.is_zero() {
        throw_bigint_division_by_zero();
    }
    bigint_alloc_integer(a % b)
}

#[no_mangle]
pub extern "C" fn js_bigint_pow(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a = bigint_integer(a);
    let b = bigint_integer(b);
    if b.is_negative() {
        throw_bigint_range_error("Exponent must be positive");
    }
    let result = if b.is_zero() {
        BigInt::from(1)
    } else if a.is_zero() || a == BigInt::from(1) {
        a
    } else if a == BigInt::from(-1) {
        if (&b & BigInt::from(1)).is_zero() {
            BigInt::from(1)
        } else {
            a
        }
    } else {
        let Some(exp) = b.to_u32() else {
            throw_bigint_overflow();
        };
        if (a.bits() - 1).saturating_mul(exp as u64) >= MAX_BIGINT_BITS {
            throw_bigint_overflow();
        }
        a.pow(exp)
    };
    bigint_alloc_integer(result)
}
