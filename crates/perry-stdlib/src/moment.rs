//! Moment module
//!
//! Native implementation of the 'moment' npm package using chrono.
//! Provides date/time manipulation with moment.js compatible API.

use crate::common::{get_handle, register_handle, Handle};
use chrono::{DateTime, Datelike, Duration, NaiveDateTime, TimeZone, Timelike, Utc};
use perry_runtime::{js_string_from_bytes, StringHeader};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Moment handle
pub struct MomentHandle {
    pub datetime: DateTime<Utc>,
    pub is_valid: bool,
}

/// Helper to convert handle to f64 for FFI.
///
/// Same scheme as dayjs: the JS-visible moment value is
/// `f64::from_bits(handle)` — a tiny denormal whose raw bits ARE the
/// handle. Instance methods receive the handle back as an i64 first
/// arg (the dispatch table's has_receiver unbox is bitcast+mask,
/// identity for small handles).
fn handle_to_f64(handle: Handle) -> f64 {
    f64::from_bits(handle as u64)
}

/// moment() -> Moment
///
/// Create a moment object for the current time.
#[no_mangle]
pub extern "C" fn js_moment_now() -> f64 {
    let handle = register_handle(MomentHandle {
        datetime: Utc::now(),
        is_valid: true,
    });
    handle_to_f64(handle)
}

/// moment(timestamp) -> Moment
///
/// Create a moment from a Unix timestamp in milliseconds.
#[no_mangle]
pub extern "C" fn js_moment_from_timestamp(timestamp_ms: f64) -> f64 {
    let secs = (timestamp_ms / 1000.0) as i64;
    let nanos = ((timestamp_ms % 1000.0) * 1_000_000.0) as u32;

    match DateTime::from_timestamp(secs, nanos) {
        Some(dt) => {
            let handle = register_handle(MomentHandle {
                datetime: dt,
                is_valid: true,
            });
            handle_to_f64(handle)
        }
        None => {
            let handle = register_handle(MomentHandle {
                datetime: Utc::now(),
                is_valid: false,
            });
            handle_to_f64(handle)
        }
    }
}

/// moment(string) -> Moment
///
/// Parse a date string.
#[no_mangle]
pub unsafe extern "C" fn js_moment_parse(date_str_ptr: *const StringHeader) -> f64 {
    let date_str = match string_from_header(date_str_ptr) {
        Some(s) => s,
        None => {
            let handle = register_handle(MomentHandle {
                datetime: Utc::now(),
                is_valid: false,
            });
            return handle_to_f64(handle);
        }
    };

    // Try parsing various formats. Note: the bare-date form must go
    // through NaiveDate (NaiveDateTime::parse_from_str("%Y-%m-%d")
    // always errors on the missing time part, which silently made
    // moment('2024-01-15') invalid).
    let datetime = date_str
        .parse::<DateTime<Utc>>()
        .or_else(|_| {
            NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%d %H:%M:%S%.f").map(|dt| dt.and_utc())
        })
        .or_else(|_| {
            NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%dT%H:%M:%S%.f").map(|dt| dt.and_utc())
        })
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
        });

    match datetime {
        Ok(dt) => {
            let handle = register_handle(MomentHandle {
                datetime: dt,
                is_valid: true,
            });
            handle_to_f64(handle)
        }
        Err(_) => {
            let handle = register_handle(MomentHandle {
                datetime: Utc::now(),
                is_valid: false,
            });
            handle_to_f64(handle)
        }
    }
}

/// moment(input?) -> Moment
///
/// The moment factory with its real argument surface: `value_bits` is
/// the raw NaN-boxed first argument (TAG_UNDEFINED when absent).
/// undefined → now; string → parse; number → epoch milliseconds.
#[no_mangle]
pub unsafe extern "C" fn js_moment_factory(value_bits: i64) -> f64 {
    use perry_runtime::JSValue;
    let bits = value_bits as u64;
    let jv = JSValue::from_bits(bits);
    if jv.is_any_string() {
        let ptr = perry_runtime::js_get_string_pointer_unified(f64::from_bits(bits))
            as *const StringHeader;
        return js_moment_parse(ptr);
    }
    if jv.is_int32() {
        return js_moment_from_timestamp(jv.as_int32() as f64);
    }
    if jv.is_number() {
        let n = f64::from_bits(bits);
        if n.is_finite() {
            return js_moment_from_timestamp(n);
        }
        // NaN / ±∞ → Invalid Date.
        return handle_to_f64(register_handle(MomentHandle {
            datetime: Utc::now(),
            is_valid: false,
        }));
    }
    if jv.is_undefined() {
        return js_moment_now();
    }
    // `null` (and any other non-undefined, non-number, non-string input)
    // → Invalid Date, matching moment(null); only undefined maps to now.
    handle_to_f64(register_handle(MomentHandle {
        datetime: Utc::now(),
        is_valid: false,
    }))
}

/// moment.format(formatString) -> string
#[no_mangle]
pub unsafe extern "C" fn js_moment_format(
    handle: i64,
    format_ptr: *const StringHeader,
) -> *mut StringHeader {
    let format_str =
        string_from_header(format_ptr).unwrap_or_else(|| "YYYY-MM-DDTHH:mm:ssZ".to_string());

    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        // Convert moment.js format to chrono format
        let chrono_format = format_str
            .replace("YYYY", "%Y")
            .replace("YY", "%y")
            .replace("MMMM", "%B")
            .replace("MMM", "%b")
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
            .replace("ZZ", "%z")
            .replace("Z", "%:z");

        let formatted = moment.datetime.format(&chrono_format).to_string();
        return js_string_from_bytes(formatted.as_ptr(), formatted.len() as u32);
    }

    std::ptr::null_mut()
}

/// moment.toISOString() -> string
#[no_mangle]
pub unsafe extern "C" fn js_moment_to_iso_string(handle: i64) -> *mut StringHeader {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        let iso = moment
            .datetime
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        return js_string_from_bytes(iso.as_ptr(), iso.len() as u32);
    }
    std::ptr::null_mut()
}

/// moment.valueOf() -> number (milliseconds since epoch)
#[no_mangle]
pub unsafe extern "C" fn js_moment_value_of(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return moment.datetime.timestamp_millis() as f64;
    }
    0.0
}

/// moment.unix() -> number (seconds since epoch)
#[no_mangle]
pub unsafe extern "C" fn js_moment_unix(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return moment.datetime.timestamp() as f64;
    }
    0.0
}

/// moment.year() -> number
#[no_mangle]
pub unsafe extern "C" fn js_moment_year(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return moment.datetime.year() as f64;
    }
    0.0
}

/// moment.month() -> number (0-11)
#[no_mangle]
pub unsafe extern "C" fn js_moment_month(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return (moment.datetime.month() - 1) as f64;
    }
    0.0
}

/// moment.date() -> number (1-31)
#[no_mangle]
pub unsafe extern "C" fn js_moment_date(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return moment.datetime.day() as f64;
    }
    0.0
}

/// moment.day() -> number (0-6, Sunday = 0)
#[no_mangle]
pub unsafe extern "C" fn js_moment_day(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return moment.datetime.weekday().num_days_from_sunday() as f64;
    }
    0.0
}

/// moment.hour() -> number
#[no_mangle]
pub unsafe extern "C" fn js_moment_hour(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return moment.datetime.hour() as f64;
    }
    0.0
}

/// moment.minute() -> number
#[no_mangle]
pub unsafe extern "C" fn js_moment_minute(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return moment.datetime.minute() as f64;
    }
    0.0
}

/// moment.second() -> number
#[no_mangle]
pub unsafe extern "C" fn js_moment_second(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return moment.datetime.second() as f64;
    }
    0.0
}

/// moment.millisecond() -> number
#[no_mangle]
pub unsafe extern "C" fn js_moment_millisecond(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return (moment.datetime.timestamp_subsec_millis()) as f64;
    }
    0.0
}

/// moment.add(amount, unit) -> Moment
#[no_mangle]
pub unsafe extern "C" fn js_moment_add(
    handle: i64,
    amount: f64,
    unit_ptr: *const StringHeader,
) -> f64 {
    let unit = string_from_header(unit_ptr).unwrap_or_else(|| "days".to_string());

    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        let amount = amount as i64;
        let duration = match unit.as_str() {
            "years" | "year" | "y" => Duration::days(amount * 365),
            "months" | "month" | "M" => Duration::days(amount * 30),
            "weeks" | "week" | "w" => Duration::weeks(amount),
            "days" | "day" | "d" => Duration::days(amount),
            "hours" | "hour" | "h" => Duration::hours(amount),
            "minutes" | "minute" | "m" => Duration::minutes(amount),
            "seconds" | "second" | "s" => Duration::seconds(amount),
            "milliseconds" | "millisecond" | "ms" => Duration::milliseconds(amount),
            _ => Duration::days(amount),
        };

        let new_datetime = moment.datetime + duration;
        let new_handle = register_handle(MomentHandle {
            datetime: new_datetime,
            is_valid: moment.is_valid,
        });
        return handle_to_f64(new_handle);
    }

    handle_to_f64(handle)
}

/// moment.subtract(amount, unit) -> Moment
#[no_mangle]
pub unsafe extern "C" fn js_moment_subtract(
    handle: i64,
    amount: f64,
    unit_ptr: *const StringHeader,
) -> f64 {
    js_moment_add(handle, -amount, unit_ptr)
}

/// moment.startOf(unit) -> Moment
#[no_mangle]
pub unsafe extern "C" fn js_moment_start_of(handle: i64, unit_ptr: *const StringHeader) -> f64 {
    let unit = string_from_header(unit_ptr).unwrap_or_else(|| "day".to_string());

    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        let dt = moment.datetime;
        let new_datetime = match unit.as_str() {
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
            _ => dt,
        };

        let new_handle = register_handle(MomentHandle {
            datetime: new_datetime,
            is_valid: moment.is_valid,
        });
        return handle_to_f64(new_handle);
    }

    handle_to_f64(handle)
}

/// moment.endOf(unit) -> Moment
#[no_mangle]
pub unsafe extern "C" fn js_moment_end_of(handle: i64, unit_ptr: *const StringHeader) -> f64 {
    let unit = string_from_header(unit_ptr).unwrap_or_else(|| "day".to_string());

    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        let dt = moment.datetime;
        let new_datetime = match unit.as_str() {
            "year" | "years" | "y" => Utc.with_ymd_and_hms(dt.year(), 12, 31, 23, 59, 59).unwrap(),
            "month" | "months" | "M" => {
                let last_day = NaiveDateTime::new(
                    chrono::NaiveDate::from_ymd_opt(dt.year(), dt.month() + 1, 1)
                        .unwrap_or_else(|| {
                            chrono::NaiveDate::from_ymd_opt(dt.year() + 1, 1, 1).unwrap()
                        })
                        .pred_opt()
                        .unwrap(),
                    chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
                );
                last_day.and_utc()
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
            _ => dt,
        };

        let new_handle = register_handle(MomentHandle {
            datetime: new_datetime,
            is_valid: moment.is_valid,
        });
        return handle_to_f64(new_handle);
    }

    handle_to_f64(handle)
}

/// moment.diff(other, unit) -> number
#[no_mangle]
pub unsafe extern "C" fn js_moment_diff(
    handle: i64,
    other_handle: i64,
    unit_ptr: *const StringHeader,
) -> f64 {
    let unit = string_from_header(unit_ptr).unwrap_or_else(|| "milliseconds".to_string());

    if let (Some(moment), Some(other)) = (
        get_handle::<MomentHandle>(handle),
        get_handle::<MomentHandle>(other_handle),
    ) {
        let diff = moment.datetime.signed_duration_since(other.datetime);

        return match unit.as_str() {
            "years" | "year" | "y" => diff.num_days() as f64 / 365.0,
            "months" | "month" | "M" => diff.num_days() as f64 / 30.0,
            "weeks" | "week" | "w" => diff.num_weeks() as f64,
            "days" | "day" | "d" => diff.num_days() as f64,
            "hours" | "hour" | "h" => diff.num_hours() as f64,
            "minutes" | "minute" | "m" => diff.num_minutes() as f64,
            "seconds" | "second" | "s" => diff.num_seconds() as f64,
            _ => diff.num_milliseconds() as f64,
        };
    }

    0.0
}

/// moment.isBefore(other) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_moment_is_before(handle: i64, other_handle: i64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    if let (Some(moment), Some(other)) = (
        get_handle::<MomentHandle>(handle),
        get_handle::<MomentHandle>(other_handle),
    ) {
        if moment.datetime < other.datetime {
            return f64::from_bits(TAG_TRUE);
        }
    }

    f64::from_bits(TAG_FALSE)
}

/// moment.isAfter(other) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_moment_is_after(handle: i64, other_handle: i64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    if let (Some(moment), Some(other)) = (
        get_handle::<MomentHandle>(handle),
        get_handle::<MomentHandle>(other_handle),
    ) {
        if moment.datetime > other.datetime {
            return f64::from_bits(TAG_TRUE);
        }
    }

    f64::from_bits(TAG_FALSE)
}

/// moment.isSame(other, unit?) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_moment_is_same(
    handle: i64,
    other_handle: i64,
    unit_ptr: *const StringHeader,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let unit = string_from_header(unit_ptr);

    if let (Some(moment), Some(other)) = (
        get_handle::<MomentHandle>(handle),
        get_handle::<MomentHandle>(other_handle),
    ) {
        let result = if let Some(unit) = unit {
            match unit.as_str() {
                "year" | "years" | "y" => moment.datetime.year() == other.datetime.year(),
                "month" | "months" | "M" => {
                    moment.datetime.year() == other.datetime.year()
                        && moment.datetime.month() == other.datetime.month()
                }
                "day" | "days" | "d" => {
                    moment.datetime.year() == other.datetime.year()
                        && moment.datetime.ordinal() == other.datetime.ordinal()
                }
                "hour" | "hours" | "h" => {
                    moment.datetime.year() == other.datetime.year()
                        && moment.datetime.ordinal() == other.datetime.ordinal()
                        && moment.datetime.hour() == other.datetime.hour()
                }
                "minute" | "minutes" | "m" => {
                    moment.datetime.year() == other.datetime.year()
                        && moment.datetime.ordinal() == other.datetime.ordinal()
                        && moment.datetime.hour() == other.datetime.hour()
                        && moment.datetime.minute() == other.datetime.minute()
                }
                _ => moment.datetime == other.datetime,
            }
        } else {
            moment.datetime == other.datetime
        };
        return if result {
            f64::from_bits(TAG_TRUE)
        } else {
            f64::from_bits(TAG_FALSE)
        };
    }

    f64::from_bits(TAG_FALSE)
}

/// moment.isBetween(start, end) -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_moment_is_between(
    handle: i64,
    start_handle: i64,
    end_handle: i64,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    if let (Some(moment), Some(start), Some(end)) = (
        get_handle::<MomentHandle>(handle),
        get_handle::<MomentHandle>(start_handle),
        get_handle::<MomentHandle>(end_handle),
    ) {
        if moment.datetime > start.datetime && moment.datetime < end.datetime {
            return f64::from_bits(TAG_TRUE);
        }
    }

    f64::from_bits(TAG_FALSE)
}

/// moment.isValid() -> boolean
#[no_mangle]
pub unsafe extern "C" fn js_moment_is_valid(handle: i64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        if moment.is_valid {
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

/// moment.clone() -> Moment
#[no_mangle]
pub unsafe extern "C" fn js_moment_clone(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        let new_handle = register_handle(MomentHandle {
            datetime: moment.datetime,
            is_valid: moment.is_valid,
        });
        return handle_to_f64(new_handle);
    }
    handle_to_f64(handle)
}

/// moment.fromNow() -> string (relative time)
#[no_mangle]
pub unsafe extern "C" fn js_moment_from_now(handle: i64) -> *mut StringHeader {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        let now = Utc::now();
        let diff = now.signed_duration_since(moment.datetime);
        let seconds = diff.num_seconds().abs();

        let result = if seconds < 60 {
            "a few seconds ago".to_string()
        } else if seconds < 3600 {
            let mins = seconds / 60;
            if mins == 1 {
                "a minute ago".to_string()
            } else {
                format!("{} minutes ago", mins)
            }
        } else if seconds < 86400 {
            let hours = seconds / 3600;
            if hours == 1 {
                "an hour ago".to_string()
            } else {
                format!("{} hours ago", hours)
            }
        } else if seconds < 2592000 {
            let days = seconds / 86400;
            if days == 1 {
                "a day ago".to_string()
            } else {
                format!("{} days ago", days)
            }
        } else if seconds < 31536000 {
            let months = seconds / 2592000;
            if months == 1 {
                "a month ago".to_string()
            } else {
                format!("{} months ago", months)
            }
        } else {
            let years = seconds / 31536000;
            if years == 1 {
                "a year ago".to_string()
            } else {
                format!("{} years ago", years)
            }
        };

        return js_string_from_bytes(result.as_ptr(), result.len() as u32);
    }

    std::ptr::null_mut()
}

/// moment.toDate() -> timestamp (for Date object creation)
#[no_mangle]
pub unsafe extern "C" fn js_moment_to_date(handle: i64) -> f64 {
    js_moment_value_of(handle)
}
