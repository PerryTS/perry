//! `Temporal.PlainDate` — wraps [`temporal_rs::PlainDate`] (#4691).
//!
//! A calendar date with no time or timezone. Defaults to the ISO-8601 calendar;
//! a calendar id string selects another (`temporal_rs` owns the calendar math).

use super::dispatch::{self, boolean, ok_or_throw, raw_arg, string, undefined};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::{Calendar, PlainDate, PlainTime, TimeZone};

const TYPE_NAME: &str = "Temporal.PlainDate";

fn wrap(d: PlainDate) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainDate(d))
}

/// Resolve an optional calendar argument (a calendar-id string) to a
/// `Calendar`, defaulting to ISO-8601. A non-string, non-undefined calendar
/// (null / number / boolean / symbol) is a `TypeError` per
/// `ToTemporalCalendarSlotValue` — the `calendar-wrong-type` cases.
fn calendar_arg(v: f64) -> Calendar {
    super::options::calendar_slot(v)
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
    coerce_date_with_overflow(v, None)
}

/// `ToTemporalDate(item, overflow)`. A `PlainDate` is cloned; a `PlainDateTime`
/// yields its date; an ISO string is parsed; a property-bag object is built via
/// partial fields under `overflow`; anything else (number/boolean/null/symbol,
/// or a non-date Temporal value) is a `TypeError`.
fn coerce_date_with_overflow(
    v: f64,
    overflow: Option<temporal_rs::options::Overflow>,
) -> PlainDate {
    match temporal_value_ref(v) {
        Some(TemporalValue::PlainDate(d)) => return d.clone(),
        Some(TemporalValue::PlainDateTime(dt)) => return dt.to_plain_date(),
        Some(TemporalValue::ZonedDateTime(z)) => return z.to_plain_date(),
        Some(_) => crate::object::throw_object_type_error(
            b"Cannot convert this Temporal value to a Temporal.PlainDate",
        ),
        None => {}
    }
    if JSValue::from_bits(v.to_bits()).is_string() {
        return ok_or_throw(dispatch::read_string(v).parse::<PlainDate>());
    }
    super::options::plain_date_from_bag(v, overflow)
}

pub fn from_static(args: &[f64]) -> f64 {
    // `from(item, options)`: a wrong-typed options arg is a TypeError, thrown
    // before the item is converted (GetOptionsObject). `overflow` also drives
    // constrain/reject for a property-bag / string item.
    let overflow = super::options::overflow(raw_arg(args, 1));
    wrap(coerce_date_with_overflow(raw_arg(args, 0), overflow))
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
        "yearOfWeek" => match d.year_of_week() {
            Some(y) => y as f64,
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

/// Parse the `toZonedDateTime` argument: either a bare time-zone identifier
/// string or an options object `{ timeZone, plainTime }`.
fn to_zoned_args(v: f64) -> (TimeZone, Option<PlainTime>) {
    if JSValue::from_bits(v.to_bits()).is_string() {
        return (super::options::timezone(v), None);
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let tz_key = crate::string::js_string_from_bytes(b"timeZone".as_ptr(), 8);
            let tz_raw = crate::object::js_object_get_field_by_name_f64(obj, tz_key);
            let tz = super::options::timezone(tz_raw);
            let pt_key = crate::string::js_string_from_bytes(b"plainTime".as_ptr(), 9);
            let pt_raw = crate::object::js_object_get_field_by_name_f64(obj, pt_key);
            return (tz, super::options::optional_plain_time(pt_raw));
        }
    }
    crate::fs::validate::throw_range_error_with_code(
        "Temporal.PlainDate.prototype.toZonedDateTime requires a time zone",
    )
}

pub fn call(recv: f64, d: &PlainDate, name: &str, args: &[f64]) -> f64 {
    match name {
        "add" => {
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(d.add(
                &super::duration::coerce_duration(raw_arg(args, 0)),
                overflow,
            )))
        }
        "subtract" => {
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(d.subtract(
                &super::duration::coerce_duration(raw_arg(args, 0)),
                overflow,
            )))
        }
        "until" => super::duration::wrap(ok_or_throw(d.until(
            &coerce_date(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "since" => super::duration::wrap(ok_or_throw(d.since(
            &coerce_date(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "equals" => {
            let other = coerce_date(raw_arg(args, 0));
            dispatch::boolean(
                d.compare_iso(&other) == std::cmp::Ordering::Equal
                    && d.calendar().identifier() == other.calendar().identifier(),
            )
        }
        "toString" => {
            string(&d.to_ixdtf_string(super::options::display_calendar(raw_arg(args, 0))))
        }
        "toJSON" | "toLocaleString" => string(&d.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let fields = super::options::calendar_fields(obj);
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(d.with(fields, overflow)))
        }
        "withCalendar" => wrap(d.with_calendar(calendar_arg(raw_arg(args, 0)))),
        "toPlainDateTime" => {
            let time = super::options::optional_plain_time(raw_arg(args, 0));
            alloc_temporal_cell(TemporalValue::PlainDateTime(ok_or_throw(
                d.to_plain_date_time(time),
            )))
        }
        "toPlainYearMonth" => alloc_temporal_cell(TemporalValue::PlainYearMonth(ok_or_throw(
            d.to_plain_year_month(),
        ))),
        "toPlainMonthDay" => alloc_temporal_cell(TemporalValue::PlainMonthDay(ok_or_throw(
            d.to_plain_month_day(),
        ))),
        "toZonedDateTime" => {
            let (tz, time) = to_zoned_args(raw_arg(args, 0));
            alloc_temporal_cell(TemporalValue::ZonedDateTime(ok_or_throw(
                d.to_zoned_date_time(tz, time),
            )))
        }
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
