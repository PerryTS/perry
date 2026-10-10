//! Exact comparison across arbitrary-precision integers, Numbers and strings.
use super::*;

fn ordering(order: std::cmp::Ordering) -> i32 {
    match order {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
#[no_mangle]
pub extern "C" fn js_bigint_cmp(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    ordering(bigint_integer(a).cmp(&bigint_integer(b)))
}
#[no_mangle]
pub extern "C" fn js_bigint_eq(a: *const BigIntHeader, b: *const BigIntHeader) -> i32 {
    let a = clean_bigint_ptr(a);
    let b = clean_bigint_ptr(b);
    if a.is_null() || b.is_null() {
        return i32::from(a == b);
    }
    // Constructors canonicalize sign extension, so equality needs no temporary.
    i32::from(unsafe {
        crate::bigint::BigIntHeader::all_limbs(a) == crate::bigint::BigIntHeader::all_limbs(b)
    })
}
pub(crate) fn string_to_bigint(raw: &str) -> Option<*mut BigIntHeader> {
    convert::parse_bigint_string(raw)
        .ok()
        .map(bigint_alloc_with_limbs)
}
pub(crate) fn bigint_cmp_f64(x: *const BigIntHeader, y: f64) -> i32 {
    if y.is_nan() {
        return 2;
    }
    if y == f64::INFINITY {
        return -1;
    }
    if y == f64::NEG_INFINITY {
        return 1;
    }
    let floor = y.floor();
    let c = ordering(bigint_integer(x).cmp(&BigInt::from_f64(floor).expect("finite integer")));
    if c == 0 && y > floor {
        -1
    } else {
        c
    }
}
