//! `Temporal.PlainYearMonth` — wraps [`temporal_rs::PlainYearMonth`] (#4694).
//!
//! A calendar year + month (e.g. a billing period), no day/time/timezone.

use super::dispatch::{self, boolean, num_arg, ok_or_throw, raw_arg, string, undefined};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::Overflow;
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
    // A calendar must be `undefined` or a calendar-id string; null / number /
    // boolean / bigint / symbol / plain object → TypeError (ToTemporalCalendar).
    crate::object::throw_object_type_error(b"calendar must be a calendar identifier string")
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
    coerce_ym_overflow(v, Overflow::Constrain)
}

/// `ToTemporalYearMonth(item, overflow)` — from a `Temporal.PlainYearMonth`
/// (clone), an ISO string, or a `{ year, month | monthCode, calendar, … }`
/// property bag (via the calendar's `year_month_from_fields`, so `monthCode`
/// and out-of-range/overflow handling work). No recognized field, a Symbol, or
/// any non-string primitive → TypeError.
fn coerce_ym_overflow(v: f64, overflow: Overflow) -> PlainYearMonth {
    if let Some(TemporalValue::PlainYearMonth(ym)) = temporal_value_ref(v) {
        return ym.clone();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(dispatch::read_string(v).parse::<PlainYearMonth>());
    }
    if jv.is_pointer() {
        if crate::symbol::is_registered_symbol(jv.as_pointer::<u8>() as usize) {
            crate::object::throw_object_type_error(
                b"Cannot convert a Symbol to a Temporal.PlainYearMonth",
            );
        }
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            // A property bag with NONE of the recognized year-month calendar
            // fields → TypeError (ToTemporalYearMonth / PrepareTemporalFields).
            let has_field = |name: &str| -> bool {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                crate::object::js_object_get_field_by_name_f64(obj, key).to_bits()
                    != crate::value::TAG_UNDEFINED
            };
            if !["year", "month", "monthCode", "era", "eraYear"]
                .iter()
                .any(|n| has_field(n))
            {
                crate::object::throw_object_type_error(
                    b"object is not a valid Temporal.PlainYearMonth property bag",
                );
            }
            let cal_key = crate::string::js_string_from_bytes(b"calendar".as_ptr(), 8);
            let cal_raw = crate::object::js_object_get_field_by_name_f64(obj, cal_key);
            let partial = temporal_rs::partial::PartialYearMonth {
                calendar_fields: super::options::year_month_fields(obj),
                calendar: calendar_arg(cal_raw),
            };
            return ok_or_throw(PlainYearMonth::from_partial(partial, Some(overflow)));
        }
    }
    // Non-string, non-object primitive → TypeError per ToTemporalYearMonth.
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.PlainYearMonth")
}

pub fn from_static(args: &[f64]) -> f64 {
    // Overflow is consulted only on the property-bag path; a string item is
    // parsed first (an invalid string throws before the `overflow` getter is
    // observed). The options bag is type-checked for every item kind.
    let item = raw_arg(args, 0);
    let opts = raw_arg(args, 1);
    let is_bag = {
        let jv = JSValue::from_bits(item.to_bits());
        jv.is_pointer()
            && temporal_value_ref(item).is_none()
            && !crate::symbol::is_registered_symbol(jv.as_pointer::<u8>() as usize)
    };
    let overflow = if is_bag {
        super::options::overflow(opts).unwrap_or_default()
    } else {
        let _ = super::options::require_options_object(opts);
        Overflow::Constrain
    };
    wrap(coerce_ym_overflow(item, overflow))
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
            super::options::overflow(raw_arg(args, 1)).unwrap_or_default(),
        ))),
        "subtract" => wrap(ok_or_throw(ym.subtract(
            &super::duration::coerce_duration(raw_arg(args, 0)),
            super::options::overflow(raw_arg(args, 1)).unwrap_or_default(),
        ))),
        "until" => super::duration::wrap(ok_or_throw(ym.until(
            &coerce_ym(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "since" => super::duration::wrap(ok_or_throw(ym.since(
            &coerce_ym(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "equals" => {
            let other = coerce_ym(raw_arg(args, 0));
            dispatch::boolean(
                ym.compare_iso(&other) == std::cmp::Ordering::Equal
                    && ym.calendar_id() == other.calendar_id(),
            )
        }
        // `toString` honors `{ calendarName }`; `toJSON`/`toLocaleString` use the
        // default ("auto") calendar display.
        "toString" => {
            string(&ym.to_ixdtf_string(super::options::display_calendar(raw_arg(args, 0))))
        }
        "toJSON" | "toLocaleString" => string(&ym.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let fields = super::options::year_month_fields(obj);
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(ym.with(fields, overflow)))
        }
        "toPlainDate" => {
            let obj =
                super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "toPlainDate");
            let day = super::options::calendar_fields(obj);
            alloc_temporal_cell(TemporalValue::PlainDate(ok_or_throw(
                ym.to_plain_date(Some(day)),
            )))
        }
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
