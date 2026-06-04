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

// --- Setters ---------------------------------------------------------------
//
// Like the getters, the instance fast path (`d.setHours(...)`) is lowered by
// codegen straight to `js_date_apply_setter`. But `Date.prototype.setHours` is
// also a plain value (`.call`/`.apply`, method extraction, the legacy
// `setYear`/`setUTC*` family with no codegen fast path), and on that reflective
// path the methods were generic no-ops: `setDate.call(realDate, 1)` no-op'd and
// `setDate.call(nonDate, 1)` silently produced garbage instead of throwing.
//
// Each setter performs `thisTimeValue(this)` (TypeError if `this` is not a
// Date) and is variadic, so these thunks read the `IMPLICIT_THIS` receiver,
// brand-check it, collect the `rest` arguments, and dispatch to the same
// `js_date_apply_setter` the instance path uses. `js_date_apply_setter` reads
// `[[DateValue]]` BEFORE coercing the arguments, so the read-before-ToNumber
// ordering holds on the reflective path too.

/// Resolve the `IMPLICIT_THIS` receiver to a Date value, or throw a `TypeError`
/// (`thisTimeValue` brand check) when it is not a Date. Returns the NaN-boxed
/// `DateCell` value (the setter dispatch needs the receiver itself, not just
/// its timestamp, so it can mutate the cell in place).
fn require_date_this() -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if crate::date::is_date_value(this) {
        this
    } else {
        super::object_ops::throw_object_type_error(b"this is not a Date object.")
    }
}

/// `field`/`is_utc` selectors mirror `crate::date::js_date_apply_setter`:
/// 0=FullYear 1=Month 2=Date 3=Hours 4=Minutes 5=Seconds 6=Milliseconds
/// 7=Time, plus 8=setYear (annexB; local-only). `is_utc != 0` picks the UTC
/// rebuild.
macro_rules! date_setter_thunk {
    ($name:ident, $is_utc:expr, $field:expr) => {
        extern "C" fn $name(_closure: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
            let this = require_date_this();
            let args = super::global_this::global_this_rest_array_values(rest);
            crate::date::js_date_apply_setter(
                this,
                $is_utc,
                $field,
                args.as_ptr(),
                args.len() as i32,
            )
        }
    };
}

date_setter_thunk!(date_set_time, 0, 7);
date_setter_thunk!(date_set_full_year, 0, 0);
date_setter_thunk!(date_set_month, 0, 1);
date_setter_thunk!(date_set_date, 0, 2);
date_setter_thunk!(date_set_hours, 0, 3);
date_setter_thunk!(date_set_minutes, 0, 4);
date_setter_thunk!(date_set_seconds, 0, 5);
date_setter_thunk!(date_set_milliseconds, 0, 6);
date_setter_thunk!(date_set_year, 0, 8);
date_setter_thunk!(date_set_utc_full_year, 1, 0);
date_setter_thunk!(date_set_utc_month, 1, 1);
date_setter_thunk!(date_set_utc_date, 1, 2);
date_setter_thunk!(date_set_utc_hours, 1, 3);
date_setter_thunk!(date_set_utc_minutes, 1, 4);
date_setter_thunk!(date_set_utc_seconds, 1, 5);
date_setter_thunk!(date_set_utc_milliseconds, 1, 6);

/// Install the brand-checked `Date.prototype` setter thunks. Called from
/// `global_this::populate_builtin_prototype_methods`'s `"Date"` arm AFTER the
/// no-op block, so these overwrite the no-op setter entries. Each is installed
/// as a variadic (`rest`) method so the optional trailing components arrive in
/// the `rest` array; `spec_length` is the ECMAScript `.length`.
pub(crate) fn install_date_proto_setters(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    // (name, func_ptr, spec `.length`)
    let methods: &[(&str, *const u8, u32)] = &[
        ("setTime", date_set_time as *const u8, 1),
        ("setFullYear", date_set_full_year as *const u8, 3),
        ("setMonth", date_set_month as *const u8, 2),
        ("setDate", date_set_date as *const u8, 1),
        ("setHours", date_set_hours as *const u8, 4),
        ("setMinutes", date_set_minutes as *const u8, 3),
        ("setSeconds", date_set_seconds as *const u8, 2),
        ("setMilliseconds", date_set_milliseconds as *const u8, 1),
        ("setYear", date_set_year as *const u8, 1),
        ("setUTCFullYear", date_set_utc_full_year as *const u8, 3),
        ("setUTCMonth", date_set_utc_month as *const u8, 2),
        ("setUTCDate", date_set_utc_date as *const u8, 1),
        ("setUTCHours", date_set_utc_hours as *const u8, 4),
        ("setUTCMinutes", date_set_utc_minutes as *const u8, 3),
        ("setUTCSeconds", date_set_utc_seconds as *const u8, 2),
        (
            "setUTCMilliseconds",
            date_set_utc_milliseconds as *const u8,
            1,
        ),
    ];
    for (name, ptr, length) in methods.iter().copied() {
        // call_fixed_arity = 0: every argument arrives in the `rest` array, so
        // one thunk shape covers the 0..=4-arg setters uniformly.
        super::global_this::install_proto_method_rest_with_length(proto_obj, name, ptr, length, 0);
    }
}
