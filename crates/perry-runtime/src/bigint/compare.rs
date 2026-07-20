//! BigInt comparison and BigInt<->Number / string comparison helpers.

use super::convert::parse_bigint_string;
use super::*;

/// Compare two BigInts (-1 if a < b, 0 if equal, 1 if a > b)
#[no_mangle]
pub extern "C" fn js_bigint_cmp(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    if a.is_null() || b.is_null() {
        return 0;
    }
    unsafe {
        let a_limbs = (*a).limbs;
        let b_limbs = (*b).limbs;
        // Fast path: both fit in i64. The vast majority of comparisons in
        // hot loops (factorial bounds, postgres int8 inequality, app id
        // ordering) hit this case.
        if let (Some(av), Some(bv)) = (fits_in_i64(&a_limbs), fits_in_i64(&b_limbs)) {
            return match av.cmp(&bv) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            };
        }
        compare_limbs(&a_limbs, &b_limbs)
    }
}

/// `StringToBigInt` (ES2024 §7.1.14), non-throwing. Returns `None` when the
/// string is not a valid BigInt literal — the abstract relational comparison
/// treats that as `undefined` (so the comparison yields `false`) rather than a
/// thrown `SyntaxError`. Leading/trailing whitespace is trimmed, an empty
/// string is `0n`, and the `0x`/`0o`/`0b` radix prefixes are accepted.
pub(crate) fn string_to_bigint(raw: &str) -> Option<*mut BigIntHeader> {
    parse_bigint_string(raw).ok().map(bigint_alloc_with_limbs)
}

/// Mathematically compare a BigInt `x` against a Number `y` (ES2024 §7.2.13,
/// the mixed BigInt/Number step). Comparison is exact — no precision loss from
/// `BigInt → f64` — because `y` is decomposed into its integer floor (converted
/// to a BigInt without rounding) and any leftover fraction.
///
/// Returns `-1` (x < y), `0` (x == y), `1` (x > y), or `2` (undefined: `y` is
/// `NaN`, the one incomparable case).
pub(crate) fn bigint_cmp_f64(x: *const BigIntHeader, y: f64) -> i32 {
    if y.is_nan() {
        return 2;
    }
    if y == f64::INFINITY {
        return -1; // x < +Infinity for every finite BigInt
    }
    if y == f64::NEG_INFINITY {
        return 1; // x > -Infinity
    }
    // Perry's BigInt is a fixed 1024-bit two's-complement value, so any Number
    // with |y| >= 2^1023 (e.g. `Number.MAX_VALUE` ~2^1024) does not round-trip
    // through `js_bigint_from_f64`: its top magnitude bit lands on the sign bit
    // and reads back as negative. Two sub-cases:
    //
    //  * `x` is a *small* BigInt (fits in i64) — far below 2^1023 in magnitude,
    //    so the comparison is decided by the sign of `y` alone. This is the
    //    `1n {<,<=,>,>=} Number.MAX_VALUE` fix (#5894): without it the lossy
    //    round-trip made `1n >= MAX_VALUE` wrongly true.
    //
    //  * `x` is a *large* BigInt (a literal near 2^1024). Such a literal already
    //    overflowed the signed 1024-bit width on parse, so `x` and the
    //    round-tripped `y` overflow *identically* — comparing their raw
    //    two's-complement limbs reproduces the exact mathematical order,
    //    including equality when the literal is exactly `MAX_VALUE` (test262
    //    `bigint-and-number-extremes` equals/does-not-equals/relational). Fall
    //    through to the raw compare; a narrower short-circuit here would flip
    //    that equality back to less-than and re-break those cases.
    //
    // 2^1023 as an exact IEEE-754 double (biased exponent 2046, zero mantissa).
    let bigint_mag_bound = f64::from_bits(0x7FE0_0000_0000_0000);
    if y.abs() >= bigint_mag_bound {
        let small = unsafe {
            let xp = clean_bigint_ptr(x);
            !xp.is_null() && fits_in_i64(&(*xp).limbs).is_some()
        };
        if small {
            return if y > 0.0 { -1 } else { 1 };
        }
        // else: large `x` — raw compare below is exact for the overflow regime.
    }
    // `y` is finite. Compare `x` with `floor(y)` as exact integers; if equal,
    // a positive fractional part of `y` makes `x` strictly smaller.
    let floor = y.floor();
    let floor_big = js_bigint_from_f64(floor);
    let c = js_bigint_cmp(x, floor_big);
    if c != 0 {
        return c;
    }
    if y > floor {
        -1
    } else {
        0
    }
}

/// Check if two BigInts are equal
#[no_mangle]
pub extern "C" fn js_bigint_eq(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    if a.is_null() || b.is_null() {
        return if a == b { 1 } else { 0 }; // both null = equal, one null = not equal
    }
    unsafe {
        if (*a).limbs == (*b).limbs {
            1
        } else {
            0
        }
    }
}

fn compare_limbs(a: &[u64; BIGINT_LIMBS], b: &[u64; BIGINT_LIMBS]) -> i32 {
    let a_neg = is_negative(a);
    let b_neg = is_negative(b);

    // Different signs: negative < positive
    if a_neg && !b_neg {
        return -1;
    }
    if !a_neg && b_neg {
        return 1;
    }

    // Same sign: unsigned comparison (works for both positive and negative in two's complement)
    for i in (0..BIGINT_LIMBS).rev() {
        if a[i] > b[i] {
            return 1;
        }
        if a[i] < b[i] {
            return -1;
        }
    }
    0
}
