//! `Temporal.PlainDateTime` — wraps [`temporal_rs::PlainDateTime`] (#4693).
//!
//! Calendar date + wall-clock time, no timezone. Composes the PlainDate and
//! PlainTime field sets.

use super::dispatch::{
    self, boolean, field_u16, field_u8, int_arg, num_arg, ok_or_throw, raw_arg, string, undefined,
};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::DifferenceSettings;
use temporal_rs::{Calendar, PlainDateTime};

const TYPE_NAME: &str = "Temporal.PlainDateTime";

fn wrap(dt: PlainDateTime) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainDateTime(dt))
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
    if let Some(TemporalValue::PlainDateTime(dt)) = temporal_value_ref(v) {
        return dt.clone();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(dispatch::read_string(v).parse::<PlainDateTime>());
    }
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let f = |name: &str| -> f64 {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let raw = crate::object::js_object_get_field_by_name_f64(obj, key);
                let n = JSValue::from_bits(raw.to_bits()).to_number();
                if n.is_finite() {
                    n
                } else {
                    0.0
                }
            };
            let cal_key = crate::string::js_string_from_bytes(b"calendar".as_ptr(), 8);
            let cal_raw = crate::object::js_object_get_field_by_name_f64(obj, cal_key);
            return ok_or_throw(PlainDateTime::new(
                f("year") as i32,
                f("month") as u8,
                f("day") as u8,
                f("hour") as u8,
                f("minute") as u8,
                f("second") as u8,
                f("millisecond") as u16,
                f("microsecond") as u16,
                f("nanosecond") as u16,
                calendar_arg(cal_raw),
            ));
        }
    }
    crate::fs::validate::throw_range_error_with_code(
        "Cannot convert value to a Temporal.PlainDateTime",
    )
}

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_dt(raw_arg(args, 0)))
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
        "daysInMonth" => dt.days_in_month() as f64,
        "daysInYear" => dt.days_in_year() as f64,
        "monthsInYear" => dt.months_in_year() as f64,
        "weekOfYear" => match dt.week_of_year() {
            Some(w) => w as f64,
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
        "add" => wrap(ok_or_throw(
            dt.add(&super::duration::coerce_duration(raw_arg(args, 0)), None),
        )),
        "subtract" => wrap(ok_or_throw(
            dt.subtract(&super::duration::coerce_duration(raw_arg(args, 0)), None),
        )),
        "until" => super::duration::wrap(ok_or_throw(
            dt.until(&coerce_dt(raw_arg(args, 0)), DifferenceSettings::default()),
        )),
        "since" => super::duration::wrap(ok_or_throw(
            dt.since(&coerce_dt(raw_arg(args, 0)), DifferenceSettings::default()),
        )),
        "equals" => {
            let other = coerce_dt(raw_arg(args, 0));
            dispatch::boolean(
                dt.compare_iso(&other) == std::cmp::Ordering::Equal
                    && dt.calendar().identifier() == other.calendar().identifier(),
            )
        }
        "toPlainDate" => alloc_temporal_cell(TemporalValue::PlainDate(dt.to_plain_date())),
        "toPlainTime" => alloc_temporal_cell(TemporalValue::PlainTime(dt.to_plain_time())),
        "toString" | "toJSON" | "toLocaleString" => string(&dt.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" | "withPlainTime" | "withCalendar" | "round" | "toZonedDateTime" => {
            crate::fs::validate::throw_range_error_with_code(
                "Temporal.PlainDateTime.prototype.with/round/toZonedDateTime is not yet \
                 implemented in Perry",
            )
        }
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
