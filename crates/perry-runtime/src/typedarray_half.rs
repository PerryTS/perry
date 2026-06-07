//! IEEE-754 binary16 (half-precision) conversion helpers for `Float16Array`
//! (#2902). Extracted from `typedarray.rs` to keep that file under the 2000-line
//! cap. `store_at`/`load_at` in `typedarray.rs` call these for `KIND_FLOAT16`.

/// Encode an f64 into an IEEE-754 binary16 (half-precision) bit pattern.
/// Round-to-nearest-even, with overflow → ±Inf and subnormal/underflow → ±0.
/// Mirrors the V8 / `Math.f16round`-then-store semantics used by Float16Array.
pub fn f64_to_f16_bits(value: f64) -> u16 {
    // Convert DIRECTLY from the f64 (no intermediate f32). Going through f32
    // first double-rounds: a value just past a half-ulp boundary gets rounded
    // to the exact halfway point by the f32 step, losing the sticky bit, so the
    // f16 round-to-even step then rounds the wrong way (e.g. the smallest f16
    // subnormal boundary `2^-25 + ε` collapsed to `2^-25` → 0 instead of
    // `2^-24`). Operating on the 52-bit f64 mantissa keeps the sticky bits.
    let bits = value.to_bits();
    let sign = (((bits >> 48) & 0x8000) as u16) & 0x8000;
    let exp = ((bits >> 52) & 0x7FF) as i32; // f64 biased exponent
    let mantissa = bits & 0x000F_FFFF_FFFF_FFFF; // 52-bit fraction

    if exp == 0x7FF {
        // Inf / NaN.
        if mantissa != 0 {
            // NaN: keep it a NaN (set a mantissa bit), drop payload.
            return sign | 0x7E00;
        }
        return sign | 0x7C00; // Inf
    }

    // ±0 (and f64 subnormals, which are far below the f16 range → ±0).
    if exp == 0 {
        return sign;
    }

    // Unbias f64 exponent (bias 1023), rebias for f16 (bias 15).
    let unbiased = exp - 1023;
    let half_exp = unbiased + 15;

    if half_exp >= 0x1F {
        // Overflow → Inf.
        return sign | 0x7C00;
    }

    // 53-bit significand with the implicit leading 1 restored.
    let significand = mantissa | 0x0010_0000_0000_0000; // bit 52 set

    // Round-half-to-even: shift `significand` right by `shift`, rounding the
    // discarded low bits to nearest, ties to even. Returns the rounded value
    // (which may carry up one extra bit).
    let round = |shift: u32| -> u64 {
        let kept = significand >> shift;
        let remainder = significand & ((1u64 << shift) - 1);
        let halfway = 1u64 << (shift - 1);
        if remainder > halfway || (remainder == halfway && (kept & 1) == 1) {
            kept + 1
        } else {
            kept
        }
    };

    if half_exp <= 0 {
        // Subnormal or underflow to zero. Express the value as a count of f16
        // subnormal units (2^-24), rounding to nearest-even. A carry out of the
        // top subnormal bit naturally yields the smallest normal (0x0400).
        // shift = 52 (f64 frac width) - 10 (f16 frac width) + (1 - half_exp)
        let shift = (43 - half_exp) as u32;
        if shift >= 64 {
            return sign; // far below the smallest subnormal → ±0
        }
        if shift == 0 {
            return sign | (significand as u16);
        }
        return sign | (round(shift) as u16);
    }

    // Normal f16. `round(42)` yields an 11-bit value `1.ffffffffff` (implicit
    // leading 1 in bit 10 + 10 fraction bits), or `0x800` if rounding carried.
    // Encoding = (half_exp << 10) | frac, so subtract the implicit 0x400 from
    // both sides: result = ((half_exp - 1) << 10) + rounded. A rounding carry
    // (rounded == 0x800) then bumps the exponent for free.
    let rounded = round(42);
    let result = (((half_exp as u32) - 1) << 10) + rounded as u32;
    if (result >> 10) >= 0x1F {
        return sign | 0x7C00; // rounding overflowed to Inf
    }
    sign | (result as u16)
}

/// Decode an IEEE-754 binary16 bit pattern into an f64.
pub fn f16_bits_to_f64(bits: u16) -> f64 {
    let sign = if (bits & 0x8000) != 0 { -1.0f64 } else { 1.0 };
    let exp = ((bits >> 10) & 0x1F) as i32;
    let mantissa = (bits & 0x03FF) as f64;
    if exp == 0 {
        // Subnormal (or zero): value = sign * mantissa * 2^-24.
        sign * mantissa * 2f64.powi(-24)
    } else if exp == 0x1F {
        if mantissa != 0.0 {
            f64::NAN
        } else {
            sign * f64::INFINITY
        }
    } else {
        // Normal: value = sign * (1 + mantissa/1024) * 2^(exp-15).
        sign * (1.0 + mantissa / 1024.0) * 2f64.powi(exp - 15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: f64) -> f64 {
        f16_bits_to_f64(f64_to_f16_bits(v))
    }

    #[test]
    fn exactly_representable_values_roundtrip() {
        for &v in &[0.0, 1.0, 1.5, 2.0, 0.5, -3.0, -2.0, 65504.0, 0.25, 100.0] {
            assert_eq!(roundtrip(v), v, "roundtrip failed for {v}");
        }
    }

    #[test]
    fn overflow_underflow_and_specials() {
        // 70000 overflows the max finite half (65504) → +Inf.
        assert!(roundtrip(70000.0).is_infinite() && roundtrip(70000.0) > 0.0);
        // Tiny value underflows to 0.
        assert_eq!(roundtrip(1e-8), 0.0);
        // Negative max finite half.
        assert_eq!(roundtrip(-65504.0), -65504.0);
        // NaN stays NaN.
        assert!(roundtrip(f64::NAN).is_nan());
        // -Inf stays -Inf.
        assert!(roundtrip(f64::NEG_INFINITY).is_infinite() && roundtrip(f64::NEG_INFINITY) < 0.0);
    }

    #[test]
    fn bit_patterns_match_spec() {
        assert_eq!(f64_to_f16_bits(1.0), 0x3C00);
        assert_eq!(f64_to_f16_bits(1.5), 0x3E00);
        assert_eq!(f64_to_f16_bits(2.0), 0x4000);
        assert_eq!(f64_to_f16_bits(-2.0), 0xC000);
        assert_eq!(f64_to_f16_bits(0.0), 0x0000);
        assert_eq!(f64_to_f16_bits(65504.0), 0x7BFF); // max finite half
    }

    #[test]
    fn direct_rounding_no_double_round() {
        // Oracle values from Node's `new Float16Array([v])` DataView bits.
        // The key non-regression: a value just above the smallest-subnormal
        // half-ulp boundary must round UP to 0x0001, not collapse to 0 — which
        // the old f64→f32→f16 double-rounding path got wrong.
        assert_eq!(f64_to_f16_bits(2.980232238769532e-8), 0x0001);
        assert_eq!(f64_to_f16_bits(5.960464477539063e-8), 0x0001); // smallest subnormal
        assert_eq!(f64_to_f16_bits(2.9802e-8), 0x0000); // just below → ties to even 0
        assert_eq!(f64_to_f16_bits(1e-7), 0x0002);
        assert_eq!(f64_to_f16_bits(0.1), 0x2E66);
        assert_eq!(f64_to_f16_bits(0.2), 0x3266);
        assert_eq!(f64_to_f16_bits(100.0), 0x5640);
        assert_eq!(f64_to_f16_bits(3.141592653589793), 0x4248);
        assert_eq!(f64_to_f16_bits(6.0997555e-5), 0x03FF); // largest subnormal-ish
        assert_eq!(f64_to_f16_bits(6.103515625e-5), 0x0400); // smallest normal
        assert_eq!(f64_to_f16_bits(70000.0), 0x7C00); // overflow → +Inf
        assert_eq!(f64_to_f16_bits(65520.0), 0x7C00); // rounds up past max → +Inf
        assert_eq!(f64_to_f16_bits(65504.1), 0x7BFF); // stays max finite
        assert_eq!(f64_to_f16_bits(32768.0), 0x7800);
    }
}
