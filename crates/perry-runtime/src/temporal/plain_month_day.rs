//! `Temporal.PlainMonthDay` — wraps [`temporal_rs::PlainMonthDay`] (#4694).
//!
//! A calendar month + day with no year (e.g. a recurring birthday/holiday).

use super::dispatch::{self, num_arg, ok_or_throw, raw_arg, string};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::Overflow;
use temporal_rs::{Calendar, PlainMonthDay};

const TYPE_NAME: &str = "Temporal.PlainMonthDay";

fn wrap(md: PlainMonthDay) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainMonthDay(md))
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

/// `new Temporal.PlainMonthDay(month, day, calendar?, referenceYear?)`.
pub fn construct(args: &[f64]) -> f64 {
    let ref_year = {
        let y = num_arg(args, 3);
        if y.is_finite() {
            Some(y as i32)
        } else {
            None
        }
    };
    // Overflow "reject": the constructor throws on an invalid month/day (e.g.
    // Feb 30) instead of constraining it to Feb 29. The `.from()` fields path
    // (`coerce_md`) keeps the spec's "constrain" default.
    wrap(ok_or_throw(PlainMonthDay::new_with_overflow(
        num_arg(args, 0) as u8,
        num_arg(args, 1) as u8,
        calendar_arg(raw_arg(args, 2)),
        Overflow::Reject,
        ref_year,
    )))
}

fn coerce_md(v: f64) -> PlainMonthDay {
    if let Some(TemporalValue::PlainMonthDay(md)) = temporal_value_ref(v) {
        return md.clone();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(dispatch::read_string(v).parse::<PlainMonthDay>());
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
            return ok_or_throw(PlainMonthDay::new_with_overflow(
                f("month") as u8,
                f("day") as u8,
                calendar_arg(cal_raw),
                Overflow::default(),
                None,
            ));
        }
    }
    crate::fs::validate::throw_range_error_with_code(
        "Cannot convert value to a Temporal.PlainMonthDay",
    )
}

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_md(raw_arg(args, 0)))
}

pub fn get(md: &PlainMonthDay, name: &str) -> Option<f64> {
    Some(match name {
        "day" => md.day() as f64,
        "monthCode" => string(md.month_code().as_str()),
        "calendarId" => string(md.calendar_id()),
        _ => return None,
    })
}

pub fn call(recv: f64, md: &PlainMonthDay, name: &str, args: &[f64]) -> f64 {
    match name {
        "equals" => {
            let other = coerce_md(raw_arg(args, 0));
            dispatch::boolean(
                md.day() == other.day()
                    && md.month_code() == other.month_code()
                    && md.calendar_id() == other.calendar_id(),
            )
        }
        "toString" | "toJSON" | "toLocaleString" => string(&md.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" | "toPlainDate" => crate::fs::validate::throw_range_error_with_code(
            "Temporal.PlainMonthDay.prototype.with/toPlainDate is not yet implemented in Perry",
        ),
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
