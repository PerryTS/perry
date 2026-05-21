//! Dynamic arithmetic dispatch: handles BigInt vs float at runtime.
//!
//! When a parameter has Type::Any (is_union=true), it may hold a BigInt
//! (NaN-boxed with BIGINT_TAG) or a regular f64. These functions check
//! the NaN-box tag at runtime and dispatch to the correct operation.

use super::*;

/// Convert a NaN-boxed JSValue to a *mut BigIntHeader for arithmetic.
/// If the value is already a BigInt, extracts the pointer.
/// Otherwise allocates a new BigInt from the f64 value.
#[inline]
unsafe fn coerce_to_bigint_ptr(val: f64) -> *mut crate::bigint::BigIntHeader {
    let jsval = JSValue::from_bits(val.to_bits());
    if jsval.is_bigint() {
        jsval.as_bigint_ptr() as *mut _
    } else {
        crate::bigint::js_bigint_from_f64(val)
    }
}

/// Dynamic multiply: BigInt * BigInt if either operand is BigInt, else f64 * f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_mul(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let a_ptr = coerce_to_bigint_ptr(a) as *const _;
        let b_ptr = coerce_to_bigint_ptr(b) as *const _;
        let result = crate::bigint::js_bigint_mul(a_ptr, b_ptr);
        return js_nanbox_bigint(result as i64);
    }
    a * b
}

/// Dynamic add: BigInt + BigInt if either operand is BigInt, else f64 + f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_add(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_add(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    a + b
}

/// Dynamic `a + b` for type-uncertain operands. Per JS spec, when either
/// operand is a string after ToPrimitive, the result is string concatenation;
/// otherwise both operands are coerced to numbers and summed (or BigInt-
/// summed when either is BigInt). The codegen dispatches here for `+` when
/// neither operand has a statically-known type — refs #486 (hono's
/// `Node.buildRegExpStr` does `k + c.buildRegExpStr()` inside a for-of loop
/// over `Object.keys(...)` results, both operands lower to plain f64s with
/// inferred type Any, the static-string-concat fast path doesn't fire, and
/// the previous fallback called `js_number_coerce` on each side and `fadd`d
/// the results — turning `"c" + ""` into `NaN + 0 = NaN`).
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_string_or_number_add(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());

    // String concat takes priority: either operand being a string forces
    // ToPrimitive on the other side via the spec's "if either is a string,
    // do concat" branch. js_string_concat_value handles the
    // `string + non-string` case (it calls js_jsvalue_to_string on the
    // non-string side); we use it for both orderings by pre-coercing the
    // other operand to string via js_jsvalue_to_string when it ISN'T a
    // string.
    if a_val.is_any_string() || b_val.is_any_string() {
        let a_str = if a_val.is_any_string() {
            js_get_string_pointer_unified(a) as *mut crate::string::StringHeader
        } else {
            js_jsvalue_to_string(a)
        };
        let b_str = if b_val.is_any_string() {
            js_get_string_pointer_unified(b) as *mut crate::string::StringHeader
        } else {
            js_jsvalue_to_string(b)
        };
        let result = crate::string::js_string_concat(a_str, b_str);
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }

    // BigInt: same as js_dynamic_add.
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_add(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }

    // Both numeric — coerce non-numbers (booleans, null, undefined) the
    // same way the static fallback path did.
    let a_num = if a_val.is_number() || a_val.is_int32() {
        a
    } else {
        crate::builtins::js_number_coerce(a)
    };
    let b_num = if b_val.is_number() || b_val.is_int32() {
        b
    } else {
        crate::builtins::js_number_coerce(b)
    };
    a_num + b_num
}

/// Dynamic subtract: BigInt - BigInt if either operand is BigInt, else f64 - f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_sub(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_sub(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    a - b
}

/// Dynamic divide: BigInt / BigInt if either operand is BigInt, else f64 / f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_div(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_div(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    a / b
}

/// Dynamic modulo: BigInt % BigInt if either operand is BigInt, else f64 % f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_mod(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_mod(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // Float modulo: a - trunc(a / b) * b
    a - (a / b).trunc() * b
}

/// Dynamic negate: -BigInt if operand is BigInt, else -f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_neg(a: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    if a_val.is_bigint() {
        let result = crate::bigint::js_bigint_neg(a_val.as_bigint_ptr());
        return js_nanbox_bigint(result as i64);
    }
    -a
}

/// Dynamic right shift: BigInt >> if either operand is BigInt, else i32 >> for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_shr(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_shr(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    // Rust `f64 as i32` saturates at i32::MAX for values >= 2^31, but JS wraps.
    let ai = (a as i64) as i32;
    let bi = ((b as i64) as i32) & 0x1f;
    (ai >> bi) as f64
}

/// Dynamic left shift: BigInt << if either operand is BigInt, else i32 << for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_shl(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_shl(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    let ai = (a as i64) as i32;
    let bi = ((b as i64) as i32) & 0x1f;
    (ai << bi) as f64
}

/// Dynamic bitwise AND: BigInt & if either operand is BigInt, else i32 & for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitand(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_and(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) & ((b as i64) as i32)) as f64
}

/// Dynamic bitwise OR: BigInt | if either operand is BigInt, else i32 | for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitor(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_or(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) | ((b as i64) as i32)) as f64
}

/// Dynamic bitwise XOR: BigInt ^ if either operand is BigInt, else i32 ^ for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitxor(a: f64, b: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    let b_val = JSValue::from_bits(b.to_bits());
    if a_val.is_bigint() || b_val.is_bigint() {
        let result = crate::bigint::js_bigint_xor(
            coerce_to_bigint_ptr(a) as *const _,
            coerce_to_bigint_ptr(b) as *const _,
        );
        return js_nanbox_bigint(result as i64);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) ^ ((b as i64) as i32)) as f64
}
