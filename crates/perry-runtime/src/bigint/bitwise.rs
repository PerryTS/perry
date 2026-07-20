//! BigInt shifts, bitwise ops, and BigInt.asIntN / asUintN.

use super::*;

/// Left-shift a two's-complement BigInt by `shift` bits (`a << shift`),
/// throwing `RangeError` when the result exceeds the signed 1024-bit range
/// (#6073). Shifting a magnitude left by N bits multiplies it by 2^N, so any
/// significant bit pushed to >= 2^1023 (positive) / > 2^1023 (negative)
/// overflows instead of silently falling off the top.
fn shl_checked(a_limbs: &[u64; BIGINT_LIMBS], shift: usize) -> [u64; BIGINT_LIMBS] {
    if shift == 0 || a_limbs == &ZERO_LIMBS {
        return *a_limbs;
    }
    // A shift of >= 1024 bits pushes even the lowest set bit of a nonzero
    // magnitude to >= 2^1024, which never fits.
    if shift >= BIGINT_BITS {
        throw_bigint_overflow();
    }

    let a_neg = is_negative(a_limbs);
    let mag = if a_neg {
        negate_limbs(a_limbs)
    } else {
        *a_limbs
    };

    // shift < 1024 and mag < 2^1023 → product < 2^2046, always within the
    // 2N-limb buffer: the top written index is <= 30, plus one carry limb.
    let mut wide = [0u64; 2 * BIGINT_LIMBS];
    let limb_shift = shift / 64;
    let bit_shift = (shift % 64) as u32;
    for i in 0..BIGINT_LIMBS {
        if mag[i] == 0 {
            continue;
        }
        let dst = i + limb_shift;
        if bit_shift == 0 {
            wide[dst] |= mag[i];
        } else {
            wide[dst] |= mag[i] << bit_shift;
            wide[dst + 1] |= mag[i] >> (64 - bit_shift);
        }
    }

    if !magnitude_fits_1024(&wide, a_neg) {
        throw_bigint_overflow();
    }
    let mut result = ZERO_LIMBS;
    result.copy_from_slice(&wide[..BIGINT_LIMBS]);
    if a_neg {
        result = negate_limbs(&result);
    }
    result
}

/// Arithmetic right-shift a limb array by `shift` bits (sign-extending).
fn shr_limbs(a_limbs: &[u64; BIGINT_LIMBS], shift: usize) -> [u64; BIGINT_LIMBS] {
    let neg = is_negative(a_limbs);
    let fill: u64 = if neg { !0u64 } else { 0u64 };
    if shift >= BIGINT_BITS {
        return [fill; BIGINT_LIMBS];
    }
    let mut result = [fill; BIGINT_LIMBS];
    let limb_shift = shift / 64;
    let bit_shift = (shift % 64) as u32;
    if bit_shift == 0 {
        for i in 0..(BIGINT_LIMBS - limb_shift) {
            result[i] = a_limbs[i + limb_shift];
        }
    } else {
        for i in 0..(BIGINT_LIMBS - limb_shift) {
            let src_idx = i + limb_shift;
            result[i] = a_limbs[src_idx] >> bit_shift;
            if src_idx + 1 < BIGINT_LIMBS {
                result[i] |= a_limbs[src_idx + 1] << (64 - bit_shift);
            } else {
                result[i] |= fill << (64 - bit_shift);
            }
        }
    }
    result
}

/// Interpret a two's-complement shift count as a signed magnitude. Returns
/// `(magnitude, count_is_negative)`. Counts beyond `BIGINT_BITS` saturate.
fn shift_count(b_limbs: &[u64; BIGINT_LIMBS]) -> (usize, bool) {
    if is_negative(b_limbs) {
        let mag = negate_limbs(b_limbs);
        // Only the low limb matters for any realistic shift; if any upper
        // limb is set the count is enormous → saturate past BIGINT_BITS.
        if mag[1..].iter().any(|&l| l != 0) {
            (BIGINT_BITS, true)
        } else {
            (mag[0] as usize, true)
        }
    } else {
        if b_limbs[1..].iter().any(|&l| l != 0) {
            (BIGINT_BITS, false)
        } else {
            (b_limbs[0] as usize, false)
        }
    }
}

/// Left shift BigInt by b bits (a << b). A negative shift count reverses
/// direction (`a << -n` === `a >> n`), matching ECMAScript.
#[no_mangle]
pub extern "C" fn js_bigint_shl(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let (shift, negative) = shift_count(&b_limbs);
    let result = if negative {
        shr_limbs(&a_limbs, shift)
    } else {
        shl_checked(&a_limbs, shift)
    };
    bigint_alloc_with_limbs(result)
}

/// Right shift BigInt by b bits (a >> b), arithmetic / sign-extending. A
/// negative shift count reverses direction (`a >> -n` === `a << n`), matching
/// ECMAScript.
#[no_mangle]
pub extern "C" fn js_bigint_shr(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let (shift, negative) = shift_count(&b_limbs);
    let result = if negative {
        shl_checked(&a_limbs, shift)
    } else {
        shr_limbs(&a_limbs, shift)
    };
    bigint_alloc_with_limbs(result)
}

/// Bitwise AND of two BigInts (a & b)
#[no_mangle]
pub extern "C" fn js_bigint_and(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let mut result = ZERO_LIMBS;

    for i in 0..BIGINT_LIMBS {
        result[i] = a_limbs[i] & b_limbs[i];
    }

    bigint_alloc_with_limbs(result)
}

/// Bitwise OR of two BigInts (a | b)
#[no_mangle]
pub extern "C" fn js_bigint_or(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let mut result = ZERO_LIMBS;
    for i in 0..BIGINT_LIMBS {
        result[i] = a_limbs[i] | b_limbs[i];
    }
    bigint_alloc_with_limbs(result)
}

/// Bitwise XOR of two BigInts (a ^ b)
#[no_mangle]
pub extern "C" fn js_bigint_xor(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    let a_limbs = bigint_limbs_or_zero(a);
    let b_limbs = bigint_limbs_or_zero(b);
    let mut result = ZERO_LIMBS;
    for i in 0..BIGINT_LIMBS {
        result[i] = a_limbs[i] ^ b_limbs[i];
    }
    bigint_alloc_with_limbs(result)
}

/// Mask a 1024-bit two's-complement limb array down to its low `bits` bits,
/// zeroing everything at or above bit index `bits`. `bits >= BIGINT_BITS`
/// leaves the value unchanged.
fn mask_low_bits(mut limbs: [u64; BIGINT_LIMBS], bits: usize) -> [u64; BIGINT_LIMBS] {
    if bits >= BIGINT_BITS {
        return limbs;
    }
    let full_limbs = bits / 64;
    let rem = bits % 64;
    for (i, l) in limbs.iter_mut().enumerate() {
        if i < full_limbs {
            // keep
        } else if i == full_limbs && rem != 0 {
            *l &= (1u64 << rem) - 1;
        } else {
            *l = 0;
        }
    }
    limbs
}

/// `BigInt.asUintN(bits, bigint)` — wrap `value` to a `bits`-wide UNSIGNED
/// integer: `value mod 2^bits`, always non-negative. (`asUintN(0, x)` → 0n.)
#[no_mangle]
pub extern "C" fn js_bigint_as_uint_n(bits: u32, value: *const BigIntHeader) -> *mut BigIntHeader {
    let limbs = bigint_limbs_or_zero(value);
    let masked = mask_low_bits(limbs, bits as usize);
    bigint_alloc_with_limbs(masked)
}

/// `BigInt.asIntN(bits, bigint)` — wrap `value` to a `bits`-wide SIGNED
/// two's-complement integer: `value mod 2^bits`, then interpret the top bit
/// (bit `bits-1`) as the sign and sign-extend. (`asIntN(0, x)` → 0n.)
#[no_mangle]
pub extern "C" fn js_bigint_as_int_n(bits: u32, value: *const BigIntHeader) -> *mut BigIntHeader {
    let bits = bits as usize;
    if bits == 0 {
        return bigint_alloc_with_limbs(ZERO_LIMBS);
    }
    if bits >= BIGINT_BITS {
        // No truncation possible within our width; value already two's-complement.
        return bigint_alloc_with_limbs(bigint_limbs_or_zero(value));
    }
    let mut masked = mask_low_bits(bigint_limbs_or_zero(value), bits);
    // If the sign bit (bit bits-1) is set, sign-extend: set all bits >= bits-1.
    let sign_limb = (bits - 1) / 64;
    let sign_pos = (bits - 1) % 64;
    let sign_set = (masked[sign_limb] >> sign_pos) & 1 == 1;
    if sign_set {
        // Set every bit from `bits` upward to 1 (two's-complement negative).
        let full_limbs = bits / 64;
        let rem = bits % 64;
        for (i, l) in masked.iter_mut().enumerate() {
            if i < full_limbs {
                // low full limbs: keep
            } else if i == full_limbs && rem != 0 {
                *l |= !((1u64 << rem) - 1);
            } else if i >= full_limbs {
                *l = u64::MAX;
            }
        }
    }
    bigint_alloc_with_limbs(masked)
}
