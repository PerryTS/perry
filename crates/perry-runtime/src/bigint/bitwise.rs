//! BigInt bitwise operations have infinite two's-complement sign extension.
use super::*;

fn shift(a: *const BigIntHeader, b: *const BigIntHeader, left: bool) -> *mut BigIntHeader {
    let a = bigint_integer(a);
    let b = bigint_integer(b);
    let left = left != b.is_negative();
    let count = b.abs().to_u64();
    let result = if a.is_zero() {
        a
    } else if left {
        let Some(count) = count else {
            throw_bigint_overflow();
        };
        if count.saturating_add(a.bits()) > MAX_BIGINT_BITS {
            throw_bigint_overflow();
        }
        a << count as usize
    } else if count.is_none_or(|n| n >= a.bits()) {
        BigInt::from(if a.is_negative() { -1 } else { 0 })
    } else {
        a >> count.unwrap() as usize
    };
    bigint_alloc_integer(result)
}

#[no_mangle]
pub extern "C" fn js_bigint_shl(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    shift(a, b, true)
}
#[no_mangle]
pub extern "C" fn js_bigint_shr(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    shift(a, b, false)
}
macro_rules! bitwise {
    ($name:ident, $op:tt) => {
        #[no_mangle]
        pub extern "C" fn $name(a: *const BigIntHeader, b: *const BigIntHeader) -> *mut BigIntHeader {
            bigint_alloc_integer(bigint_integer(a) $op bigint_integer(b))
        }
    };
}
bitwise!(js_bigint_and, &);
bitwise!(js_bigint_or, |);
bitwise!(js_bigint_xor, ^);

fn as_n(bits: u32, a: *const BigIntHeader, signed: bool) -> *mut BigIntHeader {
    let value = bigint_integer(a);
    if bits == 0 {
        return bigint_alloc_integer(BigInt::zero());
    }
    // Already representable: avoid constructing an enormous mask for a wide
    // ToIndex argument. Unsigned negative values always need wrapping.
    if (signed || !value.is_negative()) && (bits as u64) > value.bits() {
        return bigint_alloc_integer(value);
    }
    if bits as u64 > MAX_BIGINT_BITS {
        throw_bigint_overflow();
    }
    let modulus = BigInt::from(1) << bits as usize;
    let mut wrapped = value & (&modulus - 1);
    if signed && wrapped >= (&modulus >> 1usize) {
        wrapped -= modulus;
    }
    bigint_alloc_integer(wrapped)
}
#[no_mangle]
pub extern "C" fn js_bigint_as_uint_n(bits: u32, value: *const BigIntHeader) -> *mut BigIntHeader {
    as_n(bits, value, false)
}
#[no_mangle]
pub extern "C" fn js_bigint_as_int_n(bits: u32, value: *const BigIntHeader) -> *mut BigIntHeader {
    as_n(bits, value, true)
}
