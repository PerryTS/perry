//! Dayjs/date-fns module
//!
//! Native implementation of dayjs and date-fns using chrono.
//! Provides date parsing, formatting, manipulation, and comparison.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use perry_runtime::{js_string_from_bytes, StringHeader};

use crate::common::{register_handle, Handle};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Wrapper around DateTime for handle storage
pub struct DayjsHandle {
    pub datetime: DateTime<Utc>,
}

impl DayjsHandle {
    pub fn new(dt: DateTime<Utc>) -> Self {
        Self { datetime: dt }
    }
}

/// Convert Handle to f64 for return
#[inline]
fn handle_to_f64(handle: Handle) -> f64 {
    f64::from_bits(handle as u64)
}

/// Convert f64 to Handle
#[inline]
fn f64_to_handle(val: f64) -> Handle {
    val.to_bits() as i64
}

/// dayjs() -> Dayjs
///
/// Create a dayjs object for the current time.
#[no_mangle]
pub extern "C" fn js_dayjs_now() -> f64 {
    let handle = register_handle(DayjsHandle::new(Utc::now()));
    handle_to_f64(handle)
}

/// dayjs(timestamp) -> Dayjs
///
/// Create a dayjs object from a Unix timestamp (milliseconds).
#[no_mangle]
pub extern "C" fn js_dayjs_from_timestamp(timestamp: f64) -> f64 {
    let secs = (timestamp / 1000.0) as i64;
    let nanos = ((timestamp % 1000.0) * 1_000_000.0) as u32;

    if let Some(dt) = DateTime::from_timestamp(secs, nanos) {
        let handle = register_handle(DayjsHandle::new(dt));
        handle_to_f64(handle)
    } else {
        0.0 // Invalid timestamp
    }
}

/// dayjs(dateString) -> Dayjs
///
/// Parse a date string (ISO 8601 format).
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_parse(date_str_ptr: *const StringHeader) -> f64 {
    let date_str = match string_from_header(date_str_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    // Try to parse as ISO 8601
    if let Ok(dt) = DateTime::parse_from_rfc3339(&date_str) {
        let handle = register_handle(DayjsHandle::new(dt.with_timezone(&Utc)));
        return handle_to_f64(handle);
    }

    // Try to parse as YYYY-MM-DD
    if let Ok(naive) = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
        let datetime = naive.and_hms_opt(0, 0, 0).unwrap();
        let dt = Utc.from_utc_datetime(&datetime);
        let handle = register_handle(DayjsHandle::new(dt));
        return handle_to_f64(handle);
    }

    // Try to parse as YYYY-MM-DD HH:MM:SS
    if let Ok(naive) = NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%d %H:%M:%S") {
        let dt = Utc.from_utc_datetime(&naive);
        let handle = register_handle(DayjsHandle::new(dt));
        return handle_to_f64(handle);
    }

    0.0 // Invalid date string
}

/// dayjs.format(pattern) -> string
///
/// Format a date according to the given pattern.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_format(
    handle: Handle,
    pattern_ptr: *const StringHeader,
) -> *mut StringHeader {
    use crate::common::get_handle;

    let pattern = string_from_header(pattern_ptr).unwrap_or_else(|| "YYYY-MM-DD".to_string());

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        let dt = &wrapper.datetime;

        // Convert dayjs format to chrono format
        let chrono_fmt = pattern
            .replace("YYYY", "%Y")
            .replace("YY", "%y")
            .replace("MM", "%m")
            .replace("DD", "%d")
            .replace("HH", "%H")
            .replace("hh", "%I")
            .replace("mm", "%M")
            .replace("ss", "%S")
            .replace("SSS", "%3f")
            .replace("A", "%p")
            .replace("a", "%P")
            .replace("dddd", "%A")
            .replace("ddd", "%a")
            .replace("MMMM", "%B")
            .replace("MMM", "%b")
            .replace("ZZ", "%z")
            .replace("Z", "%:z");

        let formatted = dt.format(&chrono_fmt).to_string();
        js_string_from_bytes(formatted.as_ptr(), formatted.len() as u32)
    } else {
        std::ptr::null_mut()
    }
}

/// dayjs.toISOString() -> string
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_to_iso_string(handle: Handle) -> *mut StringHeader {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        let iso = wrapper.datetime.to_rfc3339();
        js_string_from_bytes(iso.as_ptr(), iso.len() as u32)
    } else {
        std::ptr::null_mut()
    }
}

/// dayjs.valueOf() -> number (Unix timestamp in milliseconds)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_value_of(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        wrapper.datetime.timestamp_millis() as f64
    } else {
        0.0
    }
}

/// dayjs.unix() -> number (Unix timestamp in seconds)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_unix(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        wrapper.datetime.timestamp() as f64
    } else {
        0.0
    }
}

/// dayjs.year() -> number
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_year(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        wrapper.datetime.year() as f64
    } else {
        0.0
    }
}

/// dayjs.month() -> number (0-11)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_month(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        (wrapper.datetime.month() - 1) as f64 // 0-indexed like JS
    } else {
        0.0
    }
}

/// dayjs.date() -> number (1-31)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_date(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        wrapper.datetime.day() as f64
    } else {
        0.0
    }
}

/// dayjs.day() -> number (0-6, Sunday=0)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_day(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        wrapper.datetime.weekday().num_days_from_sunday() as f64
    } else {
        0.0
    }
}

/// dayjs.hour() -> number (0-23)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_hour(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        wrapper.datetime.hour() as f64
    } else {
        0.0
    }
}

/// dayjs.minute() -> number (0-59)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_minute(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        wrapper.datetime.minute() as f64
    } else {
        0.0
    }
}

/// dayjs.second() -> number (0-59)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_second(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        wrapper.datetime.second() as f64
    } else {
        0.0
    }
}

/// dayjs.millisecond() -> number (0-999)
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_millisecond(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        (wrapper.datetime.nanosecond() / 1_000_000) as f64
    } else {
        0.0
    }
}

/// dayjs.add(value, unit) -> Dayjs
///
/// Add time to a date. Unit can be: 'day', 'week', 'month', 'year', 'hour', 'minute', 'second'
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_add(
    handle: Handle,
    value: f64,
    unit_ptr: *const StringHeader,
) -> f64 {
    use crate::common::get_handle;

    let unit = string_from_header(unit_ptr).unwrap_or_else(|| "day".to_string());
    let value = value as i64;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        let dt = &wrapper.datetime;

        let new_dt = match unit.as_str() {
            "day" | "days" | "d" => *dt + Duration::days(value),
            "week" | "weeks" | "w" => *dt + Duration::weeks(value),
            "month" | "months" | "M" => {
                // Adding months is more complex
                let year = dt.year();
                let month = dt.month() as i32 + value as i32;
                let (new_year, new_month) = if month > 12 {
                    (year + (month - 1) / 12, ((month - 1) % 12) + 1)
                } else if month < 1 {
                    (year + (month - 12) / 12, 12 + (month % 12))
                } else {
                    (year, month)
                };
                dt.with_year(new_year)
                    .and_then(|d| d.with_month(new_month as u32))
                    .unwrap_or(*dt)
            }
            "year" | "years" | "y" => dt.with_year(dt.year() + value as i32).unwrap_or(*dt),
            "hour" | "hours" | "h" => *dt + Duration::hours(value),
            "minute" | "minutes" | "m" => *dt + Duration::minutes(value),
            "second" | "seconds" | "s" => *dt + Duration::seconds(value),
            "millisecond" | "milliseconds" | "ms" => *dt + Duration::milliseconds(value),
            _ => *dt,
        };

        let new_handle = register_handle(DayjsHandle::new(new_dt));
        handle_to_f64(new_handle)
    } else {
        0.0
    }
}

/// dayjs.subtract(value, unit) -> Dayjs
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_subtract(
    handle: Handle,
    value: f64,
    unit_ptr: *const StringHeader,
) -> f64 {
    js_dayjs_add(handle, -value, unit_ptr)
}

/// dayjs.startOf(unit) -> Dayjs
///
/// Set to the start of a unit of time.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_start_of(handle: Handle, unit_ptr: *const StringHeader) -> f64 {
    use crate::common::get_handle;

    let unit = string_from_header(unit_ptr).unwrap_or_else(|| "day".to_string());

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        let dt = &wrapper.datetime;

        let new_dt = match unit.as_str() {
            "year" | "years" | "y" => Utc.with_ymd_and_hms(dt.year(), 1, 1, 0, 0, 0).unwrap(),
            "month" | "months" | "M" => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
                .unwrap(),
            "day" | "days" | "d" => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
                .unwrap(),
            "hour" | "hours" | "h" => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), dt.hour(), 0, 0)
                .unwrap(),
            "minute" | "minutes" | "m" => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute(), 0)
                .unwrap(),
            _ => *dt,
        };

        let new_handle = register_handle(DayjsHandle::new(new_dt));
        handle_to_f64(new_handle)
    } else {
        0.0
    }
}

/// dayjs.endOf(unit) -> Dayjs
///
/// Set to the end of a unit of time.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_end_of(handle: Handle, unit_ptr: *const StringHeader) -> f64 {
    use crate::common::get_handle;

    let unit = string_from_header(unit_ptr).unwrap_or_else(|| "day".to_string());

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        let dt = &wrapper.datetime;

        let new_dt = match unit.as_str() {
            "year" | "years" | "y" => Utc.with_ymd_and_hms(dt.year(), 12, 31, 23, 59, 59).unwrap(),
            "month" | "months" | "M" => {
                let last_day = NaiveDate::from_ymd_opt(dt.year(), dt.month() + 1, 1)
                    .unwrap_or_else(|| NaiveDate::from_ymd_opt(dt.year() + 1, 1, 1).unwrap())
                    .pred_opt()
                    .unwrap()
                    .day();
                Utc.with_ymd_and_hms(dt.year(), dt.month(), last_day, 23, 59, 59)
                    .unwrap()
            }
            "day" | "days" | "d" => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 23, 59, 59)
                .unwrap(),
            "hour" | "hours" | "h" => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), dt.hour(), 59, 59)
                .unwrap(),
            "minute" | "minutes" | "m" => Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute(), 59)
                .unwrap(),
            _ => *dt,
        };

        let new_handle = register_handle(DayjsHandle::new(new_dt));
        handle_to_f64(new_handle)
    } else {
        0.0
    }
}

/// dayjs.diff(other, unit) -> number
///
/// Get the difference between two dates.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_diff(
    handle: Handle,
    other_handle: Handle,
    unit_ptr: *const StringHeader,
) -> f64 {
    use crate::common::get_handle;

    let unit = string_from_header(unit_ptr).unwrap_or_else(|| "millisecond".to_string());

    let wrapper1 = get_handle::<DayjsHandle>(handle);
    let wrapper2 = get_handle::<DayjsHandle>(other_handle);

    if let (Some(w1), Some(w2)) = (wrapper1, wrapper2) {
        let diff = w1.datetime.signed_duration_since(w2.datetime);

        match unit.as_str() {
            "year" | "years" | "y" => (diff.num_days() / 365) as f64,
            "month" | "months" | "M" => (diff.num_days() / 30) as f64,
            "week" | "weeks" | "w" => diff.num_weeks() as f64,
            "day" | "days" | "d" => diff.num_days() as f64,
            "hour" | "hours" | "h" => diff.num_hours() as f64,
            "minute" | "minutes" | "m" => diff.num_minutes() as f64,
            "second" | "seconds" | "s" => diff.num_seconds() as f64,
            "millisecond" | "milliseconds" | "ms" | _ => diff.num_milliseconds() as f64,
        }
    } else {
        0.0
    }
}

/// dayjs.isBefore(other) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_is_before(handle: Handle, other_handle: Handle) -> f64 {
    use crate::common::get_handle;

    let wrapper1 = get_handle::<DayjsHandle>(handle);
    let wrapper2 = get_handle::<DayjsHandle>(other_handle);

    if let (Some(w1), Some(w2)) = (wrapper1, wrapper2) {
        if w1.datetime < w2.datetime {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// dayjs.isAfter(other) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_is_after(handle: Handle, other_handle: Handle) -> f64 {
    use crate::common::get_handle;

    let wrapper1 = get_handle::<DayjsHandle>(handle);
    let wrapper2 = get_handle::<DayjsHandle>(other_handle);

    if let (Some(w1), Some(w2)) = (wrapper1, wrapper2) {
        if w1.datetime > w2.datetime {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// dayjs.isSame(other) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_is_same(handle: Handle, other_handle: Handle) -> f64 {
    use crate::common::get_handle;

    let wrapper1 = get_handle::<DayjsHandle>(handle);
    let wrapper2 = get_handle::<DayjsHandle>(other_handle);

    if let (Some(w1), Some(w2)) = (wrapper1, wrapper2) {
        if w1.datetime == w2.datetime {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// dayjs.isValid() -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_is_valid(handle: Handle) -> f64 {
    use crate::common::get_handle;

    if get_handle::<DayjsHandle>(handle).is_some() {
        1.0
    } else {
        0.0
    }
}

// ============ date-fns compatible functions ============

/// format(date, formatStr) -> string (date-fns compatible)
#[no_mangle]
pub unsafe extern "C" fn js_datefns_format(
    timestamp: f64,
    pattern_ptr: *const StringHeader,
) -> *mut StringHeader {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return std::ptr::null_mut();
    }
    js_dayjs_format(f64_to_handle(handle_f64), pattern_ptr)
}

/// parseISO(dateString) -> timestamp (date-fns compatible)
#[no_mangle]
pub unsafe extern "C" fn js_datefns_parse_iso(date_str_ptr: *const StringHeader) -> f64 {
    let handle_f64 = js_dayjs_parse(date_str_ptr);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(handle_f64))
}

/// addDays(date, amount) -> timestamp (date-fns compatible)
#[no_mangle]
pub unsafe extern "C" fn js_datefns_add_days(timestamp: f64, amount: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = "day";
    let unit_ptr = js_string_from_bytes(unit.as_ptr(), unit.len() as u32);
    let new_handle_f64 = js_dayjs_add(f64_to_handle(handle_f64), amount, unit_ptr);
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

/// addMonths(date, amount) -> timestamp (date-fns compatible)
#[no_mangle]
pub unsafe extern "C" fn js_datefns_add_months(timestamp: f64, amount: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = "month";
    let unit_ptr = js_string_from_bytes(unit.as_ptr(), unit.len() as u32);
    let new_handle_f64 = js_dayjs_add(f64_to_handle(handle_f64), amount, unit_ptr);
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

/// addYears(date, amount) -> timestamp (date-fns compatible)
#[no_mangle]
pub unsafe extern "C" fn js_datefns_add_years(timestamp: f64, amount: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = "year";
    let unit_ptr = js_string_from_bytes(unit.as_ptr(), unit.len() as u32);
    let new_handle_f64 = js_dayjs_add(f64_to_handle(handle_f64), amount, unit_ptr);
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

/// differenceInDays(dateLeft, dateRight) -> number (date-fns compatible)
#[no_mangle]
pub extern "C" fn js_datefns_difference_in_days(timestamp_left: f64, timestamp_right: f64) -> f64 {
    let diff_ms = timestamp_left - timestamp_right;
    (diff_ms / (1000.0 * 60.0 * 60.0 * 24.0)).floor()
}

/// differenceInHours(dateLeft, dateRight) -> number (date-fns compatible)
#[no_mangle]
pub extern "C" fn js_datefns_difference_in_hours(timestamp_left: f64, timestamp_right: f64) -> f64 {
    let diff_ms = timestamp_left - timestamp_right;
    (diff_ms / (1000.0 * 60.0 * 60.0)).floor()
}

/// differenceInMinutes(dateLeft, dateRight) -> number (date-fns compatible)
#[no_mangle]
pub extern "C" fn js_datefns_difference_in_minutes(
    timestamp_left: f64,
    timestamp_right: f64,
) -> f64 {
    let diff_ms = timestamp_left - timestamp_right;
    (diff_ms / (1000.0 * 60.0)).floor()
}

/// isAfter(date, dateToCompare) -> boolean (date-fns compatible)
#[no_mangle]
pub extern "C" fn js_datefns_is_after(timestamp: f64, compare_timestamp: f64) -> f64 {
    if timestamp > compare_timestamp {
        1.0
    } else {
        0.0
    }
}

/// isBefore(date, dateToCompare) -> boolean (date-fns compatible)
#[no_mangle]
pub extern "C" fn js_datefns_is_before(timestamp: f64, compare_timestamp: f64) -> f64 {
    if timestamp < compare_timestamp {
        1.0
    } else {
        0.0
    }
}

/// startOfDay(date) -> timestamp (date-fns compatible)
#[no_mangle]
pub unsafe extern "C" fn js_datefns_start_of_day(timestamp: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = "day";
    let unit_ptr = js_string_from_bytes(unit.as_ptr(), unit.len() as u32);
    let new_handle_f64 = js_dayjs_start_of(f64_to_handle(handle_f64), unit_ptr);
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

/// endOfDay(date) -> timestamp (date-fns compatible)
#[no_mangle]
pub unsafe extern "C" fn js_datefns_end_of_day(timestamp: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = "day";
    let unit_ptr = js_string_from_bytes(unit.as_ptr(), unit.len() as u32);
    let new_handle_f64 = js_dayjs_end_of(f64_to_handle(handle_f64), unit_ptr);
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}
