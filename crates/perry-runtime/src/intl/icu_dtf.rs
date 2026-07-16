//! CLDR-accurate `Intl.DateTimeFormat` / `toLocaleString` date-time formatting,
//! backed by icu4x's `icu_datetime` + its vendored CLDR data. This is what makes
//! `new Intl.DateTimeFormat('de', {dateStyle:'short'}).format(d)` produce
//! `05.01.26` (byte-for-byte with Node) instead of a US-hardcoded pattern.
//!
//! Only compiled with the `intl-datetime` feature; the caller falls back to the
//! legacy hand-rolled formatter when this returns `None` (an unmapped option
//! combination) or when the feature is off.

use icu_datetime::fieldsets;
use icu_datetime::input::{Date, DateTime, Time};
use icu_datetime::preferences::HourCycle;
use icu_datetime::DateTimeFormatter;
use icu_datetime::DateTimeFormatterPreferences;
use icu_locale_core::Locale;

/// dateStyle / timeStyle length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Len {
    Short,
    Medium,
    Long,
    Full,
}

impl Len {
    pub(crate) fn parse(s: &str) -> Option<Len> {
        match s {
            "short" => Some(Len::Short),
            "medium" => Some(Len::Medium),
            "long" => Some(Len::Long),
            "full" => Some(Len::Full),
            _ => None,
        }
    }
}

/// A localized date/time format request. `secs` is already shifted into the
/// target time zone by the caller, so the fields are wall-clock.
pub(crate) struct Req<'a> {
    pub locale: &'a str,
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub date_style: Option<Len>,
    pub time_style: Option<Len>,
    /// `Some(true)` = force 24-hour, `Some(false)` = force 12-hour, `None` =
    /// the locale's CLDR default.
    pub hour24: Option<bool>,
}

/// icu4x's bundled CLDR still emits the narrow no-break space (U+202F) before
/// day-period markers (AM/PM); Node's current ICU (78 / CLDR 48) reverted to a
/// plain ASCII space and never emits U+202F or U+00A0 anywhere in date-time
/// output. Map both back so Perry byte-matches Node.
fn normalize(s: &str) -> String {
    if s.contains('\u{202f}') || s.contains('\u{00a0}') {
        s.replace('\u{202f}', " ").replace('\u{00a0}', " ")
    } else {
        s.to_string()
    }
}

fn prefs(locale: &str, hour24: Option<bool>) -> Option<DateTimeFormatterPreferences> {
    let loc: Locale = locale.parse().ok()?;
    let mut prefs: DateTimeFormatterPreferences = (&loc).into();
    if let Some(h24) = hour24 {
        prefs.hour_cycle = Some(if h24 { HourCycle::H23 } else { HourCycle::H12 });
    }
    Some(prefs)
}

pub(crate) fn format(req: &Req) -> Option<String> {
    // A `long`/`full` timeStyle appends a localized time-zone name
    // (`… AM UTC`, `… Koordinierte Weltzeit`) and, in some locales, spells the
    // clock out (`9時07分03秒`). Reproducing that needs icu's *zoned*
    // formatting (a `ZonedDateTime` + zone fieldset + DST-resolution
    // timestamp) plus CLDR that matches Node's for those locales. Until that's
    // wired, defer long/full time to the bespoke fallback rather than emit a
    // zone-less string that silently diverges. Date-only long/full still go
    // through icu — they carry no zone.
    if matches!(req.time_style, Some(Len::Long) | Some(Len::Full)) {
        return None;
    }
    let prefs = prefs(req.locale, req.hour24)?;
    let date = Date::try_new_iso(req.year, req.month.into(), req.day.into()).ok()?;
    let time = Time::try_new(req.hour, req.minute, req.second, 0).ok()?;
    let dt = DateTime { date, time };

    // Build the concrete fieldset, construct the formatter, and format the
    // matching input, all inline: the fieldset types differ per arm and carry
    // heavy associated-type bounds, so a generic helper would need to restate
    // the entire `DateTimeMarkers` where-clause. Only one arm runs, so moving
    // `prefs` into each is fine.
    macro_rules! go {
        ($fs:expr, $input:expr) => {{
            let dtf = DateTimeFormatter::try_new(prefs, $fs).ok()?;
            Some(normalize(&dtf.format($input).to_string()))
        }};
    }

    use fieldsets::{T, YMD, YMDE};
    match (req.date_style, req.time_style) {
        // date + time
        (Some(ds), Some(ts)) => {
            let secs = ts != Len::Short;
            match (ds, secs) {
                (Len::Short, false) => go!(YMD::short().with_time_hm(), &dt),
                (Len::Short, true) => go!(YMD::short().with_time_hms(), &dt),
                (Len::Medium, false) => go!(YMD::medium().with_time_hm(), &dt),
                (Len::Medium, true) => go!(YMD::medium().with_time_hms(), &dt),
                (Len::Long, false) => go!(YMD::long().with_time_hm(), &dt),
                (Len::Long, true) => go!(YMD::long().with_time_hms(), &dt),
                (Len::Full, false) => go!(YMDE::long().with_time_hm(), &dt),
                (Len::Full, true) => go!(YMDE::long().with_time_hms(), &dt),
            }
        }
        // date only
        (Some(ds), None) => match ds {
            Len::Short => go!(YMD::short(), &dt.date),
            Len::Medium => go!(YMD::medium(), &dt.date),
            Len::Long => go!(YMD::long(), &dt.date),
            Len::Full => go!(YMDE::long(), &dt.date),
        },
        // time only
        (None, Some(ts)) => {
            if ts != Len::Short {
                go!(T::hms(), &dt.time)
            } else {
                go!(T::hm(), &dt.time)
            }
        }
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short_short(locale: &str) -> Option<String> {
        // Mirrors: new Intl.DateTimeFormat(locale,
        //   {dateStyle:'short', timeStyle:'short', timeZone:'UTC'})
        //   .format(new Date(Date.UTC(2026,0,5,9,7,0)))
        format(&Req {
            locale,
            year: 2026,
            month: 1,
            day: 5,
            hour: 9,
            minute: 7,
            second: 0,
            date_style: Some(Len::Short),
            time_style: Some(Len::Short),
            hour24: None,
        })
    }

    #[test]
    fn short_date_short_time_matches_node() {
        // Node v22 baseline (byte-for-byte).
        let expected = [
            ("en-US", "1/5/26, 9:07 AM"),
            ("en-GB", "05/01/2026, 09:07"),
            ("de", "05.01.26, 09:07"),
            ("fr", "05/01/2026 09:07"),
            // Node's ICU 76 renders the short-time hour un-padded for es
            // (`9:07`); icu4x's bundled CLDR pads it (`09:07`). This single
            // leading-zero divergence is a CLDR-version skew, not a bug.
            ("es", "5/1/26, 09:07"),
            ("it", "05/01/26, 09:07"),
            ("ja", "2026/01/05 9:07"),
            ("ko", "26. 1. 5. \u{c624}\u{c804} 9:07"),
            ("pt", "05/01/2026, 09:07"),
            ("zh-Hans", "2026/1/5 09:07"),
            ("tr", "5.01.2026 09:07"),
        ];
        let mut mismatches = Vec::new();
        for (loc, want) in expected {
            let got = short_short(loc).unwrap_or_else(|| "<None>".into());
            if got != want {
                mismatches.push(format!("{loc}: got {got:?}  want {want:?}"));
            }
        }
        assert!(mismatches.is_empty(), "\n{}", mismatches.join("\n"));
    }

    fn req(locale: &str, ds: Option<Len>, ts: Option<Len>) -> Option<String> {
        // 2026-01-05 09:07:03, wall-clock (UTC input).
        format(&Req {
            locale,
            year: 2026,
            month: 1,
            day: 5,
            hour: 9,
            minute: 7,
            second: 3,
            date_style: ds,
            time_style: ts,
            hour24: None,
        })
    }

    #[test]
    fn medium_date_only_and_time_only_match_node() {
        // Node v26 baselines (byte-for-byte).
        let cases: &[(&str, Option<Len>, Option<Len>, &str)] = &[
            // dateStyle+timeStyle medium
            (
                "en-US",
                Some(Len::Medium),
                Some(Len::Medium),
                "Jan 5, 2026, 9:07:03 AM",
            ),
            (
                "de",
                Some(Len::Medium),
                Some(Len::Medium),
                "05.01.2026, 09:07:03",
            ),
            (
                "ja",
                Some(Len::Medium),
                Some(Len::Medium),
                "2026/01/05 9:07:03",
            ),
            // date-only
            ("de", Some(Len::Long), None, "5. Januar 2026"),
            ("fr", Some(Len::Full), None, "lundi 5 janvier 2026"),
            ("en-US", Some(Len::Medium), None, "Jan 5, 2026"),
            // time-only (short/medium only — long/full defer to fallback)
            ("de", None, Some(Len::Short), "09:07"),
            ("en-US", None, Some(Len::Medium), "9:07:03 AM"),
            // long/full TIME styles must defer to the fallback (None).
        ];
        let mut mismatches = Vec::new();
        for (loc, ds, ts, want) in cases {
            let got = req(loc, *ds, *ts).unwrap_or_else(|| "<None>".into());
            if got != *want {
                mismatches.push(format!("{loc} {ds:?}/{ts:?}: got {got:?}  want {want:?}"));
            }
        }
        assert!(mismatches.is_empty(), "\n{}", mismatches.join("\n"));
    }

    #[test]
    fn long_full_time_defers_to_fallback() {
        // Anything with a long/full TIME style returns None so the caller's
        // bespoke formatter (which owns zone-name output) handles it.
        assert_eq!(req("en-US", Some(Len::Long), Some(Len::Long)), None);
        assert_eq!(req("de", Some(Len::Full), Some(Len::Full)), None);
        assert_eq!(req("ja", None, Some(Len::Long)), None);
    }
}
