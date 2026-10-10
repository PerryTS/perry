//! BigInt values use GC-owned, variable-length two's-complement limbs.
//! The payload has no pointers or separately owned allocations: moving and
//! sweeping it use the arena allocation's size, just like other leaf objects.

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

use num_bigint::BigInt;
use num_traits::{FromPrimitive, Signed, ToPrimitive, Zero};

/// Inline limb capacity. This is a minimum allocation size, not a precision limit.
pub const BIGINT_LIMBS: usize = 16;
const ZERO_LIMBS: [u64; BIGINT_LIMBS] = [0; BIGINT_LIMBS];

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

/// Match V8's implementation limit, rather than the old signed i1024 limit.
const MAX_BIGINT_BITS: u64 = 1 << 30;

#[cold]
#[inline(never)]
fn throw_bigint_overflow() -> ! {
    throw_bigint_range_error("Maximum BigInt size exceeded");
}

/// The inline prefix preserves cheap low-word access for explicitly narrowing
/// APIs (BigInt64Array, Buffer, FFI). Additional limbs follow the header in the
/// same arena allocation. All whole-value readers must use `all_limbs()`.
#[repr(C)]
pub struct BigIntHeader {
    pub limb_count: usize,
    pub limbs: [u64; BIGINT_LIMBS],
}

impl BigIntHeader {
    /// Borrow the complete little-endian two's-complement payload.
    ///
    /// # Safety
    /// `ptr` must identify a live allocation containing `limb_count`
    /// words, including any words following the inline prefix. That allocation
    /// must remain live and immutable for the returned borrow.
    pub unsafe fn all_limbs<'a>(ptr: *const Self) -> &'a [u64] {
        // Every runtime constructor allocates this many contiguous words.
        unsafe {
            std::slice::from_raw_parts(
                std::ptr::addr_of!((*ptr).limbs).cast::<u64>(),
                (*ptr).limb_count,
            )
        }
    }
}

fn is_negative(limbs: &[u64]) -> bool {
    limbs.last().is_some_and(|word| word >> 63 != 0)
}

#[cfg(test)]
fn fits_in_i64(limbs: &[u64]) -> Option<i64> {
    let lo = *limbs.first().unwrap_or(&0);
    let fill = if lo >> 63 != 0 { u64::MAX } else { 0 };
    limbs[1..]
        .iter()
        .all(|word| *word == fill)
        .then_some(lo as i64)
}

#[cfg(test)]
fn write_i128(value: i128, limbs: &mut [u64; BIGINT_LIMBS]) {
    *limbs = [if value < 0 { u64::MAX } else { 0 }; BIGINT_LIMBS];
    limbs[0] = value as u64;
    limbs[1] = (value >> 64) as u64;
}

fn integer_from_limbs(limbs: &[u64]) -> BigInt {
    let bytes: Vec<u8> = limbs.iter().flat_map(|word| word.to_le_bytes()).collect();
    BigInt::from_signed_bytes_le(&bytes)
}

fn integer_limbs(value: &BigInt) -> Vec<u64> {
    if value.bits() > MAX_BIGINT_BITS {
        throw_bigint_overflow();
    }
    let mut bytes = value.to_signed_bytes_le();
    let count = bytes.len().div_ceil(8).max(BIGINT_LIMBS);
    bytes.resize(count * 8, if value.is_negative() { 0xff } else { 0 });
    bytes
        .chunks_exact(8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

pub(crate) fn bigint_alloc_with_limbs(limbs: impl AsRef<[u64]>) -> *mut BigIntHeader {
    // Canonicalize the sign extension so equal values also hash identically.
    bigint_alloc_integer(integer_from_limbs(limbs.as_ref()))
}

fn bigint_alloc_integer(value: BigInt) -> *mut BigIntHeader {
    let limbs = integer_limbs(&value);
    let size = std::mem::size_of::<BigIntHeader>() + (limbs.len() - BIGINT_LIMBS) * 8;
    let ptr = crate::arena::arena_alloc_gc(
        size,
        std::mem::align_of::<BigIntHeader>(),
        crate::gc::GC_TYPE_BIGINT,
    ) as *mut BigIntHeader;
    unsafe {
        (*ptr).limb_count = limbs.len();
        std::ptr::copy_nonoverlapping(
            limbs.as_ptr(),
            std::ptr::addr_of_mut!((*ptr).limbs).cast::<u64>(),
            limbs.len(),
        );
    }
    ptr
}

fn bigint_integer(a: *const BigIntHeader) -> BigInt {
    let a = clean_bigint_ptr(a);
    if a.is_null() {
        BigInt::zero()
    } else {
        unsafe { integer_from_limbs(crate::bigint::BigIntHeader::all_limbs(a)) }
    }
}

fn bigint_limbs_or_zero(a: *const BigIntHeader) -> Vec<u64> {
    let a = clean_bigint_ptr(a);
    if a.is_null() {
        ZERO_LIMBS.to_vec()
    } else {
        unsafe { crate::bigint::BigIntHeader::all_limbs(a).to_vec() }
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
    throw_bigint_range_error("Division by zero");
}
