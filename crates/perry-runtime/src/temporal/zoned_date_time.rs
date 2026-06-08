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
use temporal_rs::options::{Disambiguation, OffsetDisambiguation};
use temporal_rs::{Calendar, TimeZone, ZonedDateTime};

const TYPE_NAME: &str = "Temporal.ZonedDateTime";

fn wrap(z: ZonedDateTime) -> f64 {
    alloc_temporal_cell(TemporalValue::ZonedDateTime(z))
}

/// `ToBigInt(epochNanoseconds)` for the constructor's first argument: a BigInt
/// passes through, a boolean coerces to `0n`/`1n`, a string parses; a Number /
/// `undefined` / `null` / Symbol all throw `TypeError` (ToBigInt never accepts a
/// Number, even an integer one).
fn require_ns(v: f64) -> i128 {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_bigint() {
        return read_bigint_i128(v).unwrap_or_else(|| {
            crate::fs::validate::throw_range_error_with_code("Invalid epoch-nanoseconds BigInt")
        });
    }
    match v.to_bits() {
        b if b == crate::value::TAG_TRUE => return 1,
        b if b == crate::value::TAG_FALSE => return 0,
        _ => {}
    }
    if jv.is_string() {
        return dispatch::read_string(v)
            .trim()
            .parse::<i128>()
            .unwrap_or_else(|_| {
                crate::fs::validate::throw_range_error_with_code(
                    "Cannot convert string to a BigInt epoch-nanoseconds value",
                )
            });
    }
    crate::object::throw_object_type_error(
        b"Temporal.ZonedDateTime epochNanoseconds must be a BigInt",
    )
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
    coerce_zdt_with_options(v, undefined())
}

/// `ToTemporalZonedDateTime(item, options)` — a `Temporal.ZonedDateTime` (cloned),
/// an IXDTF string, or a property bag with a `timeZone` (built via
/// `from_partial`). `opts` supplies `overflow`/`disambiguation`/`offset` (only
/// consulted for the string + property-bag forms).
fn coerce_zdt_with_options(v: f64, opts: f64) -> ZonedDateTime {
    if let Some(TemporalValue::ZonedDateTime(z)) = temporal_value_ref(v) {
        return z.clone();
    }
    if let Some(partial) = super::options::zoned_partial(v) {
        return ok_or_throw(ZonedDateTime::from_partial(
            partial,
            super::options::overflow(opts),
            super::options::disambiguation(opts),
            super::options::offset_option(opts),
        ));
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(ZonedDateTime::from_utf8(
            dispatch::read_string(v).as_bytes(),
            super::options::disambiguation(opts).unwrap_or(Disambiguation::Compatible),
            super::options::offset_option(opts).unwrap_or(OffsetDisambiguation::Reject),
        ));
    }
    crate::fs::validate::throw_range_error_with_code(
        "Cannot convert value to a Temporal.ZonedDateTime",
    )
}

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_zdt_with_options(raw_arg(args, 0), raw_arg(args, 1)))
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
        "dayOfYear" => z.day_of_year() as f64,
        "daysInWeek" => z.days_in_week() as f64,
        "daysInMonth" => z.days_in_month() as f64,
        "daysInYear" => z.days_in_year() as f64,
        "monthsInYear" => z.months_in_year() as f64,
        "inLeapYear" => boolean(z.in_leap_year()),
        "monthCode" => string(z.month_code().as_str()),
        "weekOfYear" => match z.week_of_year() {
            Some(w) => w as f64,
            None => return Some(undefined()),
        },
        "yearOfWeek" => match z.year_of_week() {
            Some(y) => y as f64,
            None => return Some(undefined()),
        },
        "era" => match z.era() {
            Some(e) => string(e.as_str()),
            None => return Some(undefined()),
        },
        "eraYear" => match z.era_year() {
            Some(y) => y as f64,
            None => return Some(undefined()),
        },
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
        "add" => wrap(ok_or_throw(z.add(
            &super::duration::coerce_duration(raw_arg(args, 0)),
            super::options::overflow(raw_arg(args, 1)),
        ))),
        "subtract" => wrap(ok_or_throw(z.subtract(
            &super::duration::coerce_duration(raw_arg(args, 0)),
            super::options::overflow(raw_arg(args, 1)),
        ))),
        "until" => super::duration::wrap(ok_or_throw(z.until(
            &coerce_zdt(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "since" => super::duration::wrap(ok_or_throw(z.since(
            &coerce_zdt(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "equals" => boolean(ok_or_throw(z.equals(&coerce_zdt(raw_arg(args, 0))))),
        "toInstant" => alloc_temporal_cell(TemporalValue::Instant(z.to_instant())),
        "toPlainDate" => alloc_temporal_cell(TemporalValue::PlainDate(z.to_plain_date())),
        "toPlainTime" => alloc_temporal_cell(TemporalValue::PlainTime(z.to_plain_time())),
        "toPlainDateTime" => {
            alloc_temporal_cell(TemporalValue::PlainDateTime(z.to_plain_date_time()))
        }
        "toString" => string(&ok_or_throw(z.to_ixdtf_string(
            super::options::display_offset(raw_arg(args, 0)),
            super::options::display_time_zone(raw_arg(args, 0)),
            super::options::display_calendar(raw_arg(args, 0)),
            super::options::to_string_rounding_options(raw_arg(args, 0)),
        ))),
        "toJSON" | "toLocaleString" => string(&z.to_string()),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let fields = super::options::zoned_fields(obj);
            let opts = raw_arg(args, 1);
            wrap(ok_or_throw(z.with(
                fields,
                super::options::disambiguation(opts),
                super::options::offset_option(opts),
                super::options::overflow(opts),
            )))
        }
        "withPlainTime" => {
            let time = super::options::optional_plain_time(raw_arg(args, 0));
            wrap(ok_or_throw(z.with_plain_time(time)))
        }
        "withTimeZone" => {
            let tz = super::options::timezone(raw_arg(args, 0));
            wrap(ok_or_throw(z.with_timezone(tz)))
        }
        "withCalendar" => wrap(z.with_calendar(calendar_arg(raw_arg(args, 0)))),
        "round" => wrap(ok_or_throw(
            z.round(super::options::rounding_options(raw_arg(args, 0))),
        )),
        "startOfDay" => wrap(ok_or_throw(z.start_of_day())),
        "getTimeZoneTransition" => {
            let dir = super::options::transition_direction(raw_arg(args, 0));
            match ok_or_throw(z.get_time_zone_transition(dir)) {
                Some(next) => wrap(next),
                None => f64::from_bits(crate::value::TAG_NULL),
            }
        }
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
