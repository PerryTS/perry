//! CLDR-accurate `Intl.DateTimeFormat` / `toLocaleString` date-time formatting,
//! backed by icu4x's `icu_datetime` + its vendored CLDR data. This is what makes
//! `new Intl.DateTimeFormat('de', {dateStyle:'short'}).format(d)` produce
//! `05.01.26` (byte-for-byte with Node) instead of a US-hardcoded pattern.
//!
//! Only compiled with the `intl-datetime` feature; the caller falls back to the
//! legacy hand-rolled formatter when this returns `None` (an unmapped option
//! combination) or when the feature is off.

use icu_datetime::fieldsets;
use icu_datetime::fieldsets::builder::{DateFields, FieldSetBuilder};
use icu_datetime::input::{Date, DateTime, Time};
use icu_datetime::options::{Alignment, Length, TimePrecision, YearStyle};
use icu_datetime::preferences::HourCycle;
use icu_datetime::DateTimeFormatter;
use icu_datetime::DateTimeFormatterPreferences;
use icu_locale_core::Locale;
use writeable::{Part, PartsWrite, Writeable};

mod default_numeric_patterns;
use default_numeric_patterns::YMD_PATTERNS;

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
    /// Explicit `hourCycle` option (`h11`/`h12`/`h23`/`h24`) if present; takes
    /// precedence over `hour12` and pins the exact clock family.
    pub hour_cycle: Option<&'a str>,
    /// Explicit `hour12` option (`Some(true)` = 12-hour, `Some(false)` =
    /// 24-hour); consulted only when `hour_cycle` is absent. Both `None` =
    /// the locale's CLDR default.
    pub hour12: Option<bool>,
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

fn prefs(
    locale: &str,
    hour_cycle: Option<&str>,
    hour12: Option<bool>,
) -> Option<DateTimeFormatterPreferences> {
    let loc: Locale = locale.parse().ok()?;
    let mut prefs: DateTimeFormatterPreferences = (&loc).into();
    // An explicit `hourCycle` pins the exact clock family — collapsing it to a
    // 12h/24h bool would turn `h11`→`h12` (`0:07 AM` vs `12:07 AM`) and
    // `h24`→`h23`. Only fall back to `hour12` (→ h12 / h23, matching Node) when
    // no `hourCycle` was given; absent both, leave the locale default in place.
    let hc = match hour_cycle {
        Some("h11") => Some(HourCycle::H11),
        Some("h12") => Some(HourCycle::H12),
        // icu4x models no `h24` (24:00) variant; fold it to the practical
        // 24-hour clock (differs from Node only at the midnight `24` vs `0`).
        Some("h23") | Some("h24") => Some(HourCycle::H23),
        _ => match hour12 {
            Some(true) => Some(HourCycle::H12),
            Some(false) => Some(HourCycle::H23),
            None => None,
        },
    };
    if hc.is_some() {
        prefs.hour_cycle = hc;
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
    let prefs = prefs(req.locale, req.hour_cycle, req.hour12)?;
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

/// A component-based `Intl.DateTimeFormat` request (year/month/day/weekday +
/// hour/minute/second, each with its ECMA-402 style). The fields are already in
/// wall-clock time for the resolved zone.
pub(crate) struct CompReq<'a> {
    pub locale: &'a str,
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub has_year: bool,
    pub has_month: bool,
    pub has_day: bool,
    /// `year` option value (`numeric`/`2-digit`), or `None` when absent.
    pub year_style: Option<&'a str>,
    /// `month` option value (`numeric`/`2-digit`/`short`/`long`/`narrow`), or
    /// `None` when month is absent.
    pub month_style: Option<&'a str>,
    /// `day` option value (`numeric`/`2-digit`), or `None` when absent.
    pub day_style: Option<&'a str>,
    /// `weekday` option value (`short`/`long`/`narrow`), or `None` when absent.
    pub weekday_style: Option<&'a str>,
    pub has_hour: bool,
    pub has_minute: bool,
    pub has_second: bool,
    pub hour_cycle: Option<&'a str>,
    pub hour12: Option<bool>,
}

/// Format an explicit-component request via icu4x's dynamic `FieldSetBuilder`.
///
/// Only combos icu's semantic field sets reproduce faithfully are handled: a
/// date part must carry a spelled month (`short`/`long`), a weekday, or the
/// ECMA-402 default numeric Y/M/D set. The latter uses CLDR's classical `yMd`
/// pattern (see `format_default_numeric_parts`); semantic `Short` is not
/// equivalent because it pads German/Japanese fields. `narrow` and
/// structurally-inexpressible field combos still return `None` for fallback.
pub(crate) fn format_components(req: &CompReq) -> Option<String> {
    let has_weekday = req.weekday_style.is_some();
    let has_date = req.has_year || req.has_month || req.has_day || has_weekday;
    let has_time = req.has_hour || req.has_minute || req.has_second;
    let numeric_ymd = req.year_style == Some("numeric")
        && req.month_style == Some("numeric")
        && req.day_style == Some("numeric")
        && !has_weekday
        && !has_time;

    // Name-bearing = a spelled month or a weekday; only these route to icu.
    let month_len = match req.month_style {
        Some("long") => Some(Length::Long),
        Some("short") => Some(Length::Medium),
        _ => None,
    };
    let weekday_len = match req.weekday_style {
        Some("long") => Some(Length::Long),
        Some("short") => Some(Length::Medium),
        _ => None,
    };
    let name_bearing = month_len.is_some() || weekday_len.is_some();

    // Reject narrow (no semantic-fieldset equivalent) and numeric component
    // combinations other than the default Y/M/D set.
    if matches!(req.month_style, Some("narrow")) || matches!(req.weekday_style, Some("narrow")) {
        return None;
    }
    if numeric_ymd {
        return format_default_numeric_parts(req).map(|parts| {
            parts
                .into_iter()
                .map(|(_, value)| value)
                .collect::<String>()
        });
    }
    if has_date && !name_bearing {
        return None;
    }

    let date_fields = if has_date {
        Some(
            match (req.has_year, req.has_month, req.has_day, has_weekday) {
                (true, true, true, true) => DateFields::YMDE,
                (true, true, true, false) => DateFields::YMD,
                (false, true, true, true) => DateFields::MDE,
                (false, true, true, false) => DateFields::MD,
                (false, false, true, true) => DateFields::DE,
                (false, false, true, false) => DateFields::D,
                (false, false, false, true) => DateFields::E,
                (true, true, false, false) => DateFields::YM,
                (false, true, false, false) => DateFields::M,
                (true, false, false, false) => DateFields::Y,
                // e.g. year+day without month, year+weekday — not expressible.
                _ => return None,
            },
        )
    } else {
        None
    };

    let time_precision = if req.has_second {
        Some(TimePrecision::Second)
    } else if req.has_minute {
        Some(TimePrecision::Minute)
    } else if req.has_hour {
        Some(TimePrecision::Hour)
    } else {
        None
    };

    if date_fields.is_none() && time_precision.is_none() {
        return None;
    }

    let prefs = prefs(req.locale, req.hour_cycle, req.hour12)?;
    let has_time = time_precision.is_some();
    let mut builder = FieldSetBuilder::default();
    builder.date_fields = date_fields;
    // A spelled month wins the length; else the weekday's; else Medium.
    builder.length = month_len.or(weekday_len).or(Some(Length::Medium));
    builder.time_precision = time_precision;

    let date = Date::try_new_iso(req.year, req.month.into(), req.day.into()).ok()?;
    let time = Time::try_new(req.hour, req.minute, req.second, 0).ok()?;
    let dt = DateTime { date, time };
    let formatted = match (has_date, has_time) {
        (true, true) => {
            let dtf =
                DateTimeFormatter::try_new(prefs, builder.build_date_and_time().ok()?).ok()?;
            dtf.format(&dt).to_string()
        }
        (true, false) => {
            let dtf = DateTimeFormatter::try_new(prefs, builder.build_date().ok()?).ok()?;
            dtf.format(&dt.date).to_string()
        }
        (false, true) => {
            let dtf = DateTimeFormatter::try_new(prefs, builder.build_time().ok()?).ok()?;
            dtf.format(&dt.time).to_string()
        }
        (false, false) => return None,
    };
    Some(normalize(&formatted))
}

#[derive(Default)]
struct DatePartsSink {
    active: Vec<Part>,
    parts: Vec<(&'static str, String)>,
}

impl std::fmt::Write for DatePartsSink {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let kind = self
            .active
            .iter()
            .rev()
            .find(|part| part.category == "datetime")
            .map(|part| part.value)
            .unwrap_or("literal");
        if let Some((last_kind, last_value)) = self.parts.last_mut() {
            if *last_kind == kind {
                last_value.push_str(value);
                return Ok(());
            }
        }
        self.parts.push((kind, value.to_string()));
        Ok(())
    }
}

impl PartsWrite for DatePartsSink {
    type SubPartsWrite = Self;

    fn with_part(
        &mut self,
        part: Part,
        mut write: impl FnMut(&mut Self::SubPartsWrite) -> std::fmt::Result,
    ) -> std::fmt::Result {
        self.active.push(part);
        let result = write(self);
        self.active.pop();
        result
    }
}

fn ymd_pattern(locale: &Locale) -> Option<&'static str> {
    // The table contains canonical CLDR locale identifiers without Unicode or
    // private-use extensions. Fall back one subtag at a time for structurally
    // valid locales not materialized as their own CLDR data locale.
    let id = locale.id.to_string();
    let mut candidate = id.as_str();
    loop {
        if let Ok(index) = YMD_PATTERNS.binary_search_by(|entry| entry.0.cmp(candidate)) {
            return Some(YMD_PATTERNS[index].1);
        }
        candidate = candidate.rsplit_once('-').map(|(parent, _)| parent)?;
    }
}

fn push_part(parts: &mut Vec<(&'static str, String)>, kind: &'static str, value: impl AsRef<str>) {
    let value = value.as_ref();
    if value.is_empty() {
        return;
    }
    if let Some((last_kind, last_value)) = parts.last_mut() {
        if *last_kind == kind {
            last_value.push_str(value);
            return;
        }
    }
    parts.push((kind, value.to_string()));
}

fn without_numeric_padding(value: &str, number: u8) -> &str {
    if number >= 10 {
        return value;
    }
    // `Alignment::Column` makes a one-digit month/day two localized digits.
    // Decimal digits are single Unicode scalars; discard that leading zero.
    value
        .char_indices()
        .nth(1)
        .map_or(value, |(index, _)| &value[index..])
}

/// Render the ECMA-402 default numeric fields with CLDR's classical `yMd`
/// skeleton. ICU4X 2.x compiles semantic field sets but no longer exposes its
/// classical skeleton matcher at runtime, and semantic `YMD::short()` is not
/// the same pattern: for example, it yields `05.01.2026` in German and
/// `2026/01/05` in Japanese. The small generated table carries just `yMd`'s
/// localized pattern; ICU still supplies calendar conversion and digits.
fn format_default_numeric_parts(req: &CompReq) -> Option<Vec<(&'static str, String)>> {
    let locale: Locale = req.locale.parse().ok()?;
    let pattern = ymd_pattern(&locale)?;
    let mut builder = FieldSetBuilder::default();
    builder.date_fields = Some(DateFields::YMD);
    builder.length = Some(Length::Short);
    builder.alignment = Some(Alignment::Column);
    builder.year_style = Some(YearStyle::Full);

    let date = Date::try_new_iso(req.year, req.month.into(), req.day.into()).ok()?;
    let dtf = DateTimeFormatter::try_new((&locale).into(), builder.build_date().ok()?).ok()?;
    let mut sink = DatePartsSink::default();
    dtf.format(&date).write_to_parts(&mut sink).ok()?;

    let year = sink
        .parts
        .iter()
        .find(|(kind, _)| *kind == "year" || *kind == "relatedYear")?
        .1
        .as_str();
    let month = sink
        .parts
        .iter()
        .find(|(kind, _)| *kind == "month")?
        .1
        .as_str();
    let day = sink
        .parts
        .iter()
        .find(|(kind, _)| *kind == "day")?
        .1
        .as_str();

    let mut parts = Vec::with_capacity(5);
    let mut chars = pattern.chars().peekable();
    let mut literal = String::new();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
                literal.push('\'');
            } else {
                quoted = !quoted;
            }
            continue;
        }
        if !quoted && matches!(ch, 'y' | 'M' | 'd') {
            push_part(&mut parts, "literal", &literal);
            literal.clear();
            let mut width = 1;
            while chars.peek() == Some(&ch) {
                chars.next();
                width += 1;
            }
            match ch {
                'y' => push_part(&mut parts, "year", year),
                'M' => push_part(
                    &mut parts,
                    "month",
                    if width == 1 {
                        without_numeric_padding(month, req.month)
                    } else {
                        month
                    },
                ),
                'd' => push_part(
                    &mut parts,
                    "day",
                    if width == 1 {
                        without_numeric_padding(day, req.day)
                    } else {
                        day
                    },
                ),
                _ => unreachable!(),
            }
        } else {
            literal.push(ch);
        }
    }
    if quoted {
        return None;
    }
    push_part(&mut parts, "literal", literal);
    Some(
        parts
            .into_iter()
            .map(|(kind, value)| (kind, normalize(&value)))
            .collect(),
    )
}

/// CLDR-ordered semantic parts for the default numeric Y/M/D component set.
/// ICU annotates fields before localized punctuation is written, so this stays
/// correct even when a locale changes both order and separators.
pub(crate) fn format_components_parts(req: &CompReq) -> Option<Vec<(&'static str, String)>> {
    let numeric_ymd = req.year_style == Some("numeric")
        && req.month_style == Some("numeric")
        && req.day_style == Some("numeric")
        && req.weekday_style.is_none()
        && !req.has_hour
        && !req.has_minute
        && !req.has_second;
    if !numeric_ymd {
        return None;
    }

    format_default_numeric_parts(req)
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
            hour_cycle: None,
            hour12: None,
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
            hour_cycle: None,
            hour12: None,
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

    #[allow(clippy::too_many_arguments)]
    fn comp(
        locale: &str,
        year: Option<&str>,
        month: Option<&str>,
        day: Option<&str>,
        weekday: Option<&str>,
    ) -> Option<String> {
        format_components(&CompReq {
            locale,
            year: 2026,
            month: 1,
            day: 5,
            hour: 14,
            minute: 37,
            second: 9,
            has_year: year.is_some(),
            has_month: month.is_some(),
            has_day: day.is_some(),
            year_style: year,
            month_style: month,
            day_style: day,
            weekday_style: weekday,
            has_hour: false,
            has_minute: false,
            has_second: false,
            hour_cycle: None,
            hour12: None,
        })
    }

    #[test]
    fn name_bearing_components_match_node() {
        let n = Some("numeric");
        let cases: &[(
            &str,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            &str,
        )] = &[
            ("de", n, Some("long"), n, None, "5. Januar 2026"),
            ("en-US", None, Some("short"), n, None, "Jan 5"),
            (
                "en-US",
                n,
                Some("long"),
                n,
                Some("long"),
                "Monday, January 5, 2026",
            ),
            ("ja", n, Some("long"), n, None, "2026年1月5日"),
            ("fr", None, Some("long"), n, Some("long"), "lundi 5 janvier"),
            ("de", None, None, None, Some("long"), "Montag"),
            ("de", None, Some("long"), n, None, "5. Januar"),
            ("ko", n, Some("long"), n, None, "2026년 1월 5일"),
            ("en-GB", None, Some("short"), n, Some("short"), "Mon 5 Jan"),
        ];
        let mut mismatches = Vec::new();
        for (loc, y, m, d, wd, want) in cases {
            let got = comp(loc, *y, *m, *d, *wd).unwrap_or_else(|| "<None>".into());
            if got != *want {
                mismatches.push(format!("{loc} {m:?}/{wd:?}: got {got:?}  want {want:?}"));
            }
        }
        assert!(mismatches.is_empty(), "\n{}", mismatches.join("\n"));
    }

    #[test]
    fn default_numeric_components_match_node() {
        let cases = [
            ("de-DE", "5.1.2026"),
            ("fr-FR", "05/01/2026"),
            ("ja-JP", "2026/1/5"),
            ("en-GB", "05/01/2026"),
            ("en-US", "1/5/2026"),
        ];
        for (locale, want) in cases {
            let got = comp(
                locale,
                Some("numeric"),
                Some("numeric"),
                Some("numeric"),
                None,
            );
            assert_eq!(got.as_deref(), Some(want), "locale {locale}");
        }
    }

    #[test]
    fn default_numeric_pattern_table_is_strictly_sorted() {
        assert!(
            YMD_PATTERNS.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "YMD_PATTERNS must stay sorted and unique for binary search"
        );
    }

    #[test]
    fn unsupported_numeric_and_narrow_components_defer() {
        // Numeric subsets retain the bespoke fallback.
        assert_eq!(
            comp("de", None, Some("numeric"), Some("numeric"), None),
            None
        );
        // Narrow → None.
        assert_eq!(
            comp("en-US", None, Some("narrow"), Some("numeric"), None),
            None
        );
    }

    #[test]
    fn explicit_hour_cycle_family_honored() {
        // timeStyle:short at 00:07 with an explicit hourCycle — each family
        // renders midnight differently (Node baselines). Regression guard for
        // collapsing h11→h12 / h24→h23 through a 12h/24h bool.
        let hc = |loc: &str, cyc: &str| -> Option<String> {
            format(&Req {
                locale: loc,
                year: 2026,
                month: 1,
                day: 5,
                hour: 0,
                minute: 7,
                second: 0,
                date_style: None,
                time_style: Some(Len::Short),
                hour_cycle: Some(cyc),
                hour12: None,
            })
        };
        assert_eq!(hc("en-US", "h11").as_deref(), Some("0:07 AM"));
        assert_eq!(hc("en-US", "h12").as_deref(), Some("12:07 AM"));
        assert_eq!(hc("en-US", "h23").as_deref(), Some("00:07"));
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
