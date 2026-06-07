//! `Temporal.PlainTime` — wraps [`temporal_rs::PlainTime`] (#4692).
//!
//! Wall-clock time with no date or timezone. No calendar, so the plainest of
//! the plain types.

use super::dispatch::{self, field_u16, field_u8, int_arg, ok_or_throw, raw_arg, string};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::{DifferenceSettings, ToStringRoundingOptions};
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
    if let Some(TemporalValue::PlainTime(t)) = temporal_value_ref(v) {
        return *t;
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        let s = dispatch::read_string(v);
        return ok_or_throw(s.parse::<PlainTime>());
    }
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let f = |name: &str| -> i64 {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let raw = crate::object::js_object_get_field_by_name_f64(obj, key);
                let n = JSValue::from_bits(raw.to_bits()).to_number();
                if n.is_finite() {
                    n.trunc() as i64
                } else {
                    0
                }
            };
            return ok_or_throw(PlainTime::try_new(
                field_u8(f("hour")),
                field_u8(f("minute")),
                field_u8(f("second")),
                field_u16(f("millisecond")),
                field_u16(f("microsecond")),
                field_u16(f("nanosecond")),
            ));
        }
    }
    crate::fs::validate::throw_range_error_with_code("Cannot convert value to a Temporal.PlainTime")
}

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_time(raw_arg(args, 0)))
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
            DifferenceSettings::default(),
        ))),
        "since" => super::duration::wrap(ok_or_throw(t.since(
            &coerce_time(raw_arg(args, 0)),
            DifferenceSettings::default(),
        ))),
        "equals" => dispatch::boolean(*t == coerce_time(raw_arg(args, 0))),
        "toString" | "toJSON" | "toLocaleString" => string(
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
