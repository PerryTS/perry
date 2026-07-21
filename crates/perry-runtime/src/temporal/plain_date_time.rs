//! `Temporal.PlainDateTime` — wraps [`temporal_rs::PlainDateTime`] (#4693).
//!
//! Calendar date + wall-clock time, no timezone. Composes the PlainDate and
//! PlainTime field sets.

use super::dispatch::{
    self, boolean, field_u16, field_u8, ok_or_throw, raw_arg, string, undefined,
};
use super::{alloc_temporal_cell, temporal_value_ref_or_subclass, TemporalValue};
use crate::value::JSValue;
use temporal_rs::{Calendar, PlainDateTime};

const TYPE_NAME: &str = "Temporal.PlainDateTime";

fn wrap(dt: PlainDateTime) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainDateTime(dt))
}

fn calendar_arg(v: f64) -> Calendar {
    super::options::calendar_slot(v)
}

/// Strict calendar-identifier parse for the constructor's trailing argument
/// (an ISO string / annotation form is a RangeError, unlike `from`'s lenient
/// `calendar_slot`).
fn calendar_id_arg(v: f64) -> Calendar {
    super::options::calendar_identifier(v)
}

/// `new Temporal.PlainDateTime(year, month, day, hour?, …, nanosecond?, calendar?)`.
pub fn construct(args: &[f64]) -> f64 {
    dispatch::require_construct(TYPE_NAME);
    // Each field is `ToIntegerWithTruncation`: a non-finite (`Infinity`/`NaN` /
    // absent required field) is a RangeError, a Symbol/BigInt a TypeError — not
    // a silently-mangled `as i32`/`0`. `try_new` = overflow "reject" (out-of-range
    // fields throw); time fields saturate via `field_u8`/`field_u16` so a wrapping
    // cast can't slip e.g. `256` (hour) through as `0`.
    let year = dispatch::integer_with_truncation(raw_arg(args, 0));
    let month = dispatch::integer_with_truncation(raw_arg(args, 1));
    let day = dispatch::integer_with_truncation(raw_arg(args, 2));
    wrap(ok_or_throw(PlainDateTime::try_new(
        year as i32,
        field_u8(month),
        field_u8(day),
        field_u8(dispatch::optional_integer_with_truncation(raw_arg(args, 3))), // hour
        field_u8(dispatch::optional_integer_with_truncation(raw_arg(args, 4))), // minute
        field_u8(dispatch::optional_integer_with_truncation(raw_arg(args, 5))), // second
        field_u16(dispatch::optional_integer_with_truncation(raw_arg(args, 6))), // ms
        field_u16(dispatch::optional_integer_with_truncation(raw_arg(args, 7))), // us
        field_u16(dispatch::optional_integer_with_truncation(raw_arg(args, 8))), // ns
        calendar_id_arg(raw_arg(args, 9)),
    )))
}

fn coerce_dt(v: f64) -> PlainDateTime {
    coerce_dt_with_opts(v, undefined())
}

/// `ToTemporalDateTime(item, options)`. A `PlainDateTime` is cloned; a
/// `PlainDate` is widened to midnight; an ISO string is parsed; a property-bag
/// object is built via partial fields; anything else (number/boolean/null/symbol,
/// or a non-date Temporal value) is a `TypeError`. The `overflow` option in
/// `opts` is read *after* the item is processed (the spec / observable-overflow
/// tests: a primitive throws before options are touched, an invalid string
/// throws before options are read, and a bag reads its fields first). `opts` is
/// `undefined` for `compare`/`until`/`since` (no options arg).
fn coerce_dt_with_opts(v: f64, opts: f64) -> PlainDateTime {
    match temporal_value_ref_or_subclass(v) {
        Some(TemporalValue::PlainDateTime(dt)) => {
            let dt = dt.clone();
            let _ = super::options::overflow(opts);
            return dt;
        }
        Some(TemporalValue::PlainDate(d)) => {
            let dt = ok_or_throw(d.to_plain_date_time(None));
            let _ = super::options::overflow(opts);
            return dt;
        }
        Some(TemporalValue::ZonedDateTime(z)) => {
            let dt = z.to_plain_date_time();
            let _ = super::options::overflow(opts);
            return dt;
        }
        Some(_) => crate::object::throw_object_type_error(
            b"Cannot convert this Temporal value to a Temporal.PlainDateTime",
        ),
        None => {}
    }
    if JSValue::from_bits(v.to_bits()).is_string() {
        let dt = ok_or_throw(dispatch::read_string(v).parse::<PlainDateTime>());
        let _ = super::options::overflow(opts);
        return dt;
    }
    super::options::plain_date_time_from_bag(v, opts)
}

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_dt_with_opts(raw_arg(args, 0), raw_arg(args, 1)))
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
            // Spec reads the duration argument's fields BEFORE the options bag.
            let dur = super::duration::coerce_duration(raw_arg(args, 0));
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(dt.add(&dur, overflow)))
        }
        "subtract" => {
            let dur = super::duration::coerce_duration(raw_arg(args, 0));
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(dt.subtract(&dur, overflow)))
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
        "toString" => {
            let (rounding, calendar) = super::options::pdt_to_string_options(raw_arg(args, 0));
            string(&ok_or_throw(dt.to_ixdtf_string(rounding, calendar)))
        }
        "toJSON" => string(&dt.to_string()),
        "toLocaleString" => {
            super::options::assert_locale_string_calendar(
                dt.calendar().identifier(),
                raw_arg(args, 1),
            );
            // `components_to_timestamp` resolves only to whole seconds, so add
            // the millisecond-of-second back in — otherwise `toLocaleString`
            // with `fractionalSecondDigits` always renders `.000`
            // (test262 PlainDateTime `toLocaleString/lone-options-accepted`,
            // `{ fractionalSecondDigits: 3 }` expects `.321`).
            let epoch_ms = crate::date::components_to_timestamp(
                dt.year(),
                dt.month() as u32,
                dt.day() as u32,
                dt.hour() as u32,
                dt.minute() as u32,
                dt.second() as u32,
            ) as f64
                * 1000.0
                + dt.millisecond() as f64;
            crate::intl::temporal_locale_string(
                epoch_ms,
                raw_arg(args, 0),
                raw_arg(args, 1),
                crate::intl::TemporalLocaleCtx::PlainDateTime,
            )
        }
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let fields = super::options::with_datetime_fields(obj, dt.calendar());
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(dt.with(fields, overflow)))
        }
        "withPlainTime" => {
            // Combine this datetime's date with the provided time (or midnight).
            let time = super::options::optional_plain_time(raw_arg(args, 0));
            wrap(ok_or_throw(dt.to_plain_date().to_plain_date_time(time)))
        }
        "withCalendar" => {
            // `withCalendar` requires its argument: `ToTemporalCalendarSlotValue`
            // of `undefined` is a TypeError (the `missing-argument` case).
            if dispatch::is_undefined(raw_arg(args, 0)) {
                crate::object::throw_object_type_error(
                    b"withCalendar requires a calendar argument",
                );
            }
            wrap(dt.with_calendar(calendar_arg(raw_arg(args, 0))))
        }
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
