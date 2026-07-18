//! Native bindings for npm `dayjs` / `date-fns` — date parsing,
//! formatting, manipulation, and comparison via `chrono`. Sync,
//! handle-based, uses only perry-ffi v0.5 strings + handles.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use perry_ffi::{
    alloc_string, get_handle, read_string, register_handle, Handle, JsString, StringHeader,
};

pub struct DayjsHandle {
    pub datetime: DateTime<Utc>,
}

impl DayjsHandle {
    pub fn new(dt: DateTime<Utc>) -> Self {
        Self { datetime: dt }
    }
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

#[inline]
fn handle_to_f64(handle: Handle) -> f64 {
    f64::from_bits(handle as u64)
}

#[inline]
fn f64_to_handle(val: f64) -> Handle {
    val.to_bits() as i64
}

#[no_mangle]
pub extern "C" fn js_dayjs_now() -> f64 {
    handle_to_f64(register_handle(DayjsHandle::new(Utc::now())))
}

#[no_mangle]
pub extern "C" fn js_dayjs_from_timestamp(timestamp: f64) -> f64 {
    // NaN / ±∞ → Invalid Date (dayjs's invalid sentinel is `0.0`).
    if !timestamp.is_finite() {
        return 0.0;
    }
    // Millisecond-safe conversion: `from_timestamp_millis` handles
    // negative epochs correctly (the old secs/nanos split truncated
    // negative fractions toward zero and saturated `nanos` to 0).
    if let Some(dt) = DateTime::from_timestamp_millis(timestamp as i64) {
        handle_to_f64(register_handle(DayjsHandle::new(dt)))
    } else {
        0.0
    }
}

/// # Safety
/// `date_str_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_parse(date_str_ptr: *const StringHeader) -> f64 {
    let date_str = match read_str(date_str_ptr) {
        Some(s) => s,
        None => return 0.0,
    };

    if let Ok(dt) = DateTime::parse_from_rfc3339(&date_str) {
        return handle_to_f64(register_handle(DayjsHandle::new(dt.with_timezone(&Utc))));
    }
    if let Ok(naive) = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
        let datetime = naive.and_hms_opt(0, 0, 0).unwrap();
        let dt = Utc.from_utc_datetime(&datetime);
        return handle_to_f64(register_handle(DayjsHandle::new(dt)));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(&date_str, "%Y-%m-%d %H:%M:%S") {
        let dt = Utc.from_utc_datetime(&naive);
        return handle_to_f64(register_handle(DayjsHandle::new(dt)));
    }
    // Offset-less ISO forms ("2024-01-15T10:30:00", optional fraction) —
    // dayjs treats these as local time; this runtime is UTC-based so they
    // match Node under TZ=UTC.
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(&date_str, fmt) {
            let dt = Utc.from_utc_datetime(&naive);
            return handle_to_f64(register_handle(DayjsHandle::new(dt)));
        }
    }
    0.0
}

extern "C" {
    /// perry-runtime: extract the StringHeader pointer from any
    /// string-tagged NaN-boxed value (materializes short strings).
    fn js_get_string_pointer_unified(value: f64) -> i64;
}

/// dayjs(input?) — the factory with its real argument surface.
/// `value_bits` is the raw NaN-boxed first argument (TAG_UNDEFINED when
/// absent). undefined → now; string → ISO parse; number → epoch ms. The
/// dispatch row previously called `js_dayjs_now` with no args, silently
/// ignoring `dayjs('2024-01-15')`'s argument.
///
/// # Safety
/// `value_bits` must be valid NaN-box bits.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_factory(value_bits: i64) -> f64 {
    use perry_ffi::JsValue;
    let bits = value_bits as u64;
    let jv = JsValue::from_bits(bits);
    if jv.is_any_string() {
        let ptr = js_get_string_pointer_unified(f64::from_bits(bits)) as *const StringHeader;
        return js_dayjs_parse(ptr);
    }
    if jv.is_int32() {
        return js_dayjs_from_timestamp(jv.to_int32() as f64);
    }
    if jv.is_number() {
        let n = f64::from_bits(bits);
        if n.is_finite() {
            return js_dayjs_from_timestamp(n);
        }
    }
    js_dayjs_now()
}

/// # Safety
/// `pattern_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_format(
    handle: Handle,
    pattern_ptr: *const StringHeader,
) -> *mut StringHeader {
    let pattern = read_str(pattern_ptr).unwrap_or_else(|| "YYYY-MM-DD".to_string());

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        let dt = &wrapper.datetime;
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
        alloc_string(&formatted).as_raw()
    } else {
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub extern "C" fn js_dayjs_to_iso_string(handle: Handle) -> *mut StringHeader {
    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        alloc_string(&wrapper.datetime.to_rfc3339()).as_raw()
    } else {
        std::ptr::null_mut()
    }
}

#[no_mangle]
pub extern "C" fn js_dayjs_value_of(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| w.datetime.timestamp_millis() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_unix(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| w.datetime.timestamp() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_year(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| w.datetime.year() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_month(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| (w.datetime.month() - 1) as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_date(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| w.datetime.day() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_day(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| w.datetime.weekday().num_days_from_sunday() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_hour(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| w.datetime.hour() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_minute(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| w.datetime.minute() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_second(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| w.datetime.second() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_dayjs_millisecond(handle: Handle) -> f64 {
    get_handle::<DayjsHandle>(handle)
        .map(|w| (w.datetime.nanosecond() / 1_000_000) as f64)
        .unwrap_or(0.0)
}

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_add(
    handle: Handle,
    value: f64,
    unit_ptr: *const StringHeader,
) -> f64 {
    let unit = read_str(unit_ptr).unwrap_or_else(|| "day".to_string());
    let value = value as i64;

    if let Some(wrapper) = get_handle::<DayjsHandle>(handle) {
        let dt = &wrapper.datetime;
        let new_dt = match unit.as_str() {
            "day" | "days" | "d" => *dt + Duration::days(value),
            "week" | "weeks" | "w" => *dt + Duration::weeks(value),
            "month" | "months" | "M" => {
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
        handle_to_f64(register_handle(DayjsHandle::new(new_dt)))
    } else {
        0.0
    }
}

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_subtract(
    handle: Handle,
    value: f64,
    unit_ptr: *const StringHeader,
) -> f64 {
    js_dayjs_add(handle, -value, unit_ptr)
}

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_start_of(handle: Handle, unit_ptr: *const StringHeader) -> f64 {
    let unit = read_str(unit_ptr).unwrap_or_else(|| "day".to_string());

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
        handle_to_f64(register_handle(DayjsHandle::new(new_dt)))
    } else {
        0.0
    }
}

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_end_of(handle: Handle, unit_ptr: *const StringHeader) -> f64 {
    let unit = read_str(unit_ptr).unwrap_or_else(|| "day".to_string());

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
        handle_to_f64(register_handle(DayjsHandle::new(new_dt)))
    } else {
        0.0
    }
}

/// # Safety
/// `unit_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_dayjs_diff(
    handle: Handle,
    other_handle: Handle,
    unit_ptr: *const StringHeader,
) -> f64 {
    let unit = read_str(unit_ptr).unwrap_or_else(|| "millisecond".to_string());
    let w1 = get_handle::<DayjsHandle>(handle);
    let w2 = get_handle::<DayjsHandle>(other_handle);

    if let (Some(w1), Some(w2)) = (w1, w2) {
        let diff = w1.datetime.signed_duration_since(w2.datetime);
        match unit.as_str() {
            "year" | "years" | "y" => (diff.num_days() / 365) as f64,
            "month" | "months" | "M" => (diff.num_days() / 30) as f64,
            "week" | "weeks" | "w" => diff.num_weeks() as f64,
            "day" | "days" | "d" => diff.num_days() as f64,
            "hour" | "hours" | "h" => diff.num_hours() as f64,
            "minute" | "minutes" | "m" => diff.num_minutes() as f64,
            "second" | "seconds" | "s" => diff.num_seconds() as f64,
            _ => diff.num_milliseconds() as f64,
        }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn js_dayjs_is_before(handle: Handle, other_handle: Handle) -> f64 {
    let w1 = get_handle::<DayjsHandle>(handle);
    let w2 = get_handle::<DayjsHandle>(other_handle);
    if let (Some(w1), Some(w2)) = (w1, w2) {
        if w1.datetime < w2.datetime {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn js_dayjs_is_after(handle: Handle, other_handle: Handle) -> f64 {
    let w1 = get_handle::<DayjsHandle>(handle);
    let w2 = get_handle::<DayjsHandle>(other_handle);
    if let (Some(w1), Some(w2)) = (w1, w2) {
        if w1.datetime > w2.datetime {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn js_dayjs_is_same(handle: Handle, other_handle: Handle) -> f64 {
    let w1 = get_handle::<DayjsHandle>(handle);
    let w2 = get_handle::<DayjsHandle>(other_handle);
    if let (Some(w1), Some(w2)) = (w1, w2) {
        if w1.datetime == w2.datetime {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn js_dayjs_is_valid(handle: Handle) -> f64 {
    if get_handle::<DayjsHandle>(handle).is_some() {
        1.0
    } else {
        0.0
    }
}

// ============ date-fns compatible functions ============

/// # Safety
/// `pattern_ptr` must be null or a Perry-runtime `StringHeader`.
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

/// # Safety
/// `date_str_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_datefns_parse_iso(date_str_ptr: *const StringHeader) -> f64 {
    let handle_f64 = js_dayjs_parse(date_str_ptr);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(handle_f64))
}

#[no_mangle]
pub extern "C" fn js_datefns_add_days(timestamp: f64, amount: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = alloc_string("day");
    let new_handle_f64 = unsafe { js_dayjs_add(f64_to_handle(handle_f64), amount, unit.as_raw()) };
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

#[no_mangle]
pub extern "C" fn js_datefns_add_months(timestamp: f64, amount: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = alloc_string("month");
    let new_handle_f64 = unsafe { js_dayjs_add(f64_to_handle(handle_f64), amount, unit.as_raw()) };
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

#[no_mangle]
pub extern "C" fn js_datefns_add_years(timestamp: f64, amount: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = alloc_string("year");
    let new_handle_f64 = unsafe { js_dayjs_add(f64_to_handle(handle_f64), amount, unit.as_raw()) };
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

#[no_mangle]
pub extern "C" fn js_datefns_difference_in_days(timestamp_left: f64, timestamp_right: f64) -> f64 {
    let diff_ms = timestamp_left - timestamp_right;
    (diff_ms / (1000.0 * 60.0 * 60.0 * 24.0)).floor()
}

#[no_mangle]
pub extern "C" fn js_datefns_difference_in_hours(timestamp_left: f64, timestamp_right: f64) -> f64 {
    let diff_ms = timestamp_left - timestamp_right;
    (diff_ms / (1000.0 * 60.0 * 60.0)).floor()
}

#[no_mangle]
pub extern "C" fn js_datefns_difference_in_minutes(
    timestamp_left: f64,
    timestamp_right: f64,
) -> f64 {
    let diff_ms = timestamp_left - timestamp_right;
    (diff_ms / (1000.0 * 60.0)).floor()
}

#[no_mangle]
pub extern "C" fn js_datefns_is_after(timestamp: f64, compare_timestamp: f64) -> f64 {
    if timestamp > compare_timestamp {
        1.0
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn js_datefns_is_before(timestamp: f64, compare_timestamp: f64) -> f64 {
    if timestamp < compare_timestamp {
        1.0
    } else {
        0.0
    }
}

#[no_mangle]
pub extern "C" fn js_datefns_start_of_day(timestamp: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = alloc_string("day");
    let new_handle_f64 = unsafe { js_dayjs_start_of(f64_to_handle(handle_f64), unit.as_raw()) };
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

#[no_mangle]
pub extern "C" fn js_datefns_end_of_day(timestamp: f64) -> f64 {
    let handle_f64 = js_dayjs_from_timestamp(timestamp);
    if handle_f64 == 0.0 {
        return f64::NAN;
    }
    let unit = alloc_string("day");
    let new_handle_f64 = unsafe { js_dayjs_end_of(f64_to_handle(handle_f64), unit.as_raw()) };
    if new_handle_f64 == 0.0 {
        return f64::NAN;
    }
    js_dayjs_value_of(f64_to_handle(new_handle_f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_returns_handle() {
        let f = js_dayjs_now();
        assert_ne!(f, 0.0);
        assert!(js_dayjs_is_valid(f64_to_handle(f)) > 0.0);
    }

    #[test]
    fn from_timestamp_round_trip() {
        let ts = 1_700_000_000_000.0_f64;
        let h = js_dayjs_from_timestamp(ts);
        assert_ne!(h, 0.0);
        let back = js_dayjs_value_of(f64_to_handle(h));
        assert_eq!(back, ts);
    }

    #[test]
    fn parse_iso_format() {
        let s = alloc_string("2024-01-15T10:30:00Z");
        let h = unsafe { js_dayjs_parse(s.as_raw()) };
        assert_ne!(h, 0.0);
        assert_eq!(js_dayjs_year(f64_to_handle(h)), 2024.0);
        assert_eq!(js_dayjs_month(f64_to_handle(h)), 0.0); // Jan = 0
        assert_eq!(js_dayjs_date(f64_to_handle(h)), 15.0);
    }

    #[test]
    fn add_subtract_days() {
        let ts = 1_700_000_000_000.0_f64;
        let h = js_dayjs_from_timestamp(ts);
        let unit = alloc_string("day");
        let h2 = unsafe { js_dayjs_add(f64_to_handle(h), 1.0, unit.as_raw()) };
        let h3 = unsafe { js_dayjs_subtract(f64_to_handle(h2), 1.0, unit.as_raw()) };
        assert_eq!(js_dayjs_value_of(f64_to_handle(h3)), ts);
    }

    #[test]
    fn comparison_predicates() {
        let earlier = js_dayjs_from_timestamp(1_000_000_000_000.0);
        let later = js_dayjs_from_timestamp(2_000_000_000_000.0);
        assert_eq!(
            js_dayjs_is_before(f64_to_handle(earlier), f64_to_handle(later)),
            1.0
        );
        assert_eq!(
            js_dayjs_is_after(f64_to_handle(later), f64_to_handle(earlier)),
            1.0
        );
        assert_eq!(
            js_dayjs_is_same(f64_to_handle(earlier), f64_to_handle(earlier)),
            1.0
        );
    }

    #[test]
    fn datefns_difference_in_days() {
        let one_day_ms = 24.0 * 60.0 * 60.0 * 1000.0;
        let diff = js_datefns_difference_in_days(2.0 * one_day_ms, 0.0);
        assert_eq!(diff, 2.0);
    }
}
