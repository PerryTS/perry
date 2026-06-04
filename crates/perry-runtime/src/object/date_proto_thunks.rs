//! `Date.prototype` getter thunks with a `this` brand check.
//!
//! The instance fast path (`d.getFullYear()`) is lowered directly by codegen:
//! it reads the `DateCell` timestamp and calls the `js_date_get_*` helpers, so
//! it never touches these thunks. But `Date.prototype.getFullYear` is also a
//! plain value — `Date.prototype.getFullYear.call(x)`, `Reflect.apply`, method
//! extraction — and on that reflective path the methods were installed as
//! generic no-op thunks, so `getDate.call(realDate)` returned `[object Object]`
//! and `getDate.call(nonDate)` silently produced garbage instead of throwing.
//!
//! Per spec each `Date.prototype` getter performs `thisTimeValue(this)`, which
//! throws a `TypeError` when `this` is not an Object with a `[[DateValue]]`
//! slot. These thunks read the `IMPLICIT_THIS` receiver (set by the
//! `.call`/`.apply` dispatch), brand-check it via `is_date_value`, throw on
//! mismatch, and otherwise dispatch to the SAME `js_date_get_*` helper the
//! instance path uses — so reflective Date getter calls now also *work*.
//!
//! Installed onto `Date.prototype` by
//! `global_this::populate_builtin_prototype_methods` (after the no-op block, so
//! these real thunks overwrite the no-op getters; setters / `toX` formatters
//! stay on the no-op path for now).

use super::*;

/// Resolve the `IMPLICIT_THIS` receiver to a Date time value, or throw a
/// `TypeError` (`thisTimeValue` brand check) when it is not a Date.
fn require_date_timestamp() -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if crate::date::is_date_value(this) {
        crate::date::date_cell_timestamp(this)
    } else {
        super::object_ops::throw_object_type_error(b"this is not a Date object.")
    }
}

macro_rules! date_getter_thunk {
    ($name:ident, $rt:path) => {
        extern "C" fn $name(_closure: *const crate::closure::ClosureHeader) -> f64 {
            $rt(require_date_timestamp())
        }
    };
}

date_getter_thunk!(date_get_time, crate::date::js_date_get_time);
date_getter_thunk!(date_get_full_year, crate::date::js_date_get_full_year);
date_getter_thunk!(date_get_month, crate::date::js_date_get_month);
date_getter_thunk!(date_get_date, crate::date::js_date_get_date);
date_getter_thunk!(date_get_hours, crate::date::js_date_get_hours);
date_getter_thunk!(date_get_minutes, crate::date::js_date_get_minutes);
date_getter_thunk!(date_get_seconds, crate::date::js_date_get_seconds);
date_getter_thunk!(date_get_milliseconds, crate::date::js_date_get_milliseconds);
date_getter_thunk!(date_get_day, crate::date::js_date_get_day);
date_getter_thunk!(
    date_get_utc_full_year,
    crate::date::js_date_get_utc_full_year
);
date_getter_thunk!(date_get_utc_month, crate::date::js_date_get_utc_month);
date_getter_thunk!(date_get_utc_date, crate::date::js_date_get_utc_date);
date_getter_thunk!(date_get_utc_hours, crate::date::js_date_get_utc_hours);
date_getter_thunk!(date_get_utc_minutes, crate::date::js_date_get_utc_minutes);
date_getter_thunk!(date_get_utc_seconds, crate::date::js_date_get_utc_seconds);
date_getter_thunk!(
    date_get_utc_milliseconds,
    crate::date::js_date_get_utc_milliseconds
);
date_getter_thunk!(date_get_utc_day, crate::date::js_date_get_utc_day);
date_getter_thunk!(
    date_get_timezone_offset,
    crate::date::js_date_get_timezone_offset
);

/// Legacy `Date.prototype.getYear` — `getFullYear() - 1900` (NaN-preserving).
extern "C" fn date_get_year(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let fy = crate::date::js_date_get_full_year(require_date_timestamp());
    if fy.is_nan() {
        fy
    } else {
        fy - 1900.0
    }
}

/// Install the brand-checked `Date.prototype` getter thunks. Called from
/// `global_this::populate_builtin_prototype_methods`'s `"Date"` arm AFTER the
/// no-op block, so these overwrite the no-op getter entries.
pub(crate) fn install_date_proto_getters(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    let methods: &[(&str, *const u8)] = &[
        ("getTime", date_get_time as *const u8),
        ("valueOf", date_get_time as *const u8),
        ("getFullYear", date_get_full_year as *const u8),
        ("getMonth", date_get_month as *const u8),
        ("getDate", date_get_date as *const u8),
        ("getHours", date_get_hours as *const u8),
        ("getMinutes", date_get_minutes as *const u8),
        ("getSeconds", date_get_seconds as *const u8),
        ("getMilliseconds", date_get_milliseconds as *const u8),
        ("getDay", date_get_day as *const u8),
        ("getUTCFullYear", date_get_utc_full_year as *const u8),
        ("getUTCMonth", date_get_utc_month as *const u8),
        ("getUTCDate", date_get_utc_date as *const u8),
        ("getUTCHours", date_get_utc_hours as *const u8),
        ("getUTCMinutes", date_get_utc_minutes as *const u8),
        ("getUTCSeconds", date_get_utc_seconds as *const u8),
        ("getUTCMilliseconds", date_get_utc_milliseconds as *const u8),
        ("getUTCDay", date_get_utc_day as *const u8),
        ("getTimezoneOffset", date_get_timezone_offset as *const u8),
        ("getYear", date_get_year as *const u8),
    ];
    for (name, ptr) in methods.iter().copied() {
        super::global_this::install_proto_method(proto_obj, name, ptr, 0);
    }
}
