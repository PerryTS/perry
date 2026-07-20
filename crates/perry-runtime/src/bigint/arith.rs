//! BigInt arithmetic: negation, add/sub/mul, division, modulo, power.

use super::*;

/// Check if BigInt is negative (MSB set in two's complement)
#[no_mangle]
pub extern "C" fn js_bigint_is_negative(a: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    if a.is_null() {
        return 0;
    }
    unsafe {
        // In two's complement, negative numbers have MSB set in highest limb
        let msb = (*a).limbs[BIGINT_LIMBS - 1];
        if msb & (1u64 << 63) != 0 {
            1
        } else {
            0
        }
    }
}

/// Negate a BigInt (two's complement: flip all bits and add 1)
#[no_mangle]
pub extern "C" fn js_bigint_neg(a: *const BigIntHeader) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let mut result = ZERO_LIMBS;
    let mut carry = 1u64;

    for i in 0..BIGINT_LIMBS {
        let flipped = !a_limbs[i];
        let sum = (flipped as u128) + (carry as u128);
        result[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }

    // -2^1023 is the sole nonzero value whose two's-complement negation is
    // itself; its true negation (+2^1023) is out of range, so throw rather
    // than silently returning -2^1023 unchanged (#6073).
    if result == a_limbs && a_limbs != ZERO_LIMBS {
        throw_bigint_overflow();
    }

    bigint_alloc_with_limbs(result)
}

/// Bitwise NOT of a BigInt (`~a`).
#[no_mangle]
pub extern "C" fn js_bigint_not(a: *const BigIntHeader) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let mut result = ZERO_LIMBS;
    for i in 0..BIGINT_LIMBS {
        result[i] = !a_limbs[i];
    }
    bigint_alloc_with_limbs(result)
}

/// Check if a BigInt is zero (all limbs are zero). Returns 1 for zero, 0 for non-zero.
#[no_mangle]
pub extern "C" fn js_bigint_is_zero(a: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    if a.is_null() {
        return 1;
    }
    unsafe {
        for i in 0..BIGINT_LIMBS {
            if (*a).limbs[i] != 0 {
                return 0;
            }
        }
        1
    }
}

/// Add two BigInts
#[no_mangle]
pub extern "C" fn js_bigint_add(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    // Fast path: both operands fit in i64. i64 + i64 fits in i128 with
    // no overflow possible, then write the result back as a sign-extended
    // 1024-bit two's-complement value.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        let mut result = ZERO_LIMBS;
        write_i128((av as i128) + (bv as i128), &mut result);
        return bigint_alloc_with_limbs(result);
    }

    // Slow path: 16-limb add with carry.
    let mut result = ZERO_LIMBS;
    let mut carry = 0u64;
    for i in 0..BIGINT_LIMBS {
        let sum = (a_limbs[i] as u128) + (b_limbs[i] as u128) + (carry as u128);
        result[i] = sum as u64;
        carry = (sum >> 64) as u64;
    }
    // Signed overflow (#6073): equal-signed operands whose sum takes the
    // opposite sign wrapped past ±2^1023. Throw rather than return the ring
    // value. (Opposite-signed operands can never overflow.)
    if is_negative(&a_limbs) == is_negative(&b_limbs)
        && is_negative(&result) != is_negative(&a_limbs)
    {
        throw_bigint_overflow();
    }
    bigint_alloc_with_limbs(result)
}

/// Subtract two BigInts (a - b)
#[no_mangle]
pub extern "C" fn js_bigint_sub(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    // Fast path: both operands fit in i64. i64 - i64 fits in i128.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        let mut result = ZERO_LIMBS;
        write_i128((av as i128) - (bv as i128), &mut result);
        return bigint_alloc_with_limbs(result);
    }

    // Slow path: 16-limb subtract with borrow.
    let mut result = ZERO_LIMBS;
    let mut borrow = 0i128;
    for i in 0..BIGINT_LIMBS {
        let diff = (a_limbs[i] as i128) - (b_limbs[i] as i128) - borrow;
        if diff < 0 {
            result[i] = (diff + (1i128 << 64)) as u64;
            borrow = 1;
        } else {
            result[i] = diff as u64;
            borrow = 0;
        }
    }
    // Signed overflow (#6073): opposite-signed operands whose difference takes
    // the sign opposite the minuend wrapped past ±2^1023. (Equal-signed
    // operands can never overflow — this also catches negating -2^1023.)
    if is_negative(&a_limbs) != is_negative(&b_limbs)
        && is_negative(&result) != is_negative(&a_limbs)
    {
        throw_bigint_overflow();
    }
    bigint_alloc_with_limbs(result)
}

/// Multiply two BigInts
#[no_mangle]
pub extern "C" fn js_bigint_mul(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    // Fast path: both operands fit in i64. i64 * i64 fits exactly in i128
    // (max |product| = (2^63)^2 = 2^126 < 2^127). This eliminates the 16×16
    // schoolbook loop for the common case where values fit in a host word.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        let mut result = ZERO_LIMBS;
        write_i128((av as i128) * (bv as i128), &mut result);
        return bigint_alloc_with_limbs(result);
    }

    // Slow path: sign-and-magnitude schoolbook multiply that retains the full
    // 2N-limb product so overflow past ±2^1023 is detected, not wrapped (#6073).
    bigint_alloc_with_limbs(mul_limbs_checked(&a_limbs, &b_limbs))
}

/// Multiply two two's-complement limb arrays, throwing `RangeError` when the
/// product exceeds the signed 1024-bit range (#6073). Shared by
/// `js_bigint_mul`'s slow path and `js_bigint_pow`.
///
/// Multiplies the magnitudes (so the sign-extension words never pollute the
/// schoolbook rows) into a `2 * BIGINT_LIMBS` accumulator, checks the result
/// fits, then re-applies the sign. Using two's-complement limbs directly would
/// require carrying the sign extension through every row — sign-and-magnitude
/// is cleaner.
fn mul_limbs_checked(
    a_limbs: &[u64; BIGINT_LIMBS],
    b_limbs: &[u64; BIGINT_LIMBS],
) -> [u64; BIGINT_LIMBS] {
    let a_neg = is_negative(a_limbs);
    let b_neg = is_negative(b_limbs);
    let a_mag = if a_neg {
        negate_limbs(a_limbs)
    } else {
        *a_limbs
    };
    let b_mag = if b_neg {
        negate_limbs(b_limbs)
    } else {
        *b_limbs
    };

    // Skip trailing all-zero limbs so e.g. a 3-limb value times a 2-limb value
    // only does 3×2 word multiplies. `effective_limb_len` never under-counts an
    // in-range magnitude (<= 2^1023), so no significant limb is dropped.
    let a_len = effective_limb_len(&a_mag);
    let b_len = effective_limb_len(&b_mag);
    // The full product of two <2^1024 magnitudes is <2^2048 — it fits in the
    // 2N-limb accumulator, and the carry never escapes index `a_len+b_len-1`.
    let mut wide = [0u64; 2 * BIGINT_LIMBS];
    for i in 0..a_len {
        let mut carry = 0u128;
        for j in 0..b_len {
            let product = (a_mag[i] as u128) * (b_mag[j] as u128) + (wide[i + j] as u128) + carry;
            wide[i + j] = product as u64;
            carry = product >> 64;
        }
        let mut k = i + b_len;
        while carry != 0 && k < 2 * BIGINT_LIMBS {
            let sum = (wide[k] as u128) + carry;
            wide[k] = sum as u64;
            carry = sum >> 64;
            k += 1;
        }
    }

    let negative = a_neg != b_neg;
    if !magnitude_fits_1024(&wide, negative) {
        throw_bigint_overflow();
    }
    let mut result = ZERO_LIMBS;
    result.copy_from_slice(&wide[..BIGINT_LIMBS]);
    if negative {
        result = negate_limbs(&result);
    }
    result
}

/// Magnitude of significant limbs for an unsigned-style limb pattern.
/// For a positive value, returns 1 + index of highest non-zero limb (or 1
/// for zero, since we always need at least one word multiplied). Negative
/// values are handled by their two's-complement: limbs[15] has bit 63 set
/// and the upper limbs may be u64::MAX, so we walk from the top until we
/// find a limb that's neither all-zero nor all-ones.
#[inline(always)]
fn effective_limb_len(limbs: &[u64; BIGINT_LIMBS]) -> usize {
    // Walk from the high end, skipping consecutive all-zero or all-ones
    // limbs (the sign-extension fill). The first limb that breaks the
    // pattern is the highest "real" limb. This is sound for the
    // schoolbook multiplier because we only read the first `len` limbs.
    let fill = if (limbs[BIGINT_LIMBS - 1] >> 63) == 1 {
        u64::MAX
    } else {
        0u64
    };
    for i in (0..BIGINT_LIMBS).rev() {
        if limbs[i] != fill {
            // We need one more limb than i+1 if the next one isn't already
            // the fill (it always is, by construction), so just i+1 plus
            // one safety limb capped at BIGINT_LIMBS.
            return (i + 2).min(BIGINT_LIMBS);
        }
    }
    // All limbs are fill — value is 0 or -1. Either way we only need 1.
    1
}

/// Unsigned binary long division on magnitude limbs
fn unsigned_div_limbs(
    a: &[u64; BIGINT_LIMBS],
    b: &[u64; BIGINT_LIMBS],
) -> ([u64; BIGINT_LIMBS], [u64; BIGINT_LIMBS]) {
    let mut quotient = ZERO_LIMBS;
    let mut remainder = ZERO_LIMBS;

    for i in (0..BIGINT_BITS).rev() {
        // Shift remainder left by 1
        let mut carry = 0u64;
        for limb in remainder.iter_mut() {
            let new_carry = *limb >> 63;
            *limb = (*limb << 1) | carry;
            carry = new_carry;
        }

        // Set LSB of remainder from dividend
        let limb_idx = i / 64;
        let bit_idx = i % 64;
        remainder[0] |= (a[limb_idx] >> bit_idx) & 1;

        // If remainder >= divisor, subtract and set quotient bit
        // Use unsigned comparison for magnitude comparison
        let mut ge = true;
        for j in (0..BIGINT_LIMBS).rev() {
            if remainder[j] > b[j] {
                break;
            }
            if remainder[j] < b[j] {
                ge = false;
                break;
            }
        }
        if ge {
            subtract_limbs(&mut remainder, b);
            let q_limb_idx = i / 64;
            let q_bit_idx = i % 64;
            quotient[q_limb_idx] |= 1u64 << q_bit_idx;
        }
    }

    (quotient, remainder)
}

/// Divide two BigInts (a / b) — truncates toward zero like JavaScript
#[no_mangle]
pub extern "C" fn js_bigint_div(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    if b_limbs == ZERO_LIMBS {
        throw_bigint_division_by_zero();
    }

    // Fast path: both fit in i64. Rust's `/` on i64 truncates toward
    // zero, which is JavaScript's BigInt division semantics. The only
    // overflow case is i64::MIN / -1, which we handle via i128.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        if bv != 0 {
            let mut result = ZERO_LIMBS;
            write_i128((av as i128) / (bv as i128), &mut result);
            return bigint_alloc_with_limbs(result);
        }
    }

    let a_neg = is_negative(&a_limbs);
    let b_neg = is_negative(&b_limbs);

    // Get magnitudes
    let abs_a = if a_neg {
        negate_limbs(&a_limbs)
    } else {
        a_limbs
    };
    let abs_b = if b_neg {
        negate_limbs(&b_limbs)
    } else {
        b_limbs
    };

    let (quotient, _) = unsigned_div_limbs(&abs_a, &abs_b);

    // Result is negative if signs differ
    let result = if a_neg != b_neg && quotient != ZERO_LIMBS {
        negate_limbs(&quotient)
    } else {
        quotient
    };
    bigint_alloc_with_limbs(result)
}

/// Modulo of two BigInts (a % b) — result has sign of dividend (like JavaScript)
#[no_mangle]
pub extern "C" fn js_bigint_mod(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    if b_limbs == ZERO_LIMBS {
        throw_bigint_division_by_zero();
    }

    // Fast path: both fit in i64. JavaScript's `%` returns the sign of
    // the dividend, which is what Rust's `%` on i64 does already.
    if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
        // bv != 0 because b_limbs != ZERO_LIMBS for a positive small;
        // for a negative small we still won't hit divide-by-zero.
        if bv != 0 {
            let mut result = ZERO_LIMBS;
            write_i64(av % bv, &mut result);
            return bigint_alloc_with_limbs(result);
        }
    }

    let a_neg = is_negative(&a_limbs);
    let b_neg = is_negative(&b_limbs);

    // Get magnitudes
    let abs_a = if a_neg {
        negate_limbs(&a_limbs)
    } else {
        a_limbs
    };
    let abs_b = if b_neg {
        negate_limbs(&b_limbs)
    } else {
        b_limbs
    };

    let (_, remainder) = unsigned_div_limbs(&abs_a, &abs_b);

    // Remainder has sign of dividend
    let result = if a_neg && remainder != ZERO_LIMBS {
        negate_limbs(&remainder)
    } else {
        remainder
    };
    bigint_alloc_with_limbs(result)
}

/// Power of two BigInts (a ** b) using binary exponentiation
/// Note: b is interpreted as a u64 (only lower 64 bits are used)
#[no_mangle]
pub extern "C" fn js_bigint_pow(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);

    // ECMaScript: a negative BigInt exponent is a RangeError.
    if is_negative(&b_limbs) {
        throw_bigint_range_error("Exponent must be positive");
    }

    // Get exponent as u64 (only lower 64 bits)
    let exp = b_limbs[0];

    if exp == 0 {
        // Anything to the power of 0 is 1
        let mut result = ZERO_LIMBS;
        result[0] = 1;
        return bigint_alloc_with_limbs(result);
    }

    // Binary exponentiation
    let mut result = ZERO_LIMBS;
    result[0] = 1;
    let mut base = a_limbs;
    let mut e = exp;

    while e > 0 {
        if e & 1 == 1 {
            result = mul_limbs_checked(&result, &base);
        }
        e >>= 1;
        // Square only while another bit remains. The final squaring after the
        // top set bit is never used, and squaring it could overflow past a
        // result that itself still fits (e.g. (2^300)^3) — a false throw (#6073).
        if e > 0 {
            base = mul_limbs_checked(&base, &base);
        }
    }

    bigint_alloc_with_limbs(result)
}

fn subtract_limbs(a: &mut [u64; BIGINT_LIMBS], b: &[u64; BIGINT_LIMBS]) {
    let mut borrow = 0i128;
    for i in 0..BIGINT_LIMBS {
        let diff = (a[i] as i128) - (b[i] as i128) - borrow;
        if diff < 0 {
            a[i] = (diff + (1i128 << 64)) as u64;
            borrow = 1;
        } else {
            a[i] = diff as u64;
            borrow = 0;
        }
    }
}
