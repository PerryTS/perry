//! `Temporal.PlainTime` — wraps [`temporal_rs::PlainTime`] (#4692).
//!
//! Wall-clock time with no date or timezone. No calendar, so the plainest of
//! the plain types.

use super::dispatch::{self, field_u16, field_u8, int_arg, ok_or_throw, raw_arg, string};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::ToStringRoundingOptions;
use temporal_rs::PlainTime;

const TYPE_NAME: &str = "Temporal.PlainTime";

fn wrap(t: PlainTime) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainTime(t))
}

/// `new Temporal.PlainTime(hour?, minute?, second?, ms?, µs?, ns?)`. Out-of-range
/// fields saturate via `field_u8`/`field_u16` so `try_new` rejects them (an `as
/// u8` cast on the raw i64 would *wrap* `256` back to `0` and accept it).
pub fn construct(args: &[f64]) -> f64 {
    wrap(ok_or_throw(PlainTime::try_new(
        field_u8(int_arg(args, 0)),
        field_u8(int_arg(args, 1)),
        field_u8(int_arg(args, 2)),
        field_u16(int_arg(args, 3)),
        field_u16(int_arg(args, 4)),
        field_u16(int_arg(args, 5)),
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
    if let Some(TemporalValue::PlainTime(t)) = temporal_value_ref(v) {
        return *t;
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
    // `from(item, options)` — `GetTemporalOverflowOption` is consulted only on
    // the property-bag path; a string item is parsed first (so an invalid string
    // throws RangeError before the `overflow` getter is observed). The options
    // bag is still type-checked for every item kind.
    let item = raw_arg(args, 0);
    let opts = raw_arg(args, 1);
    let overflow = if is_time_property_bag(item) {
        super::options::overflow(opts).unwrap_or_default()
    } else {
        let _ = super::options::require_options_object(opts);
        temporal_rs::options::Overflow::Constrain
    };
    wrap(coerce_time_overflow(item, overflow))
}

/// True if `v` is a plain object (not a Temporal value, not a Symbol) — i.e. a
/// property bag that ToTemporalTime would read fields from (and thus read the
/// `overflow` option for).
fn is_time_property_bag(v: f64) -> bool {
    let jv = JSValue::from_bits(v.to_bits());
    if !jv.is_pointer() || temporal_value_ref(v).is_some() {
        return false;
    }
    !crate::symbol::is_registered_symbol(jv.as_pointer::<u8>() as usize)
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
        "toJSON" | "toLocaleString" => string(
            &t.to_ixdtf_string(ToStringRoundingOptions::default())
                .unwrap_or_default(),
        ),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let partial = super::options::partial_time(obj);
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
