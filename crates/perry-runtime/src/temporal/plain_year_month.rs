//! `Temporal.PlainYearMonth` — wraps [`temporal_rs::PlainYearMonth`] (#4694).
//!
//! A calendar year + month (e.g. a billing period), no day/time/timezone.

use super::dispatch::{self, boolean, num_arg, ok_or_throw, raw_arg, string, undefined};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::{DifferenceSettings, Overflow};
use temporal_rs::{Calendar, PlainYearMonth};

const TYPE_NAME: &str = "Temporal.PlainYearMonth";

fn wrap(ym: PlainYearMonth) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainYearMonth(ym))
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

/// `new Temporal.PlainYearMonth(year, month, calendar?, referenceDay?)`.
pub fn construct(args: &[f64]) -> f64 {
    let ref_day = {
        let d = num_arg(args, 3);
        if d.is_finite() {
            Some(d as u8)
        } else {
            None
        }
    };
    // `try_new` = overflow "reject": throw on an out-of-range month (e.g. 13)
    // rather than constraining it to December.
    wrap(ok_or_throw(PlainYearMonth::try_new(
        num_arg(args, 0) as i32,
        num_arg(args, 1) as u8,
        ref_day,
        calendar_arg(raw_arg(args, 2)),
    )))
}

fn coerce_ym(v: f64) -> PlainYearMonth {
    if let Some(TemporalValue::PlainYearMonth(ym)) = temporal_value_ref(v) {
        return ym.clone();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(dispatch::read_string(v).parse::<PlainYearMonth>());
    }
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let f = |name: &str| -> f64 {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                JSValue::from_bits(
                    crate::object::js_object_get_field_by_name_f64(obj, key).to_bits(),
                )
                .to_number()
            };
            let cal_key = crate::string::js_string_from_bytes(b"calendar".as_ptr(), 8);
            let cal_raw = crate::object::js_object_get_field_by_name_f64(obj, cal_key);
            return ok_or_throw(PlainYearMonth::new(
                f("year") as i32,
                f("month") as u8,
                None,
                calendar_arg(cal_raw),
            ));
        }
    }
    crate::fs::validate::throw_range_error_with_code(
        "Cannot convert value to a Temporal.PlainYearMonth",
    )
}

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_ym(raw_arg(args, 0)))
}

pub fn compare_static(args: &[f64]) -> f64 {
    match coerce_ym(raw_arg(args, 0)).compare_iso(&coerce_ym(raw_arg(args, 1))) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

pub fn get(ym: &PlainYearMonth, name: &str) -> Option<f64> {
    Some(match name {
        "year" => ym.year() as f64,
        "month" => ym.month() as f64,
        "daysInMonth" => ym.days_in_month() as f64,
        "daysInYear" => ym.days_in_year() as f64,
        "monthsInYear" => ym.months_in_year() as f64,
        "inLeapYear" => boolean(ym.in_leap_year()),
        "monthCode" => string(ym.month_code().as_str()),
        "calendarId" => string(ym.calendar_id()),
        "era" => match ym.era() {
            Some(e) => string(e.as_str()),
            None => return Some(undefined()),
        },
        "eraYear" => match ym.era_year() {
            Some(y) => y as f64,
            None => return Some(undefined()),
        },
        _ => return None,
    })
}

pub fn call(recv: f64, ym: &PlainYearMonth, name: &str, args: &[f64]) -> f64 {
    match name {
        "add" => wrap(ok_or_throw(ym.add(
            &super::duration::coerce_duration(raw_arg(args, 0)),
            Overflow::default(),
        ))),
        "subtract" => wrap(ok_or_throw(ym.subtract(
            &super::duration::coerce_duration(raw_arg(args, 0)),
            Overflow::default(),
        ))),
        "until" => super::duration::wrap(ok_or_throw(
            ym.until(&coerce_ym(raw_arg(args, 0)), DifferenceSettings::default()),
        )),
        "since" => super::duration::wrap(ok_or_throw(
            ym.since(&coerce_ym(raw_arg(args, 0)), DifferenceSettings::default()),
        )),
        "equals" => {
            let other = coerce_ym(raw_arg(args, 0));
            dispatch::boolean(
                ym.compare_iso(&other) == std::cmp::Ordering::Equal
                    && ym.calendar_id() == other.calendar_id(),
            )
        }
        "toString" | "toJSON" | "toLocaleString" => string(&ym.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" | "toPlainDate" => crate::fs::validate::throw_range_error_with_code(
            "Temporal.PlainYearMonth.prototype.with/toPlainDate is not yet implemented in Perry",
        ),
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
