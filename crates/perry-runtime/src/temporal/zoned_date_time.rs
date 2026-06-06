//! `Temporal.ZonedDateTime` — wraps [`temporal_rs::ZonedDateTime`] (#4695).
//!
//! A timezone-aware exact moment: the heaviest Temporal type. Backed by the
//! IANA tz database vendored through `temporal_rs`'s `compiled_data` feature,
//! so the provider-free convenience methods resolve offsets/DST internally.

use super::dispatch::{
    self, bigint_from_i128, boolean, ok_or_throw, raw_arg, read_bigint_i128, string, undefined,
};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::{DifferenceSettings, Disambiguation, OffsetDisambiguation};
use temporal_rs::{Calendar, TimeZone, ZonedDateTime};

const TYPE_NAME: &str = "Temporal.ZonedDateTime";

fn wrap(z: ZonedDateTime) -> f64 {
    alloc_temporal_cell(TemporalValue::ZonedDateTime(z))
}

fn require_ns(v: f64) -> i128 {
    match read_bigint_i128(v) {
        Some(n) => n,
        None => crate::fs::validate::throw_range_error_with_code(
            "Temporal.ZonedDateTime requires a BigInt epoch-nanoseconds value",
        ),
    }
}

fn timezone_arg(v: f64) -> TimeZone {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(TimeZone::try_from_str(&dispatch::read_string(v)));
    }
    crate::fs::validate::throw_range_error_with_code(
        "Temporal.ZonedDateTime requires a time-zone identifier string",
    )
}

fn calendar_arg(v: f64) -> Calendar {
    if dispatch::is_undefined(v) {
        return Calendar::default();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(dispatch::read_string(v).parse::<Calendar>());
    }
    Calendar::default()
}

/// `new Temporal.ZonedDateTime(epochNanoseconds: bigint, timeZone, calendar?)`.
pub fn construct(args: &[f64]) -> f64 {
    let ns = require_ns(raw_arg(args, 0));
    let tz = timezone_arg(raw_arg(args, 1));
    let cal = calendar_arg(raw_arg(args, 2));
    wrap(ok_or_throw(ZonedDateTime::try_new(ns, tz, cal)))
}

fn coerce_zdt(v: f64) -> ZonedDateTime {
    if let Some(TemporalValue::ZonedDateTime(z)) = temporal_value_ref(v) {
        return z.clone();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(ZonedDateTime::from_utf8(
            dispatch::read_string(v).as_bytes(),
            Disambiguation::default(),
            OffsetDisambiguation::Reject,
        ));
    }
    crate::fs::validate::throw_range_error_with_code(
        "Cannot convert value to a Temporal.ZonedDateTime",
    )
}

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_zdt(raw_arg(args, 0)))
}

pub fn compare_static(args: &[f64]) -> f64 {
    // Exact-time comparison via epoch nanoseconds (the spec's ZonedDateTime
    // compare orders by the underlying instant).
    let a = coerce_zdt(raw_arg(args, 0)).to_instant().as_i128();
    let b = coerce_zdt(raw_arg(args, 1)).to_instant().as_i128();
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

pub fn get(z: &ZonedDateTime, name: &str) -> Option<f64> {
    Some(match name {
        "year" => z.year() as f64,
        "month" => z.month() as f64,
        "day" => z.day() as f64,
        "hour" => z.hour() as f64,
        "minute" => z.minute() as f64,
        "second" => z.second() as f64,
        "millisecond" => z.millisecond() as f64,
        "microsecond" => z.microsecond() as f64,
        "nanosecond" => z.nanosecond() as f64,
        "dayOfWeek" => z.day_of_week() as f64,
        "epochMilliseconds" => z.epoch_milliseconds() as f64,
        "epochNanoseconds" => bigint_from_i128(z.to_instant().as_i128()),
        "offsetNanoseconds" => z.offset_nanoseconds() as f64,
        "offset" => string(&z.offset()),
        "timeZoneId" => match z.time_zone().identifier() {
            Ok(id) => string(&id),
            Err(_) => return Some(undefined()),
        },
        "calendarId" => string(z.calendar().identifier()),
        "hoursInDay" => ok_or_throw(z.hours_in_day()) as f64,
        _ => return None,
    })
}

pub fn call(recv: f64, z: &ZonedDateTime, name: &str, args: &[f64]) -> f64 {
    match name {
        "add" => wrap(ok_or_throw(
            z.add(&super::duration::coerce_duration(raw_arg(args, 0)), None),
        )),
        "subtract" => wrap(ok_or_throw(
            z.subtract(&super::duration::coerce_duration(raw_arg(args, 0)), None),
        )),
        "until" => super::duration::wrap(ok_or_throw(
            z.until(&coerce_zdt(raw_arg(args, 0)), DifferenceSettings::default()),
        )),
        "since" => super::duration::wrap(ok_or_throw(
            z.since(&coerce_zdt(raw_arg(args, 0)), DifferenceSettings::default()),
        )),
        "equals" => boolean(ok_or_throw(z.equals(&coerce_zdt(raw_arg(args, 0))))),
        "toInstant" => alloc_temporal_cell(TemporalValue::Instant(z.to_instant())),
        "toPlainDate" => alloc_temporal_cell(TemporalValue::PlainDate(z.to_plain_date())),
        "toPlainTime" => alloc_temporal_cell(TemporalValue::PlainTime(z.to_plain_time())),
        "toPlainDateTime" => {
            alloc_temporal_cell(TemporalValue::PlainDateTime(z.to_plain_date_time()))
        }
        "toString" | "toJSON" | "toLocaleString" => string(&z.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with"
        | "withPlainTime"
        | "withTimeZone"
        | "withCalendar"
        | "round"
        | "getTimeZoneTransition"
        | "startOfDay" => crate::fs::validate::throw_range_error_with_code(
            "Temporal.ZonedDateTime.prototype.with*/round/getTimeZoneTransition/startOfDay is not \
             yet implemented in Perry",
        ),
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
