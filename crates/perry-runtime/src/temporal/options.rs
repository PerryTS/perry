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
use temporal_rs::{
    Calendar, MonthCode, PlainDate, PlainDateTime, PlainMonthDay, PlainTime, PlainYearMonth,
    TimeZone, TinyAsciiStr, UtcOffset, ZonedDateTime,
};

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

/// `true` for a calendar that has no eras at all — `iso8601`, `chinese`, and
/// `dangi`. For these the `era`/`eraYear` keys are not part of any date-field
/// group, so they are never read (and a bag carrying them is treated as if they
/// were absent — `from({ era, eraYear, year, … })` ignores them, and a `with`
/// bag carrying *only* `era`/`eraYear` is empty → the `EmptyFieldsIsInvalid`
/// TypeError, NOT an `Unknown era` RangeError). The `chinese`/`dangi` lunisolar
/// calendars track years by `year` (+ optional cyclic `monthCode`), never eras.
fn calendar_has_no_eras(calendar: &Calendar) -> bool {
    calendar.is_iso() || matches!(calendar.identifier(), "chinese" | "dangi")
}

/// Read the `era` / `eraYear` calendar fields, but ONLY for a calendar that
/// actually uses eras (see [`calendar_has_no_eras`]). The era-less calendars
/// never read `era`/`eraYear` — the order-of-operations tests assert exactly
/// that (an ISO bag observes no `era`/`eraYear` gets), and the
/// `calendar-not-supporting-eras` / chinese-dangi `mutually-exclusive-fields`
/// cases require those keys to be ignored rather than rejected as an unknown era.
fn read_era_fields(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> (Option<TinyAsciiStr<19>>, Option<i32>) {
    if calendar_has_no_eras(calendar) {
        return (None, None);
    }
    let era =
        str_field(obj, "era").and_then(|s| TinyAsciiStr::<19>::try_from_utf8(s.as_bytes()).ok());
    let era_year = num_field(obj, "eraYear").map(|n| n.trunc() as i32);
    (era, era_year)
}

/// Read a date property bag's calendar fields in `ToTemporalDate` /
/// `ToTemporalMonthDay` spec order — the calendar's date field keys read
/// **alphabetically**: `day`, [`era`, `eraYear` for non-ISO calendars], `month`,
/// `monthCode`, `year`. Numeric fields go through the real `ToNumber`
/// (`valueOf`) coercion; `monthCode` is `ToPrimitiveAndRequireString` (so a
/// `toString`-bearing wrapper is observed); `month`/`day` are
/// `ToPositiveIntegerWithTruncation` (a value `< 1` is a `RangeError`, even
/// under `constrain`). The caller resolves the `calendar` slot FIRST and reads
/// `overflow` LAST. Replaces the old non-alphabetical [`calendar_fields`] read
/// (which used `str_field` for `monthCode` — dropping a wrapped value — and
/// `field_u8` for `month`/`day` — silently saturating a negative to 255).
fn read_date_fields_alpha(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> CalendarFields {
    let day = num_field(obj, "day");
    let (era, era_year) = read_era_fields(obj, calendar);
    let month = num_field(obj, "month");
    let month_code = require_string_field(obj, "monthCode").map(|s| parse_month_code(&s));
    let year = num_field(obj, "year");

    let mut f = CalendarFields::new();
    if let Some(n) = year {
        f.year = Some(n.trunc() as i32);
    }
    if let Some(n) = month {
        f.month = Some(positive_field_u8(n, "month"));
    }
    f.month_code = month_code;
    if let Some(n) = day {
        f.day = Some(positive_field_u8(n, "day"));
    }
    f.era = era;
    if let Some(y) = era_year {
        f.era_year = Some(y);
    }
    f
}

/// `ToTemporalYearMonth` field read in spec (alphabetical) order — [`era`,
/// `eraYear` for non-ISO], `month`, `monthCode`, `year` (no `day`). Same coercion
/// rules as [`read_date_fields_alpha`].
fn read_year_month_fields_alpha(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> YearMonthCalendarFields {
    let (era, era_year) = read_era_fields(obj, calendar);
    let month = num_field(obj, "month");
    let month_code = require_string_field(obj, "monthCode").map(|s| parse_month_code(&s));
    let year = num_field(obj, "year");

    let mut f = YearMonthCalendarFields::new();
    if let Some(n) = year {
        f.year = Some(n.trunc() as i32);
    }
    if let Some(n) = month {
        f.month = Some(positive_field_u8(n, "month"));
    }
    f.month_code = month_code;
    f.era = era;
    if let Some(y) = era_year {
        f.era_year = Some(y);
    }
    f
}

/// Build a [`temporal_rs::PlainDate`] from a property-bag object
/// (`ToTemporalDate` step for an Object that isn't already a Temporal value):
/// resolve the `calendar` slot FIRST, then read the date fields in alphabetical
/// order, and construct under the given `overflow`. A non-object
/// (number/boolean/null/symbol) is a `TypeError`, matching the spec's
/// "Numbers cannot be used in place of an ISO string" wrong-type cases. Used by
/// the `compare`/`until`/`since`/`equals` coercion paths (which take no options,
/// so `overflow` is precomputed / `None`); `from` uses [`plain_date_from_bag_opts`].
pub fn plain_date_from_bag(v: f64, overflow: Option<Overflow>) -> temporal_rs::PlainDate {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainDate".to_string()),
    };
    let calendar = calendar_slot(field(obj, "calendar"));
    let calendar_fields = read_date_fields_alpha(obj, &calendar);
    let partial = temporal_rs::partial::PartialDate {
        calendar_fields,
        calendar,
    };
    ok_or_throw(temporal_rs::PlainDate::from_partial(partial, overflow))
}

/// `ToTemporalDate` for `Temporal.PlainDate.from(bag, options)`: read the bag's
/// calendar + fields in spec order, then `GetTemporalOverflowOption(options)`
/// **last** (so a failing field read happens before `overflow` is observed, and
/// a primitive `options` argument throws `TypeError` only after the fields are
/// read — the `observable-get-overflow` / `options-wrong-type` order tests).
pub fn plain_date_from_bag_opts(v: f64, opts: f64) -> temporal_rs::PlainDate {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainDate".to_string()),
    };
    let calendar = calendar_slot(field(obj, "calendar"));
    let calendar_fields = read_date_fields_alpha(obj, &calendar);
    let overflow = overflow(opts);
    let partial = temporal_rs::partial::PartialDate {
        calendar_fields,
        calendar,
    };
    ok_or_throw(temporal_rs::PlainDate::from_partial(partial, overflow))
}

/// `ToTemporalYearMonth` for `Temporal.PlainYearMonth.from(bag, options)`:
/// resolve calendar, read year-month fields alphabetically, then read `overflow`
/// from `options` last.
pub fn plain_year_month_from_bag_opts(v: f64, opts: f64) -> temporal_rs::PlainYearMonth {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainYearMonth".to_string()),
    };
    let calendar = calendar_slot(field(obj, "calendar"));
    let calendar_fields = read_year_month_fields_alpha(obj, &calendar);
    require_year_month_field(&calendar_fields);
    let overflow = overflow(opts);
    let partial = temporal_rs::partial::PartialYearMonth {
        calendar_fields,
        calendar,
    };
    ok_or_throw(temporal_rs::PlainYearMonth::from_partial(partial, overflow))
}

/// `ToTemporalYearMonth` for the no-options coercion paths
/// (`compare`/`until`/`since`/`equals`): calendar + fields, `constrain` overflow.
pub fn plain_year_month_from_bag(v: f64) -> temporal_rs::PlainYearMonth {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainYearMonth".to_string()),
    };
    let calendar = calendar_slot(field(obj, "calendar"));
    let calendar_fields = read_year_month_fields_alpha(obj, &calendar);
    require_year_month_field(&calendar_fields);
    let partial = temporal_rs::partial::PartialYearMonth {
        calendar_fields,
        calendar,
    };
    ok_or_throw(temporal_rs::PlainYearMonth::from_partial(
        partial,
        Some(Overflow::Constrain),
    ))
}

/// `ToTemporalMonthDay` for `Temporal.PlainMonthDay.from(bag, options)`: resolve
/// calendar, read the date fields alphabetically (a `PlainMonthDay` bag reads the
/// same `day`/`month`/`monthCode`/`year` set as a date), then `overflow` last.
pub fn plain_month_day_from_bag_opts(v: f64, opts: f64) -> temporal_rs::PlainMonthDay {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainMonthDay".to_string()),
    };
    let calendar = calendar_slot(field(obj, "calendar"));
    let calendar_fields = read_date_fields_alpha(obj, &calendar);
    require_month_day_field(&calendar_fields);
    let overflow = overflow(opts);
    let partial = temporal_rs::partial::PartialDate {
        calendar_fields,
        calendar,
    };
    ok_or_throw(temporal_rs::PlainMonthDay::from_partial(partial, overflow))
}

/// `ToTemporalMonthDay` for the no-options `equals` coercion path.
pub fn plain_month_day_from_bag(v: f64) -> temporal_rs::PlainMonthDay {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainMonthDay".to_string()),
    };
    let calendar = calendar_slot(field(obj, "calendar"));
    let calendar_fields = read_date_fields_alpha(obj, &calendar);
    require_month_day_field(&calendar_fields);
    let partial = temporal_rs::partial::PartialDate {
        calendar_fields,
        calendar,
    };
    ok_or_throw(temporal_rs::PlainMonthDay::from_partial(
        partial,
        Some(Overflow::Constrain),
    ))
}

/// A `PlainYearMonth` property bag must carry at least one recognized year-month
/// field (`year`/`month`/`monthCode`/`era`/`eraYear`); otherwise
/// `ToTemporalYearMonth` / `PrepareTemporalFields` throws a `TypeError`. Checked
/// on the ALREADY-READ fields (not by pre-scanning the object) so `calendar` is
/// still observed first — the order-of-operations tests assert that exact order.
fn require_year_month_field(f: &YearMonthCalendarFields) {
    if f.year.is_none()
        && f.month.is_none()
        && f.month_code.is_none()
        && f.era.is_none()
        && f.era_year.is_none()
    {
        type_error("object is not a valid Temporal.PlainYearMonth property bag".to_string());
    }
}

/// A `PlainMonthDay` property bag must carry at least one recognized field
/// (`month`/`monthCode`/`day`/`year`/`era`/`eraYear`). Checked on the already-read
/// fields so `calendar` is observed first.
fn require_month_day_field(f: &CalendarFields) {
    if f.year.is_none()
        && f.month.is_none()
        && f.month_code.is_none()
        && f.day.is_none()
        && f.era.is_none()
        && f.era_year.is_none()
    {
        type_error("object is not a valid Temporal.PlainMonthDay property bag".to_string());
    }
}

/// Build a [`temporal_rs::PlainDateTime`] from a property-bag object
/// (`ToTemporalDateTime` for a non-Temporal Object). Reads, in spec order, the
/// `calendar` slot, then the date+time fields (alphabetical), then — last — the
/// `overflow` option from `opts` (so `from`'s observable-overflow / order tests
/// see options processed after the fields). `opts` is `undefined` for the
/// coercion paths (`compare`/`until`/`since`) that take no options.
pub fn plain_date_time_from_bag(v: f64, opts: f64) -> temporal_rs::PlainDateTime {
    let obj = match as_obj(v) {
        Some(o) => o,
        None => type_error("Cannot convert value to a Temporal.PlainDateTime".to_string()),
    };
    let calendar = calendar_slot(field(obj, "calendar"));
    let fields = datetime_fields(obj, &calendar);
    let overflow = overflow(opts);
    let partial = temporal_rs::partial::PartialDateTime { fields, calendar };
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
        let s = read_string(v);
        reject_bare_islamic(&s);
        return ok_or_throw(s.parse::<Calendar>());
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

/// `ToTemporalCalendarIdentifier` for a constructor's trailing `calendar`
/// argument — stricter than [`calendar_slot`]: a string must be a bare calendar
/// *identifier* (`Calendar::try_from_utf8`), so an ISO date string / calendar
/// annotation form (`"1997-12-04[u-ca=iso8601]"`, `"1111-11-11"`, `""`) is a
/// `RangeError`, not silently accepted via `ParseTemporalCalendarString`.
/// Non-string values defer to [`calendar_slot`] (undefined → ISO, a Temporal
/// value → its `[[Calendar]]`, anything else → TypeError).
pub fn calendar_identifier(v: f64) -> temporal_rs::Calendar {
    if JSValue::from_bits(v.to_bits()).is_string() {
        let s = read_string(v);
        reject_bare_islamic(&s);
        return ok_or_throw(temporal_rs::Calendar::try_from_utf8(s.as_bytes()));
    }
    calendar_slot(v)
}

/// `islamic` (the bare Hijri identifier, with no `-civil` / `-tbla` /
/// `-umalqura` variant) is *accepted* by `temporal_rs` — it falls back to the
/// tabular-Friday epoch — but per the Intl-Era-monthcode proposal a bare
/// `islamic` is recognized only by the `Intl.DateTimeFormat` constructor, never
/// as a Temporal calendar identifier. Reject it with a `RangeError` so
/// `Temporal.PlainDate.from({ calendar: "islamic", … })` and friends throw as
/// the spec requires, instead of silently constructing a Hijri date.
fn reject_bare_islamic(id: &str) {
    if id.eq_ignore_ascii_case("islamic") {
        range("unknown calendar: islamic is only supported by Intl.DateTimeFormat, not Temporal");
    }
}

// ---- `Temporal.*.prototype.toLocaleString` ---------------------------------

/// `Temporal.*.prototype.toLocaleString` calendar check for `PlainDate`,
/// `PlainDateTime`, and `ZonedDateTime` — per `HandleDateTimeTemporalDate`
/// (ecma402 sup-intl.html #1107) and the analogous `ZonedDateTime` clause
/// (#1953), a value renders only when its calendar matches the formatter's:
/// the ISO-8601 calendar (always permitted) or the calendar the locale
/// resolves to. With no explicit `calendar` option that resolved calendar is
/// the locale default, which Perry's `Intl.DateTimeFormat` reports as
/// `gregory`. Any other calendar is a `RangeError` (the test262
/// `toLocaleString/calendar-mismatch` cases).
///
/// `PlainYearMonth`/`PlainMonthDay` do NOT get the ISO carve-out — see
/// [`assert_locale_string_calendar_no_iso_carveout`].
///
/// NOTE: the `locales`/`options` arguments — which could name a *different*
/// calendar to validate against — are dropped by the `Expr::DateToLocaleString`
/// HIR lowering before the call reaches the runtime, so only the locale-default
/// calendar is consulted here. Honoring an explicit `calendar`/`dateStyle`/
/// `timeStyle` option (and the spec's option-conflict `TypeError`s) is a
/// follow-up that depends on threading those arguments through that lowering.
pub fn assert_locale_string_calendar(instance_calendar: &str) {
    if instance_calendar != "iso8601" && instance_calendar != "gregory" {
        range("calendar mismatch: the value's calendar differs from the locale calendar");
    }
}

/// `Temporal.PlainYearMonth`/`Temporal.PlainMonthDay` `toLocaleString` calendar
/// check. `HandleDateTimeTemporalYearMonth`/`HandleDateTimeTemporalMonthDay`
/// (ecma402 sup-intl.html #1132, #1157) require the instance calendar to
/// equal the formatter's calendar exactly — unlike `PlainDate`/`PlainDateTime`
/// /`ZonedDateTime`, there is no `"iso8601"`-is-always-permitted carve-out, so
/// an ISO-calendar year-month/month-day always mismatches the locale's
/// default (`gregory`) calendar and throws (test262
/// `toLocaleString/calendar-mismatch` "even when instance has the ISO
/// calendar" cases).
pub fn assert_locale_string_calendar_no_iso_carveout(instance_calendar: &str) {
    if instance_calendar != "gregory" {
        range("calendar mismatch: the value's calendar differs from the locale calendar");
    }
}

/// `H:MM:SS AM/PM`, the en-US default wall-clock rendering shared by the
/// time-bearing `toLocaleString` formatters below.
fn locale_time_12h(hour: u8, minute: u8, second: u8) -> String {
    let (h12, suffix) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    format!("{h12}:{minute:02}:{second:02} {suffix}")
}

// The valid-path formatters render straight from the calendar-aware accessors.
// Because the ISO-8601 calendar *is* the proleptic Gregorian calendar, those
// accessors return identical numbers for an `iso8601` and a `gregory` instance
// of the same date — and `assert_locale_string_calendar` has already rejected every
// other calendar — so the output is calendar-agnostic, exactly as the
// `calendar-mismatch` conformance tests require (an ISO and a Gregorian
// instance must format identically).

/// `M/D/YYYY` — `Temporal.PlainDate.prototype.toLocaleString` default form.
pub fn plain_date_locale_string(d: &PlainDate) -> String {
    format!("{}/{}/{}", d.month(), d.day(), d.year())
}

/// `M/D/YYYY, H:MM:SS AM/PM` — `Temporal.PlainDateTime` default form.
pub fn plain_date_time_locale_string(dt: &PlainDateTime) -> String {
    format!(
        "{}/{}/{}, {}",
        dt.month(),
        dt.day(),
        dt.year(),
        locale_time_12h(dt.hour(), dt.minute(), dt.second())
    )
}

/// `H:MM:SS AM/PM` — `Temporal.PlainTime` default form.
pub fn plain_time_locale_string(t: &PlainTime) -> String {
    locale_time_12h(t.hour(), t.minute(), t.second())
}

/// `M/YYYY` — `Temporal.PlainYearMonth` default form (no day component).
pub fn plain_year_month_locale_string(ym: &PlainYearMonth) -> String {
    format!("{}/{}", ym.month(), ym.year())
}

/// `M/D` — `Temporal.PlainMonthDay` default form (no year component).
pub fn plain_month_day_locale_string(md: &PlainMonthDay) -> String {
    format!("{}/{}", md.month_code().to_month_integer(), md.day())
}

/// `M/D/YYYY, H:MM:SS AM/PM` rendered in the instance's own time zone —
/// `Temporal.ZonedDateTime` default form.
pub fn zoned_date_time_locale_string(z: &ZonedDateTime) -> String {
    format!(
        "{}/{}/{}, {}",
        z.month(),
        z.day(),
        z.year(),
        locale_time_12h(z.hour(), z.minute(), z.second())
    )
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
    // Spec `ToIntegerWithTruncation` / `GetOption(type: Number)` run the real
    // abstract `ToNumber`: an object's `valueOf`/`Symbol.toPrimitive` is invoked
    // (the order-of-operations / `*-infinity-throws-rangeerror` tests observe
    // exactly one `valueOf` call) and a Symbol throws `TypeError`, NOT the plain
    // bit-level `to_number()` which returns NaN for both. A non-finite result is
    // then a `RangeError` (Temporal fields reject Infinity/NaN).
    //
    // A BigInt is a `TypeError` here: abstract `ToNumber(BigInt)` throws (unlike
    // the explicit `Number(2n)` constructor, which `js_number_coerce` permits) —
    // `roundingIncrement: 2n` and `{ year: 2n }` are wrong-type, not value 2.
    if JSValue::from_bits(raw.to_bits()).is_bigint() {
        type_error("Cannot convert a BigInt value to a number".to_string());
    }
    let n = crate::builtins::js_number_coerce(raw);
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

/// `ToPositiveIntegerWithTruncation` → `u8` slot for the `month` / `day` fields:
/// a value `< 1` is a `RangeError` (months and days are positive, even under the
/// `constrain` overflow — `from({ month: -1 })` throws). Out-of-range-high values
/// saturate to `u8::MAX` so `temporal_rs`'s own bound then rejects them.
fn positive_field_u8(n: f64, what: &str) -> u8 {
    if n.trunc() < 1.0 {
        range(match what {
            "month" => "month must be a positive integer",
            _ => "day must be a positive integer",
        });
    }
    field_u8(n.trunc() as i64)
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

/// Marshal a `round(roundTo)` argument for **PlainDateTime / ZonedDateTime**
/// (which take no `largestUnit` / `relativeTo`) — a `smallestUnit` string
/// shorthand or an options object — into [`RoundingOptions`]. Options are read
/// in spec (alphabetical) order: `roundingIncrement`, `roundingMode`,
/// `smallestUnit`. `Temporal.Duration.prototype.round` uses
/// [`duration_round_options`] instead (it additionally reads `largestUnit` and
/// `relativeTo`).
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
            if let Some(n) = num_field(obj, "roundingIncrement") {
                o.increment = Some(parse_increment(n));
            }
            o.rounding_mode = read_rounding_mode(obj);
            o.smallest_unit = read_smallest_unit(obj);
        }
        // GetOptionsObject: a non-string, non-object `roundTo` (number, boolean,
        // bigint, null, undefined, symbol) is a TypeError.
        None => type_error("round requires a unit string or an options object".to_string()),
    }
    o
}

/// Marshal a `Temporal.Duration.prototype.round(roundTo)` argument into
/// [`RoundingOptions`] + optional [`RelativeTo`]. Options are read in spec
/// (alphabetical) order: `largestUnit`, `relativeTo`, `roundingIncrement`,
/// `roundingMode`, `smallestUnit` — all read BEFORE the "both units unset"
/// algorithmic validation (which `temporal_rs` performs in `Duration::round`).
pub fn duration_round_options(arg: f64) -> (RoundingOptions, Option<RelativeTo>) {
    let mut o = RoundingOptions::default();
    o.largest_unit = None;
    o.smallest_unit = None;
    o.rounding_mode = None;
    o.increment = None;

    if JSValue::from_bits(arg.to_bits()).is_string() {
        o.smallest_unit = Some(parse_unit(&read_string(arg)));
        return (o, None);
    }
    let obj = match as_obj(arg) {
        Some(obj) => obj,
        None => type_error("round requires a unit string or an options object".to_string()),
    };
    if let Some(s) = str_field_coerce(obj, "largestUnit") {
        o.largest_unit = Some(parse_unit(&s));
    }
    let rel = relative_to_field(obj);
    if let Some(n) = num_field(obj, "roundingIncrement") {
        o.increment = Some(parse_increment(n));
    }
    o.rounding_mode = read_rounding_mode(obj);
    o.smallest_unit = read_smallest_unit(obj);
    (o, rel)
}

/// Marshal a `total(totalOf)` argument — a `unit` string or a
/// `{ unit, relativeTo }` object — into the (required) unit and optional
/// `relativeTo`. Options are read in spec (alphabetical) order: `relativeTo`,
/// `unit`.
pub fn total_options(arg: f64) -> (Unit, Option<RelativeTo>) {
    if JSValue::from_bits(arg.to_bits()).is_string() {
        return (parse_unit(&read_string(arg)), None);
    }
    match as_obj(arg) {
        Some(obj) => {
            let rel = relative_to_field(obj);
            let unit = match str_field_coerce(obj, "unit") {
                Some(s) => parse_unit(&s),
                None => range("total requires a unit"),
            };
            (unit, rel)
        }
        None => type_error("total requires a unit string or an options object".to_string()),
    }
}

/// `GetTemporalFractionalSecondDigitsOption`: read `fractionalSecondDigits` into
/// `o.precision`. A Number → floor to an integer 0..=9 (RangeError otherwise);
/// ANY non-Number (string, null, boolean, object, bigint) → ToString must equal
/// `"auto"`, else RangeError; a Symbol → TypeError (ToString of a Symbol throws).
fn read_fractional_second_digits(
    obj: *const crate::object::ObjectHeader,
    o: &mut ToStringRoundingOptions,
) {
    let raw = field(obj, "fractionalSecondDigits");
    if is_undefined(raw) {
        return;
    }
    let jv = JSValue::from_bits(raw.to_bits());
    if jv.is_number() || jv.is_int32() {
        let n = jv.to_number();
        if !n.is_finite() {
            range("fractionalSecondDigits must be \"auto\" or an integer 0-9");
        }
        let d = n.floor();
        if !(0.0..=9.0).contains(&d) {
            range("fractionalSecondDigits must be between 0 and 9");
        }
        o.precision = Precision::Digit(d as u8);
    } else {
        if unsafe { crate::symbol::js_is_symbol(raw) } != 0 {
            type_error("Cannot convert a Symbol value to a string".to_string());
        }
        let s = if jv.is_string() {
            read_string(raw)
        } else {
            let sh = crate::value::js_jsvalue_to_string_coerce(raw);
            read_string(crate::value::js_nanbox_string(sh as i64))
        };
        if s != "auto" {
            range("fractionalSecondDigits must be \"auto\" or an integer 0-9");
        }
        o.precision = Precision::Auto;
    }
}

fn read_rounding_mode(obj: *const crate::object::ObjectHeader) -> Option<RoundingMode> {
    str_field_coerce(obj, "roundingMode").map(|s| parse_rounding_mode(&s))
}

fn read_smallest_unit(obj: *const crate::object::ObjectHeader) -> Option<Unit> {
    str_field_coerce(obj, "smallestUnit").map(|s| parse_unit(&s))
}

fn read_display_calendar(obj: *const crate::object::ObjectHeader) -> DisplayCalendar {
    match str_field_coerce(obj, "calendarName") {
        Some(s) => {
            DisplayCalendar::from_str(&s).unwrap_or_else(|_| range("Invalid calendarName option"))
        }
        None => DisplayCalendar::Auto,
    }
}

/// `toString` options for the calendar-less types (Duration / Instant /
/// PlainTime), read in spec (alphabetical) order: `fractionalSecondDigits`,
/// `roundingMode`, `smallestUnit`.
pub fn to_string_rounding_options(arg: f64) -> ToStringRoundingOptions {
    let mut o = ToStringRoundingOptions::default();
    let obj = match require_options_object(arg) {
        Some(o) => o,
        None => return o, // undefined → defaults; a primitive throws TypeError
    };
    read_fractional_second_digits(obj, &mut o);
    o.rounding_mode = read_rounding_mode(obj);
    o.smallest_unit = read_smallest_unit(obj);
    o
}

/// `Temporal.PlainDateTime.prototype.toString` options, read in spec
/// (alphabetical) order: `calendarName`, `fractionalSecondDigits`,
/// `roundingMode`, `smallestUnit`.
pub fn pdt_to_string_options(arg: f64) -> (ToStringRoundingOptions, DisplayCalendar) {
    let mut o = ToStringRoundingOptions::default();
    let obj = match require_options_object(arg) {
        Some(o) => o,
        None => return (o, DisplayCalendar::Auto),
    };
    let cal = read_display_calendar(obj);
    read_fractional_second_digits(obj, &mut o);
    o.rounding_mode = read_rounding_mode(obj);
    o.smallest_unit = read_smallest_unit(obj);
    (o, cal)
}

/// `Temporal.ZonedDateTime.prototype.toString` options, read in spec
/// (alphabetical) order: `calendarName`, `fractionalSecondDigits`, `offset`,
/// `roundingMode`, `smallestUnit`, `timeZoneName`.
pub fn zdt_to_string_options(
    arg: f64,
) -> (
    ToStringRoundingOptions,
    DisplayOffset,
    DisplayTimeZone,
    DisplayCalendar,
) {
    let mut o = ToStringRoundingOptions::default();
    let obj = match require_options_object(arg) {
        Some(o) => o,
        None => {
            return (
                o,
                DisplayOffset::Auto,
                DisplayTimeZone::Auto,
                DisplayCalendar::Auto,
            )
        }
    };
    let cal = read_display_calendar(obj);
    read_fractional_second_digits(obj, &mut o);
    let offset = match str_field_coerce(obj, "offset") {
        Some(s) => DisplayOffset::from_str(&s).unwrap_or_else(|_| range("Invalid offset option")),
        None => DisplayOffset::Auto,
    };
    o.rounding_mode = read_rounding_mode(obj);
    o.smallest_unit = read_smallest_unit(obj);
    let tz = match str_field_coerce(obj, "timeZoneName") {
        Some(s) => {
            DisplayTimeZone::from_str(&s).unwrap_or_else(|_| range("Invalid timeZoneName option"))
        }
        None => DisplayTimeZone::Auto,
    };
    (o, offset, tz, cal)
}

/// Marshal an `until`/`since` options argument into [`DifferenceSettings`].
/// Options are read in spec (alphabetical) order: `largestUnit`,
/// `roundingIncrement`, `roundingMode`, `smallestUnit`. An `undefined` / absent
/// arg yields the default (auto units).
pub fn difference_settings(arg: f64) -> DifferenceSettings {
    let mut s = DifferenceSettings::default();
    if let Some(obj) = require_options_object(arg) {
        if let Some(u) = str_field_coerce(obj, "largestUnit") {
            s.largest_unit = Some(parse_unit(&u));
        }
        if let Some(n) = num_field(obj, "roundingIncrement") {
            s.increment = Some(parse_increment(n));
        }
        if let Some(m) = str_field_coerce(obj, "roundingMode") {
            s.rounding_mode = Some(parse_rounding_mode(&m));
        }
        if let Some(u) = str_field_coerce(obj, "smallestUnit") {
            s.smallest_unit = Some(parse_unit(&u));
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
    // A plain property-bag object. Per `ToRelativeTemporalObject`, the calendar
    // and (optional) timeZone slots are resolved and validated: a bag carrying
    // a `timeZone` becomes a `ZonedDateTime` (so an invalid timezone / offset
    // string is a RangeError, a wrong-typed timezone a TypeError), otherwise a
    // `PlainDate`. Previously this ignored both fields, so those validation
    // cases never threw.
    if let Some(o) = as_obj(v) {
        // `ToRelativeTemporalObject` reads the calendar + full datetime field set
        // in spec (alphabetical) order: calendar, day, hour, microsecond,
        // millisecond, minute, month, monthCode, nanosecond, offset, second,
        // timeZone, year — with `ToNumber`/`valueOf` coercion per field (the
        // order-of-operations tests observe this exact interleaving). `era`/
        // `eraYear` (read right after `day`) are part of the field set only for a
        // non-ISO calendar. A present `timeZone` makes it a `ZonedDateTime`,
        // otherwise a `PlainDate`; `monthCode` is parsed at its read position so a
        // bad month code throws before a later wrong-typed field is even read.
        let calendar = calendar_slot(field(o, "calendar"));
        let (cf, pt, offset, tz_raw) = read_zoned_bag_alpha(o, &calendar);

        if !is_undefined(tz_raw) {
            let tz = timezone(tz_raw);
            let mut p = PartialZonedDateTime::new();
            p.calendar = calendar;
            p.fields.calendar_fields = cf;
            p.fields.time = pt;
            p.fields.offset = offset;
            p.timezone = Some(tz);
            let zdt = ok_or_throw(temporal_rs::ZonedDateTime::from_partial(
                p, None, None, None,
            ));
            return Some(RelativeTo::ZonedDateTime(zdt));
        }
        let partial = temporal_rs::partial::PartialDate {
            calendar_fields: cf,
            calendar,
        };
        return Some(RelativeTo::PlainDate(ok_or_throw(
            temporal_rs::PlainDate::from_partial(partial, None),
        )));
    }
    // A non-object, non-string primitive (number, boolean, bigint, null) cannot
    // convert to a relativeTo — `ToRelativeTemporalObject` throws a TypeError
    // before any ISO parse (the `relativeto-number` case expects TypeError).
    type_error("relativeTo must be a Temporal object, ISO string, or property bag".to_string())
}

// ---- overflow / disambiguation (second-arg option objects) ----------------

/// Read an optional `overflow` (`"constrain"` | `"reject"`) from an options arg.
/// A non-object, non-undefined options arg is a `TypeError` (`GetOptionsObject`).
pub fn overflow(arg: f64) -> Option<Overflow> {
    validate_options_arg(arg);
    let obj = as_obj(arg)?;
    str_field_coerce(obj, "overflow").map(|s| parse_overflow(&s))
}

/// Read an optional `disambiguation` from an options arg. A non-object,
/// non-undefined options value is a `TypeError` (`GetOptionsObject`) — so e.g.
/// `toZonedDateTime(tz, "primitive")` throws even when no option is read.
pub fn disambiguation(arg: f64) -> Option<Disambiguation> {
    validate_options_arg(arg);
    let obj = as_obj(arg)?;
    str_field_coerce(obj, "disambiguation").map(|s| parse_disambiguation(&s))
}

/// Read an optional `offset` (offset-disambiguation) from an options arg.
pub fn offset_option(arg: f64) -> Option<OffsetDisambiguation> {
    validate_options_arg(arg);
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

/// Populate a [`PartialTime`] from an object's `hour…nanosecond` fields, read in
/// spec (alphabetical) order: hour, microsecond, millisecond, minute,
/// nanosecond, second — the order the order-of-operations tests observe.
pub fn partial_time(obj: *const crate::object::ObjectHeader) -> PartialTime {
    let mut t = PartialTime::new();
    if let Some(n) = num_field(obj, "hour") {
        t.hour = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "microsecond") {
        t.microsecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "millisecond") {
        t.millisecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "minute") {
        t.minute = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "nanosecond") {
        t.nanosecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = num_field(obj, "second") {
        t.second = Some(field_u8(n.trunc() as i64));
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

/// `Temporal.PlainYearMonth.prototype.toPlainDate(item)` reads ONLY `day` from
/// `item` — the year/month come from the receiver — so the observable order is
/// just `get day` / `valueOf`. (The old `calendar_fields` read year/month/
/// monthCode/day/era/eraYear, recording extra gets.) `day` is
/// `ToPositiveIntegerWithTruncation`.
pub fn to_plain_date_day_field(obj: *const crate::object::ObjectHeader) -> CalendarFields {
    let mut f = CalendarFields::new();
    if let Some(n) = num_field(obj, "day") {
        f.day = Some(positive_field_u8(n, "day"));
    }
    f
}

/// `Temporal.PlainMonthDay.prototype.toPlainDate(item)` reads the `YEAR`
/// calendar-field group from `item` (the month/day come from the receiver).
/// For a non-ISO calendar that group also includes `era`/`eraYear`
/// (`PrepareCalendarFields(calendar, item, « YEAR », …)` — the calendar expands
/// the requested `year` field to its era-relative keys), read **alphabetically**:
/// `era`, `eraYear`, `year`. So `toPlainDate({ era, eraYear })` observes the
/// `eraYear.valueOf` get and rejects a non-finite `eraYear` with a `RangeError`
/// (the `*-infinity-throws-rangeerror` tests). An ISO calendar reads only `year`.
pub fn to_plain_date_year_field(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> CalendarFields {
    let mut f = CalendarFields::new();
    let (era, era_year) = read_era_fields(obj, calendar);
    if let Some(n) = num_field(obj, "year") {
        f.year = Some(n.trunc() as i32);
    }
    f.era = era;
    if let Some(y) = era_year {
        f.era_year = Some(y);
    }
    f
}

/// Read the ISO date+time field set in spec (alphabetical) order — day, [era,
/// eraYear,] hour, microsecond, millisecond, minute, month, monthCode,
/// nanosecond, [offset,] second, year — with `ToNumber`/`valueOf` coercion per
/// numeric field and `monthCode`/`offset` as required strings (parsed at their
/// read position). The order-of-operations tests observe exactly this
/// interleaving. `era`/`eraYear` are read only for a **non-ISO** calendar (the
/// ISO calendar has no eras, and reading them on an ISO bag would record extra
/// observable gets); for a non-ISO calendar they ARE part of the date-field set,
/// so a `{ era, eraYear, monthCode, day }` bag resolves instead of throwing
/// `Insufficient fields` (the `era-boundary-*` / `era-*` from/with/arithmetic
/// cases). `read_offset` controls whether the `offset` field (ZonedDateTime
/// only) is read.
fn read_iso_fields_alpha(
    obj: *const crate::object::ObjectHeader,
    read_offset: bool,
    calendar: &Calendar,
) -> (CalendarFields, PartialTime, Option<UtcOffset>) {
    let day = num_field(obj, "day");
    let (era, era_year) = read_era_fields(obj, calendar);
    let hour = num_field(obj, "hour");
    let microsecond = num_field(obj, "microsecond");
    let millisecond = num_field(obj, "millisecond");
    let minute = num_field(obj, "minute");
    let month = num_field(obj, "month");
    let month_code = require_string_field(obj, "monthCode").map(|s| parse_month_code(&s));
    let nanosecond = num_field(obj, "nanosecond");
    let offset = if read_offset { offset_field(obj) } else { None };
    let second = num_field(obj, "second");
    let year = num_field(obj, "year");

    let mut cf = CalendarFields::new();
    if let Some(n) = year {
        cf.year = Some(n.trunc() as i32);
    }
    if let Some(n) = month {
        cf.month = Some(positive_field_u8(n, "month"));
    }
    cf.month_code = month_code;
    if let Some(n) = day {
        cf.day = Some(positive_field_u8(n, "day"));
    }
    cf.era = era;
    if let Some(y) = era_year {
        cf.era_year = Some(y);
    }
    let mut pt = PartialTime::new();
    if let Some(n) = hour {
        pt.hour = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = minute {
        pt.minute = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = second {
        pt.second = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = millisecond {
        pt.millisecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = microsecond {
        pt.microsecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = nanosecond {
        pt.nanosecond = Some(field_u16(n.trunc() as i64));
    }
    (cf, pt, offset)
}

/// Read a ZonedDateTime-shaped property bag's fields in spec (alphabetical)
/// order — day, [era, eraYear,] hour, microsecond, millisecond, minute, month,
/// monthCode, nanosecond, offset, second, timeZone, year — returning the parsed
/// `CalendarFields` + `PartialTime` + optional offset + the raw `timeZone` value
/// (so the caller can resolve/validate it). The caller reads `calendar` FIRST.
/// `era`/`eraYear` are read only for a non-ISO calendar (see
/// [`read_iso_fields_alpha`]). Shared by `ToTemporalZonedDateTime` (`from`) and
/// `ToRelativeTemporalObject`.
fn read_zoned_bag_alpha(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> (CalendarFields, PartialTime, Option<UtcOffset>, f64) {
    let day = num_field(obj, "day");
    let (era, era_year) = read_era_fields(obj, calendar);
    let hour = num_field(obj, "hour");
    let microsecond = num_field(obj, "microsecond");
    let millisecond = num_field(obj, "millisecond");
    let minute = num_field(obj, "minute");
    let month = num_field(obj, "month");
    let month_code = require_string_field(obj, "monthCode").map(|s| parse_month_code(&s));
    let nanosecond = num_field(obj, "nanosecond");
    let offset = offset_field(obj);
    let second = num_field(obj, "second");
    let tz_raw = field(obj, "timeZone");
    let year = num_field(obj, "year");

    let mut cf = CalendarFields::new();
    if let Some(n) = year {
        cf.year = Some(n.trunc() as i32);
    }
    if let Some(n) = month {
        cf.month = Some(positive_field_u8(n, "month"));
    }
    cf.month_code = month_code;
    if let Some(n) = day {
        cf.day = Some(positive_field_u8(n, "day"));
    }
    cf.era = era;
    if let Some(y) = era_year {
        cf.era_year = Some(y);
    }
    let mut pt = PartialTime::new();
    if let Some(n) = hour {
        pt.hour = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = minute {
        pt.minute = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = second {
        pt.second = Some(field_u8(n.trunc() as i64));
    }
    if let Some(n) = millisecond {
        pt.millisecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = microsecond {
        pt.microsecond = Some(field_u16(n.trunc() as i64));
    }
    if let Some(n) = nanosecond {
        pt.nanosecond = Some(field_u16(n.trunc() as i64));
    }
    (cf, pt, offset, tz_raw)
}

/// `RejectObjectWithCalendarOrTimeZone`: a `with`-partial bag must NOT carry a
/// `calendar` or `timeZone` property (they are read — in that order — and a
/// present value is a TypeError). The receiver supplies the calendar/zone.
fn reject_calendar_and_timezone(obj: *const crate::object::ObjectHeader) {
    if !is_undefined(field(obj, "calendar")) {
        type_error("with-fields object must not have a calendar property".to_string());
    }
    if !is_undefined(field(obj, "timeZone")) {
        type_error("with-fields object must not have a timeZone property".to_string());
    }
}

/// Populate a [`DateTimeFields`] (calendar fields + time) for `PlainDateTime.with`,
/// reading in alphabetical order. The caller performs
/// `RejectObjectWithCalendarOrTimeZone` first.
pub fn datetime_fields(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> DateTimeFields {
    let (cf, pt, _) = read_iso_fields_alpha(obj, false, calendar);
    let mut f = DateTimeFields::new();
    f.calendar_fields = cf;
    f.time = pt;
    f
}

/// `RejectObjectWithCalendarOrTimeZone` + alphabetical-order field read for
/// `PlainDateTime.with` (the receiver supplies the `calendar`, which decides
/// whether `era`/`eraYear` are part of the field set).
pub fn with_datetime_fields(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> DateTimeFields {
    reject_calendar_and_timezone(obj);
    datetime_fields(obj, calendar)
}

/// `Temporal.PlainDate.prototype.with` / `PlainMonthDay.prototype.with`
/// partial-fields read: `RejectObjectWithCalendarOrTimeZone` (reads `calendar`
/// then `timeZone`, throwing `TypeError` if present), then the calendar's date
/// fields alphabetically — exactly mirroring `PlainDateTime.with`. The receiver
/// supplies `calendar` (so era fields are read only for a non-ISO calendar) and
/// the caller reads `overflow` LAST. Replaces the old `calendar_fields` read
/// (non-alphabetical, `str_field` monthCode, `field_u8` negatives, no reject).
pub fn with_calendar_fields(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> CalendarFields {
    reject_calendar_and_timezone(obj);
    read_date_fields_alpha(obj, calendar)
}

/// `Temporal.PlainYearMonth.prototype.with` partial-fields read:
/// `RejectObjectWithCalendarOrTimeZone` then the year-month fields alphabetically.
pub fn with_year_month_fields(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> YearMonthCalendarFields {
    reject_calendar_and_timezone(obj);
    read_year_month_fields_alpha(obj, calendar)
}

/// `Temporal.PlainTime.prototype.with` partial-fields read:
/// `RejectObjectWithCalendarOrTimeZone` (a `PlainTime` bag still rejects a
/// `calendar`/`timeZone` property per spec), then the time fields alphabetically.
pub fn with_partial_time(obj: *const crate::object::ObjectHeader) -> PartialTime {
    reject_calendar_and_timezone(obj);
    partial_time(obj)
}

/// Populate a [`ZonedDateTimeFields`] (calendar fields + time + offset) for
/// `ZonedDateTime.with`, reading in alphabetical order after
/// `RejectObjectWithCalendarOrTimeZone`.
pub fn zoned_fields(
    obj: *const crate::object::ObjectHeader,
    calendar: &Calendar,
) -> ZonedDateTimeFields {
    reject_calendar_and_timezone(obj);
    let (cf, pt, offset) = read_iso_fields_alpha(obj, true, calendar);
    let mut f = ZonedDateTimeFields::new();
    f.calendar_fields = cf;
    f.time = pt;
    f.offset = offset;
    f
}

/// `ToPrimitiveAndRequireString`: read `obj.<name>` and require a String result.
/// `undefined` → `None`; a String → that string; an Object → `ToPrimitive(string)`
/// (its `toString`/`valueOf` runs), then the RESULT must be a String (else
/// TypeError); any other primitive (number / boolean / bigint / null) → TypeError;
/// a Symbol → TypeError. Used for `offset` and `monthCode` fields, which the spec
/// reads as required strings (the property-bag observer wraps even string values
/// in a `toString`-bearing object).
fn require_string_field(obj: *const crate::object::ObjectHeader, name: &str) -> Option<String> {
    let raw = field(obj, name);
    if is_undefined(raw) {
        return None;
    }
    let jv = JSValue::from_bits(raw.to_bits());
    if jv.is_string() {
        return Some(read_string(raw));
    }
    if jv.is_pointer() && unsafe { crate::symbol::js_is_symbol(raw) } == 0 {
        // Object: `ToPrimitiveAndRequireString` — OrdinaryToPrimitive with the
        // string hint (its `toString`/`valueOf` runs, which the order-of-operations
        // tests observe), then the RESULT must be a String. A custom `toString`
        // returning a non-string (`{ toString: () => 5 }`) is a TypeError, NOT a
        // further ToString coercion. A plain object with only the default
        // `Object.prototype.toString` (`None`) yields `"[object Object]"`, which
        // IS a string (and then fails the month-code/offset syntax check).
        match unsafe { crate::value::to_string::ordinary_to_primitive_string(raw) } {
            Some(prim) => {
                if JSValue::from_bits(prim.to_bits()).is_string() {
                    return Some(read_string(prim));
                }
                // non-string primitive result → require-string fails (TypeError)
            }
            None => {
                let sh = crate::value::js_jsvalue_to_string_coerce(raw);
                return Some(read_string(crate::value::js_nanbox_string(sh as i64)));
            }
        }
    }
    type_error(format!("{name} property must be a string"))
}

/// Read an optional `offset` field → [`UtcOffset`]. A present `offset` is read as
/// a required string (see [`require_string_field`]); a malformed offset string
/// is a `RangeError` — matching the property-bag `*-invalid-offset-string` cases
/// (`badOffsets` mixes both error kinds: non-strings → TypeError, bad strings →
/// RangeError).
fn offset_field(obj: *const crate::object::ObjectHeader) -> Option<UtcOffset> {
    require_string_field(obj, "offset").map(|s| ok_or_throw(UtcOffset::from_utf8(s.as_bytes())))
}

/// Build a [`PartialZonedDateTime`] from a JS property bag for
/// `Temporal.ZonedDateTime.from`. Returns `None` if `v` is not a plain object.
pub fn zoned_partial(v: f64) -> Option<PartialZonedDateTime> {
    let obj = as_obj(v)?;
    // Spec order: `calendar` first (ToTemporalCalendarSlotValue — a null / number
    // / wrong-typed value is a TypeError, not a silent ISO default), then the
    // alphabetical field set (with `offset` and `timeZone` in position), then the
    // timeZone is resolved (a missing `timeZone` is a TypeError — it is required
    // for a ZonedDateTime bag).
    let calendar = calendar_slot(field(obj, "calendar"));
    let (cf, pt, offset, tz_raw) = read_zoned_bag_alpha(obj, &calendar);
    if is_undefined(tz_raw) {
        type_error(
            "ZonedDateTime property bag is missing a required \"timeZone\" field".to_string(),
        );
    }
    let mut p = PartialZonedDateTime::new();
    p.calendar = calendar;
    p.fields.calendar_fields = cf;
    p.fields.time = pt;
    p.fields.offset = offset;
    p.timezone = Some(timezone(tz_raw));
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
            // `ToTemporalTime` of an object with no recognized time fields is a
            // TypeError (`plaintime-propertybag-no-time-units`), NOT midnight.
            type_error(
                "Temporal.PlainTime-like object must have at least one time field".to_string(),
            );
        }
        return Some(ok_or_throw(midnight().with(pt, Some(Overflow::Constrain))));
    }
    // `ToTemporalTime` of a number / null / boolean / bigint / symbol is a
    // `TypeError` (only a Temporal time, ISO string, or property bag converts) —
    // `withPlainTime`'s `argument-number` / `argument-wrong-type` cases. A
    // missing/`undefined` arg is handled above (caller defaults to midnight).
    type_error("Cannot convert value to a Temporal.PlainTime".to_string())
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
    // `GetDirectionOption`: a String is used directly; otherwise the argument is
    // run through `GetOptionsObject`, which throws a **TypeError** for `undefined`
    // (the option is required) and for any non-object primitive (boolean, number,
    // bigint, symbol, null). Only a malformed *string* / object `direction` value
    // is a RangeError.
    let s = if JSValue::from_bits(v.to_bits()).is_string() {
        read_string(v)
    } else if is_undefined(v) {
        type_error("getTimeZoneTransition: direction option is required".to_string());
    } else if let Some(obj) = as_obj(v) {
        match str_field_coerce(obj, "direction") {
            Some(s) => s,
            None => range("getTimeZoneTransition requires a direction"),
        }
    } else {
        type_error(
            "getTimeZoneTransition: direction must be a string or an options object".to_string(),
        );
    };
    TransitionDirection::from_str(&s).unwrap_or_else(|_| range("Invalid transition direction"))
}
