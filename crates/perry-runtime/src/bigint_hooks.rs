//! BigInt operations reached from generic value and object hubs.
//! Slots are filled by the BigInt installer before a program can produce a
//! BigInt. Number and string paths never enter these wrappers.
use crate::bigint::BigIntHeader;
use crate::feature_hooks::Hook;

static BIGINT_ALLOC_WITH_LIMBS: Hook<fn(&[u64]) -> *mut BigIntHeader> = Hook::empty();
static BIGINT_CMP_F64: Hook<fn(*const BigIntHeader, f64) -> i32> = Hook::empty();
static JS_BIGINT_ADD: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_AND: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_AS_INT_N: Hook<extern "C" fn(u32, *const BigIntHeader) -> *mut BigIntHeader> =
    Hook::empty();
static JS_BIGINT_AS_UINT_N: Hook<extern "C" fn(u32, *const BigIntHeader) -> *mut BigIntHeader> =
    Hook::empty();
static JS_BIGINT_CMP: Hook<extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> i32> =
    Hook::empty();
static JS_BIGINT_DIV: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_EQ: Hook<extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> i32> =
    Hook::empty();
static JS_BIGINT_FROM_F64: Hook<extern "C" fn(f64) -> *mut BigIntHeader> = Hook::empty();
static JS_BIGINT_FROM_I64: Hook<extern "C" fn(i64) -> *mut BigIntHeader> = Hook::empty();
static JS_BIGINT_FROM_STRING: Hook<extern "C" fn(*const u8, u32) -> *mut BigIntHeader> =
    Hook::empty();
static JS_BIGINT_FROM_U64: Hook<extern "C" fn(u64) -> *mut BigIntHeader> = Hook::empty();
static JS_BIGINT_IS_NEGATIVE: Hook<extern "C" fn(*const BigIntHeader) -> i32> = Hook::empty();
static JS_BIGINT_IS_ZERO: Hook<extern "C" fn(*const BigIntHeader) -> i32> = Hook::empty();
static JS_BIGINT_MOD: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_MUL: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_NEG: Hook<extern "C" fn(*const BigIntHeader) -> *mut BigIntHeader> = Hook::empty();
static JS_BIGINT_NOT: Hook<extern "C" fn(*const BigIntHeader) -> *mut BigIntHeader> = Hook::empty();
static JS_BIGINT_OR: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_POW: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_SHL: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_SHR: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_SUB: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static JS_BIGINT_TO_BUFFER: Hook<
    extern "C" fn(*const BigIntHeader, i32) -> *mut crate::buffer::BufferHeader,
> = Hook::empty();
static JS_BIGINT_TO_F64: Hook<extern "C" fn(*const BigIntHeader) -> f64> = Hook::empty();
static JS_BIGINT_TO_STRING: Hook<
    extern "C" fn(*const BigIntHeader) -> *mut crate::string::StringHeader,
> = Hook::empty();
static JS_BIGINT_TO_STRING_RADIX: Hook<
    extern "C" fn(*const BigIntHeader, i32) -> *mut crate::string::StringHeader,
> = Hook::empty();
static JS_BIGINT_XOR: Hook<
    extern "C" fn(*const BigIntHeader, *const BigIntHeader) -> *mut BigIntHeader,
> = Hook::empty();
static STRING_TO_BIGINT: Hook<fn(&str) -> Option<*mut BigIntHeader>> = Hook::empty();

pub(crate) fn install() {
    BIGINT_ALLOC_WITH_LIMBS.set(|limbs| crate::bigint::bigint_alloc_with_limbs(limbs));
    BIGINT_CMP_F64.set(crate::bigint::bigint_cmp_f64);
    JS_BIGINT_ADD.set(crate::bigint::js_bigint_add);
    JS_BIGINT_AND.set(crate::bigint::js_bigint_and);
    JS_BIGINT_AS_INT_N.set(crate::bigint::js_bigint_as_int_n);
    JS_BIGINT_AS_UINT_N.set(crate::bigint::js_bigint_as_uint_n);
    JS_BIGINT_CMP.set(crate::bigint::js_bigint_cmp);
    JS_BIGINT_DIV.set(crate::bigint::js_bigint_div);
    JS_BIGINT_EQ.set(crate::bigint::js_bigint_eq);
    JS_BIGINT_FROM_F64.set(crate::bigint::js_bigint_from_f64);
    JS_BIGINT_FROM_I64.set(crate::bigint::js_bigint_from_i64);
    JS_BIGINT_FROM_STRING.set(crate::bigint::js_bigint_from_string);
    JS_BIGINT_FROM_U64.set(crate::bigint::js_bigint_from_u64);
    JS_BIGINT_IS_NEGATIVE.set(crate::bigint::js_bigint_is_negative);
    JS_BIGINT_IS_ZERO.set(crate::bigint::js_bigint_is_zero);
    JS_BIGINT_MOD.set(crate::bigint::js_bigint_mod);
    JS_BIGINT_MUL.set(crate::bigint::js_bigint_mul);
    JS_BIGINT_NEG.set(crate::bigint::js_bigint_neg);
    JS_BIGINT_NOT.set(crate::bigint::js_bigint_not);
    JS_BIGINT_OR.set(crate::bigint::js_bigint_or);
    JS_BIGINT_POW.set(crate::bigint::js_bigint_pow);
    JS_BIGINT_SHL.set(crate::bigint::js_bigint_shl);
    JS_BIGINT_SHR.set(crate::bigint::js_bigint_shr);
    JS_BIGINT_SUB.set(crate::bigint::js_bigint_sub);
    JS_BIGINT_TO_BUFFER.set(crate::bigint::js_bigint_to_buffer);
    JS_BIGINT_TO_F64.set(crate::bigint::js_bigint_to_f64);
    JS_BIGINT_TO_STRING.set(crate::bigint::js_bigint_to_string);
    JS_BIGINT_TO_STRING_RADIX.set(crate::bigint::js_bigint_to_string_radix);
    JS_BIGINT_XOR.set(crate::bigint::js_bigint_xor);
    STRING_TO_BIGINT.set(crate::bigint::string_to_bigint);
}

#[inline]
pub(crate) fn bigint_alloc_with_limbs(limbs: impl AsRef<[u64]>) -> *mut BigIntHeader {
    BIGINT_ALLOC_WITH_LIMBS
        .get()
        .expect("BigInt producer requires its installer")(limbs.as_ref())
}

#[inline]
pub(crate) fn bigint_cmp_f64(x: *const BigIntHeader, y: f64) -> i32 {
    BIGINT_CMP_F64
        .get()
        .expect("BigInt producer requires its installer")(x, y)
}

#[inline]
pub(crate) extern "C" fn js_bigint_add(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_ADD
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_and(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_AND
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_as_int_n(
    bits: u32,
    value: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_AS_INT_N
        .get()
        .expect("BigInt producer requires its installer")(bits, value)
}

#[inline]
pub(crate) extern "C" fn js_bigint_as_uint_n(
    bits: u32,
    value: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_AS_UINT_N
        .get()
        .expect("BigInt producer requires its installer")(bits, value)
}

#[inline]
pub(crate) extern "C" fn js_bigint_cmp(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    JS_BIGINT_CMP
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_div(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_DIV
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_eq(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    JS_BIGINT_EQ
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_from_f64(value: f64) -> *mut BigIntHeader {
    JS_BIGINT_FROM_F64
        .get()
        .expect("BigInt producer requires its installer")(value)
}

#[inline]
pub(crate) extern "C" fn js_bigint_from_i64(value: i64) -> *mut BigIntHeader {
    JS_BIGINT_FROM_I64
        .get()
        .expect("BigInt producer requires its installer")(value)
}

#[inline]
pub(crate) extern "C" fn js_bigint_from_string(data: *const u8, len: u32) -> *mut BigIntHeader {
    JS_BIGINT_FROM_STRING
        .get()
        .expect("BigInt producer requires its installer")(data, len)
}

#[inline]
pub(crate) extern "C" fn js_bigint_from_u64(value: u64) -> *mut BigIntHeader {
    JS_BIGINT_FROM_U64
        .get()
        .expect("BigInt producer requires its installer")(value)
}

#[inline]
pub(crate) extern "C" fn js_bigint_is_negative(a: *const BigIntHeader) -> i32 {
    JS_BIGINT_IS_NEGATIVE
        .get()
        .expect("BigInt producer requires its installer")(a)
}

#[inline]
pub(crate) extern "C" fn js_bigint_is_zero(a: *const BigIntHeader) -> i32 {
    JS_BIGINT_IS_ZERO
        .get()
        .expect("BigInt producer requires its installer")(a)
}

#[inline]
pub(crate) extern "C" fn js_bigint_mod(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_MOD
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_mul(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_MUL
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_neg(a: *const BigIntHeader) -> *mut BigIntHeader {
    JS_BIGINT_NEG
        .get()
        .expect("BigInt producer requires its installer")(a)
}

#[inline]
pub(crate) extern "C" fn js_bigint_not(a: *const BigIntHeader) -> *mut BigIntHeader {
    JS_BIGINT_NOT
        .get()
        .expect("BigInt producer requires its installer")(a)
}

#[inline]
pub(crate) extern "C" fn js_bigint_or(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_OR
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_pow(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_POW
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_shl(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_SHL
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_shr(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_SHR
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_sub(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_SUB
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) extern "C" fn js_bigint_to_buffer(
    a: *const BigIntHeader,
    length: i32,
) -> *mut crate::buffer::BufferHeader {
    JS_BIGINT_TO_BUFFER
        .get()
        .expect("BigInt producer requires its installer")(a, length)
}

#[inline]
pub(crate) extern "C" fn js_bigint_to_f64(a: *const BigIntHeader) -> f64 {
    JS_BIGINT_TO_F64
        .get()
        .expect("BigInt producer requires its installer")(a)
}

#[inline]
pub(crate) extern "C" fn js_bigint_to_string(
    a: *const BigIntHeader,
) -> *mut crate::string::StringHeader {
    JS_BIGINT_TO_STRING
        .get()
        .expect("BigInt producer requires its installer")(a)
}

#[inline]
pub(crate) extern "C" fn js_bigint_to_string_radix(
    a: *const BigIntHeader,
    radix: i32,
) -> *mut crate::string::StringHeader {
    JS_BIGINT_TO_STRING_RADIX
        .get()
        .expect("BigInt producer requires its installer")(a, radix)
}

#[inline]
pub(crate) extern "C" fn js_bigint_xor(
    a: *const BigIntHeader,
    b: *const BigIntHeader,
) -> *mut BigIntHeader {
    JS_BIGINT_XOR
        .get()
        .expect("BigInt producer requires its installer")(a, b)
}

#[inline]
pub(crate) fn string_to_bigint(raw: &str) -> Option<*mut BigIntHeader> {
    STRING_TO_BIGINT
        .get()
        .expect("BigInt producer requires its installer")(raw)
}
