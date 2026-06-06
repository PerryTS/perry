//! Shared argument-marshalling / result-boxing helpers and the central
//! property-get + method-call routers for all Temporal types.
//!
//! The runtime touches Temporal dispatch in exactly two places —
//! `js_object_get_field_by_name` (getters) and `js_native_call_method`
//! (methods) — each with a single brand arm that forwards here. From here we
//! match on the cell's [`TemporalKind`](super::TemporalKind) and hand off to the
//! per-type module, so adding a type touches only its own file plus the two
//! `match` arms below.

use super::{temporal_value_ref, TemporalValue};
use crate::value::JSValue;

// ---- argument coercion ----------------------------------------------------

/// `ToNumber(args[i])`, or `NaN` if the argument is absent.
#[inline]
pub(crate) fn num_arg(args: &[f64], i: usize) -> f64 {
    match args.get(i) {
        Some(&v) => JSValue::from_bits(v.to_bits()).to_number(),
        None => f64::NAN,
    }
}

/// `args[i]` coerced to an integer for a Temporal numeric field. Absent /
/// non-finite → 0 (Temporal treats missing duration/time fields as 0).
#[inline]
pub(crate) fn int_arg(args: &[f64], i: usize) -> i64 {
    let n = num_arg(args, i);
    if n.is_finite() {
        n.trunc() as i64
    } else {
        0
    }
}

/// Saturate an integer Temporal time field into a `u8` slot: any value outside
/// `0..=u8::MAX` maps to `u8::MAX`. Every `u8` time field's valid maximum is
/// below 255, so `temporal_rs`'s range check then rejects it with a RangeError —
/// rather than an `as u8` cast silently *wrapping* a too-large value back into
/// range (e.g. `256 as u8 == 0`, which would be accepted as midnight).
#[inline]
pub(crate) fn field_u8(v: i64) -> u8 {
    if (0..=u8::MAX as i64).contains(&v) {
        v as u8
    } else {
        u8::MAX
    }
}

/// Sub-second (`u16`) counterpart of [`field_u8`]. Every `u16` time field caps
/// at 999, well under `u16::MAX`, so out-of-range values still reject.
#[inline]
pub(crate) fn field_u16(v: i64) -> u16 {
    if (0..=u16::MAX as i64).contains(&v) {
        v as u16
    } else {
        u16::MAX
    }
}

/// Raw argument value (NaN-boxed) at `i`, or `undefined`.
#[inline]
pub(crate) fn raw_arg(args: &[f64], i: usize) -> f64 {
    args.get(i)
        .copied()
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[inline]
pub(crate) fn is_undefined(v: f64) -> bool {
    v.to_bits() == crate::value::TAG_UNDEFINED
}

/// Read a JS string value (heap `StringHeader` or inline SSO) into a Rust
/// `String`.
pub(crate) fn read_string(value: f64) -> String {
    let ptr =
        crate::value::js_get_string_pointer_unified(value) as *const crate::string::StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// Read a JS BigInt (or a Number, leniently) as an `i128` for the epoch-time
/// APIs. BigInts are bridged through their decimal string (Perry's BigInt is a
/// limb array with no direct `i128` accessor). Returns `None` for other types.
pub(crate) fn read_bigint_i128(v: f64) -> Option<i128> {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_bigint() {
        let ptr = jv.as_bigint_ptr();
        let s_ptr = crate::bigint::js_bigint_to_string(ptr as *mut crate::bigint::BigIntHeader);
        let s = read_string(crate::value::js_nanbox_string(s_ptr as i64));
        return s.parse::<i128>().ok();
    }
    if jv.is_number() || jv.is_int32() {
        let n = jv.to_number();
        if n.is_finite() {
            return Some(n as i128);
        }
    }
    None
}

/// Box an `i128` as a JS BigInt value (via its decimal string).
pub(crate) fn bigint_from_i128(n: i128) -> f64 {
    let s = n.to_string();
    let ptr = crate::bigint::js_bigint_from_string(s.as_ptr(), s.len() as u32);
    crate::value::js_nanbox_bigint(ptr as i64)
}

// ---- result boxing --------------------------------------------------------

/// Box a Rust string as a JS string value.
#[inline]
pub(crate) fn string(s: &str) -> f64 {
    let p = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    crate::value::js_nanbox_string(p as i64)
}

#[inline]
pub(crate) fn boolean(b: bool) -> f64 {
    f64::from_bits(if b {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

#[inline]
pub(crate) fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// A JS Number from any integer-ish value.
#[inline]
pub(crate) fn number<N: Into<f64>>(n: N) -> f64 {
    n.into()
}

/// A JS Number from an `i64` / `i128` Temporal getter (e.g. `Duration.years`,
/// `Instant.epochMilliseconds`). Values beyond 2^53 lose precision exactly as
/// they do in Node (these getters return Number, not BigInt).
#[inline]
pub(crate) fn number_i128(n: i128) -> f64 {
    n as f64
}

// ---- error helpers --------------------------------------------------------

/// Surface a `temporal_rs` failure as the matching JS error. `temporal_rs`
/// renders its errors as `"<Kind>: <message>"` (e.g. `"RangeError: …"`); we
/// strip that prefix (the JS error object already carries the kind) and route
/// `TypeError`s to a JS `TypeError`, everything else to `RangeError` — which is
/// what the spec maps almost every Temporal validation/parse failure to.
pub(crate) fn throw_temporal(e: temporal_rs::TemporalError) -> ! {
    let rendered = e.to_string();
    let (kind, msg) = match rendered.split_once(": ") {
        Some((k, rest)) if k.ends_with("Error") => (k, rest),
        _ => ("RangeError", rendered.as_str()),
    };
    if kind == "TypeError" {
        crate::object::throw_object_type_error(msg.as_bytes());
    }
    crate::fs::validate::throw_range_error_with_code(msg)
}

/// Unwrap a `TemporalResult`, throwing a JS `RangeError` on `Err`.
#[inline]
pub(crate) fn ok_or_throw<T>(r: temporal_rs::TemporalResult<T>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => throw_temporal(e),
    }
}

/// `valueOf` throws on every Temporal type (the spec bans implicit ordering /
/// arithmetic coercion). Used by every type's method router.
pub(crate) fn throw_value_of(type_name: &str) -> ! {
    crate::object::throw_object_type_error(
        format!(
            "Called {type_name}.prototype.valueOf which is not supported. Use compare() or \
             equals() instead."
        )
        .as_bytes(),
    )
}

/// Throw `TypeError: <recv>.<method> is not a function` for an unknown method on
/// a Temporal receiver (matches the generic dispatch-miss message shape).
pub(crate) fn throw_no_method(type_name: &str, method: &str) -> ! {
    crate::object::throw_object_type_error(
        format!("{type_name}.prototype.{method} is not a function").as_bytes(),
    )
}

// ---- central routers ------------------------------------------------------

/// Resolve a getter (`duration.years`, `plainDate.month`, …). Returns `None`
/// for an unknown property name (caller returns `undefined`) or a non-Temporal
/// receiver.
pub fn get_property(recv: f64, name: &str) -> Option<f64> {
    match temporal_value_ref(recv)? {
        TemporalValue::Duration(d) => super::duration::get(d, name),
        TemporalValue::Instant(i) => super::instant::get(i, name),
        TemporalValue::PlainDate(d) => super::plain_date::get(d, name),
        TemporalValue::PlainTime(t) => super::plain_time::get(t, name),
        TemporalValue::PlainDateTime(dt) => super::plain_date_time::get(dt, name),
        TemporalValue::PlainYearMonth(ym) => super::plain_year_month::get(ym, name),
        TemporalValue::PlainMonthDay(md) => super::plain_month_day::get(md, name),
        TemporalValue::ZonedDateTime(z) => super::zoned_date_time::get(z, name),
    }
}

/// Dispatch an instance method (`duration.add(x)`, `instant.toString()`, …).
/// The caller has already brand-checked `recv` as a Temporal value, so an
/// unknown method name throws `TypeError` rather than returning `None`.
pub fn call_method(recv: f64, name: &str, args: &[f64]) -> f64 {
    match temporal_value_ref(recv) {
        Some(TemporalValue::Duration(d)) => super::duration::call(recv, d, name, args),
        Some(TemporalValue::Instant(i)) => super::instant::call(recv, i, name, args),
        Some(TemporalValue::PlainDate(d)) => super::plain_date::call(recv, d, name, args),
        Some(TemporalValue::PlainTime(t)) => super::plain_time::call(recv, t, name, args),
        Some(TemporalValue::PlainDateTime(dt)) => {
            super::plain_date_time::call(recv, dt, name, args)
        }
        Some(TemporalValue::PlainYearMonth(ym)) => {
            super::plain_year_month::call(recv, ym, name, args)
        }
        Some(TemporalValue::PlainMonthDay(md)) => {
            super::plain_month_day::call(recv, md, name, args)
        }
        Some(TemporalValue::ZonedDateTime(z)) => super::zoned_date_time::call(recv, z, name, args),
        None => undefined(),
    }
}
