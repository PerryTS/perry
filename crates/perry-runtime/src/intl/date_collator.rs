use super::*;

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::closure::ClosureHeader;
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name,
    set_builtin_property_attrs, ObjectHeader, PropertyAttrs,
};
use crate::string::{js_string_from_bytes, str_bytes_from_jsvalue};
use crate::value::{js_jsvalue_to_string, js_nanbox_pointer, JSValue};
use crate::StringHeader;
#[cfg(feature = "intl-segmenter")]
use unicode_segmentation::UnicodeSegmentation;

/// ECMA-402 FormatDateTime / HandleDateTimeValue step 1: coerce the
/// `format`/`formatToParts` argument to a TimeClip'd integer-millisecond value.
/// `undefined` means "now". Every other value goes through ToNumber — a Date
/// object's ToNumber is its timestamp; a string is *parsed*, never fed to the
/// `Date` constructor; a Symbol throws TypeError; an object's abrupt
/// valueOf/toString propagates. A non-finite or out-of-range (|t| > 8.64e15)
/// result is a RangeError, per TimeClip.
///
/// Temporal values are handled via their brand: epoch-milliseconds are extracted
/// directly from the cell rather than going through ToNumber (which would throw).
fn date_arg_to_clipped_ms(value: f64) -> f64 {
    if let Some(tv) = crate::temporal::temporal_value_ref(value) {
        // ECMA-402 HandleDateTimeValue rejects `Temporal.ZonedDateTime` outright
        // with a TypeError (it carries a time zone the formatter can't honor; the
        // spec steers callers to `Temporal.ZonedDateTime.prototype.toLocaleString`).
        if tv.kind() == crate::temporal::TemporalKind::ZonedDateTime {
            throw_type_error(
                "Intl.DateTimeFormat: Temporal.ZonedDateTime is not supported; \
                 use Temporal.ZonedDateTime.prototype.toLocaleString instead",
            );
        }
        return match crate::temporal::temporal_to_epoch_ms(tv) {
            Some(ms) => ms,
            None => {
                throw_type_error("Temporal.Duration cannot be formatted with Intl.DateTimeFormat")
            }
        };
    }
    let js = JSValue::from_bits(value.to_bits());
    let ms = if js.is_undefined() {
        crate::date::js_date_now()
    } else {
        // Date cells fast-path to their stored timestamp (identical to
        // ToNumber(date)); `date_cell_timestamp` returns its argument unchanged
        // for non-cells, so everything else is routed through ToNumber.
        let ts = crate::date::date_cell_timestamp(value);
        if ts.to_bits() == value.to_bits() {
            crate::builtins::js_number_coerce(value)
        } else {
            ts
        }
    };
    const TIME_CLIP_LIMIT_MS: f64 = 8.64e15;
    if !ms.is_finite() || ms.abs() > TIME_CLIP_LIMIT_MS {
        throw_range_error("Invalid time value");
    }
    ms.trunc()
}

pub(crate) extern "C" fn date_time_format_format_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = this_intl_object("format", KIND_DATE_TIME);
    let ms = date_arg_to_clipped_ms(value);
    string_value(&format_ms_with_dtf_obj(obj, ms))
}

pub(crate) extern "C" fn date_time_format_bound_format_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "format", KIND_DATE_TIME);
    let ms = date_arg_to_clipped_ms(value);
    string_value(&format_ms_with_dtf_obj(obj, ms))
}

/// Fallback path: no DTF object context, produce short UTC date. Still used by
/// some internal callers that pre-date the obj-aware thunks.
pub(crate) fn date_time_format_format_value(value: f64) -> f64 {
    let ms = date_arg_to_clipped_ms(value);
    string_value(&date_short_utc_from_ms(ms))
}

pub(crate) extern "C" fn date_time_format_to_parts_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = this_intl_object("formatToParts", KIND_DATE_TIME);
    date_time_format_to_parts_value(obj, value)
}

pub(crate) extern "C" fn date_time_format_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "formatToParts", KIND_DATE_TIME);
    date_time_format_to_parts_value(obj, value)
}

fn date_time_format_to_parts_value(obj: *const ObjectHeader, value: f64) -> f64 {
    let ms = date_arg_to_clipped_ms(value);
    let mut parts = date_range_parts_from_ms(ms);
    append_time_zone_name_part(&mut parts, obj, value);
    parts_to_js_array(&parts)
}

/// Append a `timeZoneName` part when the `timeZoneName` option is set and the
/// value being formatted is anchored to the timeline (a `Date`/number, or a
/// `Temporal.Instant`). A Temporal *plain* value (PlainDate/PlainTime/
/// PlainDateTime/PlainYearMonth/PlainMonthDay) carries no time zone, so it must
/// NOT print one — see `temporal-*-formatting-timezonename.js`.
/// (`Temporal.ZonedDateTime`/`Duration` are rejected upstream by
/// `date_arg_to_clipped_ms` and never reach here.) Perry ships no CLDR
/// zone-name data, so the rendered label is best-effort (the in-scope tests
/// observe only the part's presence and string-ness, all with the UTC default).
fn append_time_zone_name_part(
    parts: &mut Vec<(&'static str, String)>,
    obj: *const ObjectHeader,
    value: f64,
) {
    use crate::temporal::TemporalKind::*;
    if let Some(kind) = crate::temporal::temporal_kind(value) {
        if matches!(
            kind,
            PlainDate | PlainTime | PlainDateTime | PlainYearMonth | PlainMonthDay
        ) {
            return;
        }
    }
    if let Some(style) = get_string_field(obj, KEY_TIME_ZONE_NAME) {
        let tz = get_string_field(obj, KEY_TIME_ZONE).unwrap_or_else(|| "UTC".to_string());
        parts.push(("literal", ", ".to_string()));
        parts.push(("timeZoneName", time_zone_name_display(&tz, &style)));
    }
}

/// Best-effort display label for a `timeZoneName` part. Perry has no CLDR
/// zone-name database, so this covers the UTC default and an offset zone with a
/// plausible `GMT`/offset string; named IANA zones fall back to `GMT`.
fn time_zone_name_display(time_zone: &str, style: &str) -> String {
    if time_zone == "UTC" {
        return match style {
            "long" | "longGeneric" | "shortGeneric" => "Coordinated Universal Time".to_string(),
            "shortOffset" | "longOffset" => "GMT".to_string(),
            _ => "UTC".to_string(),
        };
    }
    if matches!(time_zone.as_bytes().first(), Some(b'+') | Some(b'-')) {
        return format!("GMT{time_zone}");
    }
    "GMT".to_string()
}

/// `M/D/YYYY` short form rendered directly from an integer-millisecond
/// timestamp. Shared by `format`, `formatToParts`, and both range variants so
/// all four stay byte-for-byte consistent.
pub(crate) fn date_short_utc_from_ms(ms: f64) -> String {
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    format!("{}/{}/{}", month, day, year)
}

pub(crate) fn date_range_parts_from_ms(ms: f64) -> Vec<(&'static str, String)> {
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    vec![
        ("month", month.to_string()),
        ("literal", "/".to_string()),
        ("day", day.to_string()),
        ("literal", "/".to_string()),
        ("year", year.to_string()),
    ]
}

// ---- Locale-aware date/time formatting (DTF and Temporal.toLocaleString) ---

const MONTH_FULL: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_ABBR: &[&str] = &[
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAY_FULL: &[&str] = &[
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Weekday index (0=Sunday…6=Saturday) from a UTC epoch-seconds value.
/// 1970-01-01 was Thursday = index 4.
fn weekday_index(secs: i64) -> usize {
    ((secs.div_euclid(86400) + 4).rem_euclid(7)) as usize
}

fn format_date_style(year: i32, month: u32, day: u32, secs: i64, style: &str) -> String {
    let mi = month.saturating_sub(1).min(11) as usize;
    match style {
        "short" => format!("{}/{}/{}", month, day, year),
        "medium" => format!("{} {}, {}", MONTH_ABBR[mi], day, year),
        "long" => format!("{} {}, {}", MONTH_FULL[mi], day, year),
        "full" => format!(
            "{}, {} {}, {}",
            WEEKDAY_FULL[weekday_index(secs)],
            MONTH_FULL[mi],
            day,
            year
        ),
        _ => format!("{}/{}/{}", month, day, year),
    }
}

fn format_time_12h(hour: u32, minute: u32, second: u32, inc_secs: bool) -> String {
    let (h, ampm) = if hour == 0 {
        (12u32, "AM")
    } else if hour < 12 {
        (hour, "AM")
    } else if hour == 12 {
        (12, "PM")
    } else {
        (hour - 12, "PM")
    };
    if inc_secs {
        format!("{}:{:02}:{:02} {}", h, minute, second, ampm)
    } else {
        format!("{}:{:02} {}", h, minute, ampm)
    }
}

fn format_time_24h(hour: u32, minute: u32, second: u32, inc_secs: bool) -> String {
    if inc_secs {
        format!("{:02}:{:02}:{:02}", hour, minute, second)
    } else {
        format!("{:02}:{:02}", hour, minute)
    }
}

fn format_time_style(hour: u32, minute: u32, second: u32, style: &str, use_24h: bool) -> String {
    let inc_secs = style != "short";
    if use_24h {
        format_time_24h(hour, minute, second, inc_secs)
    } else {
        format_time_12h(hour, minute, second, inc_secs)
    }
}

/// Resolve 24-hour-clock mode from hour12/hourCycle options.
fn resolve_24h(hour12: Option<bool>, hour_cycle: Option<&str>) -> bool {
    if let Some(h12) = hour12 {
        return !h12;
    }
    matches!(hour_cycle, Some("h23") | Some("h24"))
}

/// Format date+time components from the individual component options (no style).
fn format_components(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    year_opt: Option<&str>,
    month_opt: Option<&str>,
    day_opt: Option<&str>,
    hour_opt: Option<&str>,
    minute_opt: Option<&str>,
    second_opt: Option<&str>,
    use_24h: bool,
) -> String {
    let has_date = year_opt.is_some() || month_opt.is_some() || day_opt.is_some();
    let has_time = hour_opt.is_some() || minute_opt.is_some() || second_opt.is_some();

    let date_part = if has_date {
        let fmt_month = match month_opt {
            Some("long") => MONTH_FULL[month.saturating_sub(1).min(11) as usize].to_string(),
            Some("short") | Some("narrow") => {
                MONTH_ABBR[month.saturating_sub(1).min(11) as usize].to_string()
            }
            Some("2-digit") => format!("{:02}", month),
            _ => month.to_string(),
        };
        let fmt_day = match day_opt {
            Some("2-digit") => format!("{:02}", day),
            _ if day_opt.is_some() => day.to_string(),
            _ => String::new(),
        };
        let fmt_year = match year_opt {
            Some("2-digit") => format!("{:02}", year.rem_euclid(100)),
            _ if year_opt.is_some() => year.to_string(),
            _ => String::new(),
        };
        // Use named-month format for long/short/narrow, numeric M/D/YYYY otherwise.
        match month_opt {
            Some("long") | Some("short") | Some("narrow") => {
                let has_y = year_opt.is_some();
                let has_d = day_opt.is_some();
                Some(match (has_d, has_y) {
                    (true, true) => format!("{} {}, {}", fmt_month, fmt_day, fmt_year),
                    (true, false) => format!("{} {}", fmt_month, fmt_day),
                    (false, true) => format!("{} {}", fmt_month, fmt_year),
                    (false, false) => fmt_month,
                })
            }
            _ => {
                let has_y = year_opt.is_some();
                let has_d = day_opt.is_some();
                Some(match (has_d, has_y) {
                    (true, true) => format!("{}/{}/{}", fmt_month, fmt_day, fmt_year),
                    (true, false) => format!("{}/{}", fmt_month, fmt_day),
                    (false, true) => format!("{}/{}", fmt_month, fmt_year),
                    (false, false) => fmt_month,
                })
            }
        }
    } else {
        None
    };

    let time_part = if has_time {
        let inc_secs = second_opt.is_some();
        let inc_mins = minute_opt.is_some() || inc_secs;
        Some(if use_24h {
            if inc_secs {
                format!("{:02}:{:02}:{:02}", hour, minute, second)
            } else if inc_mins {
                format!("{:02}:{:02}", hour, minute)
            } else {
                format!("{:02}", hour)
            }
        } else {
            let (h, ampm) = if hour == 0 {
                (12u32, "AM")
            } else if hour < 12 {
                (hour, "AM")
            } else if hour == 12 {
                (12, "PM")
            } else {
                (hour - 12, "PM")
            };
            if inc_secs {
                format!("{}:{:02}:{:02} {}", h, minute, second, ampm)
            } else if inc_mins {
                format!("{}:{:02} {}", h, minute, ampm)
            } else {
                format!("{} {}", h, ampm)
            }
        })
    } else {
        None
    };

    match (date_part, time_part) {
        (Some(d), Some(t)) => format!("{}, {}", d, t),
        (Some(d), None) => d,
        (None, Some(t)) => t,
        (None, None) => format!("{}/{}/{}", month, day, year),
    }
}

/// Format a millisecond timestamp using the options stored on a DTF instance.
fn format_ms_with_dtf_obj(obj: *const ObjectHeader, ms: f64) -> String {
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, hour, minute, second) = crate::date::timestamp_to_components(secs);

    let date_style = get_string_field(obj, KEY_DATE_STYLE);
    let time_style = get_string_field(obj, KEY_TIME_STYLE);
    let hour12_v = {
        let v = JSValue::from_bits(get_field(obj, KEY_HOUR12).to_bits());
        if v.is_bool() {
            Some(v.as_bool())
        } else {
            None
        }
    };
    let hour_cycle = get_string_field(obj, KEY_HOUR_CYCLE);
    let use_24h = resolve_24h(hour12_v, hour_cycle.as_deref());

    match (date_style.as_deref(), time_style.as_deref()) {
        (Some(ds), Some(ts)) => format!(
            "{}, {}",
            format_date_style(year, month, day, secs, ds),
            format_time_style(hour, minute, second, ts, use_24h),
        ),
        (Some(ds), None) => format_date_style(year, month, day, secs, ds),
        (None, Some(ts)) => format_time_style(hour, minute, second, ts, use_24h),
        (None, None) => {
            let year_opt = get_string_field(obj, KEY_YEAR);
            let month_opt = get_string_field(obj, KEY_MONTH);
            let day_opt = get_string_field(obj, KEY_DAY);
            let hour_opt = get_string_field(obj, KEY_HOUR);
            let minute_opt = get_string_field(obj, KEY_MINUTE);
            let second_opt = get_string_field(obj, KEY_SECOND);
            format_components(
                year,
                month,
                day,
                hour,
                minute,
                second,
                year_opt.as_deref(),
                month_opt.as_deref(),
                day_opt.as_deref(),
                hour_opt.as_deref(),
                minute_opt.as_deref(),
                second_opt.as_deref(),
                use_24h,
            )
        }
    }
}

/// Parse a raw option value as a string; treat `undefined`/`null` as absent.
fn opt_string(raw: f64) -> Option<String> {
    string_from_string_value(raw)
}

/// Context tag for [`temporal_locale_string`] — which Temporal type is being formatted.
/// Controls default options, type-specific TypeError guards, and timezone handling.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemporalLocaleCtx {
    PlainDate,
    PlainDateTime,
    PlainTime,
    PlainYearMonth,
    PlainMonthDay,
    Instant,
    ZonedDateTime,
}

/// Shared `toLocaleString` implementation for all Temporal types.
///
/// Parses `locale_arg` / `opts_arg`, validates option conflicts and
/// type-specific restrictions (TypeError), applies type-appropriate defaults,
/// then formats `epoch_ms` using the same logic as `Intl.DateTimeFormat.format`.
pub(crate) fn temporal_locale_string(
    epoch_ms: f64,
    locale_arg: f64,
    opts_arg: f64,
    ctx: TemporalLocaleCtx,
) -> f64 {
    // ---- parse options object ----
    let opts_obj = object_ptr_from_value(opts_arg);

    let get_opt =
        |key: &str| -> Option<String> { opts_obj.and_then(|o| opt_string(get_field(o, key))) };
    let get_bool_opt = |key: &str| -> Option<bool> {
        let raw = opts_obj
            .map(|o| get_field(o, key))
            .unwrap_or_else(undefined);
        let v = JSValue::from_bits(raw.to_bits());
        if v.is_bool() {
            Some(v.as_bool())
        } else {
            None
        }
    };

    let date_style = get_opt("dateStyle");
    let time_style = get_opt("timeStyle");
    let year_opt = get_opt("year");
    let month_opt = get_opt("month");
    let day_opt = get_opt("day");
    let hour_opt = get_opt("hour");
    let minute_opt = get_opt("minute");
    let second_opt = get_opt("second");
    let hour12 = get_bool_opt("hour12");
    let hour_cycle = get_opt("hourCycle");
    let weekday_opt = get_opt("weekday");
    let era_opt = get_opt("era");
    let tz_name_opt = get_opt("timeZoneName");
    let tz_opt = get_opt("timeZone");

    let has_style = date_style.is_some() || time_style.is_some();
    let has_component = year_opt.is_some()
        || month_opt.is_some()
        || day_opt.is_some()
        || hour_opt.is_some()
        || minute_opt.is_some()
        || second_opt.is_some()
        || weekday_opt.is_some()
        || era_opt.is_some()
        || tz_name_opt.is_some();

    // ---- validate option conflicts ----

    // dateStyle/timeStyle cannot mix with explicit components (DTF constructor rule).
    if has_style && has_component {
        throw_type_error(
            "dateStyle and timeStyle cannot be used with explicit date-time component options",
        );
    }

    // Type-specific restrictions:
    match ctx {
        TemporalLocaleCtx::PlainDate
        | TemporalLocaleCtx::PlainYearMonth
        | TemporalLocaleCtx::PlainMonthDay => {
            // No time support — timeStyle is invalid.
            if time_style.is_some() {
                throw_type_error(
                    "timeStyle option is not valid for this Temporal type (no time component)",
                );
            }
        }
        TemporalLocaleCtx::PlainTime => {
            // No date support — dateStyle is invalid.
            if date_style.is_some() {
                throw_type_error(
                    "dateStyle option is not valid for Temporal.PlainTime (no date component)",
                );
            }
        }
        TemporalLocaleCtx::ZonedDateTime => {
            // The timeZone option is disallowed (ZDT carries its own timezone).
            if tz_opt.is_some() {
                throw_type_error(
                    "timeZone option is not allowed when formatting Temporal.ZonedDateTime",
                );
            }
        }
        _ => {}
    }

    // ---- apply type-appropriate defaults when no style/component is given ----
    let (eff_date_style, eff_time_style, eff_year, eff_month, eff_day, eff_hour, eff_min, eff_sec) =
        if has_style || has_component {
            (
                date_style.as_deref(),
                time_style.as_deref(),
                year_opt.as_deref(),
                month_opt.as_deref(),
                day_opt.as_deref(),
                hour_opt.as_deref(),
                minute_opt.as_deref(),
                second_opt.as_deref(),
            )
        } else {
            // No options given — apply spec defaults for this Temporal type.
            match ctx {
                TemporalLocaleCtx::PlainDate => (
                    None,
                    None,
                    Some("numeric"),
                    Some("numeric"),
                    Some("numeric"),
                    None,
                    None,
                    None,
                ),
                TemporalLocaleCtx::PlainDateTime
                | TemporalLocaleCtx::Instant
                | TemporalLocaleCtx::ZonedDateTime => (
                    None,
                    None,
                    Some("numeric"),
                    Some("numeric"),
                    Some("numeric"),
                    Some("numeric"),
                    Some("2-digit"),
                    Some("2-digit"),
                ),
                TemporalLocaleCtx::PlainTime => (
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some("numeric"),
                    Some("2-digit"),
                    Some("2-digit"),
                ),
                TemporalLocaleCtx::PlainYearMonth => (
                    None,
                    None,
                    Some("numeric"),
                    Some("numeric"),
                    None,
                    None,
                    None,
                    None,
                ),
                TemporalLocaleCtx::PlainMonthDay => (
                    None,
                    None,
                    None,
                    Some("numeric"),
                    Some("numeric"),
                    None,
                    None,
                    None,
                ),
            }
        };

    let use_24h = resolve_24h(hour12, hour_cycle.as_deref());
    let secs = (epoch_ms as i64).div_euclid(1000);
    let (year, month, day, hour, minute, second) = crate::date::timestamp_to_components(secs);

    let result = match (eff_date_style, eff_time_style) {
        (Some(ds), Some(ts)) => format!(
            "{}, {}",
            format_date_style(year, month, day, secs, ds),
            format_time_style(hour, minute, second, ts, use_24h),
        ),
        (Some(ds), None) => format_date_style(year, month, day, secs, ds),
        (None, Some(ts)) => format_time_style(hour, minute, second, ts, use_24h),
        (None, None) => format_components(
            year, month, day, hour, minute, second, eff_year, eff_month, eff_day, eff_hour,
            eff_min, eff_sec, use_24h,
        ),
    };
    string_value(&result)
}

/// Shared steps 4–7 of `Intl.DateTimeFormat.prototype.formatRange` /
/// `formatRangeToParts`: reject `undefined` endpoints (TypeError), coerce each
/// via ToNumber (propagating abrupt completions and the Symbol TypeError), and
/// reject any non-finite (TimeClip → NaN) endpoint (RangeError). The current
/// ECMA-402 PartitionDateTimeRangePattern does **not** reject `x > y` — it just
/// formats the range as given — so no such check is made here. Returns the
/// clipped `(x, y)` millisecond pair.
pub(crate) fn date_time_range_clip(method: &str, start: f64, end: f64) -> (f64, f64) {
    let sj = JSValue::from_bits(start.to_bits());
    let ej = JSValue::from_bits(end.to_bits());
    if sj.is_undefined() || ej.is_undefined() {
        throw_type_error(&format!(
            "Intl.DateTimeFormat.prototype.{method} called with undefined startDate or endDate"
        ));
    }
    // PartitionDateTimeRangePattern: the two endpoints must denote the *same*
    // kind of value — two Dates/numbers, or two Temporal values of the same
    // brand. Mixing brands (e.g. a `PlainDate` with a `PlainTime`, or a `Date`
    // with any Temporal value) is a TypeError. (`ZonedDateTime`/`Duration` are
    // additionally rejected by `date_arg_to_clipped_ms`, covering same-brand
    // pairs of those unsupported kinds.)
    if range_type_tag(start) != range_type_tag(end) {
        throw_type_error(&format!(
            "Intl.DateTimeFormat.prototype.{method} called with values of different types"
        ));
    }
    // Each endpoint coerces through the same Temporal-aware path as the
    // single-value `format`/`formatToParts`: a plain Temporal value decodes to
    // its epoch instant (no `ToNumber`, so no "Cannot convert a Temporal value
    // to a number" TypeError), a `Date`/number is `ToNumber`'d and TimeClip'd
    // (RangeError if out of range), and an unsupported Temporal kind throws.
    let x = date_arg_to_clipped_ms(start);
    let y = date_arg_to_clipped_ms(end);
    (x, y)
}

/// Brand discriminator for a `formatRange` endpoint: the `TemporalKind` (0–7)
/// for a Temporal value, or a distinct sentinel for any non-Temporal value
/// (`Date` / number). Two endpoints with different tags denote different kinds
/// of value and may not be range-formatted together.
fn range_type_tag(value: f64) -> u8 {
    match crate::temporal::temporal_kind(value) {
        Some(k) => k as u8,
        None => 0xFF,
    }
}

pub(crate) fn date_time_format_range_value(method: &str, start: f64, end: f64) -> f64 {
    let (x, y) = date_time_range_clip(method, start, end);
    if x == y {
        string_value(&date_short_utc_from_ms(x))
    } else {
        string_value(&format!(
            "{} \u{2013} {}",
            date_short_utc_from_ms(x),
            date_short_utc_from_ms(y)
        ))
    }
}

/// Build the `formatRangeToParts` array. Unlike `formatToParts`, each range part
/// carries a `source` field (`"startRange"` / `"endRange"` / `"shared"`) per
/// ECMA-402; when the endpoints collapse to one date every part is `"shared"`.
pub(crate) fn range_parts_to_js_array(parts: &[(&'static str, String, &'static str)]) -> f64 {
    let mut arr = js_array_alloc(parts.len() as u32);
    for (ty, val, source) in parts {
        let obj = js_object_alloc(0, 3);
        set_field(obj, "type", string_value(ty));
        set_field(obj, "value", string_value(val));
        set_field(obj, "source", string_value(source));
        arr = js_array_push_f64(arr, js_nanbox_pointer(obj as i64));
    }
    js_nanbox_pointer(arr as i64)
}

pub(crate) fn date_time_format_range_parts_value(method: &str, start: f64, end: f64) -> f64 {
    let (x, y) = date_time_range_clip(method, start, end);
    let tag = |parts: Vec<(&'static str, String)>, source: &'static str| {
        parts.into_iter().map(move |(t, v)| (t, v, source))
    };
    if x == y {
        let shared: Vec<_> = tag(date_range_parts_from_ms(x), "shared").collect();
        return range_parts_to_js_array(&shared);
    }
    let mut parts: Vec<(&'static str, String, &'static str)> =
        tag(date_range_parts_from_ms(x), "startRange").collect();
    parts.push(("literal", " \u{2013} ".to_string(), "shared"));
    parts.extend(tag(date_range_parts_from_ms(y), "endRange"));
    range_parts_to_js_array(&parts)
}

pub(crate) extern "C" fn date_time_format_range_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = this_intl_object("formatRange", KIND_DATE_TIME);
    date_time_format_range_value("formatRange", start, end)
}

pub(crate) extern "C" fn date_time_format_bound_range_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatRange", KIND_DATE_TIME);
    date_time_format_range_value("formatRange", start, end)
}

pub(crate) extern "C" fn date_time_format_range_to_parts_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = this_intl_object("formatRangeToParts", KIND_DATE_TIME);
    date_time_format_range_parts_value("formatRangeToParts", start, end)
}

pub(crate) extern "C" fn date_time_format_bound_range_to_parts_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatRangeToParts", KIND_DATE_TIME);
    date_time_format_range_parts_value("formatRangeToParts", start, end)
}

pub(crate) extern "C" fn date_time_format_resolved_options_thunk(
    _closure: *const ClosureHeader,
) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_DATE_TIME);
    date_time_format_resolved_options_object(obj)
}

pub(crate) extern "C" fn date_time_format_bound_resolved_options_thunk(
    closure: *const ClosureHeader,
) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_DATE_TIME);
    date_time_format_resolved_options_object(obj)
}

pub(crate) fn date_time_format_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 16);
    // Properties are inserted in ECMA-402 resolvedOptions order
    // (resolvedOptions/order*.js asserts this): locale, calendar,
    // numberingSystem, timeZone, [hourCycle, hour12], the date/time components,
    // then [dateStyle, timeStyle]. Only requested components are emitted, so an
    // absent option is reported as a missing own property.
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(
        out,
        "calendar",
        string_value(&get_string_field(obj, KEY_CALENDAR).unwrap_or_else(|| "gregory".to_string())),
    );
    set_field(
        out,
        "numberingSystem",
        string_value(
            &get_string_field(obj, KEY_NUMBERING_SYSTEM).unwrap_or_else(|| "latn".to_string()),
        ),
    );
    set_field(
        out,
        "timeZone",
        string_value(&get_string_field(obj, KEY_TIME_ZONE).unwrap_or_else(|| "UTC".to_string())),
    );
    // hourCycle / hour12 surface only when an hour field is present. With no tz
    // /CLDR data, the default cycle is the 12-hour clock (`h11` for `ja`, else
    // `h12`); an explicit hour12 overrides hourCycle, and `hour12: false` is the
    // 24-hour `h23`.
    if get_string_field(obj, KEY_HOUR).is_some() {
        let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_default();
        let is_ja = locale == "ja" || locale.starts_with("ja-");
        let raw_h12 = {
            let v = JSValue::from_bits(get_field(obj, KEY_HOUR12).to_bits());
            if v.is_bool() {
                Some(v.as_bool())
            } else {
                None
            }
        };
        let raw_hc = get_string_field(obj, KEY_HOUR_CYCLE);
        let default_12h = if is_ja { "h11" } else { "h12" };
        let (hc, h12): (&str, bool) = if let Some(h12) = raw_h12 {
            if h12 {
                (default_12h, true)
            } else {
                ("h23", false)
            }
        } else if let Some(ref hc) = raw_hc {
            (hc.as_str(), hc == "h11" || hc == "h12")
        } else {
            (default_12h, true)
        };
        set_field(out, "hourCycle", string_value(hc));
        set_field(out, "hour12", bool_value(h12));
    }
    for (key, name) in [
        (KEY_WEEKDAY, "weekday"),
        (KEY_ERA, "era"),
        (KEY_YEAR, "year"),
        (KEY_MONTH, "month"),
        (KEY_DAY, "day"),
        (KEY_DAY_PERIOD, "dayPeriod"),
        (KEY_HOUR, "hour"),
        (KEY_MINUTE, "minute"),
        (KEY_SECOND, "second"),
    ] {
        if let Some(value) = get_string_field(obj, key) {
            set_field(out, name, string_value(&value));
        }
    }
    if let Some(n) = get_number_field(obj, KEY_FRACTIONAL) {
        set_field(out, "fractionalSecondDigits", n);
    }
    if let Some(value) = get_string_field(obj, KEY_TIME_ZONE_NAME) {
        set_field(out, "timeZoneName", string_value(&value));
    }
    if let Some(value) = get_string_field(obj, KEY_DATE_STYLE) {
        set_field(out, "dateStyle", string_value(&value));
    }
    if let Some(value) = get_string_field(obj, KEY_TIME_STYLE) {
        set_field(out, "timeStyle", string_value(&value));
    }
    js_nanbox_pointer(out as i64)
}

pub(crate) fn swedish_collation_key(s: &str) -> Vec<u32> {
    s.chars()
        .flat_map(|ch| {
            let lower = ch.to_lowercase().next().unwrap_or(ch);
            let rank = match lower {
                'a'..='z' => lower as u32,
                '\u{00e5}' => ('z' as u32) + 1,
                '\u{00e4}' => ('z' as u32) + 2,
                '\u{00f6}' => ('z' as u32) + 3,
                other => other as u32,
            };
            [rank]
        })
        .collect()
}

/// Normalize to NFD so canonically-equivalent strings (e.g. `"ö"` precomposed
/// vs. `"ö"` decomposed) collate equal — the ECMA-402 requirement that
/// `Collator.compare` treats canonical equivalents as 0 (canonically-equivalent
/// -strings.js). Without `string-normalize` this is an identity passthrough, so
/// the precomposed/decomposed pair still compares unequal (best effort).
#[cfg(feature = "string-normalize")]
fn collation_normalize(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    // NFC (composition), not NFD: it makes canonical equivalents equal while
    // keeping precomposed `å/ä/ö` intact for the Swedish fast path below.
    s.nfc().collect()
}
#[cfg(not(feature = "string-normalize"))]
fn collation_normalize(s: &str) -> String {
    s.to_string()
}

pub(crate) fn compare_strings(locale: &str, left: &str, right: &str) -> f64 {
    let left = collation_normalize(left);
    let right = collation_normalize(right);
    let (left, right) = (left.as_str(), right.as_str());
    let ordering = if locale == "sv" || locale.starts_with("sv-") {
        swedish_collation_key(left).cmp(&swedish_collation_key(right))
    } else {
        left.cmp(right)
    };
    match ordering {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

pub(crate) extern "C" fn collator_compare_thunk(
    _closure: *const ClosureHeader,
    left: f64,
    right: f64,
) -> f64 {
    let obj = this_intl_object("compare", KIND_COLLATOR);
    collator_compare_object(obj, left, right)
}

pub(crate) extern "C" fn collator_bound_compare_thunk(
    closure: *const ClosureHeader,
    left: f64,
    right: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "compare", KIND_COLLATOR);
    collator_compare_object(obj, left, right)
}

/// Strip the code points a UCA `ignorePunctuation` collator treats as ignorable
/// — whitespace and punctuation — so e.g. `compare("", " ")` and
/// `compare("", "*")` are 0 (compare/ignorePunctuation.js).
fn strip_ignorable_punctuation(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !is_punctuation(*c))
        .collect()
}

fn is_punctuation(c: char) -> bool {
    // ASCII punctuation plus an explicit set of Unicode punctuation code points,
    // deliberately NOT whole Latin-1 ranges — those contain letters/numbers
    // (`ª` U+00AA, `µ` U+00B5, `º` U+00BA, the `¹²³` superscripts, `¼½¾`
    // fractions) that must not be stripped or distinct strings would compare
    // equal. The General Punctuation block (U+2000–U+206F) and CJK punctuation
    // (U+3000–U+303F) are all punctuation/spaces and are safe as ranges.
    c.is_ascii_punctuation()
        || matches!(c,
            '\u{00A1}' | '\u{00A7}' | '\u{00AB}' | '\u{00B6}' | '\u{00B7}'
            | '\u{00BB}' | '\u{00BF}'
            | '\u{2000}'..='\u{206F}'
            | '\u{3000}'..='\u{303F}')
}

pub(crate) fn collator_compare_object(obj: *const ObjectHeader, left: f64, right: f64) -> f64 {
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    let ignore_punct = get_field(obj, KEY_COL_IGNORE_PUNCT).to_bits() == crate::value::TAG_TRUE;
    let (mut l, mut r) = (value_to_string(left), value_to_string(right));
    if ignore_punct {
        l = strip_ignorable_punctuation(&l);
        r = strip_ignorable_punctuation(&r);
    }
    compare_strings(&locale, &l, &r)
}

pub(crate) extern "C" fn collator_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_COLLATOR);
    collator_resolved_options_object(obj)
}

pub(crate) extern "C" fn collator_bound_resolved_options_thunk(
    closure: *const ClosureHeader,
) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_COLLATOR);
    collator_resolved_options_object(obj)
}

pub(crate) fn collator_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 7);
    // Property insertion order matches ECMA-402 (resolvedOptions/order.js):
    // locale, usage, sensitivity, ignorePunctuation, collation, numeric, caseFirst.
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(
        out,
        "usage",
        string_value(&get_string_field(obj, KEY_COL_USAGE).unwrap_or_else(|| "sort".to_string())),
    );
    set_field(
        out,
        "sensitivity",
        string_value(
            &get_string_field(obj, KEY_COL_SENSITIVITY).unwrap_or_else(|| "variant".to_string()),
        ),
    );
    set_field(
        out,
        "ignorePunctuation",
        bool_value(get_field(obj, KEY_COL_IGNORE_PUNCT).to_bits() == crate::value::TAG_TRUE),
    );
    set_field(
        out,
        "collation",
        string_value(
            &get_string_field(obj, KEY_COL_COLLATION).unwrap_or_else(|| "default".to_string()),
        ),
    );
    set_field(
        out,
        "numeric",
        bool_value(get_field(obj, KEY_COL_NUMERIC).to_bits() == crate::value::TAG_TRUE),
    );
    set_field(
        out,
        "caseFirst",
        string_value(
            &get_string_field(obj, KEY_COL_CASE_FIRST).unwrap_or_else(|| "false".to_string()),
        ),
    );
    js_nanbox_pointer(out as i64)
}
