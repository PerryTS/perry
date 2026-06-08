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
    DifferenceSettings, Disambiguation, DisplayCalendar, DisplayOffset, DisplayTimeZone,
    OffsetDisambiguation, Overflow, RelativeTo, RoundingIncrement, RoundingMode, RoundingOptions,
    ToStringRoundingOptions, Unit,
};
use temporal_rs::parsers::Precision;
use temporal_rs::partial::{PartialTime, PartialZonedDateTime};
use temporal_rs::provider::TransitionDirection;
use temporal_rs::{Calendar, MonthCode, PlainTime, TimeZone, TinyAsciiStr, UtcOffset};

// ---- low-level JS object field reads --------------------------------------

/// Borrow `v` as a plain-object pointer, or `None` if it isn't one. A Temporal
/// cell is *also* a NaN-boxed pointer, so callers that may receive a Temporal
/// value must brand-check it first (see [`require_fields_obj`]).
fn as_obj(v: f64) -> Option<*const crate::object::ObjectHeader> {
    let jv = JSValue::from_bits(v.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    // A Symbol is NaN-boxed with POINTER_TAG but is a primitive, never a valid
    // options/fields object — reject it so callers don't deref it as one.
    if unsafe { crate::symbol::js_is_symbol(v) } != 0 {
        return None;
    }
    let obj = jv.as_pointer::<crate::object::ObjectHeader>();
    if obj.is_null() {
        None
    } else {
        Some(obj)
    }
}

/// Spec `GetOptionsObject(options)`: `undefined` → `None` (use defaults), an
/// object → `Some`, and any other value (null, boolean, number, string, bigint,
/// symbol) → `TypeError`. Used by the methods whose options argument is an
/// object-only bag (no string shorthand): `until`/`since`/`toString`/`compare`.
pub fn require_options_object(arg: f64) -> Option<*const crate::object::ObjectHeader> {
    if is_undefined(arg) {
        return None;
    }
    match as_obj(arg) {
        Some(o) => Some(o),
        None => type_error("options must be an object or undefined".to_string()),
    }
}

/// Build a [`temporal_rs::PlainDate`] from a property-bag object
/// (`ToTemporalDate` step for an Object that isn't already a Temporal value):
/// read its calendar fields + `calendar` slot into a `PartialDate` and let
/// `temporal_rs` validate/construct under the given `overflow`. A non-object
/// (number/boolean/null/symbol) is a `TypeError`, matching the spec's
/// "Numbers cannot be used in place of an ISO string" wrong-type cases.
pub fn plain_date_from_bag(v: f64, overflow: Option<Overflow>) -> temporal_rs::PlainDate {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainDate".to_string()),
    };
    let partial = temporal_rs::partial::PartialDate {
        calendar_fields: calendar_fields(obj),
        calendar: calendar_slot(field(obj, "calendar")),
    };
    ok_or_throw(temporal_rs::PlainDate::from_partial(partial, overflow))
}

/// Build a [`temporal_rs::PlainDateTime`] from a property-bag object
/// (`ToTemporalDateTime` for a non-Temporal Object). Mirrors
/// [`plain_date_from_bag`] with date + time fields.
pub fn plain_date_time_from_bag(v: f64, overflow: Option<Overflow>) -> temporal_rs::PlainDateTime {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainDateTime".to_string()),
    };
    let partial = temporal_rs::partial::PartialDateTime {
        fields: datetime_fields(obj),
        calendar: calendar_slot(field(obj, "calendar")),
    };
    ok_or_throw(temporal_rs::PlainDateTime::from_partial(partial, overflow))
}

/// `ToTemporalCalendarSlotValue`: resolve a calendar argument to a
/// [`temporal_rs::Calendar`]. `undefined` → ISO-8601; a calendar-id string →
/// that calendar; a `Temporal.*` value → its own `[[Calendar]]`. Anything else
/// (null / number / boolean / symbol / plain object) is a `TypeError` — the
/// `calendar-wrong-type` cases.
pub fn calendar_slot(v: f64) -> temporal_rs::Calendar {
    use temporal_rs::Calendar;
    if is_undefined(v) {
        return Calendar::default();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(read_string(v).parse::<Calendar>());
    }
    if let Some(tv) = super::temporal_value_ref(v) {
        return match tv {
            super::TemporalValue::PlainDate(d) => d.calendar().clone(),
            super::TemporalValue::PlainDateTime(dt) => dt.calendar().clone(),
            super::TemporalValue::PlainYearMonth(ym) => ym.calendar().clone(),
            super::TemporalValue::PlainMonthDay(md) => md.calendar().clone(),
            super::TemporalValue::ZonedDateTime(z) => z.calendar().clone(),
            _ => type_error("Temporal value has no calendar".to_string()),
        };
    }
    type_error("calendar must be a calendar identifier string or a Temporal object".to_string())
}

/// `GetOptionsObject`: an options argument must be `undefined` or an Object.
/// Any other value — number, string, boolean, bigint, **symbol** — is a
/// `TypeError`. Methods that take an options bag call this up front so a
/// wrong-typed options arg throws before any work (every
/// `*/options-wrong-type.js`). A Temporal value counts as an object here
/// (its calendar fields just won't match any option name).
pub fn validate_options_arg(arg: f64) {
    if is_undefined(arg) {
        return;
    }
    if as_obj(arg).is_none() {
        type_error("options argument must be an object or undefined".to_string());
    }
}

/// Raw (NaN-boxed) value of `obj.<name>`.
fn field(obj: *const crate::object::ObjectHeader, name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(obj, key)
}

/// `obj.<name>` as a finite number, or `None` if absent / `undefined`. A present
/// field whose `ToNumber` is non-finite (`Infinity`/`-Infinity`/`NaN`) is a
/// `RangeError` per `ToIntegerWithTruncation` — Temporal numeric fields reject
/// non-finite input (e.g. `{ year: Infinity }`), they do not silently drop it.
fn num_field(obj: *const crate::object::ObjectHeader, name: &str) -> Option<f64> {
    let raw = field(obj, name);
    if is_undefined(raw) {
        return None;
    }
    let n = JSValue::from_bits(raw.to_bits()).to_number();
    if n.is_finite() {
        Some(n)
    } else {
        range("Temporal field cannot be Infinity or NaN");
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

/// `obj.<name>` as a **ToString-coerced** string option (spec `GetOption` with
/// `type: string`). `undefined` → `None` (use the default); a Symbol throws
/// `TypeError`; everything else (number, boolean, bigint, null, object) is
/// coerced via abstract `ToString` (so an object's `toString` runs, a number
/// becomes its decimal string, etc.). The coerced string then flows into the
/// enum parser, which throws `RangeError` for an unrecognized value — matching
/// `checkStringOptionWrongType`.
fn str_field_coerce(obj: *const crate::object::ObjectHeader, name: &str) -> Option<String> {
    let raw = field(obj, name);
    if is_undefined(raw) {
        return None;
    }
    let jv = JSValue::from_bits(raw.to_bits());
    if jv.is_string() {
        return Some(read_string(raw));
    }
    if unsafe { crate::symbol::js_is_symbol(raw) } != 0 {
        type_error("Cannot convert a Symbol value to a string".to_string());
    }
    let sh = crate::value::js_jsvalue_to_string_coerce(raw);
    Some(read_string(crate::value::js_nanbox_string(sh as i64)))
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
            if let Some(s) = str_field_coerce(obj, "smallestUnit") {
                o.smallest_unit = Some(parse_unit(&s));
            }
            if let Some(s) = str_field_coerce(obj, "largestUnit") {
                o.largest_unit = Some(parse_unit(&s));
            }
            if let Some(s) = str_field_coerce(obj, "roundingMode") {
                o.rounding_mode = Some(parse_rounding_mode(&s));
            }
            if let Some(n) = num_field(obj, "roundingIncrement") {
                o.increment = Some(parse_increment(n));
            }
        }
        // GetOptionsObject: a non-string, non-object `roundTo` (number, boolean,
        // bigint, null, undefined, symbol) is a TypeError.
        None => type_error("round requires a unit string or an options object".to_string()),
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
            let unit = match str_field_coerce(obj, "unit") {
                Some(s) => parse_unit(&s),
                None => range("total requires a unit"),
            };
            (unit, relative_to_field(obj))
        }
        None => type_error("total requires a unit string or an options object".to_string()),
    }
}

/// Marshal a `toString(options)` argument into [`ToStringRoundingOptions`] —
/// the `fractionalSecondDigits` / `smallestUnit` / `roundingMode` trio shared by
/// every Temporal type's `toString`. A missing/`undefined` arg yields the
/// spec-default (auto precision, no smallest-unit override).
pub fn to_string_rounding_options(arg: f64) -> ToStringRoundingOptions {
    let mut o = ToStringRoundingOptions::default();
    let obj = match require_options_object(arg) {
        Some(o) => o,
        None => return o, // undefined → defaults; a primitive throws TypeError
    };
    if let Some(s) = str_field_coerce(obj, "smallestUnit") {
        o.smallest_unit = Some(parse_unit(&s));
    }
    if let Some(s) = str_field_coerce(obj, "roundingMode") {
        o.rounding_mode = Some(parse_rounding_mode(&s));
    }
    // GetTemporalFractionalSecondDigitsOption: "auto" or an integer 0..=9.
    let raw = field(obj, "fractionalSecondDigits");
    if !is_undefined(raw) {
        let jv = JSValue::from_bits(raw.to_bits());
        if jv.is_string() {
            if read_string(raw) != "auto" {
                range("fractionalSecondDigits must be \"auto\" or an integer 0-9");
            }
            o.precision = Precision::Auto;
        } else {
            let n = jv.to_number();
            if !n.is_finite() {
                range("fractionalSecondDigits must be \"auto\" or an integer 0-9");
            }
            let d = n.floor();
            if !(0.0..=9.0).contains(&d) {
                range("fractionalSecondDigits must be between 0 and 9");
            }
            o.precision = Precision::Digit(d as u8);
        }
    }
    o
}

/// Marshal an `until`/`since` options argument into [`DifferenceSettings`] —
/// the `largestUnit` / `smallestUnit` / `roundingMode` / `roundingIncrement`
/// quartet. An `undefined` / absent arg yields the default (auto units).
pub fn difference_settings(arg: f64) -> DifferenceSettings {
    let mut s = DifferenceSettings::default();
    if let Some(obj) = require_options_object(arg) {
        if let Some(u) = str_field_coerce(obj, "largestUnit") {
            s.largest_unit = Some(parse_unit(&u));
        }
        if let Some(u) = str_field_coerce(obj, "smallestUnit") {
            s.smallest_unit = Some(parse_unit(&u));
        }
        if let Some(m) = str_field_coerce(obj, "roundingMode") {
            s.rounding_mode = Some(parse_rounding_mode(&m));
        }
        if let Some(n) = num_field(obj, "roundingIncrement") {
            s.increment = Some(parse_increment(n));
        }
    }
    s
}

/// Parse the `calendarName` display option (`"auto"|"always"|"never"|"critical"`)
/// from a `toString` options bag. Absent → `Auto`; an invalid string → RangeError.
pub fn display_calendar(arg: f64) -> DisplayCalendar {
    match require_options_object(arg).and_then(|obj| str_field_coerce(obj, "calendarName")) {
        Some(s) => {
            DisplayCalendar::from_str(&s).unwrap_or_else(|_| range("Invalid calendarName option"))
        }
        None => DisplayCalendar::Auto,
    }
}

/// Parse the `offset` display option (`"auto"|"never"`) from a `toString` bag.
pub fn display_offset(arg: f64) -> DisplayOffset {
    match require_options_object(arg).and_then(|obj| str_field_coerce(obj, "offset")) {
        Some(s) => DisplayOffset::from_str(&s).unwrap_or_else(|_| range("Invalid offset option")),
        None => DisplayOffset::Auto,
    }
}

/// Parse the `timeZoneName` display option (`"auto"|"never"|"critical"`).
pub fn display_time_zone(arg: f64) -> DisplayTimeZone {
    match require_options_object(arg).and_then(|obj| str_field_coerce(obj, "timeZoneName")) {
        Some(s) => {
            DisplayTimeZone::from_str(&s).unwrap_or_else(|_| range("Invalid timeZoneName option"))
        }
        None => DisplayTimeZone::Auto,
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
/// A non-object, non-undefined options arg is a `TypeError` (`GetOptionsObject`).
pub fn overflow(arg: f64) -> Option<Overflow> {
    validate_options_arg(arg);
    let obj = as_obj(arg)?;
    str_field_coerce(obj, "overflow").map(|s| parse_overflow(&s))
}

/// Read an optional `disambiguation` from an options arg.
pub fn disambiguation(arg: f64) -> Option<Disambiguation> {
    let obj = as_obj(arg)?;
    str_field_coerce(obj, "disambiguation").map(|s| parse_disambiguation(&s))
}

/// Read an optional `offset` (offset-disambiguation) from an options arg.
pub fn offset_option(arg: f64) -> Option<OffsetDisambiguation> {
    let obj = as_obj(arg)?;
    str_field_coerce(obj, "offset").map(|s| parse_offset_option(&s))
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

/// Populate a [`ZonedDateTimeFields`] (calendar fields + time + offset) for
/// `ZonedDateTime.with`.
pub fn zoned_fields(obj: *const crate::object::ObjectHeader) -> ZonedDateTimeFields {
    let mut f = ZonedDateTimeFields::new();
    f.calendar_fields = calendar_fields(obj);
    f.time = partial_time(obj);
    f.offset = offset_field(obj);
    f
}

/// Read an optional `offset` string field → [`UtcOffset`] (a `RangeError` for a
/// malformed string).
fn offset_field(obj: *const crate::object::ObjectHeader) -> Option<UtcOffset> {
    str_field(obj, "offset").map(|s| ok_or_throw(UtcOffset::from_utf8(s.as_bytes())))
}

/// Resolve the `timeZone` field of a property bag to a [`TimeZone`] — a
/// tz-identifier string or another `Temporal.ZonedDateTime` (whose zone is
/// reused). A missing `timeZone` is a `TypeError` (it is a required field of a
/// `ZonedDateTime` property bag).
fn timezone_field(obj: *const crate::object::ObjectHeader) -> TimeZone {
    let raw = field(obj, "timeZone");
    if is_undefined(raw) {
        type_error(
            "ZonedDateTime property bag is missing a required \"timeZone\" field".to_string(),
        );
    }
    timezone(raw)
}

/// Build a [`PartialZonedDateTime`] from a JS property bag for
/// `Temporal.ZonedDateTime.from`. Returns `None` if `v` is not a plain object.
pub fn zoned_partial(v: f64) -> Option<PartialZonedDateTime> {
    let obj = as_obj(v)?;
    let calendar = match str_field(obj, "calendar") {
        Some(s) => ok_or_throw(s.parse::<Calendar>()),
        None => Calendar::default(),
    };
    let mut p = PartialZonedDateTime::new();
    p.calendar = calendar;
    p.fields.calendar_fields = calendar_fields(obj);
    p.fields.time = partial_time(obj);
    p.fields.offset = offset_field(obj);
    p.timezone = Some(timezone_field(obj));
    Some(p)
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

/// Read an optional `timeZone` from `Temporal.Instant.prototype.toString`'s
/// options bag — when present the instant is rendered with that zone's offset
/// (rather than the default `Z`). Absent / `undefined` → `None`.
pub fn optional_instant_timezone(arg: f64) -> Option<TimeZone> {
    let obj = as_obj(arg)?;
    let v = field(obj, "timeZone");
    if is_undefined(v) {
        return None;
    }
    Some(timezone(v))
}

/// Resolve a time-zone argument — a tz-identifier string or a
/// `Temporal.ZonedDateTime` whose zone is reused.
pub fn timezone(v: f64) -> TimeZone {
    if JSValue::from_bits(v.to_bits()).is_string() {
        // A string identifier: an invalid one is a `RangeError`.
        return ok_or_throw(TimeZone::try_from_str(&read_string(v)));
    }
    if let Some(super::TemporalValue::ZonedDateTime(z)) = super::temporal_value_ref(v) {
        return *z.time_zone();
    }
    // `ToTemporalTimeZoneIdentifier`: a non-string, non-Temporal value (symbol,
    // plain object, number, boolean, null, bigint) is never a valid time-zone
    // identifier and cannot convert to one → `TypeError` (not `RangeError`).
    type_error("time zone must be a string identifier or Temporal.ZonedDateTime".to_string());
}

/// Parse a `getTimeZoneTransition` direction argument — a `"next"`/`"previous"`
/// string or a `{ direction }` object.
pub fn transition_direction(v: f64) -> TransitionDirection {
    let s = if JSValue::from_bits(v.to_bits()).is_string() {
        read_string(v)
    } else if let Some(obj) = as_obj(v) {
        match str_field_coerce(obj, "direction") {
            Some(s) => s,
            None => range("getTimeZoneTransition requires a direction"),
        }
    } else {
        range("getTimeZoneTransition requires a direction string or object");
    };
    TransitionDirection::from_str(&s).unwrap_or_else(|_| range("Invalid transition direction"))
}
