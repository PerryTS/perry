//! Math operations runtime support

use rand::RngExt;

/// Math built-ins apply ECMAScript ToNumber, where Symbol and BigInt throw.
/// `Number(1n)` is allowed in JavaScript, so this stays separate from the
/// shared `js_number_coerce` helper used by the Number constructor.
#[no_mangle]
pub extern "C" fn js_math_to_number(value: f64) -> f64 {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if jsval.is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
    if jsval.is_pointer() {
        let ptr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
        if crate::symbol::is_registered_symbol(ptr) {
            crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a number");
        }
    }
    crate::builtins::js_number_coerce(value)
}

/// Math.trunc(x) -> number
#[no_mangle]
pub extern "C" fn js_math_trunc(value: f64) -> f64 {
    js_math_to_number(value).trunc()
}

/// Math.round(x) -> number. ECMA-262 21.3.2.28 round-half-toward-+∞ with -0
/// preservation. The naive `floor(x + 0.5)` mis-rounds `0.5 - ε/4` (the `+ 0.5`
/// add pre-rounds up to 1.0) and large odd integers where the `.5` is lost; see
/// `js_math_round_value` (test262 Math/round/S15.8.2.15_A7).
#[no_mangle]
pub extern "C" fn js_math_round(value: f64) -> f64 {
    crate::object::js_math_round_value(js_math_to_number(value))
}

/// Math.sign(x) -> number
#[no_mangle]
pub extern "C" fn js_math_sign(value: f64) -> f64 {
    let n = js_math_to_number(value);
    if n == 0.0 || n.is_nan() {
        n
    } else if n.is_sign_negative() {
        -1.0
    } else {
        1.0
    }
}

fn js_math_to_int32(value: f64) -> i32 {
    let n = js_math_to_number(value);
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    const TWO_32: f64 = 4_294_967_296.0;
    (n.trunc().rem_euclid(TWO_32) as u32) as i32
}

/// Math.imul(a, b) -> number
#[no_mangle]
pub extern "C" fn js_math_imul(a: f64, b: f64) -> f64 {
    js_math_to_int32(a).wrapping_mul(js_math_to_int32(b)) as f64
}

/// Math.pow(base, exponent) -> number
#[no_mangle]
pub extern "C" fn js_math_pow(base: f64, exp: f64) -> f64 {
    // ECMAScript Math.pow deviates from IEEE-754 `pow` in two cases that Rust's
    // `f64::powf` (libm pow) gets "wrong" for JS:
    //  - a NaN exponent is always NaN (IEEE `pow(1, NaN)` = 1).
    //  - |base| == 1 with a ±Infinity exponent is NaN (IEEE returns 1).
    // A +0/-0 exponent still yields 1 (even for a NaN base), which `powf`
    // already handles, so those fall through.
    if exp.is_nan() {
        return f64::NAN;
    }
    if exp.is_infinite() && base.abs() == 1.0 {
        return f64::NAN;
    }
    base.powf(exp)
}

/// Floating-point modulo using the C library's fmod
/// This is often faster than the inline computation a - trunc(a/b) * b
#[no_mangle]
pub extern "C" fn js_math_fmod(a: f64, b: f64) -> f64 {
    a % b // Rust's % operator maps to libm fmod
}

/// Math.log(x) -> number (natural logarithm)
#[no_mangle]
pub extern "C" fn js_math_log(x: f64) -> f64 {
    x.ln()
}

/// Math.log2(x) -> number (base-2 logarithm)
#[no_mangle]
pub extern "C" fn js_math_log2(x: f64) -> f64 {
    x.log2()
}

/// Math.log10(x) -> number (base-10 logarithm)
#[no_mangle]
pub extern "C" fn js_math_log10(x: f64) -> f64 {
    x.log10()
}

/// Math.sin(x) -> number
#[no_mangle]
pub extern "C" fn js_math_sin(x: f64) -> f64 {
    x.sin()
}

/// Math.cos(x) -> number
#[no_mangle]
pub extern "C" fn js_math_cos(x: f64) -> f64 {
    x.cos()
}

/// Math.tan(x) -> number
#[no_mangle]
pub extern "C" fn js_math_tan(x: f64) -> f64 {
    x.tan()
}

/// Math.asin(x) -> number
#[no_mangle]
pub extern "C" fn js_math_asin(x: f64) -> f64 {
    x.asin()
}

/// Math.acos(x) -> number
#[no_mangle]
pub extern "C" fn js_math_acos(x: f64) -> f64 {
    x.acos()
}

/// Math.atan(x) -> number
#[no_mangle]
pub extern "C" fn js_math_atan(x: f64) -> f64 {
    x.atan()
}

/// Math.atan2(y, x) -> number
#[no_mangle]
pub extern "C" fn js_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/// Math.cbrt(x) -> number — cube root
#[no_mangle]
pub extern "C" fn js_math_cbrt(x: f64) -> f64 {
    x.cbrt()
}

/// Math.fround(x) -> number — nearest 32-bit float
#[no_mangle]
pub extern "C" fn js_math_fround(x: f64) -> f64 {
    x as f32 as f64
}

fn round_ties_to_even(value: f64) -> u64 {
    let floor = value.floor();
    let floor_int = floor as u64;
    let frac = value - floor;
    if frac < 0.5 {
        floor_int
    } else if frac > 0.5 {
        floor_int + 1
    } else if floor_int & 1 == 0 {
        floor_int
    } else {
        floor_int + 1
    }
}

/// Math.f16round(x) -> number — nearest IEEE-754 binary16 value
#[no_mangle]
pub extern "C" fn js_math_f16round(value: f64) -> f64 {
    let x = js_math_to_number(value);
    if x == 0.0 || !x.is_finite() {
        return x;
    }

    const MIN_HALF_SUBNORMAL: f64 = 5.960464477539063e-8; // 2^-24
    const MIN_HALF_NORMAL: f64 = 0.00006103515625; // 2^-14
    const MAX_HALF_FINITE: f64 = 65504.0;
    const OVERFLOW_THRESHOLD: f64 = 65520.0;

    let negative = x.is_sign_negative();
    let abs = x.abs();
    let rounded = if abs >= OVERFLOW_THRESHOLD {
        f64::INFINITY
    } else if abs < MIN_HALF_NORMAL {
        let mantissa = round_ties_to_even(abs / MIN_HALF_SUBNORMAL);
        mantissa as f64 * MIN_HALF_SUBNORMAL
    } else {
        let exponent = (((abs.to_bits() >> 52) & 0x7ff) as i32) - 1023;
        let step = 2.0f64.powi(exponent - 10);
        let significand = round_ties_to_even(abs / step);
        let rounded = significand as f64 * step;
        if rounded > MAX_HALF_FINITE {
            f64::INFINITY
        } else {
            rounded
        }
    };

    if negative {
        -rounded
    } else {
        rounded
    }
}

/// Math.clz32(x) -> number — count leading zeros of 32-bit integer
#[no_mangle]
pub extern "C" fn js_math_clz32(x: f64) -> f64 {
    // JS spec: ToUint32(x) first. `x as i64 as u32` SATURATES for |x| >= 2^63
    // (Math.clz32(1e20) came out 0 instead of 1); use truncate-mod-2^32.
    let n = if !x.is_finite() {
        0u32
    } else {
        x.trunc().rem_euclid(4_294_967_296.0) as u32
    };
    n.leading_zeros() as f64
}

/// Math.expm1(x) -> number — exp(x) - 1 with high precision near 0
#[no_mangle]
pub extern "C" fn js_math_expm1(x: f64) -> f64 {
    x.exp_m1()
}

/// Math.log1p(x) -> number — log(1 + x) with high precision near 0
#[no_mangle]
pub extern "C" fn js_math_log1p(x: f64) -> f64 {
    x.ln_1p()
}

/// Math.sinh(x) -> number
#[no_mangle]
pub extern "C" fn js_math_sinh(x: f64) -> f64 {
    x.sinh()
}

/// Math.cosh(x) -> number
#[no_mangle]
pub extern "C" fn js_math_cosh(x: f64) -> f64 {
    x.cosh()
}

/// Math.tanh(x) -> number
#[no_mangle]
pub extern "C" fn js_math_tanh(x: f64) -> f64 {
    x.tanh()
}

/// Math.asinh(x) -> number
#[no_mangle]
pub extern "C" fn js_math_asinh(x: f64) -> f64 {
    x.asinh()
}

/// Math.acosh(x) -> number
#[no_mangle]
pub extern "C" fn js_math_acosh(x: f64) -> f64 {
    x.acosh()
}

/// Math.atanh(x) -> number
#[no_mangle]
pub extern "C" fn js_math_atanh(x: f64) -> f64 {
    x.atanh()
}

/// Math.hypot(a, b) -> number — sqrt(a² + b²), numerically stable.
/// Multi-arg forms are chained in the codegen: hypot(a, b, c) ≡ hypot(hypot(a, b), c).
#[no_mangle]
pub extern "C" fn js_math_hypot(a: f64, b: f64) -> f64 {
    a.hypot(b)
}

/// Math.random() -> number (0 <= x < 1)
#[no_mangle]
pub extern "C" fn js_math_random() -> f64 {
    crate::effect!(host_read, random);
    let mut rng = rand::rng();
    rng.random::<f64>()
}

/// Math.min(...array) -> number — find minimum value in an array
#[no_mangle]
pub extern "C" fn js_math_min_array(arr_ptr: i64) -> f64 {
    if arr_ptr == 0 {
        return f64::INFINITY;
    }
    let arr = arr_ptr as *const crate::ArrayHeader;
    let len = crate::array::js_array_length(arr) as usize;
    if len == 0 {
        return f64::INFINITY;
    }
    // Spec (sec-math.min) step 2: ToNumber EVERY arg first (observable via
    // valueOf), then reduce. A NaN must not short-circuit before later args are
    // coerced (test262 min/Math.min_each-element-coerced).
    let mut result = f64::INFINITY;
    let mut saw_nan = false;
    for i in 0..len {
        let num = js_math_to_number(crate::array::js_array_get_f64(arr, i as u32));
        if num.is_nan() {
            saw_nan = true;
        } else if num < result || (num == 0.0 && result == 0.0 && num.is_sign_negative()) {
            // ECMAScript Math.min treats -0 as smaller than +0; IEEE `<` treats
            // them as equal, so add an explicit sign-of-zero tiebreaker.
            result = num;
        }
    }
    if saw_nan {
        f64::NAN
    } else {
        result
    }
}

/// Math.max(...array) -> number — find maximum value in an array
#[no_mangle]
pub extern "C" fn js_math_max_array(arr_ptr: i64) -> f64 {
    if arr_ptr == 0 {
        return f64::NEG_INFINITY;
    }
    let arr = arr_ptr as *const crate::ArrayHeader;
    let len = crate::array::js_array_length(arr) as usize;
    if len == 0 {
        return f64::NEG_INFINITY;
    }
    // Spec (sec-math.max) step 2: ToNumber EVERY arg first (observable via
    // valueOf), then reduce — a NaN must not short-circuit before later args
    // are coerced (test262 max/Math.max_each-element-coerced).
    let mut result = f64::NEG_INFINITY;
    let mut saw_nan = false;
    for i in 0..len {
        let num = js_math_to_number(crate::array::js_array_get_f64(arr, i as u32));
        if num.is_nan() {
            saw_nan = true;
        } else if num > result || (num == 0.0 && result == 0.0 && num.is_sign_positive()) {
            // ECMAScript Math.max treats +0 as greater than -0; IEEE `>` treats
            // them as equal, so add an explicit sign-of-zero tiebreaker.
            result = num;
        }
    }
    if saw_nan {
        f64::NAN
    } else {
        result
    }
}

/// ToNumber over `n` already-evaluated arguments, in order, then the spec
/// reduction (`min`: NaN wins, -0 < +0). The arguments are copied into temp
/// roots first and re-read one at a time, so a `valueOf` that collects cannot
/// leave a later argument stale. Called by generated code for a `Math.min` /
/// `Math.max` with at least one argument not proven to be a plain number.
#[inline(never)]
unsafe fn math_minmax_n(args: *const f64, n: u32, is_min: bool) -> f64 {
    use crate::gc::{js_gc_temp_root_get, js_gc_temp_root_push, js_gc_temp_root_truncate};
    let mut result = if is_min {
        f64::INFINITY
    } else {
        f64::NEG_INFINITY
    };
    if n == 0 {
        return result;
    }
    let mut base = 0;
    for i in 0..n as usize {
        let idx = js_gc_temp_root_push((*args.add(i)).to_bits());
        if i == 0 {
            base = idx;
        }
    }
    let mut saw_nan = false;
    for i in 0..n {
        let num = js_math_to_number(f64::from_bits(js_gc_temp_root_get(base + i)));
        if num.is_nan() {
            saw_nan = true;
        } else if is_min {
            if num < result || (num == 0.0 && result == 0.0 && num.is_sign_negative()) {
                result = num;
            }
        } else if num > result || (num == 0.0 && result == 0.0 && num.is_sign_positive()) {
            result = num;
        }
    }
    js_gc_temp_root_truncate(base);
    if saw_nan {
        f64::NAN
    } else {
        result
    }
}

/// `Math.min(a1..an)` over `n` NaN-boxed values at `args` (see [`math_minmax_n`]).
///
/// # Safety
/// `args` must point at `n` readable `f64`s.
#[no_mangle]
pub unsafe extern "C" fn js_math_min_n(args: *const f64, n: u32) -> f64 {
    math_minmax_n(args, n, true)
}

/// `Math.max(a1..an)` over `n` NaN-boxed values at `args` (see [`math_minmax_n`]).
///
/// # Safety
/// `args` must point at `n` readable `f64`s.
#[no_mangle]
pub unsafe extern "C" fn js_math_max_n(args: *const f64, n: u32) -> f64 {
    math_minmax_n(args, n, false)
}

/// Fixed-arity entry points for `Math.min/max` with 2, 3 or 4 operands: the
/// operands arrive in registers (no stack buffer at the call site), are spilled
/// into a callee-local array and handed to the one implementation,
/// [`math_minmax_n`], so the semantics cannot drift from the N-ary helper.
macro_rules! math_minmax_fixed {
    ($name:ident, $is_min:expr, $n:expr, $($arg:ident),+) => {
        #[no_mangle]
        pub extern "C" fn $name($($arg: f64),+) -> f64 {
            let args = [$($arg),+];
            // SAFETY: `args` holds exactly `$n` readable f64s.
            unsafe { math_minmax_n(args.as_ptr(), $n, $is_min) }
        }
    };
}
math_minmax_fixed!(js_math_min_2, true, 2, a, b);
math_minmax_fixed!(js_math_min_3, true, 3, a, b, c);
math_minmax_fixed!(js_math_min_4, true, 4, a, b, c, d);
math_minmax_fixed!(js_math_max_2, false, 2, a, b);
math_minmax_fixed!(js_math_max_3, false, 3, a, b, c);
math_minmax_fixed!(js_math_max_4, false, 4, a, b, c, d);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_math_minmax_fixed_arity_matches_n() {
        let cases: [[f64; 4]; 4] = [
            [3.0, -0.0, 0.0, 1.0],
            [0.0, -0.0, -0.0, 5.0],
            [1.0, f64::NAN, 2.0, 0.5],
            [-7.0, 2.0, 9.0, -0.0],
        ];
        for c in cases {
            let same = |x: f64, y: f64| x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan());
            unsafe {
                assert!(same(
                    js_math_min_2(c[0], c[1]),
                    js_math_min_n(c.as_ptr(), 2)
                ));
                assert!(same(
                    js_math_max_2(c[0], c[1]),
                    js_math_max_n(c.as_ptr(), 2)
                ));
                assert!(same(
                    js_math_min_3(c[0], c[1], c[2]),
                    js_math_min_n(c.as_ptr(), 3)
                ));
                assert!(same(
                    js_math_max_3(c[0], c[1], c[2]),
                    js_math_max_n(c.as_ptr(), 3)
                ));
                assert!(same(
                    js_math_min_4(c[0], c[1], c[2], c[3]),
                    js_math_min_n(c.as_ptr(), 4)
                ));
                assert!(same(
                    js_math_max_4(c[0], c[1], c[2], c[3]),
                    js_math_max_n(c.as_ptr(), 4)
                ));
            }
        }
    }

    fn is_neg_zero(value: f64) -> bool {
        value == 0.0 && value.is_sign_negative()
    }

    #[test]
    fn js_math_minmax_n_orders_and_signed_zero() {
        let a = [3.0f64, -0.0, 0.0];
        unsafe {
            assert!(js_math_min_n(a.as_ptr(), 3).is_sign_negative());
            assert_eq!(js_math_max_n(a.as_ptr(), 3), 3.0);
            let z = [-0.0f64, 0.0];
            assert!(js_math_max_n(z.as_ptr(), 2).is_sign_positive());
            let nan = [1.0f64, f64::NAN, 2.0];
            assert!(js_math_min_n(nan.as_ptr(), 3).is_nan());
            assert!(js_math_max_n(nan.as_ptr(), 3).is_nan());
        }
    }

    #[test]
    fn js_math_sign_preserves_nan_and_signed_zero() {
        assert_eq!(js_math_sign(7.0), 1.0);
        assert_eq!(js_math_sign(-7.0), -1.0);
        assert!(js_math_sign(f64::NAN).is_nan());
        assert!(is_neg_zero(js_math_sign(-0.0)));
        assert_eq!(js_math_sign(0.0).to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn js_math_trunc_preserves_nan_infinity_and_signed_zero() {
        assert_eq!(js_math_trunc(7.9), 7.0);
        assert_eq!(js_math_trunc(-7.9), -7.0);
        assert!(js_math_trunc(f64::NAN).is_nan());
        assert_eq!(js_math_trunc(f64::INFINITY), f64::INFINITY);
        assert_eq!(js_math_trunc(f64::NEG_INFINITY), f64::NEG_INFINITY);
        assert!(is_neg_zero(js_math_trunc(-0.0)));
    }

    #[test]
    fn js_math_round_matches_spec_half_and_neg_zero() {
        // Half rounds toward +∞.
        assert_eq!(js_math_round(0.5), 1.0);
        assert_eq!(js_math_round(2.5), 3.0);
        assert_eq!(js_math_round(-0.5), 0.0);
        assert!(is_neg_zero(js_math_round(-0.5)));
        assert!(is_neg_zero(js_math_round(-0.25)));
        // `0.5 - ε/4` is strictly below a half → +0, where floor(x+0.5) gives 1.
        let x = 0.5 - f64::EPSILON / 4.0;
        assert_eq!(js_math_round(x), 0.0);
        assert!(!is_neg_zero(js_math_round(x)));
        // Large odd integers are returned unchanged (the `.5` add would vanish).
        let big = 1.0 / f64::EPSILON + 1.0;
        assert_eq!(js_math_round(big), big);
        assert_eq!(js_math_round(-big), -big);
        // NaN / ±0 / ±Infinity pass through.
        assert!(js_math_round(f64::NAN).is_nan());
        assert!(is_neg_zero(js_math_round(-0.0)));
        assert_eq!(js_math_round(f64::INFINITY), f64::INFINITY);
    }

    #[test]
    fn js_math_imul_applies_to_int32_semantics() {
        assert_eq!(js_math_imul(2.0, 4.0), 8.0);
        assert_eq!(js_math_imul(f64::NAN, 5.0), 0.0);
        assert_eq!(js_math_imul(f64::INFINITY, 5.0), 0.0);
        assert_eq!(js_math_imul(4_294_967_295.0, 5.0), -5.0);
        assert_eq!(js_math_imul(2_147_483_647.0, 2.0), -2.0);
    }
}
