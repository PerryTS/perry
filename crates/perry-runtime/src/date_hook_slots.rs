//! Concrete Date operations selected by the Date installer.
use crate::feature_hooks::Hook;
static ALLOC_DATE_CELL: Hook<fn(f64) -> f64> = Hook::empty();
pub fn alloc_date_cell(ts: f64) -> f64 {
    ALLOC_DATE_CELL.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(ts),
    )
}
static DATE_CELL_PROTOTYPE: Hook<fn(f64) -> f64> = Hook::empty();
pub fn date_cell_prototype(value: f64) -> f64 {
    DATE_CELL_PROTOTYPE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(value),
    )
}
static JS_DATE_APPLY_SETTER: Hook<extern "C" fn(f64, i32, i32, *const f64, i32) -> f64> =
    Hook::empty();
pub fn js_date_apply_setter(
    date: f64,
    is_utc: i32,
    field: i32,
    args_ptr: *const f64,
    argc: i32,
) -> f64 {
    JS_DATE_APPLY_SETTER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(date, is_utc, field, args_ptr, argc),
    )
}
static JS_DATE_COERCE_NUMBER: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_coerce_number(value: f64) -> f64 {
    JS_DATE_COERCE_NUMBER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(value),
    )
}
static JS_DATE_GET_DATE: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_date(timestamp: f64) -> f64 {
    JS_DATE_GET_DATE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_DAY: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_day(timestamp: f64) -> f64 {
    JS_DATE_GET_DAY.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_FULL_YEAR: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_full_year(timestamp: f64) -> f64 {
    JS_DATE_GET_FULL_YEAR.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_HOURS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_hours(timestamp: f64) -> f64 {
    JS_DATE_GET_HOURS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_MILLISECONDS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_milliseconds(timestamp: f64) -> f64 {
    JS_DATE_GET_MILLISECONDS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_MINUTES: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_minutes(timestamp: f64) -> f64 {
    JS_DATE_GET_MINUTES.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_MONTH: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_month(timestamp: f64) -> f64 {
    JS_DATE_GET_MONTH.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_SECONDS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_seconds(timestamp: f64) -> f64 {
    JS_DATE_GET_SECONDS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_TIME: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_time(timestamp: f64) -> f64 {
    JS_DATE_GET_TIME.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_TIMEZONE_OFFSET: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_timezone_offset(timestamp: f64) -> f64 {
    JS_DATE_GET_TIMEZONE_OFFSET.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_UTC_DATE: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_utc_date(timestamp: f64) -> f64 {
    JS_DATE_GET_UTC_DATE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_UTC_DAY: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_utc_day(timestamp: f64) -> f64 {
    JS_DATE_GET_UTC_DAY.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_UTC_FULL_YEAR: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_utc_full_year(timestamp: f64) -> f64 {
    JS_DATE_GET_UTC_FULL_YEAR.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_UTC_HOURS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_utc_hours(timestamp: f64) -> f64 {
    JS_DATE_GET_UTC_HOURS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_UTC_MILLISECONDS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_utc_milliseconds(timestamp: f64) -> f64 {
    JS_DATE_GET_UTC_MILLISECONDS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_UTC_MINUTES: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_utc_minutes(timestamp: f64) -> f64 {
    JS_DATE_GET_UTC_MINUTES.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_UTC_MONTH: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_utc_month(timestamp: f64) -> f64 {
    JS_DATE_GET_UTC_MONTH.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_GET_UTC_SECONDS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_get_utc_seconds(timestamp: f64) -> f64 {
    JS_DATE_GET_UTC_SECONDS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_NEW: Hook<extern "C" fn() -> f64> = Hook::empty();
pub fn js_date_new() -> f64 {
    JS_DATE_NEW
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_DATE_NEW_FROM_TIMESTAMP: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_new_from_timestamp(timestamp: f64) -> f64 {
    JS_DATE_NEW_FROM_TIMESTAMP.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(timestamp),
    )
}
static JS_DATE_NEW_FROM_VALUE: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub fn js_date_new_from_value(value: f64) -> f64 {
    JS_DATE_NEW_FROM_VALUE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(value),
    )
}
static JS_DATE_NEW_LOCAL_COMPONENTS: Hook<extern "C" fn(f64, f64, f64, f64, f64, f64, f64) -> f64> =
    Hook::empty();
pub fn js_date_new_local_components(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    millisecond: f64,
) -> f64 {
    JS_DATE_NEW_LOCAL_COMPONENTS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(year, month, day, hour, minute, second, millisecond),
    )
}
static JS_DATE_PARSE: Hook<extern "C" fn(*const crate::StringHeader) -> f64> = Hook::empty();
pub fn js_date_parse(str_ptr: *const crate::StringHeader) -> f64 {
    JS_DATE_PARSE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(str_ptr),
    )
}
static JS_DATE_TO_ISO_STRING: Hook<extern "C" fn(f64) -> *mut crate::StringHeader> = Hook::empty();
pub fn js_date_to_iso_string(timestamp: f64) -> *mut crate::StringHeader {
    JS_DATE_TO_ISO_STRING
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(timestamp))
}
static JS_DATE_TO_ISO_STRING_OR_THROW: Hook<extern "C" fn(f64) -> *mut crate::StringHeader> =
    Hook::empty();
pub fn js_date_to_iso_string_or_throw(timestamp: f64) -> *mut crate::StringHeader {
    JS_DATE_TO_ISO_STRING_OR_THROW
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(timestamp))
}
static JS_DATE_TO_LOCALE_STRING: Hook<extern "C" fn(f64) -> *mut crate::StringHeader> =
    Hook::empty();
pub fn js_date_to_locale_string(timestamp: f64) -> *mut crate::StringHeader {
    JS_DATE_TO_LOCALE_STRING
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(timestamp))
}
static JS_DATE_TO_STRING: Hook<extern "C" fn(f64) -> *mut crate::StringHeader> = Hook::empty();
pub fn js_date_to_string(timestamp: f64) -> *mut crate::StringHeader {
    JS_DATE_TO_STRING
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(timestamp))
}
static JS_DATE_TO_UTC_STRING: Hook<extern "C" fn(f64) -> *mut crate::StringHeader> = Hook::empty();
pub fn js_date_to_utc_string(timestamp: f64) -> *mut crate::StringHeader {
    JS_DATE_TO_UTC_STRING
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(timestamp))
}
static JS_DATE_UTC: Hook<extern "C" fn(*const f64, i32) -> f64> = Hook::empty();
pub fn js_date_utc(args_ptr: *const f64, argc: i32) -> f64 {
    JS_DATE_UTC.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(args_ptr, argc),
    )
}
pub(crate) fn install() {
    crate::perry_install_once!({
        ALLOC_DATE_CELL.set(crate::date::alloc_date_cell);
        DATE_CELL_PROTOTYPE.set(crate::date::date_cell_prototype);
        JS_DATE_APPLY_SETTER.set(crate::date::js_date_apply_setter);
        JS_DATE_COERCE_NUMBER.set(crate::date::js_date_coerce_number);
        JS_DATE_GET_DATE.set(crate::date::js_date_get_date);
        JS_DATE_GET_DAY.set(crate::date::js_date_get_day);
        JS_DATE_GET_FULL_YEAR.set(crate::date::js_date_get_full_year);
        JS_DATE_GET_HOURS.set(crate::date::js_date_get_hours);
        JS_DATE_GET_MILLISECONDS.set(crate::date::js_date_get_milliseconds);
        JS_DATE_GET_MINUTES.set(crate::date::js_date_get_minutes);
        JS_DATE_GET_MONTH.set(crate::date::js_date_get_month);
        JS_DATE_GET_SECONDS.set(crate::date::js_date_get_seconds);
        JS_DATE_GET_TIME.set(crate::date::js_date_get_time);
        JS_DATE_GET_TIMEZONE_OFFSET.set(crate::date::js_date_get_timezone_offset);
        JS_DATE_GET_UTC_DATE.set(crate::date::js_date_get_utc_date);
        JS_DATE_GET_UTC_DAY.set(crate::date::js_date_get_utc_day);
        JS_DATE_GET_UTC_FULL_YEAR.set(crate::date::js_date_get_utc_full_year);
        JS_DATE_GET_UTC_HOURS.set(crate::date::js_date_get_utc_hours);
        JS_DATE_GET_UTC_MILLISECONDS.set(crate::date::js_date_get_utc_milliseconds);
        JS_DATE_GET_UTC_MINUTES.set(crate::date::js_date_get_utc_minutes);
        JS_DATE_GET_UTC_MONTH.set(crate::date::js_date_get_utc_month);
        JS_DATE_GET_UTC_SECONDS.set(crate::date::js_date_get_utc_seconds);
        JS_DATE_NEW.set(crate::date::js_date_new);
        JS_DATE_NEW_FROM_TIMESTAMP.set(crate::date::js_date_new_from_timestamp);
        JS_DATE_NEW_FROM_VALUE.set(crate::date::js_date_new_from_value);
        JS_DATE_NEW_LOCAL_COMPONENTS.set(crate::date::js_date_new_local_components);
        JS_DATE_PARSE.set(crate::date::js_date_parse);
        JS_DATE_TO_ISO_STRING.set(crate::date::js_date_to_iso_string);
        JS_DATE_TO_ISO_STRING_OR_THROW.set(crate::date::js_date_to_iso_string_or_throw);
        JS_DATE_TO_LOCALE_STRING.set(crate::date::js_date_to_locale_string);
        JS_DATE_TO_STRING.set(crate::date::js_date_to_string);
        JS_DATE_TO_UTC_STRING.set(crate::date::js_date_to_utc_string);
        JS_DATE_UTC.set(crate::date::js_date_utc);
    });
}
