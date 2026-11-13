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
    let rounded = crate::object::js_math_round_value(js_math_to_number(value));
    // P3 B1 negative control (DESIGN-p3 rev 3.1 §5, REVIEW-p3 D4): a runtime
    // built with `PERRY_FACTS_SABOTAGE=canonnum` returns an INT32-boxed word
    // from this declared-NUM builtin, so a consumer that trusts the NUM fact
    // must be caught by its check-mode witness (rc 86). Build-time, so an
    // ordinary build compiles the branch away.
    if matches!(option_env!("PERRY_FACTS_SABOTAGE"), Some("canonnum"))
        && rounded.fract() == 0.0
        && rounded.abs() < 2_147_483_648.0
    {
        return f64::from_bits(crate::value::INTERNAL_WORD_TAG | (rounded as i32 as u32 as u64));
    }
    rounded
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

/// IEEE-754 binary16 bits of `x`: round to nearest, ties to even, on the bits
/// (exact, no float arithmetic). NaN packs as the quiet NaN 0x7E00, like V8.
pub(crate) fn f64_to_binary16(x: f64) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let mant = bits & 0x000f_ffff_ffff_ffff;
    if exp == 0x7ff {
        return sign | if mant != 0 { 0x7e00 } else { 0x7c00 };
    }
    // The binary16 biased exponent of `x`, before rounding.
    let e = exp - 1023 + 15;
    if e >= 31 {
        return sign | 0x7c00;
    }
    // Keep the top bits of the significand that binary16 holds (10 for a
    // normal, fewer for a subnormal) and round the dropped ones; a carry out
    // of the significand moves into the exponent, up to infinity.
    let (kept, shift) = if e > 0 {
        (((e as u64) << 52) | mant, 42u32)
    } else if e >= -10 {
        (mant | (1u64 << 52), (43 - e) as u32)
    } else {
        return sign; // below half the smallest subnormal: +-0
    };
    let mut half = kept >> shift;
    let rest = kept & ((1u64 << shift) - 1);
    let tie = 1u64 << (shift - 1);
    if rest > tie || (rest == tie && half & 1 == 1) {
        half += 1;
    }
    sign | half as u16
}

/// The number an IEEE-754 binary16 bit pattern denotes.
pub(crate) fn binary16_to_f64(bits: u16) -> f64 {
    let sign = ((bits & 0x8000) as u64) << 48;
    let exp = ((bits >> 10) & 0x1f) as u64;
    let mant = (bits & 0x3ff) as u64;
    let magnitude = match exp {
        // m * 2^-24, exact
        0 => mant as f64 * (1.0 / 16_777_216.0),
        31 if mant == 0 => f64::INFINITY,
        31 => return f64::NAN,
        e => f64::from_bits(((e + 1023 - 15) << 52) | (mant << 42)),
    };
    f64::from_bits(magnitude.to_bits() | sign)
}

/// Math.f16round(x) -> number — nearest IEEE-754 binary16 value
#[no_mangle]
pub extern "C" fn js_math_f16round(value: f64) -> f64 {
    let x = js_math_to_number(value);
    if x.is_nan() {
        return x;
    }
    binary16_to_f64(f64_to_binary16(x))
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

/// Math.exp(x) -> number.
///
/// ECMAScript leaves the result implementation-approximated, and the platform
/// libms (glibc, Apple's libm) round differently from V8 in the last bit for
/// some inputs: `Math.exp(-0.5 * 16 / (22 / 3) ** 2)` is `0.8617756314171563`
/// in Node and `…564` from libm. Programs print, hash and generate code from
/// these values (three.js bakes Gaussian blur weights into its WGSL), so
/// Perry computes exp with V8's algorithm, `base::ieee754::exp`: fdlibm's
/// `e_exp.c` as revised in FreeBSD msun, with V8's exact `exp(1) == E`.
///
/// V8 itself is not one function across architectures: clang builds it with
/// `-ffp-contract=on`, so on aarch64 every `a * b + c` in that source is one
/// fused multiply-add, while baseline x86-64 has no FMA and rounds twice. The
/// two Nodes disagree in the last bit for about 1 in 4000 inputs. [`madd`]
/// makes the same choice per target (Rust never contracts on its own), which
/// matches Node bit for bit on both: 3,000,000 inputs over the whole domain
/// (both reduction branches, tiny, subnormal and overflowing results) against
/// Node 26 on arm64 macOS and on x86-64 Linux.
#[no_mangle]
pub extern "C" fn js_math_exp(x: f64) -> f64 {
    const HALF: [f64; 2] = [0.5, -0.5];
    const O_THRESHOLD: f64 = 7.09782712893383973096e+02; // 0x40862E42 FEFA39EF
    const U_THRESHOLD: f64 = -7.45133219101941108420e+02; // 0xC0874910 D52D3051
    const LN2_HI: [f64; 2] = [6.93147180369123816490e-01, -6.93147180369123816490e-01];
    const LN2_LO: [f64; 2] = [1.90821492927058770002e-10, -1.90821492927058770002e-10];
    const INV_LN2: f64 = std::f64::consts::LOG2_E; // 0x3FF71547 652B82FE
    const P1: f64 = 1.66666666666666019037e-01; // 0x3FC55555 5555553E
    const P2: f64 = -2.77777777770155933842e-03; // 0xBF66C16C 16BEBD93
    const P3: f64 = 6.61375632143793436117e-05; // 0x3F11566A AF25DE2C
    const P4: f64 = -1.65339022054652515390e-06; // 0xBEBBBD41 C5D26BF1
    const P5: f64 = 4.13813679705723846039e-08; // 0x3E663769 72BEA4D0
    const E: f64 = std::f64::consts::E; // 0x4005BF0A 8B145769
    const HUGE: f64 = 1.0e+300;
    const TWOM1000: f64 = 9.33263618503218878990e-302; // 2**-1000
    const TWO1023: f64 = 8.988465674311579539e307; // 2**1023

    let bits = x.to_bits();
    let mut hx = (bits >> 32) as u32;
    let xsb = (hx >> 31) as usize;
    hx &= 0x7fff_ffff;

    // Non-finite and out-of-range arguments.
    if hx >= 0x4086_2E42 {
        // |x| >= 709.78...
        if hx >= 0x7ff0_0000 {
            if ((hx & 0x000f_ffff) | bits as u32) != 0 {
                return x + x; // NaN
            }
            return if xsb == 0 { x } else { 0.0 }; // exp(+-inf) = {inf, 0}
        }
        if x > O_THRESHOLD {
            return HUGE * HUGE; // overflow
        }
        if x < U_THRESHOLD {
            return TWOM1000 * TWOM1000; // underflow
        }
    }

    // Argument reduction: x = k*ln2 + r, |r| <= 0.5*ln2.
    let mut x = x;
    let (hi, lo, k): (f64, f64, i32);
    if hx > 0x3FD6_2E42 {
        // |x| > 0.5 ln2
        if hx < 0x3FF0_A2B2 {
            // and |x| < 1.5 ln2
            if x == 1.0 {
                return E; // V8: the reduction below gets exp(1)'s last bit wrong
            }
            hi = x - LN2_HI[xsb];
            lo = LN2_LO[xsb];
            k = 1 - xsb as i32 - xsb as i32;
        } else {
            k = madd(INV_LN2, x, HALF[xsb]) as i32;
            let t = k as f64;
            hi = madd(-t, LN2_HI[0], x); // t*ln2HI is exact here
            lo = t * LN2_LO[0];
        }
        x = hi - lo;
    } else if hx < 0x3E30_0000 {
        // |x| < 2**-28
        if HUGE + x > 1.0 {
            return 1.0 + x; // inexact
        }
        hi = 0.0;
        lo = 0.0;
        k = 0;
    } else {
        hi = 0.0;
        lo = 0.0;
        k = 0;
    }

    // x is now in the primary range.
    let t = x * x;
    let twopk = if k >= -1021 {
        f64::from_bits(((0x3FF0_0000 + (k << 20)) as u32 as u64) << 32)
    } else {
        f64::from_bits(((0x3FF0_0000 + ((k + 1000) << 20)) as u32 as u64) << 32)
    };
    // x - t*(P1 + t*(P2 + t*(P3 + t*(P4 + t*P5)))), contracted as clang does.
    let c = madd(
        -t,
        madd(t, madd(t, madd(t, madd(t, P5, P4), P3), P2), P1),
        x,
    );
    if k == 0 {
        return 1.0 - ((x * c) / (c - 2.0) - x);
    }
    let y = 1.0 - ((lo - (x * c) / (2.0 - c)) - hi);
    if k >= -1021 {
        if k == 1024 {
            return y * 2.0 * TWO1023;
        }
        y * twopk
    } else {
        y * twopk * TWOM1000
    }
}

/// `a * b + c` the way clang compiles it in V8 (`-ffp-contract=on`): one fused
/// multiply-add on aarch64, a multiply and an add on targets without a
/// baseline FMA. See [`js_math_exp`].
#[inline(always)]
fn madd(a: f64, b: f64, c: f64) -> f64 {
    #[cfg(target_arch = "aarch64")]
    {
        a.mul_add(b, c)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        a * b + c
    }
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
    #[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
    {
        return crate::snap_census::input_call("random.Math", &format!("{:?}", ()), || {
            crate::effect!(host_read, random);
            let mut rng = rand::rng();
            rng.random::<f64>()
        });
    }

    #[cfg(not(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64")))]
    {
        crate::effect!(host_read, random);
        let mut rng = rand::rng();
        rng.random::<f64>()
    }
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

    /// `js_math_exp` is Node's `Math.exp` bit for bit. The first three inputs
    /// are ones where arm64 and x86-64 Node disagree (FMA contraction in V8's
    /// build, see `js_math_exp`); the rest agree on both.
    #[test]
    fn js_math_exp_matches_node_bits() {
        let split: [(f64, f64, f64); 3] = [
            // (x, arm64 Node, x86-64 Node)
            (-0.27762589510530233, 0.7575801829621506, 0.7575801829621505),
            (1.0231953831389546, 2.782070356661735, 2.7820703566617344),
            (
                -25.956580764614046,
                5.335808135302985e-12,
                5.335808135302986e-12,
            ),
        ];
        for (x, arm64, x86) in split {
            let want = if cfg!(target_arch = "aarch64") {
                arm64
            } else {
                x86
            };
            assert_eq!(js_math_exp(x).to_bits(), want.to_bits(), "exp({x})");
        }
        let common: [(f64, f64); 8] = [
            // three.js BloomNode's 22-tap Gaussian weight: libm gives ...564.
            (
                -0.5 * 16.0 / ((22.0 / 3.0) * (22.0 / 3.0)),
                0.8617756314171563,
            ),
            (1.0, std::f64::consts::E),
            (-1.0, 0.36787944117144233),
            (709.78, 1.7928227943945155e308),
            (-745.13, 5e-324),
            (1e-300, 1.0),
            (f64::INFINITY, f64::INFINITY),
            (f64::NEG_INFINITY, 0.0),
        ];
        for (x, want) in common {
            assert_eq!(js_math_exp(x).to_bits(), want.to_bits(), "exp({x})");
        }
        assert_eq!(js_math_exp(-0.0).to_bits(), 1.0f64.to_bits());
        assert!(js_math_exp(f64::NAN).is_nan());
        assert_eq!(js_math_exp(710.0), f64::INFINITY);
        assert_eq!(js_math_exp(-746.0).to_bits(), 0.0f64.to_bits());
    }

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
