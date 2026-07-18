//! ECMA-402 time-zone resolution for `Intl.DateTimeFormat` and the
//! `toLocaleString` family. Split out of `intl.rs` to keep it under the
//! 2000-line cap. `use super::*` preserves access to the parent's helpers
//! (`get_option_string`, `throw_range_error`, the offset-zone validators).

use super::*;

/// Resolve the effective time zone for a `toLocaleString`-style call and for the
/// `Intl.DateTimeFormat` constructor: the `timeZone` option if present,
/// otherwise the host zone (ECMA-402 DefaultTimeZone), canonicalized.
///
/// An explicit `timeZone` option that names no recognized zone is a RangeError
/// (ECMA-402 CreateDateTimeFormat / ToLocaleString). But DefaultTimeZone — no
/// option given — must ALWAYS yield a valid zone: an unrecognized host zone (a
/// broken `TZ`) falls back to `"UTC"`, never throws.
pub(crate) fn resolved_date_time_zone(options: f64) -> String {
    let explicit = get_option_string(options, "timeZone");
    let tz = explicit
        .clone()
        .unwrap_or_else(|| crate::date::host_time_zone_name().to_string());
    let canonical = if matches!(tz.as_bytes().first(), Some(b'+') | Some(b'-')) {
        is_valid_offset_time_zone(&tz).then(|| canonicalize_offset_time_zone(&tz))
    } else {
        canonicalize_named_time_zone(&tz)
    };
    match canonical {
        Some(c) => c,
        None if explicit.is_some() => {
            throw_range_error(&format!("Invalid time zone specified: {tz}"))
        }
        None => "UTC".to_string(),
    }
}

/// Structurally validate + canonicalize a named IANA time zone. Perry has no
/// tz database, so this checks the identifier shape (and a table of legacy
/// single-component zones) rather than membership. Returns `None` for a
/// malformed / unrecognized identifier.
pub(crate) fn canonicalize_named_time_zone(tz: &str) -> Option<String> {
    if tz.eq_ignore_ascii_case("UTC") || tz.eq_ignore_ascii_case("Etc/UTC") {
        return Some("UTC".to_string());
    }
    if !tz.is_ascii() {
        return None;
    }
    // Legacy single-component IANA zones / links that carry no '/'.
    const SINGLE_WORD_ZONES: &[&str] = &[
        "GMT",
        "GMT0",
        "Zulu",
        "Universal",
        "UCT",
        "Greenwich",
        "Navajo",
        "Eire",
        "Iceland",
        "Cuba",
        "Egypt",
        "Hongkong",
        "Iran",
        "Israel",
        "Japan",
        "Jamaica",
        "Libya",
        "Poland",
        "Portugal",
        "PRC",
        "Singapore",
        "Turkey",
        "ROC",
        "ROK",
        "W-SU",
        "Factory",
        "EST",
        "MST",
        "HST",
        "EST5EDT",
        "CST6CDT",
        "MST7MDT",
        "PST8PDT",
    ];
    if SINGLE_WORD_ZONES.iter().any(|z| z.eq_ignore_ascii_case(tz)) {
        return Some(tz.to_string());
    }
    let segments: Vec<&str> = tz.split('/').collect();
    if segments.len() < 2 {
        return None;
    }
    let mut has_alpha = false;
    for seg in &segments {
        if seg.is_empty() {
            return None;
        }
        for b in seg.bytes() {
            if b.is_ascii_alphabetic() {
                has_alpha = true;
            } else if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'+' || b == b'-') {
                return None;
            }
        }
    }
    has_alpha.then(|| tz.to_string())
}
