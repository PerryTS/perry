//! Unit tests for `date.rs`, split out to keep it under the 2000-line
//! cap. `use super::*` resolves against the parent module.

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

#[cfg(feature = "intl-datetime")]
#[test]
fn compiled_tzdb_resolves_named_zone_and_dst() {
    assert_eq!(
        canonicalize_tzdb_name("europe/berlin").as_deref(),
        Some("Europe/Berlin")
    );
    assert_eq!(canonicalize_tzdb_name("Mars/Olympus"), None);

    // 2026-01-07T06:05Z is CET (+01:00); 2026-09-07T06:05Z is
    // CEST (+02:00). Both are explicit non-host zone lookups.
    assert_eq!(zone_offset_seconds("Europe/Berlin", 1_767_765_900), 3_600);
    assert_eq!(zone_offset_seconds("Europe/Berlin", 1_788_761_100), 7_200);
}

#[test]
fn utc_getters_ignore_process_timezone() {
    const CHILD_MARKER: &str = "PERRY_DATE_UTC_GETTER_CHILD";
    if std::env::var_os(CHILD_MARKER).is_some() {
        // 2025-06-20T00:00:00.000Z is still June 19 in Los Angeles.
        // The three UTC calendar getters must nevertheless keep the UTC
        // date, while the old delegation to local getters returned 19.
        // The `get_date == 19` assertion proves the child's TZ actually
        // took effect (subject-is-live guard, not part of the regression).
        let timestamp = 1_750_377_600_000.0;
        assert_eq!(js_date_get_date(timestamp), 19.0);
        assert_eq!(js_date_get_utc_full_year(timestamp), 2025.0);
        assert_eq!(js_date_get_utc_month(timestamp), 5.0);
        assert_eq!(js_date_get_utc_date(timestamp), 20.0);
        return;
    }

    // The UCRT's `TZ` parser understands only the POSIX `tzn[+-]hh dzn`
    // shape — an IANA id like `America/Los_Angeles` silently degrades to
    // UTC on Windows (#7356), which would fail the subject-is-live guard
    // above. Use the spelling each platform's localtime honors; both put
    // the child in US-Pacific time (June ⇒ UTC-7 under DST).
    #[cfg(windows)]
    let tz = "PST8PDT";
    #[cfg(not(windows))]
    let tz = "America/Los_Angeles";

    let status = std::process::Command::new(std::env::current_exe().expect("current test exe"))
        .arg("date::tests::utc_getters_ignore_process_timezone")
        .arg("--exact")
        .env("TZ", tz)
        .env(CHILD_MARKER, "1")
        .status()
        .expect("spawn timezone-isolated date getter test");
    assert!(status.success(), "timezone-isolated child failed");
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

/// #9449: ECMA-262 §21.4.3.2 reads an offsetless date-TIME as LOCAL wall
/// clock and an offsetless date-ONLY form as UTC. The local half is
/// asserted through the local-component decoder so the test is host-zone
/// independent; the UTC half and the explicit designators are fixed
/// epochs and are asserted directly.
#[test]
fn test_date_parse_iso_offsetless_datetime_is_local() {
    let wall = |s: &str| {
        let ts = parse_date_string(s);
        assert!(!ts.is_nan(), "expected a valid date for {s:?}");
        let (y, mo, d, h, mi, sec, _) = timestamp_to_local_components((ts as i64).div_euclid(1000));
        (y, mo, d, h, mi, sec)
    };
    // Both separators, every clock precision.
    assert_eq!(wall("2026-09-01T10:30"), (2026, 9, 1, 10, 30, 0));
    assert_eq!(wall("2026-09-01T10:30:45"), (2026, 9, 1, 10, 30, 45));
    assert_eq!(wall("2026-09-01T10:30:45.123"), (2026, 9, 1, 10, 30, 45));
    assert_eq!(wall("2026-09-01 10:30"), (2026, 9, 1, 10, 30, 0));
    assert_eq!(wall("2026-09-01 10:30:45.123"), (2026, 9, 1, 10, 30, 45));
    assert_eq!(wall("2026-09-01T00:00"), (2026, 9, 1, 0, 0, 0));
    // A January row and a July row: a FIXED offset in place of the offset
    // in effect AT THAT INSTANT breaks one of the two in any DST zone.
    assert_eq!(wall("2026-01-15T10:30"), (2026, 1, 15, 10, 30, 0));
    assert_eq!(wall("2026-07-15T10:30"), (2026, 7, 15, 10, 30, 0));
    // Sub-second precision survives the conversion (zone offsets are whole
    // minutes, so the millisecond is untouched in every host zone).
    assert_eq!(parse_date_string("2026-09-01T10:30:45.123") % 1000.0, 123.0);

    // The date-ONLY half must stay UTC, and an explicit designator must
    // keep winning in the date-time form.
    assert_eq!(parse_date_string("2020-01-02"), 1_577_923_200_000.0);
    assert_eq!(
        parse_date_string("2020-01-02T03:04:05.006Z"),
        1_577_934_245_006.0
    );
    assert_eq!(
        parse_date_string("2020-01-02T03:04:05+02:30"),
        1_577_925_245_000.0
    );
    // A trailing GMT / UTC / UT / Z WORD is a designator too — node
    // accepts it in the space-separated spelling, and it was silently
    // ignored here while every offsetless form was read as UTC.
    for s in [
        "2020-01-02 03:04:05 GMT",
        "2020-01-02 03:04:05 UTC",
        "2020-01-02 03:04:05 ut",
        "2020-01-02 03:04:05 z",
        "2020-01-02 03:04:05 Z",
    ] {
        assert_eq!(parse_date_string(s), 1_577_934_245_000.0, "{s:?}");
    }
    // ...but a trailing parenthesised comment is not, so it stays local.
    assert_eq!(wall("2026-09-01 10:30 (comment)"), (2026, 9, 1, 10, 30, 0));
}

/// #9509: the ISO/space parser must consume its complete tail. V8 accepts a
/// fixed set of zone and meridiem tokens; an unknown or unseparated word is
/// Invalid Date rather than ignored.
#[test]
fn test_date_parse_iso_tail_tokens_are_consumed() {
    let wall = |s: &str| {
        let ts = parse_date_string(s);
        assert!(!ts.is_nan(), "expected a valid date for {s:?}");
        let (y, mo, d, h, mi, sec, _) = timestamp_to_local_components((ts as i64).div_euclid(1000));
        (y, mo, d, h, mi, sec)
    };

    assert_eq!(wall("2026-09 10:30"), (2026, 9, 1, 10, 30, 0));
    assert_eq!(wall("2026-09-01  10:30"), (2026, 9, 1, 10, 30, 0));
    assert_eq!(wall("2026-09-01 10:30 PM"), (2026, 9, 1, 22, 30, 0));
    assert_eq!(wall("2026-09-01 12:30 AM"), (2026, 9, 1, 0, 30, 0));
    assert_eq!(wall("2026-09-01 12:30 PM"), (2026, 9, 1, 12, 30, 0));

    let midnight = 1_788_220_800_000.0;
    for s in ["2026-09-01 GMT", "2026-09-01 Z", "2026-09-01Z"] {
        assert_eq!(parse_date_string(s), midnight, "{s:?}");
    }
    assert_eq!(
        parse_date_string("2026-09-01 EST"),
        midnight + 5.0 * 3_600_000.0
    );
    assert_eq!(
        parse_date_string("2026-09-01 PDT"),
        midnight + 7.0 * 3_600_000.0
    );
    assert_eq!(
        parse_date_string("2026-09-01 10:30 PM EST"),
        midnight + 27.5 * 3_600_000.0
    );
    assert_eq!(
        parse_date_string("2026-09-01 12:30 AM PST"),
        midnight + 8.5 * 3_600_000.0
    );

    for bad in [
        "2026-09-01 10:30GMT",
        "2026-09-01 10:30EST",
        "2026-09-01 10:30PM",
        "2026-09-01 10:30 XYZ",
        "2026-09-01 10:30:45oops",
    ] {
        assert!(
            parse_date_string(bad).is_nan(),
            "expected Invalid Date for {bad:?}"
        );
    }
}

/// #9414: the numeric slash grammar node accepts as its
/// implementation-defined format. Measured against
/// `node --experimental-strip-types`, not derived from the spec (which
/// leaves this format undefined).
#[test]
fn test_date_parse_slash_grammar() {
    // Zone-designated forms are timezone-deterministic, so they can be
    // pinned to an exact instant.
    assert_eq!(parse_date_string("2026/09/01 GMT"), 1_788_220_800_000.0);
    assert_eq!(parse_date_string("09/01/2026 GMT"), 1_788_220_800_000.0);
    assert_eq!(parse_date_string("2026/9 GMT"), 1_788_220_800_000.0);
    assert_eq!(
        parse_date_string("2026/9/1 10:30:45.123 UTC"),
        1_788_258_645_123.0
    );
    assert_eq!(
        parse_date_string("2026/09/01 10:30 GMT+0500"),
        1_788_240_600_000.0
    );
    // Two-digit years: 0..=49 → 2000s, 50..=99 → 1900s.
    assert_eq!(parse_date_string("09/01/26 GMT"), 1_788_220_800_000.0);
    assert_eq!(parse_date_string("99/1/1 GMT"), 915_148_800_000.0);
    assert_eq!(parse_date_string("1/2/3 GMT"), 1_041_465_600_000.0);
    // A day past the end of its month ROLLS OVER (node: 2 March 2026),
    // while an out-of-range month or a zero day is Invalid Date.
    assert_eq!(parse_date_string("2026/02/30 GMT"), 1_772_409_600_000.0);
    assert_eq!(parse_date_string("2026/09/31 GMT"), 1_790_812_800_000.0);
    // Clock edge cases: 24:00 rolls to the next midnight, 25:00 is Invalid.
    assert_eq!(
        parse_date_string("2026/09/01 24:00 GMT"),
        1_788_307_200_000.0
    );
    assert_eq!(
        parse_date_string("2026/09/01 3:04 PM GMT"),
        1_788_275_040_000.0
    );
    // A named month may also be slash-separated.
    assert_eq!(parse_date_string("Sep/01/2026 GMT"), 1_788_220_800_000.0);

    for bad in [
        "2026/13/01",       // month out of range
        "2026/00/01",       // month zero
        "2026/09/00",       // day zero
        "2026/09/32",       // day out of range
        "31/1/2026",        // 31 read as a month
        "13/1/2026",        // 13 read as a month
        "0/1/2026",         // Y/M/D with 2026 as the day
        "9/2026",           // M/D with 2026 as the day
        "2026/1/2/3",       // a fourth component
        "2026/09/01T10:30", // 'T' is ISO-only, not this grammar
        "2026/09/01 25:00", // hour out of range
        "2026/09/01 10:60", // minute out of range
        "2026/09/01 +0500", // bare offset with no clock
    ] {
        assert!(
            parse_date_string(bad).is_nan(),
            "expected Invalid Date for {bad:?}"
        );
    }

    // Without a zone designator the components are LOCAL wall clock — the
    // property that separates this branch from the ISO one. Assert it
    // through the local-component decoder so the test is host-zone
    // independent.
    let ts = parse_date_string("2026/09/01 10:30:45");
    assert!(!ts.is_nan());
    let (y, mo, d, h, mi, s, _) = timestamp_to_local_components((ts as i64).div_euclid(1000));
    assert_eq!((y, mo, d, h, mi, s), (2026, 9, 1, 10, 30, 45));
    let midnight = parse_date_string("09/01/2026");
    let (y, mo, d, h, mi, s, _) = timestamp_to_local_components((midnight as i64).div_euclid(1000));
    assert_eq!((y, mo, d, h, mi, s), (2026, 9, 1, 0, 0, 0));
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
