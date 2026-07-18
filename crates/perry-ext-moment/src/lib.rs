//! Native bindings for the npm `moment` package — momentjs-compatible
//! date manipulation via `chrono`. Sync, handle-based, uses only
//! perry-ffi v0.5 strings + handles.
//!
//! Note: the boolean predicates (`isBefore` / `isAfter` / `isSame` /
//! `isBetween` / `isValid`) return NaN-boxed `TAG_TRUE` / `TAG_FALSE`
//! f64s — same ABI as perry-stdlib's existing copy. Don't change to
//! 0.0/1.0 without updating the codegen-side dispatch.

use chrono::{DateTime, Datelike, Duration, NaiveDateTime, TimeZone, Timelike, Utc};
use perry_ffi::{
    alloc_string, get_handle, read_string, register_handle, Handle, JsString, StringHeader,
};

const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

pub struct MomentHandle {
    pub datetime: DateTime<Utc>,
    pub is_valid: bool,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// Same handle scheme as perry-ext-dayjs: the JS-visible moment value
/// is `f64::from_bits(handle)` — a tiny denormal whose raw bits ARE the
/// handle. Instance methods receive the handle back as an i64 first arg
/// (the dispatch table's has_receiver unbox is bitcast+mask, identity
/// for small handles); other-moment args arrive as raw NA_JSV bits,
/// which equal the handle for the same reason.
#[inline]
fn handle_to_f64(handle: Handle) -> f64 {
    f64::from_bits(handle as u64)
}

#[inline]
fn js_bool(b: bool) -> f64 {
    if b {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

#[no_mangle]
pub extern "C" fn js_moment_now() -> f64 {
    handle_to_f64(register_handle(MomentHandle {
        datetime: Utc::now(),
        is_valid: true,
    }))
}

#[no_mangle]
pub extern "C" fn js_moment_from_timestamp(timestamp_ms: f64) -> f64 {
    let secs = (timestamp_ms / 1000.0) as i64;
    let nanos = ((timestamp_ms % 1000.0) * 1_000_000.0) as u32;
    match DateTime::from_timestamp(secs, nanos) {
        Some(dt) => handle_to_f64(register_handle(MomentHandle {
            datetime: dt,
            is_valid: true,
        })),
        None => handle_to_f64(register_handle(MomentHandle {
            datetime: Utc::now(),
            is_valid: false,
        })),
    }
}

/// # Safety
/// `date_str_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_moment_parse(date_str_ptr: *const StringHeader) -> f64 {
    let date_str = match read_str(date_str_ptr) {
        Some(s) => s,
        None => {
            return handle_to_f64(register_handle(MomentHandle {
                datetime: Utc::now(),
                is_valid: false,
            }));
        }
    };

    // Note: the bare-date form must go through NaiveDate
    // (NaiveDateTime::parse_from_str("%Y-%m-%d") always errors on the
    // missing time part, which silently made moment('2024-01-15')
    // invalid).
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
        Ok(dt) => handle_to_f64(register_handle(MomentHandle {
            datetime: dt,
            is_valid: true,
        })),
        Err(_) => handle_to_f64(register_handle(MomentHandle {
            datetime: Utc::now(),
            is_valid: false,
        })),
    }
}

extern "C" {
    /// perry-runtime: extract the StringHeader pointer from any
    /// string-tagged NaN-boxed value (materializes short strings).
    fn js_get_string_pointer_unified(value: f64) -> i64;
}

/// moment(input?) — the factory with its real argument surface.
/// `value_bits` is the raw NaN-boxed first argument (TAG_UNDEFINED when
/// absent). undefined → now; string → parse; number → epoch ms.
///
/// # Safety
/// `value_bits` must be valid NaN-box bits.
#[no_mangle]
pub unsafe extern "C" fn js_moment_factory(value_bits: i64) -> f64 {
    use perry_ffi::JsValue;
    let bits = value_bits as u64;
    let jv = JsValue::from_bits(bits);
    if jv.is_any_string() {
        let ptr = js_get_string_pointer_unified(f64::from_bits(bits)) as *const StringHeader;
        return js_moment_parse(ptr);
    }
    if jv.is_int32() {
        return js_moment_from_timestamp(jv.to_int32() as f64);
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

/// # Safety
/// `format_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_moment_format(
    handle: i64,
    format_ptr: *const StringHeader,
) -> *mut StringHeader {
    let format_str = read_str(format_ptr).unwrap_or_else(|| "YYYY-MM-DDTHH:mm:ssZ".to_string());

    if let Some(moment) = get_handle::<MomentHandle>(handle) {
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
        return alloc_string(&formatted).as_raw();
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn js_moment_to_iso_string(handle: i64) -> *mut StringHeader {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return alloc_string(
            &moment
                .datetime
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        )
        .as_raw();
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn js_moment_value_of(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.timestamp_millis() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_unix(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.timestamp() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_year(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.year() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_month(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| (m.datetime.month() - 1) as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_date(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.day() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_day(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.weekday().num_days_from_sunday() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_hour(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.hour() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_minute(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.minute() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_second(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.second() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_moment_millisecond(handle: i64) -> f64 {
    get_handle::<MomentHandle>(handle)
        .map(|m| m.datetime.timestamp_subsec_millis() as f64)
        .unwrap_or(0.0)
}

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_moment_add(
    handle: i64,
    amount: f64,
    unit_ptr: *const StringHeader,
) -> f64 {
    let handle_v = handle;
    let unit = read_str(unit_ptr).unwrap_or_else(|| "days".to_string());

    if let Some(moment) = get_handle::<MomentHandle>(handle_v) {
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

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_moment_subtract(
    handle: i64,
    amount: f64,
    unit_ptr: *const StringHeader,
) -> f64 {
    js_moment_add(handle, -amount, unit_ptr)
}

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_moment_start_of(handle: i64, unit_ptr: *const StringHeader) -> f64 {
    let handle_v = handle;
    let unit = read_str(unit_ptr).unwrap_or_else(|| "day".to_string());

    if let Some(moment) = get_handle::<MomentHandle>(handle_v) {
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

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_moment_end_of(handle: i64, unit_ptr: *const StringHeader) -> f64 {
    let handle_v = handle;
    let unit = read_str(unit_ptr).unwrap_or_else(|| "day".to_string());

    if let Some(moment) = get_handle::<MomentHandle>(handle_v) {
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

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_moment_diff(
    handle: i64,
    other_handle: i64,
    unit_ptr: *const StringHeader,
) -> f64 {
    let handle_v = handle;
    let other_v = other_handle;
    let unit = read_str(unit_ptr).unwrap_or_else(|| "milliseconds".to_string());

    if let (Some(moment), Some(other)) = (
        get_handle::<MomentHandle>(handle_v),
        get_handle::<MomentHandle>(other_v),
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

#[no_mangle]
pub extern "C" fn js_moment_is_before(handle: i64, other_handle: i64) -> f64 {
    if let (Some(moment), Some(other)) = (
        get_handle::<MomentHandle>(handle),
        get_handle::<MomentHandle>(other_handle),
    ) {
        return js_bool(moment.datetime < other.datetime);
    }
    js_bool(false)
}

#[no_mangle]
pub extern "C" fn js_moment_is_after(handle: i64, other_handle: i64) -> f64 {
    if let (Some(moment), Some(other)) = (
        get_handle::<MomentHandle>(handle),
        get_handle::<MomentHandle>(other_handle),
    ) {
        return js_bool(moment.datetime > other.datetime);
    }
    js_bool(false)
}

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_moment_is_same(
    handle: i64,
    other_handle: i64,
    unit_ptr: *const StringHeader,
) -> f64 {
    let unit = read_str(unit_ptr);

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
        return js_bool(result);
    }
    js_bool(false)
}

#[no_mangle]
pub extern "C" fn js_moment_is_between(handle: i64, start_handle: i64, end_handle: i64) -> f64 {
    if let (Some(moment), Some(start), Some(end)) = (
        get_handle::<MomentHandle>(handle),
        get_handle::<MomentHandle>(start_handle),
        get_handle::<MomentHandle>(end_handle),
    ) {
        return js_bool(moment.datetime > start.datetime && moment.datetime < end.datetime);
    }
    js_bool(false)
}

#[no_mangle]
pub extern "C" fn js_moment_is_valid(handle: i64) -> f64 {
    if let Some(moment) = get_handle::<MomentHandle>(handle) {
        return js_bool(moment.is_valid);
    }
    js_bool(false)
}

#[no_mangle]
pub extern "C" fn js_moment_clone(handle: i64) -> f64 {
    let handle_v = handle;
    if let Some(moment) = get_handle::<MomentHandle>(handle_v) {
        return handle_to_f64(register_handle(MomentHandle {
            datetime: moment.datetime,
            is_valid: moment.is_valid,
        }));
    }
    handle_to_f64(handle)
}

#[no_mangle]
pub extern "C" fn js_moment_from_now(handle: i64) -> *mut StringHeader {
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
        return alloc_string(&result).as_raw();
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn js_moment_to_date(handle: i64) -> f64 {
    js_moment_value_of(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JS-value f64 → i64 handle, the same bitcast the dispatch table's
    /// receiver unbox performs.
    fn h(v: f64) -> i64 {
        v.to_bits() as i64
    }

    #[test]
    fn now_returns_valid_handle() {
        let f = js_moment_now();
        assert_ne!(f, 0.0);
        let valid = js_moment_is_valid(h(f));
        assert_eq!(valid.to_bits(), TAG_TRUE);
    }

    #[test]
    fn from_timestamp_round_trip() {
        let ts = 1_700_000_000_000.0_f64;
        let m = js_moment_from_timestamp(ts);
        assert_eq!(js_moment_value_of(h(m)), ts);
    }

    #[test]
    fn add_subtract_days_round_trip() {
        let m = js_moment_from_timestamp(1_700_000_000_000.0);
        let unit = alloc_string("days");
        let m2 = unsafe { js_moment_add(h(m), 1.0, unit.as_raw()) };
        let m3 = unsafe { js_moment_subtract(h(m2), 1.0, unit.as_raw()) };
        assert_eq!(js_moment_value_of(h(m)), js_moment_value_of(h(m3)));
    }

    #[test]
    fn comparison_predicates() {
        let earlier = js_moment_from_timestamp(1_000_000_000_000.0);
        let later = js_moment_from_timestamp(2_000_000_000_000.0);
        assert_eq!(
            js_moment_is_before(h(earlier), h(later)).to_bits(),
            TAG_TRUE
        );
        assert_eq!(js_moment_is_after(h(later), h(earlier)).to_bits(), TAG_TRUE);
        let null = std::ptr::null::<StringHeader>();
        assert_eq!(
            unsafe { js_moment_is_same(h(earlier), h(earlier), null) }.to_bits(),
            TAG_TRUE
        );
    }

    #[test]
    fn clone_preserves_datetime() {
        let m = js_moment_from_timestamp(1_700_000_000_000.0);
        let m2 = js_moment_clone(h(m));
        assert_eq!(js_moment_value_of(h(m)), js_moment_value_of(h(m2)));
    }

    #[test]
    fn parse_iso_marks_valid() {
        let s = alloc_string("2024-01-15T10:30:00Z");
        let m = unsafe { js_moment_parse(s.as_raw()) };
        assert_eq!(js_moment_year(h(m)), 2024.0);
        assert_eq!(js_moment_month(h(m)), 0.0);
        assert_eq!(js_moment_date(h(m)), 15.0);
        assert_eq!(js_moment_is_valid(h(m)).to_bits(), TAG_TRUE);
    }

    #[test]
    fn parse_bare_date_is_valid_utc_midnight() {
        let s = alloc_string("2024-01-15");
        let m = unsafe { js_moment_parse(s.as_raw()) };
        assert_eq!(js_moment_is_valid(h(m)).to_bits(), TAG_TRUE);
        assert_eq!(js_moment_year(h(m)), 2024.0);
        assert_eq!(js_moment_hour(h(m)), 0.0);
    }

    #[test]
    fn parse_garbage_marks_invalid() {
        let s = alloc_string("not a date");
        let m = unsafe { js_moment_parse(s.as_raw()) };
        assert_eq!(js_moment_is_valid(h(m)).to_bits(), TAG_FALSE);
    }

    #[test]
    fn factory_routes_string_number_undefined() {
        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        // string → parse
        let s = alloc_string("2024-01-15T10:30:00Z");
        let sv = perry_ffi::JsValue::from_string_ptr(s.as_raw());
        let m = unsafe { js_moment_factory(sv.bits() as i64) };
        assert_eq!(js_moment_year(h(m)), 2024.0);
        // number → epoch ms
        let n = 1_700_000_000_000.0_f64;
        let m2 = unsafe { js_moment_factory(n.to_bits() as i64) };
        assert_eq!(js_moment_value_of(h(m2)), n);
        // undefined → now (valid)
        let m3 = unsafe { js_moment_factory(TAG_UNDEFINED as i64) };
        assert_eq!(js_moment_is_valid(h(m3)).to_bits(), TAG_TRUE);
    }
}
