//! Date operations runtime support
//!
//! Provides JavaScript Date functionality using system time.
//!
//! A `Date` is a **reference type**: a NaN-boxed pointer to a 1-slot mutable
//! [`DateCell`] holding the millisecond timestamp. This gives JS-correct
//! aliasing semantics — a setter mutation made through any binding (alias,
//! function parameter, closure capture) is visible through all of them
//! (#2089). Getters dereference the cell; setters mutate it in place.

use std::time::{SystemTime, UNIX_EPOCH};

const NANBOX_PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// A `Date` is a **reference type**: a NaN-boxed POINTER (POINTER_TAG) to
/// this 1-slot mutable heap cell, *not* a raw f64 timestamp. Aliases
/// (`const b = a`), function parameters, and closure captures all share
/// the same cell, so an in-place setter mutation is visible through every
/// binding — this is what fixes #2089 (effect `DateTime.add`). The cell is
/// arena-allocated as `GC_TYPE_DATE_CELL`: non-movable (a NaN-boxed pointer
/// living in a plain f64/DOUBLE local is kept alive by the conservative
/// stack scan, and the stable address means that un-shadow-rooted pointer
/// never goes stale across a GC) and pointer-free (`ts` is a raw IEEE
/// double, so no write barrier is needed when a setter mutates it).
#[repr(C)]
pub struct DateCell {
    pub ts: f64,
}

/// Allocate a fresh Date cell holding `ts` and return it as a NaN-boxed
/// pointer (an f64 carrying POINTER_TAG). `ts` may be NaN — that is an
/// *Invalid Date*, still a real Date object for `typeof` / `instanceof`
/// (the cell pointer is what makes it a Date, independent of its time
/// value), so unlike the old value-type representation there is no need
/// for a separate NaN sentinel bit pattern.
pub fn alloc_date_cell(ts: f64) -> f64 {
    unsafe {
        let ptr = crate::arena::arena_alloc_gc(
            std::mem::size_of::<DateCell>(),
            8,
            crate::gc::GC_TYPE_DATE_CELL,
        ) as *mut DateCell;
        (*ptr).ts = ts;
        f64::from_bits(crate::value::JSValue::pointer(ptr as *const u8).bits())
    }
}

/// The canonical Invalid Date value — a fresh cell whose time value is NaN.
#[inline]
pub fn date_invalid() -> f64 {
    alloc_date_cell(f64::NAN)
}

/// True if `addr` (a cleaned heap address, NOT NaN-boxed bits) points at a
/// `DateCell`. Reads the `GcHeader.obj_type`: for any live `is_pointer()`
/// value the header is always present and valid, and because `DateCell` is
/// non-movable its address is stable, so this is an exact identity check
/// with no side-table registry to keep in sync.
#[inline]
pub fn is_date_cell_addr(addr: usize) -> bool {
    if addr < 0x1000 + crate::gc::GC_HEADER_SIZE {
        return false;
    }
    unsafe {
        let header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*header).obj_type == crate::gc::GC_TYPE_DATE_CELL
    }
}

/// True if `value` is a Date — i.e. a NaN-boxed pointer to a `DateCell`.
/// Replaces the old value-keyed `is_registered_date_bits`.
#[inline]
pub fn is_date_value(value: f64) -> bool {
    let bits = value.to_bits();
    if !crate::value::JSValue::from_bits(bits).is_pointer() {
        return false;
    }
    is_date_cell_addr((bits & NANBOX_PTR_MASK) as usize)
}

/// Read the timestamp out of a Date value. If `value` is a `DateCell`
/// pointer, dereference it; otherwise (a raw numeric timestamp handed to a
/// Date method directly — e.g. via `Date.UTC()` or a legacy path) pass it
/// through unchanged. Every public Date getter/formatter funnels its
/// receiver through this so it works whether it was given a cell or a bare
/// number.
#[inline]
pub fn date_cell_timestamp(value: f64) -> f64 {
    let bits = value.to_bits();
    if crate::value::JSValue::from_bits(bits).is_pointer() {
        let addr = (bits & NANBOX_PTR_MASK) as usize;
        if is_date_cell_addr(addr) {
            return unsafe { (*(addr as *const DateCell)).ts };
        }
    }
    value
}

/// Mutate the `DateCell` `value` points at, writing `ts` into it. No-op if
/// `value` is not a cell (e.g. a raw timestamp). Returns `ts` — the numeric
/// millisecond value a JS Date setter evaluates to. `DateCell` is
/// pointer-free, so this raw f64 store needs no GC write barrier.
#[inline]
fn date_cell_store(value: f64, ts: f64) -> f64 {
    let bits = value.to_bits();
    if crate::value::JSValue::from_bits(bits).is_pointer() {
        let addr = (bits & NANBOX_PTR_MASK) as usize;
        if is_date_cell_addr(addr) {
            unsafe {
                (*(addr as *mut DateCell)).ts = ts;
            }
        }
    }
    ts
}

/// The string every Date string-method returns when the time value is
/// NaN. Matches `String(new Date(NaN))` / `new Date(NaN).toDateString()`
/// etc. Without this, the formatters cast NaN to `0i64` and emit a bogus
/// `1970-01-01…` string (the `<garbage>` reported in issue #748).
fn invalid_date_string() -> *mut crate::StringHeader {
    let s = "Invalid Date";
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

fn throw_invalid_time_value() -> ! {
    let msg = "Invalid time value";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_rangeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Coerce a value to a number for an ordered relational comparison
/// (`date1 < date2`, `date < ms`, …). A Date is a NaN-boxed `DateCell`
/// pointer whose raw bits are a NaN — bare `fcmp` would compare unordered
/// (always false) — so codegen routes non-statically-numeric relational
/// operands through here to dereference the timestamp first. Plain numbers
/// pass straight through.
#[no_mangle]
pub extern "C" fn js_date_coerce_number(value: f64) -> f64 {
    date_cell_timestamp(value)
}

/// Back-compat shim for the old value-keyed identity check. Date is now a
/// reference type, so identity is "the value is a pointer to a `DateCell`".
/// Kept so external callers (e.g. `perry-stdlib`'s querystring) need only a
/// trivial update. Prefer [`is_date_value`].
#[inline]
pub fn is_registered_date_bits(bits: u64) -> bool {
    is_date_value(f64::from_bits(bits))
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
        let tz_offset = tm.tm_gmtoff;
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

/// Create a new Date from current time, returning a NaN-boxed DateCell pointer.
#[no_mangle]
pub extern "C" fn js_date_new() -> f64 {
    alloc_date_cell(js_date_now())
}

/// Create a new Date from a timestamp (milliseconds since epoch).
/// A NaN timestamp produces a recognizable Invalid Date (a cell with NaN ts).
#[no_mangle]
pub extern "C" fn js_date_new_from_timestamp(timestamp: f64) -> f64 {
    alloc_date_cell(timestamp)
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
    } else if is_date_value(value) {
        // `new Date(anotherDate)` copies the source's time value (and would
        // otherwise read the pointer bits as a bogus timestamp).
        date_cell_timestamp(value)
    } else {
        // Numeric timestamp
        value
    };
    alloc_date_cell(result)
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

        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
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
    let timestamp = date_cell_timestamp(timestamp);
    timestamp
}

/// Convert Date to ISO 8601 string (date.toISOString())
/// Returns a pointer to a StringHeader
#[no_mangle]
pub extern "C" fn js_date_to_iso_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let millis = (ts_ms % 1000).unsigned_abs() as u32;

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

/// Date.prototype.toISOString() — unlike the shared ISO formatter used by
/// JSON internals, the public method must throw RangeError for Invalid Date.
#[no_mangle]
pub extern "C" fn js_date_to_iso_string_or_throw(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        throw_invalid_time_value();
    }
    js_date_to_iso_string(timestamp)
}

/// Get the full year (date.getFullYear()) in LOCAL time.
#[no_mangle]
pub extern "C" fn js_date_get_full_year(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    ts_ms.rem_euclid(1000) as f64
}

/// Get the day of week (0-6, Sunday=0) in LOCAL time (date.getDay()).
#[no_mangle]
pub extern "C" fn js_date_get_day(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
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

/// `new Date(year, month, day?, hour?, minute?, second?, ms?)` — local time.
///
/// JS semantics: month is 0-indexed, defaults are day=1, hour/min/sec/ms=0,
/// year < 100 is rebased to 1900+year. Arguments arrive as NaN-boxed f64
/// values — strings (which dayjs's parseDate uses, capturing regex groups
/// `r[1] = "2024"` etc.) need to be coerced via the string→number path so
/// that the `(year, month, ...)` form is taken instead of falling back to
/// the single-arg "timestamp" form. Without this, dayjs's `format()` ends
/// up reading garbage out of a `getTime()` like `1.9e+214` and reports the
/// year as `292278994`.
///
/// Returns a NaN-boxed `DateCell` pointer; `instanceof Date` recognizes it
/// by the cell's `GcHeader` type.
#[no_mangle]
pub extern "C" fn js_date_new_local_components(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    millisecond: f64,
) -> f64 {
    fn coerce(v: f64, default: f64) -> f64 {
        let bits = v.to_bits();
        let tag = (bits >> 48) & 0xFFFF;
        if tag == 0x7FFF {
            // NaN-boxed string — parse via the same path Number(str) uses.
            let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
            if ptr.is_null() || (ptr as usize) < 0x1000 {
                return f64::NAN;
            }
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                match std::str::from_utf8(bytes) {
                    Ok(s) => {
                        let s = s.trim();
                        if s.is_empty() {
                            default
                        } else {
                            s.parse::<f64>().unwrap_or(f64::NAN)
                        }
                    }
                    Err(_) => f64::NAN,
                }
            }
        } else if tag == 0x7FFC && (bits & 0xFF) == 0x01 {
            // undefined — for required args this is NaN, for optional ones default
            default
        } else {
            v
        }
    }
    let y = coerce(year, f64::NAN);
    let m = coerce(month, f64::NAN);
    let d = coerce(day, 1.0);
    let h = coerce(hour, 0.0);
    let mi = coerce(minute, 0.0);
    let s = coerce(second, 0.0);
    let ms = coerce(millisecond, 0.0);
    if y.is_nan()
        || m.is_nan()
        || d.is_nan()
        || h.is_nan()
        || mi.is_nan()
        || s.is_nan()
        || ms.is_nan()
    {
        return alloc_date_cell(f64::NAN);
    }
    let mut year_i = y as i32;
    // ECMA-262: if 0 <= year < 100, year = 1900 + year.
    if (0..100).contains(&year_i) {
        year_i += 1900;
    }
    let month_i = m as i32;
    let day_u = d as u32;
    let hour_u = h as u32;
    let minute_u = mi as u32;
    let second_u = s as u32;
    let ms_i = ms as i64;
    // JS month is 0-based; components_to_timestamp wants 1-based. Months
    // outside 1..=12 normalize (e.g. month=12 → next year January) via
    // year/month rollover, mirroring the way JS resolves out-of-range
    // components.
    let total_months = year_i as i64 * 12 + month_i as i64;
    let rolled_year = total_months.div_euclid(12) as i32;
    let rolled_month = (total_months.rem_euclid(12) + 1) as u32;
    let local_secs =
        components_to_timestamp(rolled_year, rolled_month, day_u, hour_u, minute_u, second_u);
    // Reinterpret the local-clock components as UTC by subtracting the
    // local tz offset at that instant. We find the offset by asking
    // localtime_r for the local components of the bare `local_secs`
    // (treated as a UTC epoch); the resulting tz_offset is the delta.
    let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(local_secs);
    let utc_secs = local_secs - tz_offset;
    alloc_date_cell((utc_secs * 1000 + ms_i) as f64)
}

// --- UTC getters: same impl as the regular getters since we store UTC internally ---

#[no_mangle]
pub extern "C" fn js_date_get_utc_day(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return f64::NAN;
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    weekday_from_timestamp(secs) as f64
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_full_year(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    js_date_get_full_year(timestamp)
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_month(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    js_date_get_month(timestamp)
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_date(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    js_date_get_date(timestamp)
}

#[no_mangle]
pub extern "C" fn js_date_get_utc_hours(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
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
    let timestamp = date_cell_timestamp(timestamp);
    js_date_get_milliseconds(timestamp)
}

/// date.valueOf() — same as getTime(), returns ms timestamp.
#[no_mangle]
pub extern "C" fn js_date_value_of(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
    timestamp
}

/// date.getTimezoneOffset() — returns the difference in minutes between
/// UTC and the local timezone at the given instant. Positive for locales
/// west of UTC, negative for those east (matches the JS/Node convention).
#[no_mangle]
pub extern "C" fn js_date_get_timezone_offset(timestamp: f64) -> f64 {
    let timestamp = date_cell_timestamp(timestamp);
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
//
// `date` is the receiver — a NaN-boxed DateCell pointer. We dereference its
// current time value, rebuild, then write the new value back INTO the same
// cell so the mutation is visible through every alias/param/closure that
// holds this Date (#2089). The numeric ms is returned (what a JS setter
// evaluates to).

fn rebuild_with(
    date: f64,
    year: Option<i32>,
    month: Option<u32>,
    day: Option<u32>,
    hour: Option<u32>,
    minute: Option<u32>,
    second: Option<u32>,
    millisecond: Option<i64>,
) -> f64 {
    // A NaN time value coerces to 0 via `as i64`, matching the prior
    // value-type behavior (e.g. `setUTCFullYear` reviving an Invalid Date
    // from the epoch per ECMA-262 §21.4.4.40's "if t is NaN, set t to +0").
    let timestamp = date_cell_timestamp(date);
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
    date_cell_store(date, (new_secs * 1000 + new_ms) as f64)
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

// --- Local-time setters (#1187) ---
//
// `setHours` / `setDate` / `setMonth` / `setFullYear` / etc. interpret their
// argument in the running process's *local* timezone, so the rebuild has to
// round-trip through `timestamp_to_local_components`. The local-clock
// components get reassembled with the requested component swapped, then we
// subtract the tz offset at that instant to land back at a true UTC epoch.
// Mirrors the conversion in `js_date_new_local_components`. NaN passes
// through untouched so an Invalid Date stays an Invalid Date.

fn rebuild_local_with(
    date: f64,
    year: Option<i32>,
    month: Option<u32>,
    day: Option<u32>,
    hour: Option<u32>,
    minute: Option<u32>,
    second: Option<u32>,
    millisecond: Option<i64>,
) -> f64 {
    let timestamp = date_cell_timestamp(date);
    if timestamp.is_nan() {
        // Setting a component on an Invalid Date leaves it invalid.
        return date_cell_store(date, timestamp);
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let cur_ms = ts_ms.rem_euclid(1000);
    let (cy, cm, cd, ch, cmi, cs, _) = timestamp_to_local_components(secs);
    let new_year = year.unwrap_or(cy);
    let new_month = month.unwrap_or(cm);
    let new_day = day.unwrap_or(cd);
    let new_hour = hour.unwrap_or(ch);
    let new_minute = minute.unwrap_or(cmi);
    let new_second = second.unwrap_or(cs);
    let new_ms = millisecond.unwrap_or(cur_ms);
    let local_secs = components_to_timestamp(
        new_year, new_month, new_day, new_hour, new_minute, new_second,
    );
    let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(local_secs);
    let utc_secs = local_secs - tz_offset;
    // Write the rebuilt value back into the receiver's cell and return the
    // numeric ms (what a JS Date setter evaluates to).
    date_cell_store(date, (utc_secs * 1000 + new_ms) as f64)
}

#[no_mangle]
pub extern "C" fn js_date_set_full_year(timestamp: f64, value: f64) -> f64 {
    rebuild_local_with(
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
pub extern "C" fn js_date_set_month(timestamp: f64, value: f64) -> f64 {
    // JS months are 0-based; components_to_timestamp wants 1-based.
    rebuild_local_with(
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
pub extern "C" fn js_date_set_date(timestamp: f64, value: f64) -> f64 {
    rebuild_local_with(
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
pub extern "C" fn js_date_set_hours(timestamp: f64, value: f64) -> f64 {
    rebuild_local_with(
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
pub extern "C" fn js_date_set_minutes(timestamp: f64, value: f64) -> f64 {
    rebuild_local_with(
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
pub extern "C" fn js_date_set_seconds(timestamp: f64, value: f64) -> f64 {
    rebuild_local_with(
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
pub extern "C" fn js_date_set_milliseconds(timestamp: f64, value: f64) -> f64 {
    rebuild_local_with(
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

/// `date.setTime(ms)` — replace the entire time value in place. A NaN `value`
/// makes the receiver an Invalid Date. Returns the numeric ms.
#[no_mangle]
pub extern "C" fn js_date_set_time(date: f64, value: f64) -> f64 {
    date_cell_store(date, value)
}

// --- String-returning Date methods ---

const WEEKDAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `date.toString()` / `String(date)` / `` `${date}` `` — the full local
/// date+time string, e.g. "Mon Jan 15 2024 12:30:45 GMT+0100 (local)", or
/// "Invalid Date". #2089: Date is a reference type now, so the generic
/// object-to-string path would otherwise print "[object Object]" (the old
/// value-type rep printed the bare millisecond number — also non-conformant).
/// The timezone long-name isn't reproduced (Perry has no tz database), so this
/// is close to but not byte-identical with Node for valid dates; Invalid Date
/// matches exactly.
#[no_mangle]
pub extern "C" fn js_date_to_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, hour, minute, second, tz_offset) = timestamp_to_local_components(secs);
    let dow = weekday_from_timestamp(secs + tz_offset) as usize;
    let sign = if tz_offset >= 0 { '+' } else { '-' };
    let abs_off = tz_offset.abs();
    let off_h = abs_off / 3600;
    let off_m = (abs_off % 3600) / 60;
    let s = format!(
        "{} {} {:02} {:04} {:02}:{:02}:{:02} GMT{}{:02}{:02} (local)",
        WEEKDAY_NAMES[dow],
        MONTH_NAMES[(month - 1) as usize],
        day,
        year,
        hour,
        minute,
        second,
        sign,
        off_h,
        off_m
    );
    alloc_runtime_string(&s)
}

/// date.toDateString() — e.g. "Mon Jan 15 2024" (local time).
#[no_mangle]
pub extern "C" fn js_date_to_date_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
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
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
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
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
    let ts_ms = timestamp as i64;
    let secs = ts_ms.div_euclid(1000);
    let (year, month, day, _, _, _, _) = timestamp_to_local_components(secs);
    let s = format!("{}/{}/{}", month, day, year);
    alloc_runtime_string(&s)
}

/// date.toLocaleTimeString() — simple H:MM:SS AM/PM en-US style (local time).
#[no_mangle]
pub extern "C" fn js_date_to_locale_time_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
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

/// `(n).toLocaleString()` for numeric receivers (#600). Formats with
/// thousands separators in en-US — `(12345).toLocaleString() === "12,345"`.
/// The HIR lowers `n.toLocaleString()` on any receiver to
/// `Expr::DateToLocaleString(n)` (the variant predates numeric
/// support), so the LLVM codegen routes statically-Number receivers
/// here and Date receivers to `js_date_to_locale_string` below. Decimal
/// part follows JS spec: trailing zeros after the decimal stripped,
/// leading sign preserved, NaN/Infinity passed through as the literal
/// strings "NaN" / "Infinity" / "-Infinity".
#[no_mangle]
pub extern "C" fn js_number_to_locale_string(n: f64) -> *mut crate::StringHeader {
    if n.is_nan() {
        return alloc_runtime_string("NaN");
    }
    if n.is_infinite() {
        return alloc_runtime_string(if n > 0.0 { "Infinity" } else { "-Infinity" });
    }
    let negative = n < 0.0;
    let abs = n.abs();
    // Split into integer and decimal parts. JS's default
    // `Number.prototype.toLocaleString()` shows up to 3 fraction digits
    // (Intl.NumberFormat default `maximumFractionDigits`), trailing
    // zeros stripped.
    let int_part = abs.trunc() as u64;
    let frac = abs - abs.trunc();
    // Format integer part with comma every 3 digits (en-US).
    let int_str = int_part.to_string();
    let mut grouped = String::new();
    let bytes = int_str.as_bytes();
    let len = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        let from_end = len - i;
        grouped.push(b as char);
        if from_end > 1 && from_end % 3 == 1 {
            grouped.push(',');
        }
    }
    let mut out = if negative {
        format!("-{}", grouped)
    } else {
        grouped
    };
    if frac != 0.0 {
        // 3 fraction digits, trailing zeros stripped.
        let frac_str = format!("{:.3}", frac);
        // frac_str = "0.xxx" — keep "xxx", strip trailing zeros.
        let frac_digits = frac_str.trim_start_matches("0.").trim_end_matches('0');
        if !frac_digits.is_empty() {
            out.push('.');
            out.push_str(frac_digits);
        }
    }
    alloc_runtime_string(&out)
}

/// date.toLocaleString() — combined date and time (local time).
#[no_mangle]
pub extern "C" fn js_date_to_locale_string(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return invalid_date_string();
    }
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

/// date.toJSON() — null for Invalid Date, otherwise the ISO string.
#[no_mangle]
pub extern "C" fn js_date_to_json(timestamp: f64) -> *mut crate::StringHeader {
    let timestamp = date_cell_timestamp(timestamp);
    if timestamp.is_nan() {
        return std::ptr::null_mut();
    }
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
    let days = if is_negative {
        -((abs_secs / 86400) as i64)
            - if !abs_secs.is_multiple_of(86400) {
                1
            } else {
                0
            }
    } else {
        (abs_secs / 86400) as i64
    };

    // For negative timestamps, adjust time components
    let (hour, minute, second) = if is_negative && !abs_secs.is_multiple_of(86400) {
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
