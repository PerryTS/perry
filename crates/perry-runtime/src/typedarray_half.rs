//! IEEE-754 binary16 (half-precision) conversion helpers for `Float16Array`
//! (#2902). Extracted from `typedarray.rs` to keep that file under the 2000-line
//! cap. `store_at`/`load_at` in `typedarray.rs` call these for `KIND_FLOAT16`.

/// Encode an f64 into an IEEE-754 binary16 (half-precision) bit pattern.
/// Round-to-nearest-even, with overflow → ±Inf and subnormal/underflow → ±0.
/// Mirrors the V8 / `Math.f16round`-then-store semantics used by Float16Array.
pub fn f64_to_f16_bits(value: f64) -> u16 {
    // Work from the f32 rounding of the value first (matches JS, which stores
    // through the double→half path; an intermediate f32 keeps the mantissa
    // handling simple and is exact for all f16-representable inputs).
    let f = value as f32;
    let bits = f.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32; // f32 biased exponent
    let mantissa = bits & 0x007F_FFFF;

    if exp == 0xFF {
        // Inf / NaN.
        if mantissa != 0 {
            // NaN: keep it a NaN (set a mantissa bit), drop payload.
            return sign | 0x7E00;
        }
        return sign | 0x7C00; // Inf
    }

    // Unbias f32 exponent, rebias for f16 (bias 15).
    let unbiased = exp - 127;
    let half_exp = unbiased + 15;

    if half_exp >= 0x1F {
        // Overflow → Inf.
        return sign | 0x7C00;
    }

    if half_exp <= 0 {
        // Subnormal or underflow to zero.
        if half_exp < -10 {
            return sign; // too small → ±0
        }
        // Build the f16 subnormal mantissa with round-to-nearest-even.
        // Implicit leading 1 of the f32 mantissa is restored, then shifted.
        let m = mantissa | 0x0080_0000; // 24-bit significand (1.mantissa)
        let shift = (14 - half_exp) as u32; // 14 = 23 - 10 + 1
        let half_m = m >> shift;
        // Round half to even.
        let remainder = m & ((1u32 << shift) - 1);
        let halfway = 1u32 << (shift - 1);
        let mut result = half_m as u16;
        if remainder > halfway || (remainder == halfway && (half_m & 1) == 1) {
            result += 1;
        }
        return sign | result;
    }

    // Normal f16. Take top 10 mantissa bits, round-to-nearest-even on the rest.
    let half_m = (mantissa >> 13) as u16;
    let remainder = mantissa & 0x1FFF;
    let halfway = 0x1000;
    let mut result = ((half_exp as u16) << 10) | half_m;
    if remainder > halfway || (remainder == halfway && (half_m & 1) == 1) {
        result += 1; // carry naturally rolls into exponent if mantissa overflows
    }
    sign | result
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
}
