//! Date string parsing (`Date.parse` / `new Date(string)` support).
//!
//! Extracted from the parent `date` module (the #4680-adjacent file-size
//! split, keeping `date.rs` under the 2,000-line CI cap). Holds the ISO 8601 /
//! MySQL and RFC-1123 / IETF / month-name string grammars. `parse_date_string`
//! is the only entry point the parent calls; the per-grammar helpers stay
//! private here. Shared time math (`make_utc_ms`, `time_clip`,
//! `timestamp_to_local_components`) lives in the parent and is reached via
//! `super::` (a child module can see its ancestor's private items).

use super::{make_utc_ms, time_clip, timestamp_to_local_components};

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
pub(super) fn parse_date_string(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return f64::NAN;
    }

    // Date.parse always TimeClips: a parsed instant outside ±8.64e15 ms (the
    // supported Date range) is Invalid (`Date.parse("-271821-04-19T23:59:59.999Z")`
    // → NaN, one ms below the minimum; test262 Date/parse/time-value-maximum-range).
    if let Some(ts) = parse_iso8601(s) {
        return time_clip(ts);
    }
    if let Some(ts) = parse_rfc_or_named(s) {
        return time_clip(ts);
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
    // Year: either a 4-digit "YYYY" or an expanded "±YYYYYY" (mandatory sign,
    // exactly 6 digits) per the ECMAScript Date Time String Format. "-000000"
    // is explicitly NOT a valid representation (negative-zero year), so it is
    // rejected. (test262 Date/{parse,prototype/toString}/...-year, where
    // `new Date('-000001-07-01T00:00Z')` must parse, not yield Invalid Date.)
    let (year, year_end): (i64, usize) = if b.first() == Some(&b'+') || b.first() == Some(&b'-') {
        if b.len() < 7 || !b[1..7].iter().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let mag: i64 = s[1..7].parse().ok()?;
        if b[0] == b'-' {
            if mag == 0 {
                return None;
            }
            (-mag, 7)
        } else {
            (mag, 7)
        }
    } else {
        if b.len() < 4 || !b[0..4].iter().all(|c| c.is_ascii_digit()) {
            return None;
        }
        (s[0..4].parse().ok()?, 4)
    };
    let mut month1: u32 = 1;
    let mut day: i64 = 1;
    let mut hour: i64 = 0;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    let mut millis: i64 = 0;
    let mut idx = year_end;

    // Year only ("YYYY" / "±YYYYYY").
    if s.len() == year_end {
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
    if b.get(year_end) != Some(&b'-') {
        return None;
    }
    if b.len() < year_end + 3 {
        return None;
    }
    month1 = s[year_end + 1..year_end + 3].parse().ok()?;
    if !(1..=12).contains(&month1) {
        return None;
    }
    idx = year_end + 3;
    let mut has_day = false;
    if b.get(idx) == Some(&b'-') {
        if b.len() < idx + 3 {
            return None;
        }
        day = s[idx + 1..idx + 3].parse().ok()?;
        if !(1..=31).contains(&day) {
            return None;
        }
        idx += 3;
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
