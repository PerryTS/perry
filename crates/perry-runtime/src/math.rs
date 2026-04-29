//! Math operations runtime support

use rand::Rng;

/// Math.pow(base, exponent) -> number
#[no_mangle]
pub extern "C" fn js_math_pow(base: f64, exp: f64) -> f64 {
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

/// Math.clz32(x) -> number — count leading zeros of 32-bit integer
#[no_mangle]
pub extern "C" fn js_math_clz32(x: f64) -> f64 {
    // JS spec: convert to UInt32 first
    let n = if x.is_nan() || x.is_infinite() {
        0u32
    } else {
        x as i64 as u32
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
    let mut rng = rand::thread_rng();
    rng.gen::<f64>()
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
    let mut result = f64::INFINITY;
    for i in 0..len {
        let num = crate::array::js_array_get_f64(arr, i as u32);
        if num.is_nan() {
            return f64::NAN;
        }
        if num < result {
            result = num;
        }
    }
    result
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
    let mut result = f64::NEG_INFINITY;
    for i in 0..len {
        let num = crate::array::js_array_get_f64(arr, i as u32);
        if num.is_nan() {
            return f64::NAN;
        }
        if num > result {
            result = num;
        }
    }
    result
}
