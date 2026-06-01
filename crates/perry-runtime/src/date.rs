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
    if let Some(now) = crate::timer::js_mock_timers_date_now() {
        return now;
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// performance.now() — high-resolution time in milliseconds (sub-ms precision).
/// Returns ms since performance.timeOrigin as f64.
#[no_mangle]
pub extern "C" fn js_performance_now() -> f64 {
    crate::perf_hooks::performance_now_ms()
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

/// Number of days from the civil date 1970-01-01 to `y-m-d` (m is 1-based,
/// d is 1-based). Howard Hinnant's `days_from_civil`, generalized to accept
/// arbitrary integer components so day/month overflow and underflow (e.g.
/// `day = 0`, `month = 13`, negative days) normalize the way the JS Date
/// MakeDay/MakeDate algorithm requires. Works for any year in range.
fn days_from_civil(year: i64, month: u32, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64; // [0, 399]
    let m = month as i64;
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day - 1; // day-of-year, day may be off-range
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // can be off [0,146096]
    era * 146097 + doe - 719468
}

/// Assemble a UTC millisecond timestamp from broken-down components using the
/// JS Date algorithm: month is 0-based and may be any integer (rolls into
/// the year), `day` may be 0/negative/overflow (normalized via day count),
/// and time fields likewise normalize. Mirrors ECMA-262 MakeDate(MakeDay,
/// MakeTime). All arithmetic is i64 so out-of-range components match Node.
fn make_utc_ms(year: i64, month0: i64, day: i64, hour: i64, min: i64, sec: i64, ms: i64) -> f64 {
    // Month rollover into the year (handles negative months too).
    let total_months = year * 12 + month0;
    let norm_year = total_months.div_euclid(12);
    let norm_month1 = (total_months.rem_euclid(12) + 1) as u32; // 1..=12
    let days = days_from_civil(norm_year, norm_month1, day);
    let secs = days * 86400 + hour * 3600 + min * 60 + sec;
    (secs * 1000 + ms) as f64
}

/// Apply the ECMA-262 `0..=99 => 1900 + y` rebasing for an integral year
/// when (and only when) it falls in that range.
#[inline]
fn rebase_two_digit_year(year: f64) -> f64 {
    if year.fract() == 0.0 && (0.0..100.0).contains(&year) {
        year + 1900.0
    } else {
        year
    }
}

/// Parse a date string into a millisecond timestamp (UTC). Returns NaN for
/// unrecognized input. Implements the well-defined subset of the Date Time
/// String grammar plus the common RFC-1123 / IETF / month-name forms Node
/// accepts:
///   - ISO 8601: "YYYY", "YYYY-MM", "YYYY-MM-DD", with optional
///     "THH:MM[:SS[.sss]]" and an optional "Z" / "+HH:MM" / "-HH:MM" offset.
///     Date-only forms are UTC; date-time forms without an offset are also
///     treated as UTC (matching V8's ISO handling).
///   - "YYYY-MM-DD HH:MM:SS" (space separator, MySQL form).
///   - RFC-1123 / IETF: "Thu, 01 Jan 1970 00:00:00 GMT",
///     "01 Jan 1970 00:00:00 GMT" (with optional weekday and optional
///     trailing GMT/UTC/+offset).
///   - Month-name forms: "March 7, 2020", "Jan 15 2024".
fn parse_date_string(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return f64::NAN;
    }

    if let Some(ts) = parse_iso8601(s) {
        return ts;
    }
    if let Some(ts) = parse_rfc_or_named(s) {
        return ts;
    }
    f64::NAN
}

/// Parse an integer offset of the form `Z`, `+HH:MM`, `-HH:MM`, `+HHMM`, or
/// `+HH`. Returns the offset in minutes east of UTC (`Z` => 0). `None` if the
/// remainder is not a valid zone designator.
fn parse_tz_offset(rest: &str) -> Option<i64> {
    let rest = rest.trim();
    if rest.is_empty() {
        // No designator at all — caller decides the default.
        return Some(i64::MAX); // sentinel "absent"
    }
    if rest == "Z" || rest.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let bytes = rest.as_bytes();
    let sign = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let body = &rest[1..];
    let (hh, mm) = if let Some((h, m)) = body.split_once(':') {
        (h, m)
    } else if body.len() == 4 {
        (&body[0..2], &body[2..4])
    } else if body.len() == 2 {
        (body, "0")
    } else {
        return None;
    };
    let h: i64 = hh.parse().ok()?;
    let m: i64 = mm.parse().ok()?;
    Some(sign * (h * 60 + m))
}

/// ISO 8601 / MySQL branch. Returns `Some(ms)` on success.
fn parse_iso8601(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    // Year-only "YYYY" or "+YYYYYY" not handled here (rare); require a 4-digit
    // year prefix.
    if b.len() < 4 || !b[0..4].iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year: i64 = s[0..4].parse().ok()?;
    let mut month1: u32 = 1;
    let mut day: i64 = 1;
    let mut hour: i64 = 0;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    let mut millis: i64 = 0;
    let mut idx = 4;

    // "YYYY" only.
    if s.len() == 4 {
        return Some(make_utc_ms(
            year,
            month1 as i64 - 1,
            day,
            hour,
            minute,
            second,
            millis,
        ));
    }
    // Require a '-' for month.
    if b.get(4) != Some(&b'-') {
        return None;
    }
    if b.len() < 7 {
        return None;
    }
    month1 = s[5..7].parse().ok()?;
    if !(1..=12).contains(&month1) {
        return None;
    }
    idx = 7;
    let mut has_day = false;
    if b.get(7) == Some(&b'-') {
        if b.len() < 10 {
            return None;
        }
        day = s[8..10].parse().ok()?;
        if !(1..=31).contains(&day) {
            return None;
        }
        idx = 10;
        has_day = true;
    }

    // Time part (after 'T' or ' ').
    let mut tz_minutes_east: Option<i64> = None; // None => "no offset present"
    if idx < s.len() {
        let sep = b[idx];
        if sep != b'T' && sep != b' ' {
            return None;
        }
        // Month-only "YYYY-MM" cannot carry a time component.
        if !has_day {
            return None;
        }
        let time_str = &s[idx + 1..];
        // Split off a trailing zone designator. Scan for the first of
        // 'Z', '+', '-' after the HH:MM[:SS[.sss]] body.
        let zone_pos = time_str
            .char_indices()
            .find(|(i, c)| *i > 0 && (*c == 'Z' || *c == '+' || *c == '-'))
            .map(|(i, _)| i);
        let (clock, zone) = match zone_pos {
            Some(p) => (&time_str[..p], &time_str[p..]),
            None => (time_str, ""),
        };
        let cb = clock.as_bytes();
        if clock.len() < 5 || cb[2] != b':' {
            return None;
        }
        hour = clock[0..2].parse().ok()?;
        minute = clock[3..5].parse().ok()?;
        if clock.len() >= 8 && cb[5] == b':' {
            second = clock[6..8].parse().ok()?;
            if clock.len() > 9 && cb[8] == b'.' {
                let frac = &clock[9..];
                let frac_digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !frac_digits.is_empty() {
                    millis = normalize_millis(&frac_digits);
                }
            }
        }
        if !zone.is_empty() {
            match parse_tz_offset(zone) {
                Some(v) if v == i64::MAX => {}
                Some(v) => tz_minutes_east = Some(v),
                None => return None,
            }
        }
    }
    let base = make_utc_ms(year, month1 as i64 - 1, day, hour, minute, second, millis);
    // Apply zone offset: a clock with offset +HH:MM is `offset` ahead of UTC,
    // so UTC = clock - offset.
    let adjusted = if let Some(off) = tz_minutes_east {
        base - (off * 60_000) as f64
    } else {
        base
    };
    let _ = idx;
    Some(adjusted)
}

/// Normalize a run of fractional-second digits to a 0..=999 millisecond value.
fn normalize_millis(digits: &str) -> i64 {
    // Take the first 3 digits, zero-pad on the right.
    let mut ms = 0i64;
    for (i, c) in digits.chars().take(3).enumerate() {
        let d = c.to_digit(10).unwrap_or(0) as i64;
        ms += d * 10i64.pow(2 - i as u32);
    }
    ms
}

const FULL_MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn month_from_name(tok: &str) -> Option<u32> {
    let t = tok.trim_end_matches(',').to_ascii_lowercase();
    if t.len() < 3 {
        return None;
    }
    let abbr = &t[..3];
    FULL_MONTHS
        .iter()
        .position(|m| m.starts_with(abbr) && t.len() <= m.len() && m.starts_with(&t))
        .map(|i| (i + 1) as u32)
}

/// RFC-1123 / IETF and month-name string forms. Token-based, timezone-aware.
fn parse_rfc_or_named(s: &str) -> Option<f64> {
    // Drop a leading weekday token like "Thu," or "Thursday,".
    let raw = s.replace(',', " ");
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    let mut year: Option<i64> = None;
    let mut month: Option<u32> = None;
    let mut day: Option<i64> = None;
    let mut hour: i64 = 0;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    let mut tz_minutes_east: Option<i64> = None;

    for tok in &tokens {
        // Weekday name → skip.
        let low = tok.to_ascii_lowercase();
        if ["sun", "mon", "tue", "wed", "thu", "fri", "sat"]
            .iter()
            .any(|w| low.starts_with(w))
            && month_from_name(tok).is_none()
            && !tok.chars().next().unwrap_or(' ').is_ascii_digit()
        {
            continue;
        }
        // Month name.
        if let Some(m) = month_from_name(tok) {
            month = Some(m);
            continue;
        }
        // Time "HH:MM[:SS]".
        if tok.contains(':') {
            let parts: Vec<&str> = tok.split(':').collect();
            if parts.len() >= 2 {
                hour = parts[0].parse().ok()?;
                minute = parts[1].parse().ok()?;
                if parts.len() >= 3 {
                    second = parts[2].parse().unwrap_or(0);
                }
                continue;
            }
        }
        // Timezone words / offsets.
        if low == "gmt" || low == "utc" || low == "z" {
            tz_minutes_east = Some(0);
            continue;
        }
        if let Some(stripped) = tok.strip_prefix("GMT").or_else(|| tok.strip_prefix("UTC")) {
            if let Some(off) = parse_tz_offset(stripped) {
                if off != i64::MAX {
                    tz_minutes_east = Some(off);
                }
            }
            continue;
        }
        if (tok.starts_with('+') || tok.starts_with('-')) && tok.len() >= 3 {
            if let Some(off) = parse_tz_offset(tok) {
                if off != i64::MAX {
                    tz_minutes_east = Some(off);
                    continue;
                }
            }
        }
        // Pure number → day or year. A 4+-digit number is unambiguously the
        // year; otherwise it's the day-of-month if one hasn't been seen yet
        // and it is in range (RFC-1123 puts the day before the year, e.g.
        // "01 Jan 1970"), else the year.
        if let Ok(n) = tok.parse::<i64>() {
            let is_four_digit = tok.trim_start_matches(['+', '-']).len() >= 4;
            if is_four_digit && year.is_none() {
                year = Some(n);
            } else if day.is_none() && (1..=31).contains(&n) {
                day = Some(n);
            } else if year.is_none() {
                year = Some(n);
            }
            continue;
        }
    }

    let y = year?;
    let m = month?;
    let d = day.unwrap_or(1);
    // RFC/IETF dates without an explicit zone are treated as local time by
    // Node; but the common HTTP-date forms always carry GMT, and our test
    // surface only uses GMT/offset forms. Default to UTC when a zone token
    // was seen; otherwise treat the named-month form (e.g. "March 7, 2020")
    // as local time to match Node.
    let base = make_utc_ms(y, m as i64 - 1, d, hour, minute, second, 0);
    match tz_minutes_east {
        Some(off) => Some(base - (off * 60_000) as f64),
        None => {
            // Local-time interpretation: subtract local tz offset at that
            // instant (mirrors js_date_new_local_components).
            let secs = (base as i64).div_euclid(1000);
            let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(secs);
            Some(base - (tz_offset * 1000) as f64)
        }
    }
}

/// Convert date components (UTC) to Unix timestamp in seconds.
/// Inverse of timestamp_to_components. Used only by the Windows tz-offset
/// fallback; superseded elsewhere by [`make_utc_ms`] (which normalizes
/// out-of-range components).
#[cfg_attr(not(windows), allow(dead_code))]
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

/// `Date.UTC(year, month?, day?, hour?, minute?, second?, ms?)` (#2826).
///
/// Codegen passes a buffer of NaN-boxed JS values plus the actual argument
/// count so the runtime can apply Node-correct defaults:
///   - `argc == 0` → NaN (the required `year` is `undefined`);
///   - omitted `month` → 0, omitted `day` → 1, omitted time fields → 0;
///   - integral `year` in `0..=99` is rebased to `1900 + year`;
///   - any provided component that coerces to NaN → NaN (Invalid);
///   - out-of-range components (day 0, month 12, …) normalize via the Date
///     MakeDay/MakeTime algorithm.
///
/// Returns the millisecond timestamp (a plain number, not a DateCell).
#[no_mangle]
pub extern "C" fn js_date_utc(args_ptr: *const f64, argc: i32) -> f64 {
    let argc = argc.max(0) as usize;
    if argc == 0 {
        return f64::NAN;
    }
    let args = unsafe { std::slice::from_raw_parts(args_ptr, argc) };
    let get = |i: usize, default: f64| -> f64 {
        if i < argc {
            jsvalue_to_number(args[i])
        } else {
            default
        }
    };
    let year = rebase_two_digit_year(get(0, f64::NAN));
    let month = get(1, 0.0);
    let day = get(2, 1.0);
    let hour = get(3, 0.0);
    let minute = get(4, 0.0);
    let second = get(5, 0.0);
    let ms = get(6, 0.0);
    if year.is_nan()
        || month.is_nan()
        || day.is_nan()
        || hour.is_nan()
        || minute.is_nan()
        || second.is_nan()
        || ms.is_nan()
    {
        return f64::NAN;
    }
    make_utc_ms(
        year as i64,
        month as i64,
        day as i64,
        hour as i64,
        minute as i64,
        second as i64,
        ms as i64,
    )
}

/// Keepalive anchor for `js_date_utc` — codegen-only `#[no_mangle]` symbols
/// get dead-stripped by the auto-optimize whole-program LLVM bitcode rebuild
/// without a `#[used]` reference (see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_JS_DATE_UTC: extern "C" fn(*const f64, i32) -> f64 = js_date_utc;

/// Coerce a NaN-boxed JS value to a number (ECMAScript ToNumber, restricted
/// to the inputs Date setters/constructors actually receive). `undefined`
/// becomes NaN; numbers pass through; numeric strings parse; everything else
/// is NaN.
fn jsvalue_to_number(v: f64) -> f64 {
    let bits = v.to_bits();
    let tag = (bits >> 48) & 0xFFFF;
    match tag {
        0x7FFF => {
            // NaN-boxed heap string.
            let ptr = (bits & NANBOX_PTR_MASK) as *const crate::StringHeader;
            if ptr.is_null() || (ptr as usize) < 0x1000 {
                return f64::NAN;
            }
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                match std::str::from_utf8(bytes) {
                    Ok(s) => {
                        let t = s.trim();
                        if t.is_empty() {
                            0.0
                        } else {
                            t.parse::<f64>().unwrap_or(f64::NAN)
                        }
                    }
                    Err(_) => f64::NAN,
                }
            }
        }
        0x7FFC => {
            // boxed sentinel: undefined / null / false / true
            match bits & 0xFF {
                0x01 => f64::NAN, // undefined
                0x02 => 0.0,      // null → 0
                0x03 => 0.0,      // false
                0x04 => 1.0,      // true
                _ => f64::NAN,
            }
        }
        0x7FFE => {
            // INT32
            ((bits & 0xFFFF_FFFF) as u32 as i32) as f64
        }
        _ => v, // plain f64 double
    }
}

/// True if a NaN-boxed JS value is `undefined`.
#[inline]
fn jsvalue_is_undefined(v: f64) -> bool {
    let bits = v.to_bits();
    ((bits >> 48) & 0xFFFF) == 0x7FFC && (bits & 0xFF) == 0x01
}

/// `Date.prototype.set*` family with optional trailing arguments (#2851).
///
/// `field` selects which component the *leading* argument sets:
///   0=FullYear 1=Month 2=Date 3=Hours 4=Minutes 5=Seconds 6=Milliseconds
///   7=Time (setTime). `is_utc != 0` selects the UTC rebuild, else local.
///
/// `args_ptr`/`argc` carry the NaN-boxed call arguments. Per Node:
///   - supplied components update; omitted *trailing* components keep their
///     current value;
///   - a leading `undefined` (e.g. `setHours()`) coerces to NaN and makes
///     the Date Invalid;
///   - any supplied component that coerces to NaN makes the Date Invalid.
/// The receiver cell is mutated in place; the numeric ms result is returned.
#[no_mangle]
pub extern "C" fn js_date_apply_setter(
    date: f64,
    is_utc: i32,
    field: i32,
    args_ptr: *const f64,
    argc: i32,
) -> f64 {
    let argc = argc.max(0) as usize;
    let args = if argc == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(args_ptr, argc) }
    };
    // setTime: single value, replaces the whole time.
    if field == 7 {
        let v = if argc == 0 {
            f64::NAN
        } else {
            jsvalue_to_number(args[0])
        };
        return date_cell_store(date, v);
    }
    // `req(0)` is the *leading*, required component: an omitted leading
    // argument coerces to NaN (e.g. `setHours()` → Invalid Date). Trailing
    // optional components use `opt(i)`: an omitted (or `undefined`) trailing
    // argument is `None`, i.e. "keep the current field". A *present* arg
    // coerces via ToNumber (NaN-propagating either way).
    let opt = |i: usize| -> Option<f64> {
        if i < argc && !jsvalue_is_undefined(args[i]) {
            Some(jsvalue_to_number(args[i]))
        } else {
            None
        }
    };
    let req = |i: usize| -> Option<f64> {
        if i < argc {
            Some(jsvalue_to_number(args[i]))
        } else {
            Some(f64::NAN)
        }
    };
    // Map (field, positional index) → the seven rebuild slots.
    // Slots: year, month0, day, hour, minute, second, ms. The first slot of
    // each setter is the required leading component.
    let (year, month0, day, hour, minute, second, ms) = match field {
        0 => (req(0), opt(1), opt(2), None, None, None, None), // setFullYear(y, mo?, d?)
        1 => (None, req(0), opt(1), None, None, None, None),   // setMonth(mo, d?)
        2 => (None, None, req(0), None, None, None, None),     // setDate(d)
        3 => (None, None, None, req(0), opt(1), opt(2), opt(3)), // setHours(h, mi?, s?, ms?)
        4 => (None, None, None, None, req(0), opt(1), opt(2)), // setMinutes(mi, s?, ms?)
        5 => (None, None, None, None, None, req(0), opt(1)),   // setSeconds(s, ms?)
        6 => (None, None, None, None, None, None, req(0)),     // setMilliseconds(ms)
        _ => return date_cell_store(date, f64::NAN),
    };
    if is_utc != 0 {
        rebuild_with(date, year, month0, day, hour, minute, second, ms)
    } else {
        rebuild_local_with(date, year, month0, day, hour, minute, second, ms)
    }
}

#[used]
static KEEP_JS_DATE_APPLY_SETTER: extern "C" fn(f64, i32, i32, *const f64, i32) -> f64 =
    js_date_apply_setter;

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
    // ECMA-262: if 0 <= year < 100 (integral), year = 1900 + year.
    let year_f = rebase_two_digit_year(y);
    // Assemble the local-clock components (month 0-based, overflow
    // normalizes), then reinterpret as UTC by subtracting the local tz
    // offset at that instant.
    let local_ms = make_utc_ms(
        year_f as i64,
        m as i64,
        d as i64,
        h as i64,
        mi as i64,
        s as i64,
        ms as i64,
    );
    let local_secs = (local_ms as i64).div_euclid(1000);
    let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(local_secs);
    alloc_date_cell(local_ms - (tz_offset * 1000) as f64)
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
    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(timestamp) {
        return payload;
    }
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

/// Rebuild a Date's timestamp in UTC with the provided component overrides.
/// Each override is an `Option<f64>`: `None` keeps the current field, `Some`
/// replaces it (and a `Some(NaN)` makes the whole Date Invalid). `month` is
/// **0-based** (JS convention), consistent with `js_date_apply_setter`.
#[allow(clippy::too_many_arguments)]
fn rebuild_with(
    date: f64,
    year: Option<f64>,
    month0: Option<f64>,
    day: Option<f64>,
    hour: Option<f64>,
    minute: Option<f64>,
    second: Option<f64>,
    millisecond: Option<f64>,
) -> f64 {
    let timestamp = date_cell_timestamp(date);
    let (cy, cm0, cd, ch, cmi, cs, cur_ms) = if timestamp.is_nan() {
        // Setting the year revives an Invalid Date (ECMA MakeDate seeds from
        // year/0/1); any other component setter on an Invalid Date is a no-op.
        if year.is_none() {
            return date_cell_store(date, timestamp);
        }
        (1970i64, 0i64, 1i64, 0i64, 0i64, 0i64, 0i64)
    } else {
        let ts_ms = timestamp as i64;
        let secs = ts_ms.div_euclid(1000);
        let cur_ms = ts_ms.rem_euclid(1000);
        let (cy, cm, cd, ch, cmi, cs) = timestamp_to_components(secs);
        (
            cy as i64,
            cm as i64 - 1,
            cd as i64,
            ch as i64,
            cmi as i64,
            cs as i64,
            cur_ms,
        )
    };
    // If any provided override is NaN, the Date becomes Invalid.
    for o in [year, month0, day, hour, minute, second, millisecond]
        .into_iter()
        .flatten()
    {
        if o.is_nan() {
            return date_cell_store(date, f64::NAN);
        }
    }
    let ms = make_utc_ms(
        year.map(|v| v as i64).unwrap_or(cy),
        month0.map(|v| v as i64).unwrap_or(cm0),
        day.map(|v| v as i64).unwrap_or(cd),
        hour.map(|v| v as i64).unwrap_or(ch),
        minute.map(|v| v as i64).unwrap_or(cmi),
        second.map(|v| v as i64).unwrap_or(cs),
        millisecond.map(|v| v as i64).unwrap_or(cur_ms),
    );
    date_cell_store(date, ms)
}

// --- Local-time setters (#1187 / #2851) ---
//
// `setHours` / `setDate` / `setMonth` / `setFullYear` / etc. interpret their
// arguments in the running process's *local* timezone, so the rebuild has to
// round-trip through `timestamp_to_local_components`. The local-clock
// components get reassembled with the requested components swapped, then we
// subtract the tz offset at that instant to land back at a true UTC epoch.
// Mirrors the conversion in `js_date_new_local_components`. Only setting the
// year revives an Invalid Date; other component setters leave it invalid.

/// Local-time analogue of [`rebuild_with`]. Same `Option<f64>` override
/// contract; `month` is 0-based.
#[allow(clippy::too_many_arguments)]
fn rebuild_local_with(
    date: f64,
    year: Option<f64>,
    month0: Option<f64>,
    day: Option<f64>,
    hour: Option<f64>,
    minute: Option<f64>,
    second: Option<f64>,
    millisecond: Option<f64>,
) -> f64 {
    let timestamp = date_cell_timestamp(date);
    let (cy, cm0, cd, ch, cmi, cs, cur_ms) = if timestamp.is_nan() {
        if year.is_none() {
            return date_cell_store(date, timestamp);
        }
        (1970i64, 0i64, 1i64, 0i64, 0i64, 0i64, 0i64)
    } else {
        let ts_ms = timestamp as i64;
        let secs = ts_ms.div_euclid(1000);
        let cur_ms = ts_ms.rem_euclid(1000);
        let (cy, cm, cd, ch, cmi, cs, _) = timestamp_to_local_components(secs);
        (
            cy as i64,
            cm as i64 - 1,
            cd as i64,
            ch as i64,
            cmi as i64,
            cs as i64,
            cur_ms,
        )
    };
    for o in [year, month0, day, hour, minute, second, millisecond]
        .into_iter()
        .flatten()
    {
        if o.is_nan() {
            return date_cell_store(date, f64::NAN);
        }
    }
    let local_ms = make_utc_ms(
        year.map(|v| v as i64).unwrap_or(cy),
        month0.map(|v| v as i64).unwrap_or(cm0),
        day.map(|v| v as i64).unwrap_or(cd),
        hour.map(|v| v as i64).unwrap_or(ch),
        minute.map(|v| v as i64).unwrap_or(cmi),
        second.map(|v| v as i64).unwrap_or(cs),
        millisecond.map(|v| v as i64).unwrap_or(cur_ms),
    );
    let local_secs = (local_ms as i64).div_euclid(1000);
    let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(local_secs);
    date_cell_store(date, local_ms - (tz_offset * 1000) as f64)
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

    // Helpers for the setter API: a plain f64 is already its own NaN-boxed
    // number; `undefined` is the boxed sentinel.
    fn undef() -> f64 {
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
    fn set_utc(date: f64, field: i32, args: &[f64]) -> f64 {
        js_date_apply_setter(date, 1, field, args.as_ptr(), args.len() as i32)
    }
    fn set_local(date: f64, field: i32, args: &[f64]) -> f64 {
        js_date_apply_setter(date, 0, field, args.as_ptr(), args.len() as i32)
    }

    #[test]
    fn test_full_year_setters_revive_invalid_date_only() {
        let local = date_invalid();
        let local_result = set_local(local, 0, &[2020.0]);
        assert!(!local_result.is_nan());
        assert!(!date_cell_timestamp(local).is_nan());
        assert_eq!(js_date_get_full_year(local), 2020.0);
        assert_eq!(js_date_get_month(local), 0.0);
        assert_eq!(js_date_get_date(local), 1.0);
        assert_eq!(js_date_get_hours(local), 0.0);

        let utc = date_invalid();
        let utc_result = set_utc(utc, 0, &[2020.0]);
        assert_eq!(utc_result, 1_577_836_800_000.0);
        assert_eq!(date_cell_timestamp(utc), 1_577_836_800_000.0);

        let local_month = date_invalid();
        assert!(set_local(local_month, 1, &[0.0]).is_nan());
        assert!(date_cell_timestamp(local_month).is_nan());

        let utc_month = date_invalid();
        assert!(set_utc(utc_month, 1, &[0.0]).is_nan());
        assert!(date_cell_timestamp(utc_month).is_nan());
    }

    fn args(vals: &[f64]) -> f64 {
        js_date_utc(vals.as_ptr(), vals.len() as i32)
    }

    #[test]
    fn test_date_utc_defaults_and_rebasing() {
        // #2826
        assert!(args(&[]).is_nan());
        assert_eq!(args(&[2020.0]), 1_577_836_800_000.0);
        assert_eq!(args(&[2020.0, 0.0]), 1_577_836_800_000.0);
        assert_eq!(args(&[2020.0, 0.0, 1.0]), 1_577_836_800_000.0);
        // day 0 → previous day
        assert_eq!(args(&[2020.0, 0.0, 0.0]), 1_577_750_400_000.0);
        // year 0..99 → 1900+year
        assert_eq!(args(&[0.0, 0.0, 1.0]), -2_208_988_800_000.0);
        assert_eq!(args(&[99.0, 0.0, 1.0]), 915_148_800_000.0);
        // year 100 is literal
        assert_eq!(args(&[100.0, 0.0, 1.0]), -59_011_459_200_000.0);
        // month overflow rolls into next year
        assert_eq!(args(&[2020.0, 12.0, 1.0]), 1_609_459_200_000.0);
        // NaN arg → Invalid
        assert!(args(&[f64::NAN]).is_nan());
    }

    #[test]
    fn test_date_parse_grammar() {
        // #2827 — timezone-deterministic forms only.
        assert_eq!(
            parse_date_string("2020-01-02T03:04:05.006Z"),
            1_577_934_245_006.0
        );
        assert_eq!(parse_date_string("2020-01-02"), 1_577_923_200_000.0);
        assert_eq!(
            parse_date_string("2020-01-02T03:04:05+02:30"),
            1_577_925_245_000.0
        );
        assert_eq!(parse_date_string("Thu, 01 Jan 1970 00:00:00 GMT"), 0.0);
        assert_eq!(parse_date_string("01 Jan 1970 00:00:00 GMT"), 0.0);
        assert_eq!(parse_date_string("2020"), 1_577_836_800_000.0);
        assert!(parse_date_string("not a date").is_nan());
    }

    #[test]
    fn test_setter_optional_args() {
        // #2851 — setUTCFullYear(year, month, date)
        let d = alloc_date_cell(1_577_934_245_006.0); // 2020-01-02T03:04:05.006Z
        let r = set_utc(d, 0, &[2021.0, 5.0, 7.0]);
        assert_eq!(r, 1_623_035_045_006.0);
        assert_eq!(date_cell_timestamp(d), 1_623_035_045_006.0);

        // setUTCHours(h, m, s, ms)
        let d = alloc_date_cell(1_577_934_245_006.0);
        let r = set_utc(d, 3, &[8.0, 9.0, 10.0, 11.0]);
        assert_eq!(r, 1_577_952_550_011.0);

        // setUTCMinutes(m, s, ms)
        let d = alloc_date_cell(1_577_934_245_006.0);
        let r = set_utc(d, 4, &[9.0, 10.0, 11.0]);
        assert_eq!(r, 1_577_934_550_011.0);

        // setUTCHours() with no args → NaN / Invalid
        let d = alloc_date_cell(1_577_934_245_006.0);
        assert!(set_utc(d, 3, &[]).is_nan());
        assert!(date_cell_timestamp(d).is_nan());

        // omitted trailing args keep current fields
        let d = alloc_date_cell(1_577_934_245_006.0);
        let r = set_utc(d, 3, &[8.0]); // only hour
        assert_eq!(r, 1_577_952_245_006.0); // 2020-01-02T08:04:05.006Z

        // leading undefined → NaN
        let d = alloc_date_cell(1_577_934_245_006.0);
        assert!(set_utc(d, 3, &[undef()]).is_nan());
    }
}
