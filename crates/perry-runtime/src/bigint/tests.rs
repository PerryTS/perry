//! Unit tests for the BigInt runtime (relocated verbatim from the
//! monolithic bigint.rs during the module-directory split).

#![cfg(test)]

use super::convert::parse_bigint_string;
use super::*;

#[test]
fn test_bigint_from_u64() {
    let bi = js_bigint_from_u64(12345);
    unsafe {
        assert_eq!((*bi).limbs[0], 12345);
        assert_eq!((*bi).limbs[1], 0);
    }
}

#[test]
fn test_bigint_add() {
    let a = js_bigint_from_u64(100);
    let b = js_bigint_from_u64(200);
    let c = js_bigint_add(a, b);
    unsafe {
        assert_eq!((*c).limbs[0], 300);
    }
}

#[test]
fn test_bigint_mul() {
    let a = js_bigint_from_u64(1000);
    let b = js_bigint_from_u64(2000);
    let c = js_bigint_mul(a, b);
    unsafe {
        assert_eq!((*c).limbs[0], 2_000_000);
    }
}

#[test]
fn test_bigint_from_i128_parts_preserves_wide_small_result() {
    let value = (i64::MAX as i128) + 1;
    let lo = value as u128 as u64;
    let hi = ((value as u128) >> 64) as u64 as i64;
    let bi = js_bigint_from_i128_parts(lo, hi);
    unsafe {
        assert_eq!((*bi).limbs[0], 0x8000_0000_0000_0000);
        assert_eq!((*bi).limbs[1], 0);
        assert!(fits_in_i64(&(*bi).limbs).is_none());
    }

    let negative = -((i64::MAX as i128) + 2);
    let lo = negative as u128 as u64;
    let hi = ((negative as u128) >> 64) as u64 as i64;
    let bi = js_bigint_from_i128_parts(lo, hi);
    unsafe {
        assert_eq!((*bi).limbs[0], 0x7fff_ffff_ffff_ffff);
        assert_eq!((*bi).limbs[1], u64::MAX);
        assert_eq!((*bi).limbs[BIGINT_LIMBS - 1], u64::MAX);
        assert!(fits_in_i64(&(*bi).limbs).is_none());
    }
}

#[test]
fn test_bigint_from_string() {
    let s = "123456789";
    let bi = js_bigint_from_string(s.as_ptr(), s.len() as u32);
    unsafe {
        assert_eq!((*bi).limbs[0], 123456789);
    }
}

#[test]
fn test_bigint_from_hex() {
    let s = "0xFFFFFFFFFFFFFFFF"; // max u64
    let bi = js_bigint_from_string(s.as_ptr(), s.len() as u32);
    unsafe {
        assert_eq!((*bi).limbs[0], u64::MAX);
        assert_eq!((*bi).limbs[1], 0);
    }
}

#[test]
fn test_bigint_mul_3limb() {
    // 1e39 * 2e39 = 2e78
    let s1 = "1000000000000000000000000000000000000000";
    let s2 = "2000000000000000000000000000000000000000";
    let a = js_bigint_from_string(s1.as_ptr(), s1.len() as u32);
    let b = js_bigint_from_string(s2.as_ptr(), s2.len() as u32);

    let a_f64 = js_bigint_to_f64(a);
    let b_f64 = js_bigint_to_f64(b);
    assert!(
        (a_f64 - 1e39).abs() / 1e39 < 1e-15,
        "a parse wrong: {}",
        a_f64
    );
    assert!(
        (b_f64 - 2e39).abs() / 2e39 < 1e-15,
        "b parse wrong: {}",
        b_f64
    );

    let c = js_bigint_mul(a, b);
    let c_f64 = js_bigint_to_f64(c);
    assert!(
        (c_f64 - 2e78).abs() / 2e78 < 1e-15,
        "3L*3L multiply wrong: got {}, expected 2e78",
        c_f64
    );
}

#[test]
fn test_bigint_mul_shifted() {
    // Reproduce: a = 46903565894391149, shifted = a << 96, b = 392217725163781510767080209313900517
    // shifted * b should be ~1.458e81
    let sa = "46903565894391149";
    let sb = "392217725163781510767080209313900517";
    let a = js_bigint_from_string(sa.as_ptr(), sa.len() as u32);
    let b96 = js_bigint_from_u64(96);
    let shifted = js_bigint_shl(a, b96);
    let b = js_bigint_from_string(sb.as_ptr(), sb.len() as u32);

    let product = js_bigint_mul(shifted, b);
    let product_f64 = js_bigint_to_f64(product);

    // Expected: ~1.458e81
    assert!(
        product_f64 > 1e80,
        "shifted*b too small: got {}, expected ~1.458e81",
        product_f64
    );
}

#[test]
fn test_bigint_div_large() {
    // Test division: (1e39 * 2e39) / 1e39 = 2e39
    let s1 = "1000000000000000000000000000000000000000";
    let s2 = "2000000000000000000000000000000000000000";
    let a = js_bigint_from_string(s1.as_ptr(), s1.len() as u32);
    let b = js_bigint_from_string(s2.as_ptr(), s2.len() as u32);
    let product = js_bigint_mul(a, b);
    let quotient = js_bigint_div(product, a);
    let q_f64 = js_bigint_to_f64(quotient);
    assert!(
        (q_f64 - 2e39).abs() / 2e39 < 1e-15,
        "division wrong: got {}, expected 2e39",
        q_f64
    );
}

// -- Tests added with the small-int fast path (v0.5.730) --
//
// These verify that the fast path agrees byte-for-byte with the
// existing 16-limb slow path on the boundaries (positive×positive,
// negative×positive, factorial growth, comparison ordering, modulo
// sign-of-dividend semantics, and the i64 boundary where the path
// promotes to the schoolbook multiplier).

/// Helper to read a freshly-allocated bigint as i64 (panics if it
/// doesn't fit — used in tests for clarity).
fn read_as_i64(p: *const BigIntHeader) -> i64 {
    unsafe { fits_in_i64(&(*p).limbs).expect("expected to fit in i64") }
}

#[test]
fn fast_path_mul_positive_positive() {
    let a = js_bigint_from_i64(1_000_000);
    let b = js_bigint_from_i64(2_500_000);
    let c = js_bigint_mul(a, b);
    assert_eq!(read_as_i64(c), 2_500_000_000_000);
}

#[test]
fn fast_path_mul_negative_positive() {
    let a = js_bigint_from_i64(-7);
    let b = js_bigint_from_i64(11);
    let c = js_bigint_mul(a, b);
    assert_eq!(read_as_i64(c), -77);
}

#[test]
fn fast_path_mul_negative_negative() {
    let a = js_bigint_from_i64(-1234);
    let b = js_bigint_from_i64(-5678);
    let c = js_bigint_mul(a, b);
    assert_eq!(read_as_i64(c), 1234 * 5678);
}

#[test]
fn fast_path_factorial_20_within_i64() {
    // 20! = 2432902008176640000 < i64::MAX, exercises the fast path
    // through every step of the loop.
    let mut acc = js_bigint_from_i64(1);
    for i in 2..=20i64 {
        let nb = js_bigint_from_i64(i);
        acc = js_bigint_mul(acc, nb);
    }
    assert_eq!(read_as_i64(acc), 2_432_902_008_176_640_000);
}

#[test]
fn slow_path_factorial_21_overflows_i64() {
    // 21! = 51090942171709440000 > i64::MAX, exercises the
    // promotion from fast path to slow path mid-multiply.
    let mut acc = js_bigint_from_i64(1);
    for i in 2..=21i64 {
        let nb = js_bigint_from_i64(i);
        acc = js_bigint_mul(acc, nb);
    }
    unsafe {
        // 21! = 51_090_942_171_709_440_000 = (limbs[1]<<64) | limbs[0]
        //     = 2 * 2^64 + 14_197_454_024_290_336_768
        let limbs = (*acc).limbs;
        assert_eq!(limbs[0], 14_197_454_024_290_336_768);
        assert_eq!(limbs[1], 2);
        for &l in &limbs[2..] {
            assert_eq!(l, 0);
        }
    }
}

#[test]
fn fast_path_add_sub_signed() {
    // (a, b) ∈ {±} test grid
    let a = js_bigint_from_i64(100);
    let b = js_bigint_from_i64(-30);
    assert_eq!(read_as_i64(js_bigint_add(a, b)), 70);
    assert_eq!(read_as_i64(js_bigint_sub(a, b)), 130);
    let a = js_bigint_from_i64(-100);
    let b = js_bigint_from_i64(-30);
    assert_eq!(read_as_i64(js_bigint_add(a, b)), -130);
    assert_eq!(read_as_i64(js_bigint_sub(a, b)), -70);
}

#[test]
fn fast_path_cmp_signed() {
    let a = js_bigint_from_i64(5);
    let b = js_bigint_from_i64(10);
    assert_eq!(js_bigint_cmp(a, b), -1);
    assert_eq!(js_bigint_cmp(b, a), 1);
    assert_eq!(js_bigint_cmp(a, a), 0);

    let neg = js_bigint_from_i64(-1);
    let pos = js_bigint_from_i64(1);
    assert_eq!(js_bigint_cmp(neg, pos), -1);
    assert_eq!(js_bigint_cmp(pos, neg), 1);
}

#[test]
fn fast_path_mod_sign_of_dividend() {
    // ECMAScript: BigInt `%` returns sign of dividend.
    // 17n % 5n === 2n; -17n % 5n === -2n; 17n % -5n === 2n.
    let m = |a: i64, b: i64| -> i64 {
        let av = js_bigint_from_i64(a);
        let bv = js_bigint_from_i64(b);
        read_as_i64(js_bigint_mod(av, bv))
    };
    assert_eq!(m(17, 5), 2);
    assert_eq!(m(-17, 5), -2);
    assert_eq!(m(17, -5), 2);
    assert_eq!(m(-17, -5), -2);
    assert_eq!(m(0, 5), 0);
}

#[test]
fn fast_path_div_truncate_toward_zero() {
    // ECMAScript BigInt `/` truncates toward zero.
    let d = |a: i64, b: i64| -> i64 {
        let av = js_bigint_from_i64(a);
        let bv = js_bigint_from_i64(b);
        read_as_i64(js_bigint_div(av, bv))
    };
    assert_eq!(d(7, 2), 3);
    assert_eq!(d(-7, 2), -3);
    assert_eq!(d(7, -2), -3);
    assert_eq!(d(-7, -2), 3);
}

// -- #2754 / #2907: BigInt() coercion semantics --

#[test]
fn coerce_boolean_inputs() {
    use crate::value::JSValue;
    let t = js_bigint_from_f64(f64::from_bits(JSValue::bool(true).bits()));
    assert_eq!(read_as_i64(t), 1);
    let f = js_bigint_from_f64(f64::from_bits(JSValue::bool(false).bits()));
    assert_eq!(read_as_i64(f), 0);
}

#[test]
fn coerce_finite_integer_number() {
    // Plain f64 (real Number) integer → exact BigInt.
    let b = js_bigint_from_f64(42.0);
    assert_eq!(read_as_i64(b), 42);
    let b = js_bigint_from_f64(-7.0);
    assert_eq!(read_as_i64(b), -7);
}

#[test]
fn coerce_large_integer_number_preserved() {
    // 2^60 fits in f64 exactly and exceeds nothing; verify the full
    // value is preserved (not saturated/truncated).
    let v = (1u64 << 60) as f64;
    let b = js_bigint_from_f64(v);
    assert_eq!(read_as_i64(b), 1i64 << 60);
}

// -- #2907: string parsing validation --

fn parse(s: &str) -> Result<i64, ()> {
    parse_bigint_string(s).map(|limbs| fits_in_i64(&limbs).expect("fits"))
}

#[test]
fn parse_radix_prefixes_and_whitespace() {
    assert_eq!(parse("0x10"), Ok(16));
    assert_eq!(parse("0o17"), Ok(15));
    assert_eq!(parse("0b101"), Ok(5));
    assert_eq!(parse("  42  "), Ok(42));
    assert_eq!(parse(""), Ok(0));
    assert_eq!(parse("  "), Ok(0));
    assert_eq!(parse("+5"), Ok(5));
    assert_eq!(parse("-5"), Ok(-5));
}

#[test]
fn parse_invalid_strings_reject() {
    assert_eq!(parse("bad"), Err(()));
    assert_eq!(parse("12abc34"), Err(()));
    assert_eq!(parse("0x"), Err(()));
    assert_eq!(parse("0xG"), Err(()));
    assert_eq!(parse("1_000"), Err(()));
    assert_eq!(parse("+"), Err(()));
}

// -- #2908: shift direction-reversing + pow --

#[test]
fn shift_negative_count_reverses_direction() {
    // 1n << -1n === 1n >> 1n === 0n
    let one = js_bigint_from_i64(1);
    let neg_one = js_bigint_from_i64(-1);
    assert_eq!(read_as_i64(js_bigint_shl(one, neg_one)), 0);
    // 8n >> -1n === 8n << 1n === 16n
    let eight = js_bigint_from_i64(8);
    assert_eq!(read_as_i64(js_bigint_shr(eight, neg_one)), 16);
    // Sanity: positive counts still work.
    let four = js_bigint_from_i64(4);
    assert_eq!(read_as_i64(js_bigint_shl(one, four)), 16);
    let two = js_bigint_from_i64(2);
    assert_eq!(read_as_i64(js_bigint_shr(eight, two)), 2);
}

#[test]
fn permission_bitwise_values_match_node() {
    let bitfield = js_bigint_from_i64(9216);
    let zero = js_bigint_from_i64(0);
    let one = js_bigint_from_i64(1);
    let eleven = js_bigint_from_i64(11);
    let thirteen = js_bigint_from_i64(13);

    let manage_messages = js_bigint_shl(one, thirteen);
    let send_messages = js_bigint_shl(one, eleven);
    let and_result = js_bigint_and(bitfield, manage_messages);
    let or_result = js_bigint_or(zero, send_messages);
    let not_result = js_bigint_not(send_messages);

    assert_eq!(read_as_i64(manage_messages), 8192);
    assert_eq!(read_as_i64(send_messages), 2048);
    assert_eq!(read_as_i64(and_result), 8192);
    assert_eq!(read_as_i64(or_result), 2048);
    assert_eq!(read_as_i64(not_result), -2049);
}

#[test]
fn pow_non_negative() {
    let two = js_bigint_from_i64(2);
    let three = js_bigint_from_i64(3);
    assert_eq!(read_as_i64(js_bigint_pow(two, three)), 8);
    let zero = js_bigint_from_i64(0);
    assert_eq!(read_as_i64(js_bigint_pow(two, zero)), 1);
}

#[test]
fn fits_in_i64_boundary() {
    // i64::MIN encodes as limbs[0]=0x80...0 limbs[1..]=u64::MAX.
    let min = js_bigint_from_i64(i64::MIN);
    assert_eq!(read_as_i64(min), i64::MIN);
    // i64::MAX encodes as limbs[0]=0x7F...F limbs[1..]=0.
    let max = js_bigint_from_i64(i64::MAX);
    assert_eq!(read_as_i64(max), i64::MAX);
    // 2^63 (= i64::MAX + 1, doesn't fit in i64) must NOT fit.
    // Build it via add to avoid going through js_bigint_from_i64
    // (which only takes i64).
    let one = js_bigint_from_i64(1);
    let beyond = js_bigint_add(max, one);
    unsafe {
        assert!(
            fits_in_i64(&(*beyond).limbs).is_none(),
            "i64::MAX + 1 should not fit in i64"
        );
    }
}

// -- #6073: fixed 1024-bit range — overflow detection --
//
// The throwing side aborts the test process (`js_throw` with no active try
// frame calls `std::process::exit`), so these cover the pure fit predicate
// and that large-but-in-range arithmetic still succeeds without a false
// overflow. End-to-end `RangeError` behavior is exercised by a `.ts` probe.

/// Build 2^n (n < 1023 so it stays representable).
fn pow2(n: i64) -> *mut BigIntHeader {
    js_bigint_pow(js_bigint_from_i64(2), js_bigint_from_i64(n))
}

#[test]
fn magnitude_fits_1024_boundaries() {
    let mut m = [0u64; 2 * BIGINT_LIMBS];
    // magnitude < 2^1023 → representable with either sign.
    m[BIGINT_LIMBS - 1] = 0x7fff_ffff_ffff_ffff;
    assert!(magnitude_fits_1024(&m, false));
    assert!(magnitude_fits_1024(&m, true));

    // magnitude == 2^1023 → only the negative endpoint (-2^1023) fits.
    let mut m = [0u64; 2 * BIGINT_LIMBS];
    m[BIGINT_LIMBS - 1] = 1u64 << 63;
    assert!(!magnitude_fits_1024(&m, false));
    assert!(magnitude_fits_1024(&m, true));

    // magnitude in (2^1023, 2^1024) → fits neither sign.
    m[0] = 1;
    assert!(!magnitude_fits_1024(&m, false));
    assert!(!magnitude_fits_1024(&m, true));

    // any bit at or above 2^1024 → never fits.
    let mut m = [0u64; 2 * BIGINT_LIMBS];
    m[BIGINT_LIMBS] = 1;
    assert!(!magnitude_fits_1024(&m, false));
    assert!(!magnitude_fits_1024(&m, true));
}

#[test]
fn pow_large_in_range_no_false_overflow() {
    // 2^1000 < 2^1023: single bit at index 1000, positive.
    let p = pow2(1000);
    unsafe {
        let limbs = (*p).limbs;
        assert_eq!(limbs[1000 / 64], 1u64 << (1000 % 64));
        assert!(!is_negative(&limbs));
        for (i, &l) in limbs.iter().enumerate() {
            if i != 1000 / 64 {
                assert_eq!(l, 0, "limb {i} should be zero");
            }
        }
    }
    // (2^300)^3 = 2^900: the guarded loop must not throw on the final
    // unused base squaring (2^300)^4 = 2^1200.
    let cube = js_bigint_pow(pow2(300), js_bigint_from_i64(3));
    unsafe {
        assert_eq!((*cube).limbs[900 / 64], 1u64 << (900 % 64));
    }
}

#[test]
fn mul_large_in_range_no_false_overflow() {
    // (2^500) * (2^500) = 2^1000 via the schoolbook slow path.
    let c = js_bigint_mul(pow2(500), pow2(500));
    unsafe {
        assert_eq!((*c).limbs[1000 / 64], 1u64 << (1000 % 64));
    }
}

#[test]
fn add_sub_large_in_range_no_false_overflow() {
    // 2^1000 + 2^999 = bits 1000 and 999 set (both in limb 15).
    let sum = js_bigint_add(pow2(1000), pow2(999));
    unsafe {
        let limbs = (*sum).limbs;
        assert_eq!(
            limbs[999 / 64],
            (1u64 << (999 % 64)) | (1u64 << (1000 % 64))
        );
    }
    // 2^1000 - 2^999 = 2^999.
    let diff = js_bigint_sub(pow2(1000), pow2(999));
    unsafe {
        assert_eq!((*diff).limbs[999 / 64], 1u64 << (999 % 64));
    }
}

#[test]
fn shl_large_in_range_no_false_overflow() {
    // 1n << 1022n = 2^1022, still positive and in range.
    let r = js_bigint_shl(js_bigint_from_i64(1), js_bigint_from_i64(1022));
    unsafe {
        let limbs = (*r).limbs;
        assert_eq!(limbs[1022 / 64], 1u64 << (1022 % 64));
        assert!(!is_negative(&limbs));
    }
}

#[test]
fn from_string_radix_hex_ignores_excess_leading_zeros() {
    // 304 hex digits: 300 leading zeros + "beef". More than 256 digits, but
    // the excess are all zero, so the value (0xbeef) fits and must NOT
    // false-throw on the >1024-bit stream (regression for the CodeRabbit
    // finding on #6073 — leftover high-order zeros are harmless).
    let s = format!("{}beef", "0".repeat(300));
    let bi = js_bigint_from_string_radix(s.as_ptr(), s.len() as u32, 16);
    unsafe {
        let limbs = (*bi).limbs;
        assert_eq!(limbs[0], 0xbeef);
        for &l in &limbs[1..] {
            assert_eq!(l, 0);
        }
    }

    // Decimal leading zeros likewise must not throw (the long-mult path
    // never carries out of the top limb while the magnitude stays small).
    assert_eq!(parse("000000123").unwrap(), 123);
}

// -- #1781: SSO short-string coercion --

/// #1781: `BigInt("123")` — a numeric string <= 5 bytes is an inline SSO
/// value (tag 0x7FF9). `is_string()` is STRING_TAG-only, so pre-fix it
/// fell through to the `value as i64` arm (the SSO f64 is NaN → 0), and
/// `BigInt("123")` produced `0n`. Route through the unified decoder.
#[test]
fn bigint_from_f64_parses_sso_numeric_strings() {
    for s in ["0", "1", "42", "123", "12345"] {
        let v = crate::value::JSValue::try_short_string(s.as_bytes())
            .expect("numeric string <= 5 bytes encodes as inline SSO");
        assert!(v.is_short_string(), "{s:?} should be an inline SSO value");
        let bi = js_bigint_from_f64(f64::from_bits(v.bits()));
        assert!(!bi.is_null(), "null BigInt for {s:?}");
        let out = js_bigint_to_string(bi);
        let got = unsafe {
            let len = (*out).byte_len as usize;
            let data = (out as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            std::str::from_utf8(std::slice::from_raw_parts(data, len))
                .unwrap()
                .to_string()
        };
        assert_eq!(got, s, "BigInt({s:?}) mismatch");
    }
}

// -- unsigned_div_limbs fast paths: power-of-two divisor, single-limb --
// -- divisor, and the bit-length-bounded general loop --

/// Read the raw limbs of a freshly-allocated bigint.
fn limbs_of(p: *const BigIntHeader) -> [u64; BIGINT_LIMBS] {
    unsafe { (*p).limbs }
}

/// Build a bigint from the given low limbs (rest zero — non-negative value).
fn bigint_from_low_limbs(low: &[u64]) -> *mut BigIntHeader {
    let mut limbs = ZERO_LIMBS;
    limbs[..low.len()].copy_from_slice(low);
    bigint_alloc_with_limbs(limbs)
}

#[test]
fn pow2_divisor_mod_and_div_multi_limb() {
    // 192-bit dividend % 2^64: remainder = low limb, quotient = high limbs.
    let a = bigint_from_low_limbs(&[0xAAAA_AAAA_AAAA_AAAA, 0xBBBB_BBBB_BBBB_BBBB, 0xCCCC]);
    let two_pow_64 = bigint_from_low_limbs(&[0, 1]);

    let r = limbs_of(js_bigint_mod(a, two_pow_64));
    assert_eq!(r[0], 0xAAAA_AAAA_AAAA_AAAA);
    assert!(r[1..].iter().all(|&l| l == 0));

    let q = limbs_of(js_bigint_div(a, two_pow_64));
    assert_eq!(q[0], 0xBBBB_BBBB_BBBB_BBBB);
    assert_eq!(q[1], 0xCCCC);
    assert!(q[2..].iter().all(|&l| l == 0));

    // Non-limb-aligned power of two: % 2^13 keeps the low 13 bits.
    let two_pow_13 = js_bigint_from_i64(1 << 13);
    let r = limbs_of(js_bigint_mod(a, two_pow_13));
    assert_eq!(r[0], 0xAAAA_AAAA_AAAA_AAAA & ((1 << 13) - 1));
    assert!(r[1..].iter().all(|&l| l == 0));

    // 2^70 (bit 6 of limb 1): quotient must stitch bits across limbs.
    let two_pow_70 = bigint_from_low_limbs(&[0, 1 << 6]);
    let q = limbs_of(js_bigint_div(a, two_pow_70));
    assert_eq!(q[0], (0xBBBB_BBBB_BBBB_BBBBu64 >> 6) | (0xCCCCu64 << 58));
    assert_eq!(q[1], 0xCCCC >> 6);
    assert!(q[2..].iter().all(|&l| l == 0));
}

#[test]
fn pow2_divisor_mod_sign_of_dividend_and_canonical_zero() {
    let two_pow_64 = bigint_from_low_limbs(&[0, 1]);

    // -(2^100 + 5) % 2^64 === -5 (sign follows dividend; ECMA remainder).
    let mag = bigint_from_low_limbs(&[5, 1 << 36]);
    let a = js_bigint_neg(mag);
    assert_eq!(read_as_i64(js_bigint_mod(a, two_pow_64)), -5);

    // -(2^100) % 2^64 === 0 with NO negative-zero pattern: all limbs zero.
    let mag = bigint_from_low_limbs(&[0, 1 << 36]);
    let a = js_bigint_neg(mag);
    assert_eq!(limbs_of(js_bigint_mod(a, two_pow_64)), ZERO_LIMBS);
}

#[test]
fn pow2_divisor_edge_cases() {
    let two_pow_64 = bigint_from_low_limbs(&[0, 1]);

    // Dividend smaller than divisor: unchanged. (2^64 exceeds i64, so this
    // exercises the limb path, not the i64 fast path.)
    let five = js_bigint_from_i64(5);
    assert_eq!(read_as_i64(js_bigint_mod(five, two_pow_64)), 5);

    // Dividend == divisor: remainder 0, quotient 1.
    let a = bigint_from_low_limbs(&[0, 1]);
    assert_eq!(limbs_of(js_bigint_mod(a, two_pow_64)), ZERO_LIMBS);
    assert_eq!(read_as_i64(js_bigint_div(a, two_pow_64)), 1);

    // Multi-limb dividend % 1n == 0n, / 1n == dividend.
    let one = js_bigint_from_i64(1);
    let big = bigint_from_low_limbs(&[7, 8, 9]);
    assert_eq!(limbs_of(js_bigint_mod(big, one)), ZERO_LIMBS);
    assert_eq!(limbs_of(js_bigint_div(big, one)), limbs_of(big));
}

#[test]
fn single_limb_divisor_hardware_div() {
    // (2^64) % FNV prime — expected value computed independently via u128.
    let prime = 1_099_511_628_211u64;
    let two_pow_64 = bigint_from_low_limbs(&[0, 1]);
    let b = js_bigint_from_i64(prime as i64);
    let expected = ((1u128 << 64) % prime as u128) as u64;
    let r = limbs_of(js_bigint_mod(two_pow_64, b));
    assert_eq!(r[0], expected);
    assert!(r[1..].iter().all(|&l| l == 0));

    // 200+-bit dividend: verify the division identity a == q*b + r, r < b,
    // using independently-tested mul/add/cmp.
    let a = bigint_from_low_limbs(&[12345, 0xDEAD_BEEF, 0, 0x1_0000]);
    let q = js_bigint_div(a, b);
    let r = js_bigint_mod(a, b);
    assert_eq!(js_bigint_cmp(r, b), -1);
    let recomposed = js_bigint_add(js_bigint_mul(q, b), r);
    assert_eq!(limbs_of(recomposed), limbs_of(a));
}

#[test]
fn general_path_multi_limb_divisor_identity() {
    // Divisor 2^64 + 1: two limbs, not a power of two → general bit loop.
    let a = bigint_from_low_limbs(&[0x1111_2222_3333_4444, 0x5555, 0, 0, 0xF00D]);
    let b = bigint_from_low_limbs(&[1, 1]);
    let q = js_bigint_div(a, b);
    let r = js_bigint_mod(a, b);
    assert_eq!(js_bigint_cmp(r, b), -1);
    let recomposed = js_bigint_add(js_bigint_mul(q, b), r);
    assert_eq!(limbs_of(recomposed), limbs_of(a));
}

/// Deterministic xorshift64* — keeps the cross-check reproducible.
fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn limbs_from_i128(v: i128) -> [u64; BIGINT_LIMBS] {
    let mut limbs = ZERO_LIMBS;
    write_i128(v, &mut limbs);
    limbs
}

fn bigint_from_i128(v: i128) -> *mut BigIntHeader {
    bigint_alloc_with_limbs(limbs_from_i128(v))
}

/// Randomized differential check of `js_bigint_div`/`js_bigint_mod` against
/// host `i128` arithmetic, which has exactly JS BigInt semantics (`/`
/// truncates toward zero, `%` takes the sign of the dividend). Divisor
/// shapes are chosen to hit all three `unsigned_div_limbs` tiers:
/// power-of-two, single-limb, and multi-limb-general.
#[test]
fn div_mod_cross_check_against_i128_all_tiers() {
    let mut s = 0x9E37_79B9_7F4A_7C15u64;
    for iter in 0..3000u32 {
        // Dividend: random, width-varied, masked to 127 bits so it is a
        // non-negative i128 whose negation cannot overflow.
        let width = (xorshift64(&mut s) % 128) as u32;
        let a128 =
            (((xorshift64(&mut s) as u128) << 64) | xorshift64(&mut s) as u128) >> (127 - width);
        let a_i = (a128 & (u128::MAX >> 1)) as i128;

        // Divisor: cycle the three tiers.
        let b_i = match iter % 3 {
            // Tier 1: exact power of two, 2^0 .. 2^126.
            0 => 1i128 << (xorshift64(&mut s) % 127),
            // Tier 2: single 64-bit limb (nonzero).
            1 => (xorshift64(&mut s) | 1) as i128,
            // Tier 3: two limbs, high limb nonzero (never a power of two
            // because bit 0 is forced on).
            _ => {
                let hi = xorshift64(&mut s) | 1;
                ((((hi as u128) << 64) | (xorshift64(&mut s) | 1) as u128) & (u128::MAX >> 1))
                    as i128
            }
        };
        assert!(b_i > 0, "divisor must be positive and nonzero");

        for &(sa, sb) in &[(1i128, 1i128), (-1, 1), (1, -1), (-1, -1)] {
            let av = sa * a_i;
            let bv = sb * b_i;
            let a = bigint_from_i128(av);
            let b = bigint_from_i128(bv);

            assert_eq!(
                limbs_of(js_bigint_div(a, b)),
                limbs_from_i128(av / bv),
                "div mismatch iter={iter} a={av} b={bv}"
            );
            assert_eq!(
                limbs_of(js_bigint_mod(a, b)),
                limbs_from_i128(av % bv),
                "mod mismatch iter={iter} a={av} b={bv}"
            );
        }
    }
}

#[test]
fn zero_dividend_and_wide_pow2_edges() {
    let zero = js_bigint_from_i64(0);
    let two_pow_64 = bigint_from_low_limbs(&[0, 1]);
    assert_eq!(limbs_of(js_bigint_mod(zero, two_pow_64)), ZERO_LIMBS);
    assert_eq!(limbs_of(js_bigint_div(zero, two_pow_64)), ZERO_LIMBS);

    // Dividend one below the divisor: passes straight through.
    let all_ones = bigint_from_low_limbs(&[u64::MAX]);
    assert_eq!(limbs_of(js_bigint_mod(all_ones, two_pow_64)), {
        let mut e = ZERO_LIMBS;
        e[0] = u64::MAX;
        e
    });
    assert_eq!(limbs_of(js_bigint_div(all_ones, two_pow_64)), ZERO_LIMBS);

    // Near-widest power-of-two divisor: 2^1021, dividend 2^1022 + 12345
    // (top bit of the top limb stays clear, so the value is positive).
    // q = 2, r = 12345.
    let mut b = ZERO_LIMBS;
    b[BIGINT_LIMBS - 1] = 1u64 << 61; // 2^1021
    let mut a = ZERO_LIMBS;
    a[BIGINT_LIMBS - 1] = 1u64 << 62; // 2^1022
    a[0] = 12345;
    let a = bigint_alloc_with_limbs(a);
    let b = bigint_alloc_with_limbs(b);
    assert_eq!(read_as_i64(js_bigint_mod(a, b)), 12345);
    assert_eq!(read_as_i64(js_bigint_div(a, b)), 2);
}
