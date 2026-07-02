//! `Temporal.PlainMonthDay` — wraps [`temporal_rs::PlainMonthDay`] (#4694).
//!
//! A calendar month + day with no year (e.g. a recurring birthday/holiday).

use super::dispatch::{self, ok_or_throw, raw_arg, string};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::Overflow;
use temporal_rs::PlainMonthDay;

const TYPE_NAME: &str = "Temporal.PlainMonthDay";

fn wrap(md: PlainMonthDay) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainMonthDay(md))
}

/// `new Temporal.PlainMonthDay(month, day, calendar?, referenceYear?)`.
pub fn construct(args: &[f64]) -> f64 {
    dispatch::require_construct(TYPE_NAME);
    // Spec order: `ToIntegerWithTruncation(month)`, `…(day)`,
    // `ToTemporalCalendarIdentifier(calendar)`, then the reference ISO year
    // (defaults to 1972). Real `ToNumber` observes `valueOf` and rejects
    // `Infinity`/`NaN` at the field's read position.
    let month = dispatch::integer_with_truncation(raw_arg(args, 0));
    let day = dispatch::integer_with_truncation(raw_arg(args, 1));
    let cal = super::options::calendar_identifier(raw_arg(args, 2));
    let ref_year = {
        let raw = raw_arg(args, 3);
        if dispatch::is_undefined(raw) {
            None
        } else {
            Some(
                dispatch::integer_with_truncation(raw).clamp(i32::MIN as i64, i32::MAX as i64)
                    as i32,
            )
        }
    };
    // Overflow "reject": the constructor throws on an invalid month/day (e.g.
    // Feb 30) instead of constraining it to Feb 29. The `.from()` fields path
    // (`coerce_md`) keeps the spec's "constrain" default.
    wrap(ok_or_throw(PlainMonthDay::new_with_overflow(
        dispatch::field_u8(month),
        dispatch::field_u8(day),
        cal,
        Overflow::Reject,
        ref_year,
    )))
}

/// `ToTemporalMonthDay(item)` for the no-options `equals` coercion path: a
/// `Temporal.PlainMonthDay` (clone), an ISO string, or a property bag (read in
/// spec order with `constrain` overflow).
fn coerce_md(v: f64) -> PlainMonthDay {
    if let Some(TemporalValue::PlainMonthDay(md)) = temporal_value_ref(v) {
        return md.clone();
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(dispatch::read_string(v).parse::<PlainMonthDay>());
    }
    if jv.is_pointer() {
        if crate::symbol::is_registered_symbol(jv.as_pointer::<u8>() as usize) {
            crate::object::throw_object_type_error(
                b"Cannot convert a Symbol to a Temporal.PlainMonthDay",
            );
        }
        if !jv.as_pointer::<crate::object::ObjectHeader>().is_null() {
            return super::options::plain_month_day_from_bag(v);
        }
    }
    // Non-string, non-object primitive → TypeError per ToTemporalMonthDay.
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.PlainMonthDay")
}

pub fn from_static(args: &[f64]) -> f64 {
    // `ToTemporalMonthDay(item, options)`: a string is parsed FIRST (invalid →
    // `RangeError`), a `PlainMonthDay` is cloned after `overflow` is read
    // (validating `options`), and a property bag reads its calendar + fields,
    // then `overflow` LAST. (Previously this dropped `options` entirely, so a
    // wrong-typed `options`, `overflow: "reject"`, or a bad overflow string
    // never threw.)
    let item = raw_arg(args, 0);
    let opts = raw_arg(args, 1);
    if JSValue::from_bits(item.to_bits()).is_string() {
        let md = ok_or_throw(dispatch::read_string(item).parse::<PlainMonthDay>());
        let _ = super::options::overflow(opts);
        return wrap(md);
    }
    if let Some(TemporalValue::PlainMonthDay(md)) = temporal_value_ref(item) {
        let _ = super::options::overflow(opts);
        return wrap(md.clone());
    }
    // Any other object — including a non-PlainMonthDay Temporal value such as a
    // `PlainDate` — is read as a property bag (`ToTemporalMonthDay` reads its
    // `month`/`monthCode`/`day` getters), NOT a TypeError.
    let jv = JSValue::from_bits(item.to_bits());
    if jv.is_pointer() && !crate::symbol::is_registered_symbol(jv.as_pointer::<u8>() as usize) {
        return wrap(super::options::plain_month_day_from_bag_opts(item, opts));
    }
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.PlainMonthDay")
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
            // Two `PlainMonthDay`s are equal iff their full ISO date —
            // `[[ISOYear]]` (the reference year), `[[ISOMonth]]`/`[[ISODay]]`, and
            // calendar — all match. The reference year IS observable here: two
            // month-days that differ only in reference year are NOT equal.
            dispatch::boolean(
                md.day() == other.day()
                    && md.month_code() == other.month_code()
                    && md.reference_year() == other.reference_year()
                    && md.calendar_id() == other.calendar_id(),
            )
        }
        // `toString` honors `{ calendarName }`; `toJSON`/`toLocaleString` use the
        // default ("auto") calendar display.
        "toString" => {
            string(&md.to_ixdtf_string(super::options::display_calendar(raw_arg(args, 0))))
        }
        "toJSON" => string(&md.to_string()),
        "toLocaleString" => {
            super::options::assert_locale_string_calendar(md.calendar().identifier());
            let epoch_ms = crate::date::components_to_timestamp(
                1970,
                md.month_code().to_month_integer() as u32,
                md.day() as u32,
                0,
                0,
                0,
            ) as f64
                * 1000.0;
            crate::intl::temporal_locale_string(
                epoch_ms,
                raw_arg(args, 0),
                raw_arg(args, 1),
                crate::intl::TemporalLocaleCtx::PlainMonthDay,
            )
        }
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let fields = super::options::with_calendar_fields(obj, md.calendar());
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(md.with(fields, overflow)))
        }
        "toPlainDate" => {
            let obj =
                super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "toPlainDate");
            let year = super::options::to_plain_date_year_field(obj, md.calendar());
            alloc_temporal_cell(TemporalValue::PlainDate(ok_or_throw(
                md.to_plain_date(Some(year)),
            )))
        }
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
