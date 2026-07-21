//! `Temporal.PlainTime` — wraps [`temporal_rs::PlainTime`] (#4692).
//!
//! Wall-clock time with no date or timezone. No calendar, so the plainest of
//! the plain types.

use super::dispatch::{self, field_u16, field_u8, ok_or_throw, raw_arg, string};
use super::{alloc_temporal_cell, temporal_value_ref_or_subclass, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::ToStringRoundingOptions;
use temporal_rs::PlainTime;

const TYPE_NAME: &str = "Temporal.PlainTime";

fn wrap(t: PlainTime) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainTime(t))
}

/// `ToIntegerWithTruncation` for an optional `PlainTime` constructor field
/// (absent / `undefined` → 0), as an `i64`. Real `ToNumber` observes `valueOf`
/// and rejects `Infinity`/`NaN` with a `RangeError` at the field's read position.
fn time_field(args: &[f64], i: usize) -> i64 {
    dispatch::optional_integer_with_truncation(raw_arg(args, i))
}

/// `new Temporal.PlainTime(hour?, minute?, second?, ms?, µs?, ns?)`. Out-of-range
/// fields saturate via `field_u8`/`field_u16` so `try_new` rejects them (an `as
/// u8` cast on the raw i64 would *wrap* `256` back to `0` and accept it).
pub fn construct(args: &[f64]) -> f64 {
    dispatch::require_construct(TYPE_NAME);
    wrap(ok_or_throw(PlainTime::try_new(
        field_u8(time_field(args, 0)),
        field_u8(time_field(args, 1)),
        field_u8(time_field(args, 2)),
        field_u16(time_field(args, 3)),
        field_u16(time_field(args, 4)),
        field_u16(time_field(args, 5)),
    )))
}

fn coerce_time(v: f64) -> PlainTime {
    // ToTemporalTime's default overflow is "constrain".
    coerce_time_overflow(v, temporal_rs::options::Overflow::Constrain)
}

/// `ToTemporalTime(item, overflow)`. From a `Temporal.PlainTime` (clone), an ISO
/// time string, or a property bag (out-of-range fields handled per `overflow` —
/// `from()` reads it from options, defaulting to constrain). A property bag with
/// no recognized time field, or any non-string primitive (number / bigint /
/// boolean / null / symbol), throws TypeError; a malformed string → RangeError.
fn coerce_time_overflow(v: f64, overflow: temporal_rs::options::Overflow) -> PlainTime {
    // A `PlainTime` / `PlainDateTime` / `ZonedDateTime` supplies its time via
    // its internal slot — NO observable property getter calls
    // (`argument-plaindatetime`'s fast-path assertion). Reading `hour`…`second`
    // off such a value as a bag (the old fallthrough) called the getters.
    match temporal_value_ref_or_subclass(v) {
        Some(TemporalValue::PlainTime(t)) => return *t,
        Some(TemporalValue::PlainDateTime(dt)) => return dt.to_plain_time(),
        Some(TemporalValue::ZonedDateTime(z)) => return z.to_plain_time(),
        _ => {}
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        let s = dispatch::read_string(v);
        return ok_or_throw(s.parse::<PlainTime>());
    }
    if jv.is_pointer() {
        // A Symbol is POINTER-tagged but is NOT a property bag — `ToTemporalTime`
        // of a Symbol throws TypeError, not "read its `hour` field".
        if crate::symbol::is_registered_symbol(jv.as_pointer::<u8>() as usize) {
            crate::object::throw_object_type_error(
                b"Cannot convert a Symbol to a Temporal.PlainTime",
            );
        }
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let partial = super::options::partial_time(obj);
            // ToTemporalTime of a property bag with NO recognized time field
            // throws TypeError (PrepareTemporalFields requires ≥1 field).
            if partial.is_empty() {
                crate::object::throw_object_type_error(
                    b"object is not a valid Temporal.PlainTime property bag (no time fields)",
                );
            }
            // Apply the partial onto midnight under `overflow` so e.g.
            // `{ minute: 60 }` constrains to 59 (default) or rejects.
            let base = ok_or_throw(PlainTime::try_new(0, 0, 0, 0, 0, 0));
            return ok_or_throw(base.with(partial, Some(overflow)));
        }
    }
    // A non-string, non-object primitive (number / boolean / bigint / symbol /
    // null / undefined) → TypeError per ToTemporalTime (only a *malformed
    // string* yields RangeError, handled in the `is_string` branch above).
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.PlainTime")
}

pub fn from_static(args: &[f64]) -> f64 {
    // `ToTemporalTime(item, options)`. A string is parsed FIRST (invalid →
    // `RangeError`) and only then is `overflow` read (so a bad string throws
    // before a wrong-typed `options` would); a `PlainTime` / `PlainDateTime` /
    // `ZonedDateTime` takes its time slot directly (after reading `overflow`,
    // which validates `options`); a property bag reads its time fields, then
    // `overflow` LAST.
    let item = raw_arg(args, 0);
    let opts = raw_arg(args, 1);
    if JSValue::from_bits(item.to_bits()).is_string() {
        let t = ok_or_throw(dispatch::read_string(item).parse::<PlainTime>());
        let _ = super::options::overflow(opts);
        return wrap(t);
    }
    match temporal_value_ref_or_subclass(item) {
        Some(TemporalValue::PlainTime(t)) => {
            let _ = super::options::overflow(opts);
            return wrap(*t);
        }
        Some(TemporalValue::PlainDateTime(dt)) => {
            let _ = super::options::overflow(opts);
            return wrap(dt.to_plain_time());
        }
        Some(TemporalValue::ZonedDateTime(z)) => {
            let _ = super::options::overflow(opts);
            return wrap(z.to_plain_time());
        }
        Some(_) => crate::object::throw_object_type_error(
            b"Cannot convert this Temporal value to a Temporal.PlainTime",
        ),
        None => {}
    }
    let jv = JSValue::from_bits(item.to_bits());
    if jv.is_pointer() && !crate::symbol::is_registered_symbol(jv.as_pointer::<u8>() as usize) {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let partial = super::options::partial_time(obj);
            // `ToTemporalTime` of a bag with NO recognized time field → TypeError
            // (`PrepareTemporalFields` requires ≥1 field), BEFORE `overflow`.
            if partial.is_empty() {
                crate::object::throw_object_type_error(
                    b"object is not a valid Temporal.PlainTime property bag (no time fields)",
                );
            }
            let overflow =
                super::options::overflow(opts).unwrap_or(temporal_rs::options::Overflow::Constrain);
            let base = ok_or_throw(PlainTime::try_new(0, 0, 0, 0, 0, 0));
            return wrap(ok_or_throw(base.with(partial, Some(overflow))));
        }
    }
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.PlainTime")
}

pub fn compare_static(args: &[f64]) -> f64 {
    let a = coerce_time(raw_arg(args, 0));
    let b = coerce_time(raw_arg(args, 1));
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

pub fn get(t: &PlainTime, name: &str) -> Option<f64> {
    Some(match name {
        "hour" => t.hour() as f64,
        "minute" => t.minute() as f64,
        "second" => t.second() as f64,
        "millisecond" => t.millisecond() as f64,
        "microsecond" => t.microsecond() as f64,
        "nanosecond" => t.nanosecond() as f64,
        _ => return None,
    })
}

pub fn call(recv: f64, t: &PlainTime, name: &str, args: &[f64]) -> f64 {
    match name {
        "add" => wrap(ok_or_throw(
            t.add(&super::duration::coerce_duration(raw_arg(args, 0))),
        )),
        "subtract" => wrap(ok_or_throw(
            t.subtract(&super::duration::coerce_duration(raw_arg(args, 0))),
        )),
        "until" => super::duration::wrap(ok_or_throw(t.until(
            &coerce_time(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "since" => super::duration::wrap(ok_or_throw(t.since(
            &coerce_time(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "equals" => dispatch::boolean(*t == coerce_time(raw_arg(args, 0))),
        // `toString` honors `{ fractionalSecondDigits, smallestUnit, roundingMode }`;
        // `toJSON`/`toLocaleString` always use default (auto) precision.
        "toString" => string(&ok_or_throw(t.to_ixdtf_string(
            super::options::to_string_rounding_options(raw_arg(args, 0)),
        ))),
        "toJSON" => string(
            &t.to_ixdtf_string(ToStringRoundingOptions::default())
                .unwrap_or_default(),
        ),
        "toLocaleString" => {
            // Include the millisecond-of-second so `fractionalSecondDigits`
            // renders the real sub-second fraction, not `.000`
            // (mirrors PlainDateTime toLocaleString).
            let epoch_ms = crate::date::components_to_timestamp(
                1970,
                1,
                1,
                t.hour() as u32,
                t.minute() as u32,
                t.second() as u32,
            ) as f64
                * 1000.0
                + t.millisecond() as f64;
            crate::intl::temporal_locale_string(
                epoch_ms,
                raw_arg(args, 0),
                raw_arg(args, 1),
                crate::intl::TemporalLocaleCtx::PlainTime,
            )
        }
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let partial = super::options::with_partial_time(obj);
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(t.with(partial, overflow)))
        }
        "round" => wrap(ok_or_throw(
            t.round(super::options::rounding_options(raw_arg(args, 0))),
        )),
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
