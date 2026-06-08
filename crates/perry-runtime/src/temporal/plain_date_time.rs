//! `Temporal.PlainDateTime` — wraps [`temporal_rs::PlainDateTime`] (#4693).
//!
//! Calendar date + wall-clock time, no timezone. Composes the PlainDate and
//! PlainTime field sets.

use super::dispatch::{
    self, boolean, field_u16, field_u8, int_arg, num_arg, ok_or_throw, raw_arg, string, undefined,
};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::{Calendar, PlainDateTime};

const TYPE_NAME: &str = "Temporal.PlainDateTime";

fn wrap(dt: PlainDateTime) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainDateTime(dt))
}

fn calendar_arg(v: f64) -> Calendar {
    super::options::calendar_slot(v)
}

/// `new Temporal.PlainDateTime(year, month, day, hour?, …, nanosecond?, calendar?)`.
pub fn construct(args: &[f64]) -> f64 {
    // `try_new` = overflow "reject": throw on out-of-range fields instead of
    // constraining. Time fields saturate via `field_u8`/`field_u16` so a wrapping
    // `as u8` cast can't slip e.g. `256` (hour) through as `0`.
    wrap(ok_or_throw(PlainDateTime::try_new(
        num_arg(args, 0) as i32,     // year
        num_arg(args, 1) as u8,      // month
        num_arg(args, 2) as u8,      // day
        field_u8(int_arg(args, 3)),  // hour
        field_u8(int_arg(args, 4)),  // minute
        field_u8(int_arg(args, 5)),  // second
        field_u16(int_arg(args, 6)), // ms
        field_u16(int_arg(args, 7)), // us
        field_u16(int_arg(args, 8)), // ns
        calendar_arg(raw_arg(args, 9)),
    )))
}

fn coerce_dt(v: f64) -> PlainDateTime {
    coerce_dt_with_overflow(v, None)
}

/// `ToTemporalDateTime(item, overflow)`. A `PlainDateTime` is cloned; a
/// `PlainDate` is widened to midnight; an ISO string is parsed; a property-bag
/// object is built via partial fields under `overflow`; anything else
/// (number/boolean/null/symbol, or a non-date Temporal value) is a `TypeError`.
fn coerce_dt_with_overflow(
    v: f64,
    overflow: Option<temporal_rs::options::Overflow>,
) -> PlainDateTime {
    match temporal_value_ref(v) {
        Some(TemporalValue::PlainDateTime(dt)) => return dt.clone(),
        Some(TemporalValue::PlainDate(d)) => return ok_or_throw(d.to_plain_date_time(None)),
        Some(TemporalValue::ZonedDateTime(z)) => return z.to_plain_date_time(),
        Some(_) => crate::object::throw_object_type_error(
            b"Cannot convert this Temporal value to a Temporal.PlainDateTime",
        ),
        None => {}
    }
    if JSValue::from_bits(v.to_bits()).is_string() {
        return ok_or_throw(dispatch::read_string(v).parse::<PlainDateTime>());
    }
    super::options::plain_date_time_from_bag(v, overflow)
}

pub fn from_static(args: &[f64]) -> f64 {
    let overflow = super::options::overflow(raw_arg(args, 1));
    wrap(coerce_dt_with_overflow(raw_arg(args, 0), overflow))
}

pub fn compare_static(args: &[f64]) -> f64 {
    match coerce_dt(raw_arg(args, 0)).compare_iso(&coerce_dt(raw_arg(args, 1))) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

pub fn get(dt: &PlainDateTime, name: &str) -> Option<f64> {
    Some(match name {
        // date set
        "year" => dt.year() as f64,
        "month" => dt.month() as f64,
        "day" => dt.day() as f64,
        "dayOfWeek" => dt.day_of_week() as f64,
        "dayOfYear" => dt.day_of_year() as f64,
        "daysInWeek" => dt.days_in_week() as f64,
        "daysInMonth" => dt.days_in_month() as f64,
        "daysInYear" => dt.days_in_year() as f64,
        "monthsInYear" => dt.months_in_year() as f64,
        "weekOfYear" => match dt.week_of_year() {
            Some(w) => w as f64,
            None => return Some(undefined()),
        },
        "yearOfWeek" => match dt.year_of_week() {
            Some(y) => y as f64,
            None => return Some(undefined()),
        },
        "inLeapYear" => boolean(dt.in_leap_year()),
        "monthCode" => string(dt.month_code().as_str()),
        "calendarId" => string(dt.calendar().identifier()),
        "era" => match dt.era() {
            Some(e) => string(e.as_str()),
            None => return Some(undefined()),
        },
        "eraYear" => match dt.era_year() {
            Some(y) => y as f64,
            None => return Some(undefined()),
        },
        // time set
        "hour" => dt.hour() as f64,
        "minute" => dt.minute() as f64,
        "second" => dt.second() as f64,
        "millisecond" => dt.millisecond() as f64,
        "microsecond" => dt.microsecond() as f64,
        "nanosecond" => dt.nanosecond() as f64,
        _ => return None,
    })
}

pub fn call(recv: f64, dt: &PlainDateTime, name: &str, args: &[f64]) -> f64 {
    match name {
        "add" => {
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(dt.add(
                &super::duration::coerce_duration(raw_arg(args, 0)),
                overflow,
            )))
        }
        "subtract" => {
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(dt.subtract(
                &super::duration::coerce_duration(raw_arg(args, 0)),
                overflow,
            )))
        }
        "until" => super::duration::wrap(ok_or_throw(dt.until(
            &coerce_dt(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "since" => super::duration::wrap(ok_or_throw(dt.since(
            &coerce_dt(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "equals" => {
            let other = coerce_dt(raw_arg(args, 0));
            dispatch::boolean(
                dt.compare_iso(&other) == std::cmp::Ordering::Equal
                    && dt.calendar().identifier() == other.calendar().identifier(),
            )
        }
        "toPlainDate" => alloc_temporal_cell(TemporalValue::PlainDate(dt.to_plain_date())),
        "toPlainTime" => alloc_temporal_cell(TemporalValue::PlainTime(dt.to_plain_time())),
        "toString" => string(&ok_or_throw(dt.to_ixdtf_string(
            super::options::to_string_rounding_options(raw_arg(args, 0)),
            super::options::display_calendar(raw_arg(args, 0)),
        ))),
        "toJSON" | "toLocaleString" => string(&dt.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let fields = super::options::datetime_fields(obj);
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(dt.with(fields, overflow)))
        }
        "withPlainTime" => {
            // Combine this datetime's date with the provided time (or midnight).
            let time = super::options::optional_plain_time(raw_arg(args, 0));
            wrap(ok_or_throw(dt.to_plain_date().to_plain_date_time(time)))
        }
        "withCalendar" => wrap(dt.with_calendar(calendar_arg(raw_arg(args, 0)))),
        "round" => wrap(ok_or_throw(
            dt.round(super::options::rounding_options(raw_arg(args, 0))),
        )),
        "toZonedDateTime" => {
            let tz = super::options::timezone(raw_arg(args, 0));
            let disambiguation = super::options::disambiguation(raw_arg(args, 1))
                .unwrap_or(temporal_rs::options::Disambiguation::Compatible);
            alloc_temporal_cell(TemporalValue::ZonedDateTime(ok_or_throw(
                dt.to_zoned_date_time(tz, disambiguation),
            )))
        }
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
