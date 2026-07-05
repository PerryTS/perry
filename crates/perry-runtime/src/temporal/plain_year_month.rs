//! `Temporal.PlainYearMonth` — wraps [`temporal_rs::PlainYearMonth`] (#4694).
//!
//! A calendar year + month (e.g. a billing period), no day/time/timezone.

use super::dispatch::{self, boolean, ok_or_throw, raw_arg, string, undefined};
use super::{
    alloc_temporal_cell, temporal_value_ref, temporal_value_ref_or_subclass, TemporalValue,
};
use crate::value::JSValue;
use temporal_rs::PlainYearMonth;

const TYPE_NAME: &str = "Temporal.PlainYearMonth";

fn wrap(ym: PlainYearMonth) -> f64 {
    alloc_temporal_cell(TemporalValue::PlainYearMonth(ym))
}

/// `new Temporal.PlainYearMonth(year, month, calendar?, referenceDay?)`.
pub fn construct(args: &[f64]) -> f64 {
    dispatch::require_construct(TYPE_NAME);
    // Spec order: `ToIntegerWithTruncation(year)`, `…(month)`,
    // `ToTemporalCalendarIdentifier(calendar)`, then the reference ISO day
    // (defaults to 1). Real `ToNumber` observes `valueOf` and rejects
    // `Infinity`/`NaN` at the field's read position.
    let year = dispatch::integer_with_truncation(raw_arg(args, 0));
    let month = dispatch::integer_with_truncation(raw_arg(args, 1));
    let cal = super::options::calendar_identifier(raw_arg(args, 2));
    let ref_day = {
        let raw = raw_arg(args, 3);
        if dispatch::is_undefined(raw) {
            None
        } else {
            Some(dispatch::field_u8(dispatch::integer_with_truncation(raw)))
        }
    };
    // `try_new` = overflow "reject": throw on an out-of-range month (e.g. 13)
    // rather than constraining it to December.
    wrap(ok_or_throw(PlainYearMonth::try_new(
        year.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        dispatch::field_u8(month),
        ref_day,
        cal,
    )))
}

/// `ToTemporalYearMonth(item)` for the no-options coercion paths
/// (`compare`/`until`/`since`/`equals`): a `Temporal.PlainYearMonth` (clone), an
/// ISO string, or a `{ year, month | monthCode, calendar, … }` property bag
/// (read in spec order with `constrain` overflow). No recognized field, a
/// Symbol, or any non-string primitive → TypeError.
fn coerce_ym(v: f64) -> PlainYearMonth {
    if let Some(TemporalValue::PlainYearMonth(ym)) = temporal_value_ref_or_subclass(v) {
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
        if !jv.as_pointer::<crate::object::ObjectHeader>().is_null() {
            return super::options::plain_year_month_from_bag(v);
        }
    }
    // Non-string, non-object primitive → TypeError per ToTemporalYearMonth.
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.PlainYearMonth")
}

pub fn from_static(args: &[f64]) -> f64 {
    // `ToTemporalYearMonth(item, options)`. As with `PlainDate.from`, the point
    // `options` is processed is observable: a string is parsed FIRST (invalid →
    // `RangeError`), a `PlainYearMonth` is cloned after `overflow` is read
    // (validating `options`), and a property bag reads its calendar + fields,
    // then `overflow` LAST.
    let item = raw_arg(args, 0);
    let opts = raw_arg(args, 1);
    if JSValue::from_bits(item.to_bits()).is_string() {
        let ym = ok_or_throw(dispatch::read_string(item).parse::<PlainYearMonth>());
        let _ = super::options::overflow(opts);
        return wrap(ym);
    }
    if let Some(TemporalValue::PlainYearMonth(ym)) = temporal_value_ref_or_subclass(item) {
        let _ = super::options::overflow(opts);
        return wrap(ym.clone());
    }
    // Any other object — including a non-PlainYearMonth Temporal value such as a
    // `PlainDate` — is read as a property bag (`ToTemporalYearMonth` treats it as
    // an object and reads its `year`/`month`/`monthCode` getters), NOT a TypeError.
    let jv = JSValue::from_bits(item.to_bits());
    if jv.is_pointer() && !crate::symbol::is_registered_symbol(jv.as_pointer::<u8>() as usize) {
        return wrap(super::options::plain_year_month_from_bag_opts(item, opts));
    }
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.PlainYearMonth")
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
        "toJSON" => string(&ym.to_string()),
        "toLocaleString" => {
            super::options::assert_locale_string_calendar_no_iso_carveout(
                ym.calendar().identifier(),
                raw_arg(args, 1),
            );
            let epoch_ms =
                crate::date::components_to_timestamp(ym.year(), ym.month() as u32, 1, 0, 0, 0)
                    as f64
                    * 1000.0;
            crate::intl::temporal_locale_string(
                epoch_ms,
                raw_arg(args, 0),
                raw_arg(args, 1),
                crate::intl::TemporalLocaleCtx::PlainYearMonth,
            )
        }
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "with" => {
            let obj = super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "with");
            let fields = super::options::with_year_month_fields(obj, ym.calendar());
            let overflow = super::options::overflow(raw_arg(args, 1));
            wrap(ok_or_throw(ym.with(fields, overflow)))
        }
        "toPlainDate" => {
            let obj =
                super::options::require_fields_obj(raw_arg(args, 0), TYPE_NAME, "toPlainDate");
            let day = super::options::to_plain_date_day_field(obj);
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
