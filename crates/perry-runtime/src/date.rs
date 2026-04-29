//! Date operations runtime support
//!
//! Provides JavaScript Date functionality using system time.
//! Dates are represented internally as i64 timestamps (milliseconds since Unix epoch).

use std::cell::RefCell;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

thread_local! {
    /// Bit patterns of f64 values that came from a `new Date(...)` constructor
    /// (or `Date.now()` / a Date method). `instanceof Date` consults this set
    /// — without it, every finite number would match because Perry stores
    /// Date as a raw f64 timestamp with no tag.
    ///
    /// False positives are possible (a regular number equal to a registered
    /// Date timestamp), but in practice the set is small and dominated by
    /// recently-minted Date timestamps.
    static DATE_REGISTRY: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
}

/// Mark `bits` as a Date so future `instanceof Date` checks can recognize it.
pub fn register_date_bits(bits: u64) {
    DATE_REGISTRY.with(|r| {
        r.borrow_mut().insert(bits);
    });
}

/// True when `bits` were previously registered as a Date.
pub fn is_registered_date_bits(bits: u64) -> bool {
    DATE_REGISTRY.with(|r| r.borrow().contains(&bits))
}

/// Convert a UTC timestamp (seconds) to local-time components.
/// Returns (year, month [1-12], day, hour, minute, second, tz_offset_seconds).
/// tz_offset_seconds is the number of seconds that need to be added to the
/// UTC timestamp to get the local-time representation (i.e. local - UTC).
#[cfg(unix)]
fn timestamp_to_local_components(secs: i64) -> (i32, u32, u32, u32, u32, u32, i64) {
    unsafe {
        let t: libc::time_t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        let res = libc::localtime_r(&t, &mut tm);
        if res.is_null() {
            let (y, m, d, h, mi, s) = timestamp_to_components(secs);
            return (y, m, d, h, mi, s, 0);
        }
        let year = tm.tm_year + 1900;
        let month = (tm.tm_mon + 1) as u32;
        let day = tm.tm_mday as u32;
        let hour = tm.tm_hour as u32;
        let minute = tm.tm_min as u32;
        let second = tm.tm_sec as u32;
        let tz_offset = tm.tm_gmtoff as i64;
        (year, month, day, hour, minute, second, tz_offset)
    }
}

#[cfg(windows)]
fn timestamp_to_local_components(secs: i64) -> (i32, u32, u32, u32, u32, u32, i64) {
    unsafe {
        let t: libc::time_t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        let err = libc::localtime_s(&mut tm, &t);
        if err != 0 {
            let (y, m, d, h, mi, s) = timestamp_to_components(secs);
            return (y, m, d, h, mi, s, 0);
        }
        let year = tm.tm_year + 1900;
        let month = (tm.tm_mon + 1) as u32;
        let day = tm.tm_mday as u32;
        let hour = tm.tm_hour as u32;
        let minute = tm.tm_min as u32;
        let second = tm.tm_sec as u32;
        let mut utm: libc::tm = std::mem::zeroed();
        let tz_offset = if libc::gmtime_s(&mut utm, &t) == 0 {
            let local_secs = components_to_timestamp(year, month, day, hour, minute, second);
            let utc_secs = components_to_timestamp(
                utm.tm_year + 1900,
                (utm.tm_mon + 1) as u32,
                utm.tm_mday as u32,
                utm.tm_hour as u32,
                utm.tm_min as u32,
                utm.tm_sec as u32,
            );
            local_secs - utc_secs
        } else {
            0
        };
        (year, month, day, hour, minute, second, tz_offset)
    }
}

/// Get current timestamp in milliseconds (Date.now())
#[no_mangle]
pub extern "C" fn js_date_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// performance.now() — high-resolution time in milliseconds (sub-ms precision).
/// Returns ms since UNIX_EPOCH as f64; the float retains microsecond resolution.
#[no_mangle]
pub extern "C" fn js_performance_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Create a new Date from current time, returning timestamp in milliseconds
#[no_mangle]
pub extern "C" fn js_date_new() -> f64 {
    let v = js_date_now();
    register_date_bits(v.to_bits());
    v
}

/// Create a new Date from a timestamp (milliseconds since epoch)
#[no_mangle]
pub extern "C" fn js_date_new_from_timestamp(timestamp: f64) -> f64 {
    register_date_bits(timestamp.to_bits());
    timestamp
}

/// Create a new Date from a value that could be a number or a NaN-boxed string.
/// Checks for STRING_TAG (0x7FFF) in the top 16 bits; if found, parses the string
/// as a date. Otherwise treats the value as a numeric timestamp.
#[no_mangle]
pub extern "C" fn js_date_new_from_value(value: f64) -> f64 {
    let bits = value.to_bits();
    let tag = (bits >> 48) & 0xFFFF;
    let result = if tag == 0x7FFF {
        // NaN-boxed string — extract pointer and parse
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        if ptr.is_null() || (ptr as usize) < 0x1000 {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    parse_date_string(s)
                } else {
                    f64::NAN
                }
            }
        }
    } else {
        // Numeric timestamp
        value
    };
    if !result.is_nan() {
        register_date_bits(result.to_bits());
    }
    result
}

/// Parse a date string into a millisecond timestamp.
/// Supports ISO 8601 and common formats:
///   "2024-01-15"
///   "2024-01-15T12:30:45"
///   "2024-01-15T12:30:45Z"
///   "2024-01-15T12:30:45.123Z"
///   "2024-01-15 12:30:45" (MySQL format)
///   "Jan 15, 2024"
///   Numeric strings (treated as timestamps)
fn parse_date_string(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return f64::NAN;
    }

    // Try as numeric timestamp first
    if let Ok(n) = s.parse::<f64>() {
        return n;
    }

    // Try ISO 8601 / MySQL datetime formats
    // "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM:SS" or "YYYY-MM-DD HH:MM:SS"
    if s.len() >= 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' {
        let year: i32 = match s[0..4].parse() {
            Ok(v) => v,
            Err(_) => return f64::NAN,
        };
        let month: u32 = match s[5..7].parse() {
            Ok(v) => v,
            Err(_) => return f64::NAN,
        };
        let day: u32 = match s[8..10].parse() {
            Ok(v) => v,
            Err(_) => return f64::NAN,
        };

        if month < 1 || month > 12 || day < 1 || day > 31 {
            return f64::NAN;
        }

        let mut hour: u32 = 0;
        let mut minute: u32 = 0;
        let mut second: u32 = 0;
        let mut millis: u32 = 0;

        // Parse time part if present (after T or space)
        let rest = &s[10..];
        if rest.len() >= 6 && (rest.starts_with('T') || rest.starts_with(' ')) {
            let time_str = &rest[1..];
            if time_str.len() >= 5 && time_str.as_bytes()[2] == b':' {
                hour = match time_str[0..2].parse() {
                    Ok(v) => v,
                    Err(_) => return f64::NAN,
                };
                minute = match time_str[3..5].parse() {
                    Ok(v) => v,
                    Err(_) => return f64::NAN,
                };
                if time_str.len() >= 8 && time_str.as_bytes()[5] == b':' {
                    second = match time_str[6..8].parse() {
                        Ok(v) => v,
                        Err(_) => return f64::NAN,
                    };
                    // Milliseconds after '.'
                    if time_str.len() >= 10 && time_str.as_bytes()[8] == b'.' {
                        let ms_end = time_str[9..]
                            .find(|c: char| !c.is_ascii_digit())
                            .unwrap_or(time_str.len() - 9);
                        let ms_str = &time_str[9..9 + ms_end];
                        millis = match ms_str.parse::<u32>() {
                            Ok(v) => {
                                // Normalize to 3 digits
                                match ms_str.len() {
                                    1 => v * 100,
                                    2 => v * 10,
                                    3 => v,
                                    _ => v / 10u32.pow(ms_str.len() as u32 - 3),
                                }
                            }
                            Err(_) => 0,
                        };
                    }
                }
            }
        }

        // Convert to timestamp using the same algorithm as timestamp_to_components (inverse)
        let ts = components_to_timestamp(year, month, day, hour, minute, second);
        return (ts * 1000 + millis as i64) as f64;
    }

    f64::NAN
}

/// Convert date components (UTC) to Unix timestamp in seconds.
/// Inverse of timestamp_to_components.
fn components_to_timestamp(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> i64 {
    // Howard Hinnant's civil_from_days (inverse of days_from_civil)
    let y = if month <= 2 {
        year as i64 - 1
    } else {
        year as i64
    };
    let m = if month <= 2 {
        month as i64 + 9
    } else {
        month as i64 - 3
    };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u64;
    let doy = (153 * m as u64 + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe as i64 - 719468;

    days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64
}

/// Get timestamp from Date (date.getTime())
/// Since we store dates as timestamps, this is an identity function
#[no_mangle]
pub extern "C" fn js_date_get_time(timestamp: f64) -> f64 {
    timestamp
}

/// Convert Date to ISO 8601 string (date.toISOString())
/// Returns a pointer to a StringHeader
#[no_mangle]
pub extern "C" fn js_date_to_iso_string(timestamp: f64) -> *mut crate::StringHeader {
    use std::alloc::{alloc, Layout};

    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let millis = (ts_ms % 1000).abs() as u32;

    // Calculate date components from Unix timestamp
    // This is a simplified implementation - proper implementation would use chrono crate
    let (year, month, day, hour, minute, second) = timestamp_to_components(secs);

    // Format as ISO 8601: YYYY-MM-DDTHH:mm:ss.sssZ
    let iso_string = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hour, minute, second, millis
    );

    crate::string::js_string_from_bytes(iso_string.as_ptr(), iso_string.len() as u32)
}

/// Get the full year (date.getFullYear()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_full_year(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, _, _, _, _, _, _) = timestamp_to_local_components(secs);
    year as f64
}

/// Get the month (0-11) (date.getMonth()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_month(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, month, _, _, _, _, _) = timestamp_to_local_components(secs);
    (month - 1) as f64 // JavaScript months are 0-indexed
}

/// Get the day of month (1-31) (date.getDate()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_date(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, day, _, _, _, _) = timestamp_to_local_components(secs);
    day as f64
}

/// Get the hour (0-23) (date.getHours()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_hours(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, hour, _, _, _) = timestamp_to_local_components(secs);
    hour as f64
}

/// Get the minutes (0-59) (date.getMinutes()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_minutes(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, minute, _, _) = timestamp_to_local_components(secs);
    minute as f64
}

/// Get the seconds (0-59) (date.getSeconds()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_seconds(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, _, second, _) = timestamp_to_local_components(secs);
    second as f64
}

/// Get the milliseconds (0-999) (date.getMilliseconds())
#[no_mangle]
pub extern "C" fn js_date_get_milliseconds(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    ts_ms.rem_euclid(1000) as f64
}

/// Get the day of week (0-6, Sunday=0) in LOCAL time (date.getDay()).
#[no_mangle]
pub extern "C" fn js_date_get_day(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(secs);
    // Compute weekday from local-equivalent seconds
    let local_secs = secs + tz_offset;
    weekday_from_timestamp(local_secs) as f64
}

// =====================================================================================
// v0.4.69 — Date method gap fill: parse, UTC, getUTC*, setUTC*, valueOf, toLocale*, etc.
// =====================================================================================

/// Compute the UTC day of week (0=Sunday, 6=Saturday) for a second-precision timestamp.
fn weekday_from_timestamp(secs: i64) -> u32 {
    // 1970-01-01 was a Thursday (day 4 in JS day-of-week semantics).
    let days = if secs >= 0 {
        secs / 86400
    } else {
        (secs - 86399) / 86400 // floor division for negatives
    };
    let dow = (days + 4).rem_euclid(7);
    dow as u32
}

/// Allocate a StringHeader pointer holding `s`.
fn alloc_runtime_string(s: &str) -> *mut crate::StringHeader {
    // Use the standard string allocator which sets both utf16_len and byte_len
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Date.parse(isoString) — parse an ISO 8601 string and return ms since epoch.
/// Returns NaN for invalid input.
#[no_mangle]
pub extern "C" fn js_date_parse(str_ptr: *const crate::StringHeader) -> f64 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return f64::NAN;
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(bytes) {
            Ok(s) => parse_date_string(s),
            Err(_) => f64::NAN,
        }
    }
}

/// Date.UTC(year, month, day, hour, minute, second, ms) — all f64.
/// month is 0-indexed (matches JS). Defaults: day=1, hour/min/sec/ms=0.
#[no_mangle]
pub extern "C" fn js_date_utc(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    millisecond: f64,
) -> f64 {
    let y = year as i32;
    // JS month is 0-based but components_to_timestamp expects 1-based
    let m = (month as i32 + 1) as u32;
    let d = day as u32;
    let h = hour as u32;
    let mi = minute as u32;
    let s = second as u32;
    let ms = millisecond as i64;
    let secs = components_to_timestamp(y, m, d, h, mi, s);
    (secs * 1000 + ms) as f64
}

// --- UTC getters: same impl as the regular getters since we store UTC internally ---

#[no_mangle]
pub extern "C" fn js_date_get_utc_day(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    weekday_from_timestamp(secs) as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_full_year(timestamp: f64) -> f64 {
    js_date_get_full_year(timestamp)
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_month(timestamp: f64) -> f64 {
    js_date_get_month(timestamp)
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_date(timestamp: f64) -> f64 {
    js_date_get_date(timestamp)
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_hours(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, hour, _, _) = timestamp_to_components(secs);
    hour as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_minutes(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, minute, _) = timestamp_to_components(secs);
    minute as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_seconds(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, _, second) = timestamp_to_components(secs);
    second as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_milliseconds(timestamp: f64) -> f64 {
    js_date_get_milliseconds(timestamp)
}

/// date.valueOf() — same as getTime(), returns ms timestamp.
#[no_mangle]
pub extern "C" fn js_date_value_of(timestamp: f64) -> f64 {
    timestamp
}

/// date.getTimezoneOffset() — returns the difference in minutes between
/// UTC and the local timezone at the given instant. Positive for locales
/// west of UTC, negative for those east (matches the JS/Node convention).
#[no_mangle]
pub extern "C" fn js_date_get_timezone_offset(timestamp: f64) -> f64 {
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, _, _, _, tz_offset_secs) = timestamp_to_local_components(secs);
    // tz_offset_secs is "seconds east of UTC" (positive for east).
    // JS getTimezoneOffset returns "minutes west of UTC" — opposite sign,
    // minute granularity.
    (-tz_offset_secs / 60) as f64
}

// --- UTC setters: rebuild the timestamp with one component replaced ---

fn rebuild_with(
    timestamp: f64,
    year: Option<i32>,
    month: Option<u32>,
    day: Option<u32>,
    hour: Option<u32>,
    minute: Option<u32>,
    second: Option<u32>,
    millisecond: Option<i64>,
) -> f64 {
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let cur_ms = ts_ms.rem_euclid(1000);
    let (cy, cm, cd, ch, cmi, cs) = timestamp_to_components(secs);
    let new_secs = components_to_timestamp(
        year.unwrap_or(cy),
        month.unwrap_or(cm),
        day.unwrap_or(cd),
        hour.unwrap_or(ch),
        minute.unwrap_or(cmi),
        second.unwrap_or(cs),
    );
    let new_ms = millisecond.unwrap_or(cur_ms);
    (new_secs * 1000 + new_ms) as f64
}

#[no_mangle]
pub extern "C" fn js_date_set_utc_full_year(timestamp: f64, value: f64) -> f64 {
    rebuild_with(
        timestamp,
        Some(value as i32),
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

#[no_mangle]
pub extern "C" fn js_date_set_utc_month(timestamp: f64, value: f64) -> f64 {
    // JS months are 0-based; components_to_timestamp wants 1-based.
    rebuild_with(
        timestamp,
        None,
        Some(value as u32 + 1),
        None,
        None,
        None,
        None,
        None,
    )
}

#[no_mangle]
pub extern "C" fn js_date_set_utc_date(timestamp: f64, value: f64) -> f64 {
    rebuild_with(
        timestamp,
        None,
        None,
        Some(value as u32),
        None,
        None,
        None,
        None,
    )
}

#[no_mangle]
pub extern "C" fn js_date_set_utc_hours(timestamp: f64, value: f64) -> f64 {
    rebuild_with(
        timestamp,
        None,
        None,
        None,
        Some(value as u32),
        None,
        None,
        None,
    )
}

#[no_mangle]
pub extern "C" fn js_date_set_utc_minutes(timestamp: f64, value: f64) -> f64 {
    rebuild_with(
        timestamp,
        None,
        None,
        None,
        None,
        Some(value as u32),
        None,
        None,
    )
}

#[no_mangle]
pub extern "C" fn js_date_set_utc_seconds(timestamp: f64, value: f64) -> f64 {
    rebuild_with(
        timestamp,
        None,
        None,
        None,
        None,
        None,
        Some(value as u32),
        None,
    )
}

#[no_mangle]
pub extern "C" fn js_date_set_utc_milliseconds(timestamp: f64, value: f64) -> f64 {
    rebuild_with(
        timestamp,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(value as i64),
    )
}

// --- String-returning Date methods ---

const WEEKDAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// date.toDateString() — e.g. "Mon Jan 15 2024" (local time).
#[no_mangle]
pub extern "C" fn js_date_to_date_string(timestamp: f64) -> *mut crate::StringHeader {
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, _, _, _, tz_offset) = timestamp_to_local_components(secs);
    let dow = weekday_from_timestamp(secs + tz_offset) as usize;
    let s = format!(
        "{} {} {:02} {:04}",
        WEEKDAY_NAMES[dow],
        MONTH_NAMES[(month - 1) as usize],
        day,
        year
    );
    alloc_runtime_string(&s)
}

/// date.toTimeString() — e.g. "12:30:45 GMT+0100 (local)" (local time).
#[no_mangle]
pub extern "C" fn js_date_to_time_string(timestamp: f64) -> *mut crate::StringHeader {
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, hour, minute, second, tz_offset) = timestamp_to_local_components(secs);
    let sign = if tz_offset >= 0 { '+' } else { '-' };
    let abs_off = tz_offset.abs();
    let off_h = abs_off / 3600;
    let off_m = (abs_off % 3600) / 60;
    let s = format!(
        "{:02}:{:02}:{:02} GMT{}{:02}{:02} (local)",
        hour, minute, second, sign, off_h, off_m
    );
    alloc_runtime_string(&s)
}

/// date.toLocaleDateString() — simple en-US-style date (local time).
#[no_mangle]
pub extern "C" fn js_date_to_locale_date_string(timestamp: f64) -> *mut crate::StringHeader {
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, _, _, _, _) = timestamp_to_local_components(secs);
    let s = format!("{}/{}/{}", month, day, year);
    alloc_runtime_string(&s)
}

/// date.toLocaleTimeString() — simple H:MM:SS AM/PM en-US style (local time).
#[no_mangle]
pub extern "C" fn js_date_to_locale_time_string(timestamp: f64) -> *mut crate::StringHeader {
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (_, _, _, hour, minute, second, _) = timestamp_to_local_components(secs);
    let (h12, suffix) = if hour == 0 {
        (12, "AM")
    } else if hour < 12 {
        (hour, "AM")
    } else if hour == 12 {
        (12, "PM")
    } else {
        (hour - 12, "PM")
    };
    let s = format!("{}:{:02}:{:02} {}", h12, minute, second, suffix);
    alloc_runtime_string(&s)
}

/// date.toLocaleString() — combined date and time (local time).
#[no_mangle]
pub extern "C" fn js_date_to_locale_string(timestamp: f64) -> *mut crate::StringHeader {
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, hour, minute, second, _) = timestamp_to_local_components(secs);
    let (h12, suffix) = if hour == 0 {
        (12, "AM")
    } else if hour < 12 {
        (hour, "AM")
    } else if hour == 12 {
        (12, "PM")
    } else {
        (hour - 12, "PM")
    };
    let s = format!(
        "{}/{}/{}, {}:{:02}:{:02} {}",
        month, day, year, h12, minute, second, suffix
    );
    alloc_runtime_string(&s)
}

/// date.toJSON() — same as toISOString() per ECMA-262.
#[no_mangle]
pub extern "C" fn js_date_to_json(timestamp: f64) -> *mut crate::StringHeader {
    js_date_to_iso_string(timestamp)
}

/// Convert Unix timestamp (seconds) to date components (year, month, day, hour, minute, second)
/// Returns components in UTC
pub fn timestamp_to_components(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Handle negative timestamps (dates before 1970)
    let is_negative = secs < 0;
    let abs_secs = if is_negative { -secs } else { secs } as u64;

    // Extract time of day
    let second = (abs_secs % 60) as u32;
    let minute = ((abs_secs / 60) % 60) as u32;
    let hour = ((abs_secs / 3600) % 24) as u32;

    // Calculate days from Unix epoch
    let mut days = if is_negative {
        -((abs_secs / 86400) as i64) - if abs_secs % 86400 != 0 { 1 } else { 0 }
    } else {
        (abs_secs / 86400) as i64
    };

    // For negative timestamps, adjust time components
    let (hour, minute, second) = if is_negative && abs_secs % 86400 != 0 {
        let remaining = abs_secs % 86400;
        let adjusted = 86400 - remaining;
        (
            ((adjusted / 3600) % 24) as u32,
            ((adjusted / 60) % 60) as u32,
            (adjusted % 60) as u32,
        )
    } else {
        (hour, minute, second)
    };

    // Days since 1970-01-01
    // Using a simplified algorithm based on Howard Hinnant's date algorithms
    let z = days + 719468; // Days from 0000-03-01 to 1970-01-01 is 719468

    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = (z - era * 146097) as u32; // Day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // Year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // Day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // Month proxy [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // Day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // Month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m, d, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_now() {
        let now = js_date_now();
        // Should be a reasonable timestamp (after 2020)
        assert!(now > 1577836800000.0); // 2020-01-01
    }

    #[test]
    fn test_timestamp_to_components() {
        // Test Unix epoch (1970-01-01 00:00:00 UTC)
        let (y, m, d, h, min, s) = timestamp_to_components(0);
        assert_eq!((y, m, d, h, min, s), (1970, 1, 1, 0, 0, 0));

        // Test 2024-01-15 12:30:45 UTC (timestamp: 1705321845)
        let (y, m, d, h, min, s) = timestamp_to_components(1705321845);
        assert_eq!((y, m, d, h, min, s), (2024, 1, 15, 12, 30, 45));
    }
}
