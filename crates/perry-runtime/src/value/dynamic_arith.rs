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

/// Throw `TypeError: Cannot mix BigInt and other types, use explicit
/// conversions`, matching Node when a BigInt operand is combined with a
/// non-BigInt operand in an arithmetic / bitwise operation (#2908).
#[cold]
unsafe fn throw_mix_bigint() -> ! {
    let msg = b"Cannot mix BigInt and other types, use explicit conversions";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

/// Enforce Node's rule that BigInt operators require *both* operands to be
/// BigInt. Returns true when both are BigInt (proceed with the BigInt op),
/// false when neither is (use the numeric path), and throws a TypeError when
/// exactly one operand is a BigInt.
#[inline]
unsafe fn both_bigint_or_throw(a: f64, b: f64) -> bool {
    let a_big = JSValue::from_bits(a.to_bits()).is_bigint();
    let b_big = JSValue::from_bits(b.to_bits()).is_bigint();
    if a_big && b_big {
        true
    } else if a_big || b_big {
        throw_mix_bigint();
    } else {
        false
    }
}

type BigIntBinaryOp = extern "C" fn(
    *const crate::bigint::BigIntHeader,
    *const crate::bigint::BigIntHeader,
) -> *mut crate::bigint::BigIntHeader;

#[inline]
unsafe fn coerce_to_bigint_handle<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    value: &crate::gc::RuntimeHandle<'scope>,
) -> crate::gc::RuntimeHandle<'scope> {
    let ptr = coerce_to_bigint_ptr(value.get_nanbox_f64());
    scope.root_bigint_ptr(ptr as *const crate::bigint::BigIntHeader)
}

#[inline]
unsafe fn dynamic_bigint_binary_op(a: f64, b: f64, op: BigIntBinaryOp) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let a_handle = scope.root_nanbox_f64(a);
    let b_handle = scope.root_nanbox_f64(b);
    dynamic_bigint_binary_op_from_handles(&scope, &a_handle, &b_handle, op)
}

#[inline]
unsafe fn dynamic_bigint_binary_op_from_handles<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    a: &crate::gc::RuntimeHandle<'scope>,
    b: &crate::gc::RuntimeHandle<'scope>,
    op: BigIntBinaryOp,
) -> f64 {
    let a_bigint = coerce_to_bigint_handle(scope, a);
    let b_bigint = coerce_to_bigint_handle(scope, b);
    let result = op(
        a_bigint.get_raw_const_ptr::<crate::bigint::BigIntHeader>(),
        b_bigint.get_raw_const_ptr::<crate::bigint::BigIntHeader>(),
    );
    js_nanbox_bigint(result as i64)
}

/// Dynamic multiply: BigInt * BigInt if either operand is BigInt, else f64 * f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_mul(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_mul);
    }
    a * b
}

/// Dynamic add: BigInt + BigInt if either operand is BigInt, else f64 + f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_add(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_add);
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let a_handle = scope.root_nanbox_f64(a);
    let b_handle = scope.root_nanbox_f64(b);
    let a_val = JSValue::from_bits(a_handle.get_nanbox_f64().to_bits());
    let b_val = JSValue::from_bits(b_handle.get_nanbox_f64().to_bits());

    // String concat takes priority: either operand being a string forces
    // ToPrimitive on the other side via the spec's "if either is a string,
    // do concat" branch. js_string_concat_value handles the
    // `string + non-string` case (it calls js_jsvalue_to_string on the
    // non-string side); we use it for both orderings by pre-coercing the
    // other operand to string via js_jsvalue_to_string when it ISN'T a
    // string.
    if a_val.is_any_string() || b_val.is_any_string() {
        let a_str = if JSValue::from_bits(a_handle.get_nanbox_f64().to_bits()).is_any_string() {
            js_get_string_pointer_unified(a_handle.get_nanbox_f64())
                as *const crate::string::StringHeader
        } else {
            js_jsvalue_to_string(a_handle.get_nanbox_f64()) as *const crate::string::StringHeader
        };
        let a_str_handle = scope.root_string_ptr(a_str);
        let b_str = if JSValue::from_bits(b_handle.get_nanbox_f64().to_bits()).is_any_string() {
            js_get_string_pointer_unified(b_handle.get_nanbox_f64())
                as *const crate::string::StringHeader
        } else {
            js_jsvalue_to_string(b_handle.get_nanbox_f64()) as *const crate::string::StringHeader
        };
        let b_str_handle = scope.root_string_ptr(b_str);
        let result = crate::string::js_string_concat(
            a_str_handle.get_raw_const_ptr(),
            b_str_handle.get_raw_const_ptr(),
        );
        return f64::from_bits(JSValue::string_ptr(result).bits());
    }

    // BigInt: same as js_dynamic_add. Neither operand is a string here
    // (the concat branch above already handled that), so a mixed
    // BigInt/Number `+` throws TypeError just like Node.
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op_from_handles(
            &scope,
            &a_handle,
            &b_handle,
            crate::bigint::js_bigint_add,
        );
    }

    // Both numeric — coerce non-numbers (booleans, null, undefined) the
    // same way the static fallback path did.
    let a_num = if a_val.is_number() || a_val.is_int32() {
        a_handle.get_nanbox_f64()
    } else {
        crate::builtins::js_number_coerce(a_handle.get_nanbox_f64())
    };
    let b_num = if b_val.is_number() || b_val.is_int32() {
        b_handle.get_nanbox_f64()
    } else {
        crate::builtins::js_number_coerce(b_handle.get_nanbox_f64())
    };
    a_num + b_num
}

/// Dynamic subtract: BigInt - BigInt if either operand is BigInt, else f64 - f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_sub(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_sub);
    }
    a - b
}

/// Dynamic divide: BigInt / BigInt if either operand is BigInt, else f64 / f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_div(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_div);
    }
    a / b
}

/// Dynamic modulo: BigInt % BigInt if either operand is BigInt, else f64 % f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_mod(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_mod);
    }
    // Float modulo: a - trunc(a / b) * b
    a - (a / b).trunc() * b
}

/// Dynamic negate: -BigInt if operand is BigInt, else -f64.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_neg(a: f64) -> f64 {
    let a_val = JSValue::from_bits(a.to_bits());
    if a_val.is_bigint() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let a_handle = scope.root_bigint_ptr(a_val.as_bigint_ptr());
        let result = crate::bigint::js_bigint_neg(
            a_handle.get_raw_const_ptr::<crate::bigint::BigIntHeader>(),
        );
        return js_nanbox_bigint(result as i64);
    }
    -a
}

/// Dynamic right shift: BigInt >> if either operand is BigInt, else i32 >> for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_shr(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_shr);
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
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_shl);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    let ai = (a as i64) as i32;
    let bi = ((b as i64) as i32) & 0x1f;
    (ai << bi) as f64
}

/// Dynamic bitwise AND: BigInt & if either operand is BigInt, else i32 & for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitand(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_and);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) & ((b as i64) as i32)) as f64
}

/// Dynamic bitwise OR: BigInt | if either operand is BigInt, else i32 | for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitor(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_or);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) | ((b as i64) as i32)) as f64
}

/// Dynamic bitwise XOR: BigInt ^ if either operand is BigInt, else i32 ^ for numbers.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_bitxor(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_xor);
    }
    // JS ToInt32: f64 -> i64 -> i32 (wrapping), NOT f64 -> i32 (saturating).
    (((a as i64) as i32) ^ ((b as i64) as i32)) as f64
}

/// Dynamic exponentiation: `BigInt ** BigInt` when both operands are BigInt
/// (#2908), else numeric `Math.pow`. A mixed BigInt/Number `**` throws
/// TypeError; a negative BigInt exponent throws RangeError (handled inside
/// `js_bigint_pow`).
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_pow(a: f64, b: f64) -> f64 {
    if both_bigint_or_throw(a, b) {
        return dynamic_bigint_binary_op(a, b, crate::bigint::js_bigint_pow);
    }
    crate::math::js_math_pow(a, b)
}

/// Dynamic unsigned right shift. BigInts have no `>>>` operator in
/// ECMAScript, so any BigInt operand throws TypeError (#2908); otherwise
/// numeric ToUint32 `>>>`.
#[no_mangle]
pub unsafe extern "C" fn js_dynamic_ushr(a: f64, b: f64) -> f64 {
    let a_big = JSValue::from_bits(a.to_bits()).is_bigint();
    let b_big = JSValue::from_bits(b.to_bits()).is_bigint();
    if a_big || b_big {
        let msg = b"BigInts have no unsigned right shift, use >> instead";
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(s);
        crate::exception::js_throw(js_nanbox_pointer(err as i64));
    }
    // JS ToUint32 then logical shift, count masked to 5 bits.
    let ai = (a as i64) as u32;
    let bi = ((b as i64) as i32 as u32) & 0x1f;
    (ai >> bi) as f64
}

#[used]
static KEEP_DYNAMIC_POW: unsafe extern "C" fn(f64, f64) -> f64 = js_dynamic_pow;
#[used]
static KEEP_DYNAMIC_USHR: unsafe extern "C" fn(f64, f64) -> f64 = js_dynamic_ushr;
