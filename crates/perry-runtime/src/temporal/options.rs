//! Shared marshalling of Temporal **options** and **fields** objects (#4727).
//!
//! The options-/fields-object-heavy methods — `round`/`total`,
//! `with`/`withCalendar`/`withPlainTime`/`withTimeZone`, and the calendar
//! conversions — all need to turn a plain JS object (or, for `round`/`total`, a
//! bare string shorthand) into the corresponding `temporal_rs` argument type.
//! That marshalling lives here so every per-type module shares one
//! implementation instead of re-deriving it.

use super::dispatch::{field_u16, field_u8, is_undefined, ok_or_throw, read_string};
use crate::value::JSValue;
use core::str::FromStr;
use temporal_rs::fields::{
    CalendarFields, DateTimeFields, YearMonthCalendarFields, ZonedDateTimeFields,
};
use temporal_rs::options::{
    Disambiguation, OffsetDisambiguation, Overflow, RelativeTo, RoundingIncrement, RoundingMode,
    RoundingOptions, Unit,
};
use temporal_rs::partial::PartialTime;
use temporal_rs::provider::TransitionDirection;
use temporal_rs::{MonthCode, PlainTime, TimeZone, TinyAsciiStr};

// ---- low-level JS object field reads --------------------------------------

/// Borrow `v` as a plain-object pointer, or `None` if it isn't one. A Temporal
/// cell is *also* a NaN-boxed pointer, so callers that may receive a Temporal
/// value must brand-check it first (see [`require_fields_obj`]).
fn as_obj(v: f64) -> Option<*const crate::object::ObjectHeader> {
    let jv = JSValue::from_bits(v.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let obj = jv.as_pointer::<crate::object::ObjectHeader>();
    if obj.is_null() {
        None
    } else {
        Some(obj)
    }
}

/// Raw (NaN-boxed) value of `obj.<name>`.
fn field(obj: *const crate::object::ObjectHeader, name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(obj, key)
}

/// `obj.<name>` as a finite number, or `None` if absent / undefined / non-finite.
fn num_field(obj: *const crate::object::ObjectHeader, name: &str) -> Option<f64> {
    let raw = field(obj, name);
    if is_undefined(raw) {
        return None;
    }
    let n = JSValue::from_bits(raw.to_bits()).to_number();
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

/// `obj.<name>` as a string, or `None` if absent / undefined / not a string.
fn str_field(obj: *const crate::object::ObjectHeader, name: &str) -> Option<String> {
    let raw = field(obj, name);
    if is_undefined(raw) || !JSValue::from_bits(raw.to_bits()).is_string() {
        return None;
    }
    Some(read_string(raw))
}

#[inline]
fn range(msg: &str) -> ! {
    crate::fs::validate::throw_range_error_with_code(msg)
}

#[inline]
fn type_error(msg: String) -> ! {
    crate::object::throw_object_type_error(msg.as_bytes())
}

// ---- enum-from-string parsers ---------------------------------------------

fn parse_unit(s: &str) -> Unit {
    Unit::from_str(s).unwrap_or_else(|_| range("Invalid Temporal unit"))
}

fn parse_rounding_mode(s: &str) -> RoundingMode {
    RoundingMode::from_str(s).unwrap_or_else(|_| range("Invalid roundingMode"))
}

fn parse_overflow(s: &str) -> Overflow {
    Overflow::from_str(s).unwrap_or_else(|_| range("Invalid overflow option"))
}

fn parse_disambiguation(s: &str) -> Disambiguation {
    Disambiguation::from_str(s).unwrap_or_else(|_| range("Invalid disambiguation option"))
}

fn parse_offset_option(s: &str) -> OffsetDisambiguation {
    OffsetDisambiguation::from_str(s).unwrap_or_else(|_| range("Invalid offset option"))
}

fn parse_increment(n: f64) -> RoundingIncrement {
    // GetRoundingIncrementOption: ToIntegerWithTruncation, then range-validate.
    let i = n.trunc();
    if !(1.0..=1_000_000_000.0).contains(&i) {
        range("roundingIncrement must be between 1 and 1e9");
    }
    ok_or_throw(RoundingIncrement::try_new(i as u32))
}

fn parse_month_code(s: &str) -> MonthCode {
    ok_or_throw(MonthCode::try_from_utf8(s.as_bytes()))
}

// ---- rounding / total -----------------------------------------------------

/// Marshal a `round(roundTo)` argument — a `smallestUnit` string shorthand or
/// an options object — into [`RoundingOptions`].
///
/// `largest_unit` starts *unset* (not `Auto`, unlike `RoundingOptions::default`)
/// so `Duration.round`'s "smallestUnit and largestUnit both unset" check fires
/// correctly; the other types' `round` resolvers ignore `largest_unit`.
pub fn rounding_options(arg: f64) -> RoundingOptions {
    let mut o = RoundingOptions::default();
    o.largest_unit = None;
    o.smallest_unit = None;
    o.rounding_mode = None;
    o.increment = None;

    if JSValue::from_bits(arg.to_bits()).is_string() {
        o.smallest_unit = Some(parse_unit(&read_string(arg)));
        return o;
    }
    match as_obj(arg) {
        Some(obj) => {
            if let Some(s) = str_field(obj, "smallestUnit") {
                o.smallest_unit = Some(parse_unit(&s));
            }
            if let Some(s) = str_field(obj, "largestUnit") {
                o.largest_unit = Some(parse_unit(&s));
            }
            if let Some(s) = str_field(obj, "roundingMode") {
                o.rounding_mode = Some(parse_rounding_mode(&s));
            }
            if let Some(n) = num_field(obj, "roundingIncrement") {
                o.increment = Some(parse_increment(n));
            }
        }
        None => range("round requires a unit string or an options object"),
    }
    o
}

/// Marshal a `total(totalOf)` argument — a `unit` string or a
/// `{ unit, relativeTo }` object — into the (required) unit and optional
/// `relativeTo`.
pub fn total_options(arg: f64) -> (Unit, Option<RelativeTo>) {
    if JSValue::from_bits(arg.to_bits()).is_string() {
        return (parse_unit(&read_string(arg)), None);
    }
    match as_obj(arg) {
        Some(obj) => {
            let unit = match str_field(obj, "unit") {
                Some(s) => parse_unit(&s),
                None => range("total requires a unit"),
            };
            (unit, relative_to_field(obj))
        }
        None => range("total requires a unit string or an options object"),
    }
}

/// Read an optional `relativeTo` from a `round`/`total` options object.
pub fn relative_to(arg: f64) -> Option<RelativeTo> {
    relative_to_field(as_obj(arg)?)
}

fn relative_to_field(obj: *const crate::object::ObjectHeader) -> Option<RelativeTo> {
    let v = field(obj, "relativeTo");
    if is_undefined(v) {
        return None;
    }
    // A Temporal PlainDate / PlainDateTime / ZonedDateTime value.
    if let Some(tv) = super::temporal_value_ref(v) {
        return Some(match tv {
            super::TemporalValue::ZonedDateTime(z) => RelativeTo::ZonedDateTime(z.clone()),
            super::TemporalValue::PlainDate(d) => RelativeTo::PlainDate(d.clone()),
            super::TemporalValue::PlainDateTime(dt) => RelativeTo::PlainDate(dt.to_plain_date()),
            _ => range("relativeTo must be a PlainDate or ZonedDateTime"),
        });
    }
    // A string (ZonedDateTime form, falling back to PlainDate).
    if JSValue::from_bits(v.to_bits()).is_string() {
        return Some(ok_or_throw(RelativeTo::try_from_str(&read_string(v))));
    }
    // A plain fields object → build a PlainDate from its calendar fields.
    if let Some(o) = as_obj(v) {
        let partial = temporal_rs::partial::PartialDate {
            calendar_fields: calendar_fields(o),
            calendar: temporal_rs::Calendar::default(),
        };
        return Some(RelativeTo::PlainDate(ok_or_throw(
            temporal_rs::PlainDate::from_partial(partial, None),
        )));
    }
    range("relativeTo must be a PlainDate, ZonedDateTime, string, or fields object")
}

// ---- overflow / disambiguation (second-arg option objects) ----------------

/// Read an optional `overflow` (`"constrain"` | `"reject"`) from an options arg.
pub fn overflow(arg: f64) -> Option<Overflow> {
    let obj = as_obj(arg)?;
    str_field(obj, "overflow").map(|s| parse_overflow(&s))
}

/// Read an optional `disambiguation` from an options arg.
pub fn disambiguation(arg: f64) -> Option<Disambiguation> {
    let obj = as_obj(arg)?;
    str_field(obj, "disambiguation").map(|s| parse_disambiguation(&s))
}

/// Read an optional `offset` (offset-disambiguation) from an options arg.
pub fn offset_option(arg: f64) -> Option<OffsetDisambiguation> {
    let obj = as_obj(arg)?;
    str_field(obj, "offset").map(|s| parse_offset_option(&s))
}

// ---- fields objects (`with` partials) -------------------------------------

/// Require `arg` to be a plain object suitable as a `with(...)` partial-fields
/// bag, throwing a `TypeError` for a non-object or a Temporal value (which is
/// never a valid fields bag per spec).
pub fn require_fields_obj(
    arg: f64,
    type_name: &str,
    method: &str,
) -> *const crate::object::ObjectHeader {
    if super::temporal_value_ref(arg).is_some() {
        type_error(format!(
            "{type_name}.prototype.{method} expects a plain fields object, not a Temporal value"
        ));
    }
    match as_obj(arg) {
        Some(o) => o,
        None => type_error(format!(
            "{type_name}.prototype.{method} requires an object argument"
        )),
    }
}

/// Populate a [`PartialTime`] from an object's `hour…nanosecond` fields.
pub fn partial_time(obj: *const crate::object::ObjectHeader) -> PartialTime {
    let mut t = PartialTime::new();
    if let Some(n) = num_field(obj, "hour") {
        t.hour = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "minute") {
        t.minute = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "second") {
        t.second = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "millisecond") {
        t.millisecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "microsecond") {
        t.microsecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "nanosecond") {
        t.nanosecond = Some(field_u16(n.trunc() as i64));
    }
    t
}

/// Populate a [`CalendarFields`] from an object's calendar fields
/// (`year`/`month`/`monthCode`/`day`/`era`/`eraYear`).
pub fn calendar_fields(obj: *const crate::object::ObjectHeader) -> CalendarFields {
    let mut f = CalendarFields::new();
    if let Some(n) = num_field(obj, "year") {
        f.year = Some(n.trunc() as i32);
    }
    if let Some(n) = num_field(obj, "month") {
        f.month = Some(field_u8(n.trunc() as i64));
    }
    if let Some(s) = str_field(obj, "monthCode") {
        f.month_code = Some(parse_month_code(&s));
    }
    if let Some(n) = num_field(obj, "day") {
        f.day = Some(field_u8(n.trunc() as i64));
    }
    if let Some(s) = str_field(obj, "era") {
        f.era = TinyAsciiStr::<19>::try_from_utf8(s.as_bytes()).ok();
    }
    if let Some(n) = num_field(obj, "eraYear") {
        f.era_year = Some(n.trunc() as i32);
    }
    f
}

/// Populate a [`YearMonthCalendarFields`] from an object's year/month fields.
pub fn year_month_fields(obj: *const crate::object::ObjectHeader) -> YearMonthCalendarFields {
    let mut f = YearMonthCalendarFields::new();
    if let Some(n) = num_field(obj, "year") {
        f.year = Some(n.trunc() as i32);
    }
    if let Some(n) = num_field(obj, "month") {
        f.month = Some(field_u8(n.trunc() as i64));
    }
    if let Some(s) = str_field(obj, "monthCode") {
        f.month_code = Some(parse_month_code(&s));
    }
    if let Some(s) = str_field(obj, "era") {
        f.era = TinyAsciiStr::<19>::try_from_utf8(s.as_bytes()).ok();
    }
    if let Some(n) = num_field(obj, "eraYear") {
        f.era_year = Some(n.trunc() as i32);
    }
    f
}

/// Populate a [`DateTimeFields`] (calendar fields + time) for `PlainDateTime.with`.
pub fn datetime_fields(obj: *const crate::object::ObjectHeader) -> DateTimeFields {
    let mut f = DateTimeFields::new();
    f.calendar_fields = calendar_fields(obj);
    f.time = partial_time(obj);
    f
}

/// Populate a [`ZonedDateTimeFields`] (calendar fields + time) for
/// `ZonedDateTime.with`. The `offset` partial is left unset (rarely used in
/// `.with` and would require UTC-offset parsing).
pub fn zoned_fields(obj: *const crate::object::ObjectHeader) -> ZonedDateTimeFields {
    let mut f = ZonedDateTimeFields::new();
    f.calendar_fields = calendar_fields(obj);
    f.time = partial_time(obj);
    f
}

// ---- conversion helpers ---------------------------------------------------

/// Midnight (`00:00:00`), used as the default time for date→datetime/zdt
/// conversions when no `plainTime` is supplied.
fn midnight() -> PlainTime {
    ok_or_throw(PlainTime::try_new(0, 0, 0, 0, 0, 0))
}

/// Resolve an optional `plainTime`-like argument to a [`PlainTime`]:
/// `undefined` → `None` (caller defaults to midnight), a `Temporal.PlainTime`,
/// an ISO time string, or a `{ hour, … }` partial-time object.
pub fn optional_plain_time(v: f64) -> Option<PlainTime> {
    if is_undefined(v) {
        return None;
    }
    if let Some(super::TemporalValue::PlainTime(t)) = super::temporal_value_ref(v) {
        return Some(*t);
    }
    if JSValue::from_bits(v.to_bits()).is_string() {
        return Some(ok_or_throw(read_string(v).parse::<PlainTime>()));
    }
    if let Some(o) = as_obj(v) {
        let pt = partial_time(o);
        if pt.is_empty() {
            return Some(midnight());
        }
        return Some(ok_or_throw(midnight().with(pt, Some(Overflow::Constrain))));
    }
    None
}

/// Resolve a time-zone argument — a tz-identifier string or a
/// `Temporal.ZonedDateTime` whose zone is reused.
pub fn timezone(v: f64) -> TimeZone {
    if JSValue::from_bits(v.to_bits()).is_string() {
        return ok_or_throw(TimeZone::try_from_str(&read_string(v)));
    }
    if let Some(super::TemporalValue::ZonedDateTime(z)) = super::temporal_value_ref(v) {
        return *z.time_zone();
    }
    range("expected a time-zone identifier string");
}

/// Parse a `getTimeZoneTransition` direction argument — a `"next"`/`"previous"`
/// string or a `{ direction }` object.
pub fn transition_direction(v: f64) -> TransitionDirection {
    let s = if JSValue::from_bits(v.to_bits()).is_string() {
        read_string(v)
    } else if let Some(obj) = as_obj(v) {
        match str_field(obj, "direction") {
            Some(s) => s,
            None => range("getTimeZoneTransition requires a direction"),
        }
    } else {
        range("getTimeZoneTransition requires a direction string or object");
    };
    TransitionDirection::from_str(&s).unwrap_or_else(|_| range("Invalid transition direction"))
}
