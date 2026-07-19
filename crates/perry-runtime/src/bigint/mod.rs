//! BigInt runtime support for Perry
//!
//! Provides 1024-bit integer arithmetic for cryptocurrency operations.
//! Uses 16 x u64 limbs in little-endian order.
//! 1024 bits is needed because secp256k1 (used by ethers.js/noble-curves)
//! has a ~256-bit prime, and intermediate products (a*b before mod reduction)
//! can be ~512 bits. With 512-bit two's complement, bit 511 is the sign bit,
//! causing false negatives. 1024 bits keeps the sign bit at bit 1023.

mod arith;
mod bitwise;
mod compare;
mod convert;
#[cfg(test)]
mod tests;

pub use arith::*;
pub use bitwise::*;
pub use compare::*;
pub(crate) use compare::{bigint_cmp_f64, string_to_bigint};
pub use convert::*;

/// Number of 64-bit limbs in a BigInt (1024 bits total)
pub const BIGINT_LIMBS: usize = 16;
/// Total number of bits
const BIGINT_BITS: usize = BIGINT_LIMBS * 64;

const ZERO_LIMBS: [u64; BIGINT_LIMBS] = [0; BIGINT_LIMBS];
const DIVISION_BY_ZERO_MESSAGE: &[u8] = b"Division by zero";

/// Throw a `TypeError` with the given message (matches Node's BigInt coercion
/// and operator errors). Never returns.
#[cold]
fn throw_bigint_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Throw a `RangeError` with the given message. Never returns.
#[cold]
fn throw_bigint_range_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Throw a `SyntaxError` with the given message. Never returns.
#[cold]
fn throw_bigint_syntax_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// V8 caps BigInt precision and throws `RangeError: Maximum BigInt size
/// exceeded` past it. Perry's representation is a fixed 1024-bit two's
/// complement value (issue #6073); rather than silently wrapping past
/// ±2^1023 like an `i1024`, arithmetic that would overflow throws this same
/// `RangeError` — a loud, catchable failure until true arbitrary precision
/// lands. Its threshold is lower than V8's, but the observable shape matches.
const MAX_BIGINT_SIZE_MESSAGE: &str = "Maximum BigInt size exceeded";

/// Throw the "Maximum BigInt size exceeded" `RangeError` (#6073). Never returns.
#[cold]
#[inline(never)]
fn throw_bigint_overflow() -> ! {
    throw_bigint_range_error(MAX_BIGINT_SIZE_MESSAGE);
}

/// A signed 1024-bit BigInt spans `[-2^1023, 2^1023 - 1]`. Given a
/// non-negative magnitude in little-endian limbs (`mag.len() >= BIGINT_LIMBS`)
/// and the sign the value will carry, report whether it is representable
/// without wrapping. Any set bit at or above 2^1024 (a nonzero limb past the
/// low 16) never fits; a magnitude in `[2^1023, 2^1024)` fits only when it is
/// exactly `2^1023` *and* negative — the single `-2^1023` endpoint.
fn magnitude_fits_1024(mag: &[u64], negative: bool) -> bool {
    debug_assert!(mag.len() >= BIGINT_LIMBS);
    if mag[BIGINT_LIMBS..].iter().any(|&l| l != 0) {
        return false;
    }
    let top = mag[BIGINT_LIMBS - 1];
    if top >> 63 == 0 {
        // magnitude < 2^1023 — representable with either sign.
        return true;
    }
    // magnitude >= 2^1023 — only exactly -2^1023 is representable.
    negative && top == 1u64 << 63 && mag[..BIGINT_LIMBS - 1].iter().all(|&l| l == 0)
}

/// Decode a 1024-bit two's-complement value into a host i64 if it fits.
/// Layout: positive small → all upper limbs zero AND limb[0] high bit clear;
/// negative small → all upper limbs `u64::MAX` AND limb[0] high bit set.
/// Returns None for anything that needs more than 64 bits to represent.
#[inline(always)]
fn fits_in_i64(limbs: &[u64; BIGINT_LIMBS]) -> Option<i64> {
    let lo = limbs[0];
    let hi_bit = lo >> 63;
    let expected_fill = if hi_bit == 0 { 0u64 } else { u64::MAX };
    for &l in &limbs[1..] {
        if l != expected_fill {
            return None;
        }
    }
    Some(lo as i64)
}

/// Write a host i64 into a 1024-bit two's-complement limb array,
/// sign-extending the upper limbs.
#[inline(always)]
fn write_i64(value: i64, limbs: &mut [u64; BIGINT_LIMBS]) {
    let fill = if value < 0 { u64::MAX } else { 0u64 };
    *limbs = [fill; BIGINT_LIMBS];
    limbs[0] = value as u64;
}

/// Write a host i128 into a 1024-bit two's-complement limb array,
/// sign-extending the upper 14 limbs.
#[inline(always)]
fn write_i128(value: i128, limbs: &mut [u64; BIGINT_LIMBS]) {
    let fill = if value < 0 { u64::MAX } else { 0u64 };
    *limbs = [fill; BIGINT_LIMBS];
    let bits = value as u128;
    limbs[0] = bits as u64;
    limbs[1] = (bits >> 64) as u64;
}

/// BigInt is stored as a heap-allocated 1024-bit integer
/// Layout: 128 bytes (16 x u64)
#[repr(C)]
pub struct BigIntHeader {
    /// The 1024-bit value stored as 16 x u64 in little-endian order
    pub limbs: [u64; BIGINT_LIMBS],
}

/// Allocate a BigInt from the arena (bump-pointer, no per-object Vec/HashSet tracking).
///
/// Switching from gc_malloc to arena_alloc_gc eliminates the dominant per-call
/// overhead: system malloc (~30 ns) + MALLOC_STATE Vec push (~10 ns) +
/// HashSet insert (~30 ns) = ~70 ns → reduced to ~20 ns bump-pointer.
/// Arena objects are discovered by linear block walking at GC time; the mark
/// phase already handles GC_TYPE_BIGINT (no child references to trace).
#[inline]
fn bigint_alloc() -> *mut BigIntHeader {
    let raw = crate::arena::arena_alloc_gc(
        std::mem::size_of::<BigIntHeader>(),
        std::mem::align_of::<BigIntHeader>(),
        crate::gc::GC_TYPE_BIGINT,
    );
    raw as *mut BigIntHeader
}

#[inline]
pub(crate) fn bigint_alloc_with_limbs(limbs: [u64; BIGINT_LIMBS]) -> *mut BigIntHeader {
    let ptr = bigint_alloc();
    unsafe {
        (*ptr).limbs = limbs;
    }
    ptr
}

#[inline(always)]
fn bigint_limbs_or_zero(a: *const BigIntHeader) -> [u64; BIGINT_LIMBS] {
    let a = clean_bigint_ptr(a);
    if a.is_null() {
        ZERO_LIMBS
    } else {
        unsafe { (*a).limbs }
    }
}

/// Strip NaN-boxing tags from a BigInt pointer (defensive guard).
/// Returns null if the value is not a valid bigint pointer.
#[inline(always)]
pub fn clean_bigint_ptr(p: *const BigIntHeader) -> *const BigIntHeader {
    let bits = p as u64;
    let top16 = bits >> 48;
    if top16 >= 0x7FF8 {
        // NaN-boxed value — extract lower 48 bits
        let raw = (bits & 0x0000_FFFF_FFFF_FFFF) as *const BigIntHeader;
        if (raw as usize) < 0x10000 {
            return std::ptr::null();
        }
        raw
    } else if bits < 0x10000 {
        std::ptr::null()
    } else if top16 != 0 {
        // Non-zero upper 16 bits but not NaN-boxed — not a valid heap pointer
        // (e.g., raw f64 bits from js_nanbox_get_bigint fallback)
        std::ptr::null()
    } else {
        p
    }
}

#[inline(always)]
pub fn clean_bigint_ptr_mut(p: *mut BigIntHeader) -> *mut BigIntHeader {
    clean_bigint_ptr(p as *const BigIntHeader) as *mut BigIntHeader
}

#[cold]
fn throw_bigint_division_by_zero() -> ! {
    let msg = crate::string::js_string_from_bytes(
        DIVISION_BY_ZERO_MESSAGE.as_ptr(),
        DIVISION_BY_ZERO_MESSAGE.len() as u32,
    );
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Check if a bigint value is negative (high bit of highest limb is set = two's complement negative)
fn is_negative(limbs: &[u64; BIGINT_LIMBS]) -> bool {
    (limbs[BIGINT_LIMBS - 1] >> 63) == 1
}

/// Negate limbs in place (two's complement: flip all bits and add 1)
fn negate_limbs(limbs: &[u64; BIGINT_LIMBS]) -> [u64; BIGINT_LIMBS] {
    let mut result = ZERO_LIMBS;
    let mut carry = 1u64;
    for i in 0..BIGINT_LIMBS {
        let flipped = !limbs[i];
        let sum = (flipped as u128) + (carry as u128);
        result[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }
    result
}
