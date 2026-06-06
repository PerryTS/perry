//! `Temporal.PlainDate` — wraps [`temporal_rs::PlainDate`] (#4691).
//!
//! A calendar date with no time or timezone. Defaults to the ISO-8601 calendar;
//! a calendar id string selects another (`temporal_rs` owns the calendar math).

use super::dispatch::{self, boolean, ok_or_throw, raw_arg, string, undefined};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::DifferenceSettings;
use temporal_rs::{Calendar, PlainDate};

const TYPE_NAME: &str = "Temporal.PlainDate";

fn wrap(d: PlainDate) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainDate(d))
}

/// Resolve an optional calendar argument (a calendar-id string) to a
/// `Calendar`, defaulting to ISO-8601.
fn calendar_arg(v: f64) -> Calendar {
    if dispatch::is_undefined(v) {
        return Calendar::default();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        let s = dispatch::read_string(v);
        return ok_or_throw(s.parse::<Calendar>());
    }
    Calendar::default()
}

/// `new Temporal.PlainDate(year, month, day, calendar?)`.
pub fn construct(args: &[f64]) -> f64 {
    let year = dispatch::num_arg(args, 0);
    let month = dispatch::num_arg(args, 1);
    let day = dispatch::num_arg(args, 2);
    let cal = calendar_arg(raw_arg(args, 3));
    // `try_new` = overflow "reject": the constructor throws on out-of-range
    // fields (e.g. month 13) rather than silently constraining to 2021-12-01.
    wrap(ok_or_throw(PlainDate::try_new(
        year as i32,
        month as u8,
        day as u8,
        cal,
    )))
}

fn coerce_date(v: f64) -> PlainDate {
    if let Some(TemporalValue::PlainDate(d)) = temporal_value_ref(v) {
        return d.clone();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        let s = dispatch::read_string(v);
        return ok_or_throw(s.parse::<PlainDate>());
    }
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let f = |name: &str| -> f64 {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let raw = crate::object::js_object_get_field_by_name_f64(obj, key);
                JSValue::from_bits(raw.to_bits()).to_number()
            };
            let cal_key = crate::string::js_string_from_bytes(b"calendar".as_ptr(), 8);
            let cal_raw = crate::object::js_object_get_field_by_name_f64(obj, cal_key);
            return wrap_inner(f("year"), f("month"), f("day"), calendar_arg(cal_raw));
        }
    }
    crate::fs::validate::throw_range_error_with_code("Cannot convert value to a Temporal.PlainDate")
}

fn wrap_inner(year: f64, month: f64, day: f64, cal: Calendar) -> PlainDate {
    ok_or_throw(PlainDate::new(year as i32, month as u8, day as u8, cal))
}

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_date(raw_arg(args, 0)))
}

pub fn compare_static(args: &[f64]) -> f64 {
    let a = coerce_date(raw_arg(args, 0));
    let b = coerce_date(raw_arg(args, 1));
    match a.compare_iso(&b) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

pub fn get(d: &PlainDate, name: &str) -> Option<f64> {
    Some(match name {
        "year" => d.year() as f64,
        "month" => d.month() as f64,
        "day" => d.day() as f64,
        "dayOfWeek" => d.day_of_week() as f64,
        "dayOfYear" => d.day_of_year() as f64,
        "daysInWeek" => d.days_in_week() as f64,
        "daysInMonth" => d.days_in_month() as f64,
        "daysInYear" => d.days_in_year() as f64,
        "monthsInYear" => d.months_in_year() as f64,
        "weekOfYear" => match d.week_of_year() {
            Some(w) => w as f64,
            None => return Some(undefined()),
        },
        "inLeapYear" => boolean(d.in_leap_year()),
        "monthCode" => string(d.month_code().as_str()),
        "calendarId" => string(d.calendar().identifier()),
        "era" => match d.era() {
            Some(e) => string(e.as_str()),
            None => return Some(undefined()),
        },
        "eraYear" => match d.era_year() {
            Some(y) => y as f64,
            None => return Some(undefined()),
        },
        _ => return None,
    })
}

pub fn call(recv: f64, d: &PlainDate, name: &str, args: &[f64]) -> f64 {
    match name {
        "add" => wrap(ok_or_throw(
            d.add(&super::duration::coerce_duration(raw_arg(args, 0)), None),
        )),
        "subtract" => wrap(ok_or_throw(
            d.subtract(&super::duration::coerce_duration(raw_arg(args, 0)), None),
        )),
        "until" => super::duration::wrap(ok_or_throw(d.until(
            &coerce_date(raw_arg(args, 0)),
            DifferenceSettings::default(),
        ))),
        "since" => super::duration::wrap(ok_or_throw(d.since(
            &coerce_date(raw_arg(args, 0)),
            DifferenceSettings::default(),
        ))),
        "equals" => {
            let other = coerce_date(raw_arg(args, 0));
            dispatch::boolean(
                d.compare_iso(&other) == std::cmp::Ordering::Equal
                    && d.calendar().identifier() == other.calendar().identifier(),
            )
        }
        "toString" | "toJSON" | "toLocaleString" => string(&d.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" | "withCalendar" | "toPlainDateTime" | "toPlainYearMonth" | "toPlainMonthDay"
        | "toZonedDateTime" => crate::fs::validate::throw_range_error_with_code(
            "Temporal.PlainDate.prototype.with/withCalendar/toX is not yet implemented in Perry",
        ),
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
