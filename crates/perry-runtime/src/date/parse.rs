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
///   - #9414: the numeric slash forms node also accepts — "2026/09/01",
///     "2026/9/1", "09/01/2026", with an optional trailing clock and zone.
///     These are LOCAL time, not UTC (see `parse_slash_date`).
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
    if let Some(ts) = parse_slash_date(s) {
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
    let mut idx = year_end + 3;
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

/// Node/V8 accept a purely numeric, slash-separated date — `"2026/09/01"`,
/// `"2026/9/1"`, `"09/01/2026"` — as the ECMA-262 §21.4.3.2
/// "implementation-defined format". The spec deliberately says nothing about
/// it, so this branch reproduces V8's `DateParser::DayComposer::Write`
/// measured against `node --experimental-strip-types`, not a reading of the
/// standard:
///
///   * Up to three numeric components are collected in source order and
///     padded with `1`. If the FIRST one is not a valid day-of-month
///     (`1..=31`) the triple is Y/M/D, otherwise it is M/D/Y — which is what
///     makes `"2026/09/01"` year-first and `"09/01/2026"` US month-first
///     without any lookahead.
///   * A year in `0..=49` maps to `2000..=2049`, one in `50..=99` to
///     `1950..=1999` (so `"09/01/26"` is 2026 and `"99/1/1"` is 1999).
///   * The month must be `1..=12` and the day `1..=31`, but a day past the
///     end of its month ROLLS OVER rather than failing: node's
///     `new Date("2026/02/30")` is 2 March 2026. `"2026/13/01"` and
///     `"2026/09/00"` are Invalid Date.
///   * With no zone designator the components are LOCAL wall-clock time —
///     unlike the ISO branch above, which is UTC. This is the difference
///     that makes `new Date("2026/09/01").getHours() === 0` everywhere.
///
/// Only reached when the input actually contains a `/`, so the ISO,
/// RFC-1123 and month-name grammars above keep their existing behaviour
/// untouched.
fn parse_slash_date(s: &str) -> Option<f64> {
    if !s.contains('/') {
        return None;
    }
    // `/` and `,` are both separators here ("2026/09/01,10:30" parses), so
    // flatten them to spaces and work token-by-token like the RFC branch.
    let normalized = s.replace([',', '/'], " ");

    let mut comps: Vec<i64> = Vec::new();
    let mut named_month: Option<i64> = None;
    let mut hour: i64 = 0;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    let mut millis: i64 = 0;
    let mut saw_time = false;
    let mut pm: Option<bool> = None;
    let mut tz_minutes_east: Option<i64> = None;

    for tok in normalized.split_whitespace() {
        let low = tok.to_ascii_lowercase();
        // Parenthesized trailing comment — `"2026/09/01 (comment)"` is valid.
        if tok.starts_with('(') {
            continue;
        }
        // Weekday name (never a month name, never a number).
        if ["sun", "mon", "tue", "wed", "thu", "fri", "sat"]
            .iter()
            .any(|w| low.starts_with(w))
            && month_from_name(tok).is_none()
        {
            continue;
        }
        if let Some(m) = month_from_name(tok) {
            if named_month.is_some() {
                return None;
            }
            named_month = Some(m as i64);
            continue;
        }
        if tok.contains(':') {
            if saw_time {
                return None;
            }
            let parts: Vec<&str> = tok.split(':').collect();
            if parts.len() < 2 || parts.len() > 3 {
                return None;
            }
            hour = parts[0].parse().ok()?;
            minute = parts[1].parse().ok()?;
            if let Some(sec_tok) = parts.get(2) {
                let (whole, frac) = match sec_tok.split_once('.') {
                    Some((w, f)) => (w, Some(f)),
                    None => (*sec_tok, None),
                };
                second = whole.parse().ok()?;
                if let Some(frac) = frac {
                    if frac.is_empty() || !frac.bytes().all(|c| c.is_ascii_digit()) {
                        return None;
                    }
                    millis = normalize_millis(frac);
                }
            }
            saw_time = true;
            continue;
        }
        if low == "am" || low == "a.m." {
            pm = Some(false);
            continue;
        }
        if low == "pm" || low == "p.m." {
            pm = Some(true);
            continue;
        }
        if low == "gmt" || low == "utc" || low == "ut" || low == "z" {
            tz_minutes_east = Some(0);
            continue;
        }
        if low.starts_with("gmt") || low.starts_with("utc") {
            let off = parse_tz_offset(&tok[3..])?;
            if off != i64::MAX {
                tz_minutes_east = Some(off);
            }
            continue;
        }
        // A bare `+HHMM` / `-HH:MM` is a zone designator only AFTER a clock
        // has been read; before one, V8 treats the sign as a separator and the
        // digits as another date component, which is why node's
        // `new Date("2026/09/01 +0500")` is Invalid Date (a fourth component)
        // while `new Date("2026/09/01 10:30 +0500")` is not.
        if saw_time && (tok.starts_with('+') || tok.starts_with('-')) && tok.len() >= 3 {
            let off = parse_tz_offset(tok)?;
            if off != i64::MAX {
                tz_minutes_east = Some(off);
            }
            continue;
        }
        // Numeric date component. A leading sign is a separator, not part of
        // the number (`new Date("-2026/09/01")` is the same instant as
        // `new Date("2026/09/01")` in node).
        let digits = tok.trim_start_matches(['+', '-']);
        if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
            if comps.len() == 3 {
                return None;
            }
            comps.push(digits.parse().ok()?);
            continue;
        }
        // Anything else (a stray word, a named zone abbreviation) is not part
        // of this grammar.
        return None;
    }

    if comps.is_empty() {
        return None;
    }
    // Day and month default to 1 (V8 `DayComposer::Write`).
    while comps.len() < 3 {
        comps.push(1);
    }
    let is_day = |x: i64| (1..=31).contains(&x);
    let (mut year, month, day) = match named_month {
        None => {
            if is_day(comps[0]) {
                // M/D/Y
                (comps[2], comps[0], comps[1])
            } else {
                // Y/M/D
                (comps[0], comps[1], comps[2])
            }
        }
        Some(m) => {
            if is_day(comps[0]) {
                (comps[1], m, comps[0])
            } else {
                (comps[0], m, comps[1])
            }
        }
    };
    if (0..=49).contains(&year) {
        year += 2000;
    } else if (50..=99).contains(&year) {
        year += 1900;
    }
    if !(1..=12).contains(&month) || !is_day(day) {
        return None;
    }

    // Clock validation (V8 `TimeComposer::Write`): 24:00:00.000 is the only
    // hour-24 form accepted, and `am`/`pm` require a 12-hour clock.
    match pm {
        Some(is_pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour = if is_pm {
                if hour == 12 {
                    12
                } else {
                    hour + 12
                }
            } else if hour == 12 {
                0
            } else {
                hour
            };
        }
        None => {
            if hour > 24 || (hour == 24 && (minute != 0 || second != 0 || millis != 0)) {
                return None;
            }
        }
    }
    if minute > 59 || second > 59 {
        return None;
    }

    let base = make_utc_ms(year, month - 1, day, hour, minute, second, millis);
    match tz_minutes_east {
        Some(off) => Some(base - (off * 60_000) as f64),
        None => {
            // No designator: the components are LOCAL wall-clock time.
            let secs = (base as i64).div_euclid(1000);
            let (_, _, _, _, _, _, tz_offset) = timestamp_to_local_components(secs);
            Some(base - (tz_offset * 1000) as f64)
        }
    }
}
