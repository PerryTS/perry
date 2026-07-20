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
    // NaN / ±∞ → Invalid Date (dayjs's invalid sentinel is `0.0`).
    if !timestamp.is_finite() {
        return 0.0; // Invalid timestamp
    }

    // Millisecond-safe conversion: `from_timestamp_millis` handles
    // negative epochs correctly (the old secs/nanos split truncated
    // negative fractions toward zero and saturated `nanos` to 0).
    if let Some(dt) = DateTime::from_timestamp_millis(timestamp as i64) {
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

    // Offset-less ISO forms ("2024-01-15T10:30:00", optional fraction) —
    // dayjs treats these as local time; Perry's date runtime is UTC-based
    // so they match Node under TZ=UTC.
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&date_str, fmt) {
            let dt = Utc.from_utc_datetime(&naive);
            let handle = register_handle(DayjsHandle::new(dt));
            return handle_to_f64(handle);
        }
    }

    0.0 // Invalid date string
}

/// dayjs(input?) -> Dayjs
///
/// The dayjs factory with its real argument surface: `value_bits` is the
/// raw NaN-boxed first argument (TAG_UNDEFINED when absent). undefined →
/// now; string → ISO parse; number → epoch milliseconds. The dispatch
/// row previously called `js_dayjs_now` with no args, silently ignoring
/// `dayjs('2024-01-15')`'s argument.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_factory(value_bits: i64) -> f64 {
    use perry_runtime::JSValue;
    let bits = value_bits as u64;
    let jv = JSValue::from_bits(bits);
    if jv.is_any_string() {
        let ptr = perry_runtime::js_get_string_pointer_unified(f64::from_bits(bits))
            as *const StringHeader;
        return js_dayjs_parse(ptr);
    }
    if jv.is_int32() {
        return js_dayjs_from_timestamp(jv.as_int32() as f64);
    }
    if jv.is_number() {
        let n = f64::from_bits(bits);
        if n.is_finite() {
            return js_dayjs_from_timestamp(n);
        }
    }
    js_dayjs_now()
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
///
/// date-fns uses Unicode-LDML-ish tokens (lowercase `yyyy` for year,
/// `dd` for day-of-month, `MM` for month, etc.) and formats in the
/// **local** timezone — distinct from dayjs which uses uppercase
/// `YYYY`/`DD` and is happy to format in UTC. We can't reuse
/// `js_dayjs_format` for this because (a) the token replacements miss,
/// and (b) local-vs-UTC swings the day across a midnight boundary.
/// Refs date-fns blocker.
#[no_mangle]
pub unsafe extern "C" fn js_datefns_format(
    timestamp: f64,
    pattern_ptr: *const StringHeader,
) -> *mut StringHeader {
    use chrono::Local;
    if timestamp.is_nan() {
        return std::ptr::null_mut();
    }
    let pattern = string_from_header(pattern_ptr).unwrap_or_else(|| "yyyy-MM-dd".to_string());
    // `new Date(year, monthIndex, ...)` stores a UTC ms timestamp whose
    // wall-clock representation in local time is the literal components
    // the user passed. So formatting it in `Local` reproduces those
    // components — which is exactly what Node's date-fns does.
    let secs = (timestamp / 1000.0) as i64;
    let nanos = ((timestamp.rem_euclid(1000.0)) * 1_000_000.0) as u32;
    let dt = match chrono::DateTime::from_timestamp(secs, nanos) {
        Some(d) => d.with_timezone(&Local),
        None => return std::ptr::null_mut(),
    };
    let formatted = format_date_fns_pattern(&pattern, &dt);
    js_string_from_bytes(formatted.as_ptr(), formatted.len() as u32)
}

/// Translate a date-fns format pattern against a chrono datetime.
///
/// Handles the most common Unicode-LDML tokens that date-fns supports.
/// Single-quoted literals (`'literal'`) pass through untouched. Tokens
/// not recognized below pass through unchanged — matching date-fns'
/// permissive behavior on unknown letters.
fn format_date_fns_pattern<Tz: chrono::TimeZone>(pattern: &str, dt: &chrono::DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    use chrono::{Datelike, Timelike};
    let bytes = pattern.as_bytes();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // Single-quoted literal — anything between `'...'` is emitted
        // verbatim. `''` is a single quote.
        if c == b'\'' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'\'' {
                out.push('\'');
                i += 1;
                continue;
            }
            while i < bytes.len() && bytes[i] != b'\'' {
                out.push(bytes[i] as char);
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c as char);
            i += 1;
            continue;
        }
        // Read a run of the same letter.
        let start = i;
        while i < bytes.len() && bytes[i] == c {
            i += 1;
        }
        let run = i - start;
        // date-fns ordinal suffix: a single-letter token immediately
        // followed by literal `o` formats the corresponding field as an
        // English ordinal (`do` → "6th", `Mo` → "1st", `yo` → "2020th",
        // `Do` → day-of-year ordinal, `Qo`/`qo` → quarter ordinal). The
        // letters that participate are exactly those listed in the
        // date-fns tokenizer character class:
        // `[yYQqMLwIdDecihHKkms]o`. We only honour the suffix when the
        // run length is 1, mirroring date-fns's own behavior.
        let ordinal = run == 1
            && i < bytes.len()
            && bytes[i] == b'o'
            && matches!(
                c,
                b'y' | b'Y'
                    | b'Q'
                    | b'q'
                    | b'M'
                    | b'L'
                    | b'w'
                    | b'I'
                    | b'd'
                    | b'D'
                    | b'e'
                    | b'c'
                    | b'i'
                    | b'h'
                    | b'H'
                    | b'K'
                    | b'k'
                    | b'm'
                    | b's'
            );
        if ordinal {
            // Consume the trailing `o`.
            i += 1;
            let n: i64 = match c {
                b'y' | b'Y' => dt.year() as i64,
                b'Q' | b'q' => (((dt.month() - 1) / 3) + 1) as i64,
                b'M' | b'L' => dt.month() as i64,
                b'w' | b'I' => dt.iso_week().week() as i64,
                b'd' => dt.day() as i64,
                b'D' => dt.ordinal() as i64,
                b'e' | b'c' | b'i' => dt.weekday().number_from_monday() as i64,
                b'h' => {
                    let h12 = dt.hour() % 12;
                    if h12 == 0 {
                        12
                    } else {
                        h12 as i64
                    }
                }
                b'H' => dt.hour() as i64,
                b'K' => (dt.hour() % 12) as i64,
                b'k' => {
                    let h = dt.hour();
                    if h == 0 {
                        24
                    } else {
                        h as i64
                    }
                }
                b'm' => dt.minute() as i64,
                b's' => dt.second() as i64,
                _ => 0,
            };
            out.push_str(&english_ordinal(n));
            continue;
        }
        match c {
            b'y' => match run {
                1 => out.push_str(&format!("{}", dt.year())),
                2 => out.push_str(&format!("{:02}", dt.year() % 100)),
                _ => out.push_str(&format!("{:0width$}", dt.year(), width = run)),
            },
            b'M' => match run {
                1 => out.push_str(&format!("{}", dt.month())),
                2 => out.push_str(&format!("{:02}", dt.month())),
                3 => out.push_str(short_month_name(dt.month())),
                _ => out.push_str(long_month_name(dt.month())),
            },
            b'd' => match run {
                1 => out.push_str(&format!("{}", dt.day())),
                _ => out.push_str(&format!("{:02}", dt.day())),
            },
            b'H' => match run {
                1 => out.push_str(&format!("{}", dt.hour())),
                _ => out.push_str(&format!("{:02}", dt.hour())),
            },
            b'h' => {
                let h12 = dt.hour() % 12;
                let h12 = if h12 == 0 { 12 } else { h12 };
                match run {
                    1 => out.push_str(&format!("{}", h12)),
                    _ => out.push_str(&format!("{:02}", h12)),
                }
            }
            b'm' => match run {
                1 => out.push_str(&format!("{}", dt.minute())),
                _ => out.push_str(&format!("{:02}", dt.minute())),
            },
            b's' => match run {
                1 => out.push_str(&format!("{}", dt.second())),
                _ => out.push_str(&format!("{:02}", dt.second())),
            },
            b'S' => {
                // Fractional seconds. date-fns uses S/SS/SSS for 1/2/3
                // digits of millisecond precision.
                let ms = dt.nanosecond() / 1_000_000;
                match run {
                    1 => out.push_str(&format!("{}", ms / 100)),
                    2 => out.push_str(&format!("{:02}", ms / 10)),
                    _ => out.push_str(&format!("{:03}", ms)),
                }
            }
            b'a' => {
                // am/pm marker. date-fns: a/aa → "AM"/"PM",
                // aaa → "am"/"pm", aaaa → "a.m."/"p.m.", aaaaa → "a"/"p".
                let pm = dt.hour() >= 12;
                let s = match run {
                    3 => {
                        if pm {
                            "pm"
                        } else {
                            "am"
                        }
                    }
                    4 => {
                        if pm {
                            "p.m."
                        } else {
                            "a.m."
                        }
                    }
                    5 => {
                        if pm {
                            "p"
                        } else {
                            "a"
                        }
                    }
                    _ => {
                        if pm {
                            "PM"
                        } else {
                            "AM"
                        }
                    }
                };
                out.push_str(s);
            }
            b'E' => {
                // Day-of-week abbreviations. EEEE = long, EEE/EE/E = short.
                let wd = dt.weekday();
                if run >= 4 {
                    out.push_str(long_weekday_name(wd));
                } else {
                    out.push_str(short_weekday_name(wd));
                }
            }
            b'X' | b'x' => {
                // Time-zone offset. Approximate as ±HH:MM (date-fns has
                // many variants; this covers the common case).
                out.push_str(&format!("{}", dt.offset()));
            }
            _ => {
                // Unknown letter run — emit verbatim (date-fns throws
                // on truly unknown tokens, but conservatively passing
                // through is closer to dayjs's behavior).
                for _ in 0..run {
                    out.push(c as char);
                }
            }
        }
    }
    out
}

/// English ordinal suffix for a non-negative integer.
/// 1 → "1st", 2 → "2nd", 3 → "3rd", 11 → "11th", 21 → "21st", etc.
fn english_ordinal(n: i64) -> String {
    let abs = n.unsigned_abs();
    let suffix = match (abs % 100, abs % 10) {
        (11, _) | (12, _) | (13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{}{}", n, suffix)
}

fn short_month_name(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "",
    }
}

fn long_month_name(m: u32) -> &'static str {
    match m {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "",
    }
}

fn short_weekday_name(w: chrono::Weekday) -> &'static str {
    match w {
        chrono::Weekday::Mon => "Mon",
        chrono::Weekday::Tue => "Tue",
        chrono::Weekday::Wed => "Wed",
        chrono::Weekday::Thu => "Thu",
        chrono::Weekday::Fri => "Fri",
        chrono::Weekday::Sat => "Sat",
        chrono::Weekday::Sun => "Sun",
    }
}

fn long_weekday_name(w: chrono::Weekday) -> &'static str {
    match w {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    }
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
