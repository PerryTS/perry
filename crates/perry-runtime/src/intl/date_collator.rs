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
    let temporal_kind = crate::temporal::temporal_kind(value);
    if let Some(kind) = temporal_kind {
        validate_temporal_dtf_overlap(kind, obj);
    }
    let ms = date_arg_to_clipped_ms(value);
    string_value(&format_ms_with_dtf_obj(obj, ms, temporal_kind))
}

pub(crate) extern "C" fn date_time_format_bound_format_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "format", KIND_DATE_TIME);
    let temporal_kind = crate::temporal::temporal_kind(value);
    if let Some(kind) = temporal_kind {
        validate_temporal_dtf_overlap(kind, obj);
    }
    let ms = date_arg_to_clipped_ms(value);
    string_value(&format_ms_with_dtf_obj(obj, ms, temporal_kind))
}

/// `get Intl.DateTimeFormat.prototype.format` — the ECMA-402 accessor. Validates
/// that `this` is an initialized DateTimeFormat (TypeError otherwise) and returns
/// the instance's [[BoundFormat]] (stored in KEY_DTF_BOUND_FORMAT, set at
/// construction with name `""` and length 1).
pub(crate) extern "C" fn date_time_format_format_getter_thunk(
    _closure: *const ClosureHeader,
) -> f64 {
    let obj = this_intl_object("format", KIND_DATE_TIME);
    get_field(obj, KEY_DTF_BOUND_FORMAT)
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
    if let Some(kind) = crate::temporal::temporal_kind(value) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_to_parts_value(obj, value)
}

pub(crate) extern "C" fn date_time_format_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "formatToParts", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(value) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_to_parts_value(obj, value)
}

fn date_time_format_to_parts_value(obj: *const ObjectHeader, value: f64) -> f64 {
    let temporal_kind = crate::temporal::temporal_kind(value);
    let ms = date_arg_to_clipped_ms(value);
    let mut parts = format_parts_with_dtf_obj(obj, ms, temporal_kind);
    append_time_zone_name_part(&mut parts, obj, value);
    parts_to_js_array(&parts)
}

/// Decompose the formatted output into typed parts matching `format_ms_with_dtf_obj`.
fn format_parts_with_dtf_obj(
    obj: *const ObjectHeader,
    ms: f64,
    temporal_kind: Option<crate::temporal::TemporalKind>,
) -> Vec<(&'static str, String)> {
    use crate::temporal::TemporalKind::*;
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, hour, minute, second) = crate::date::timestamp_to_components(secs);
    let mi = month.saturating_sub(1).min(11) as usize;

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

    // Apply same Temporal-kind style filtering as format_ms_with_dtf_obj.
    let (eff_date_style, eff_time_style) = match temporal_kind {
        Some(PlainDate | PlainYearMonth | PlainMonthDay) => {
            if date_style.is_some() {
                (date_style.as_deref(), None)
            } else {
                (date_style.as_deref(), time_style.as_deref())
            }
        }
        Some(PlainTime) => {
            if time_style.is_some() {
                (None, time_style.as_deref())
            } else {
                (date_style.as_deref(), time_style.as_deref())
            }
        }
        _ => (date_style.as_deref(), time_style.as_deref()),
    };

    let date_parts = |ds: &str| -> Vec<(&'static str, String)> {
        let wi = weekday_index(secs);
        match ds {
            "short" => vec![
                ("month", month.to_string()),
                ("literal", "/".to_string()),
                ("day", day.to_string()),
                ("literal", "/".to_string()),
                ("year", year.to_string()),
            ],
            "medium" => vec![
                ("month", MONTH_ABBR[mi].to_string()),
                ("literal", " ".to_string()),
                ("day", day.to_string()),
                ("literal", ", ".to_string()),
                ("year", year.to_string()),
            ],
            "long" => vec![
                ("month", MONTH_FULL[mi].to_string()),
                ("literal", " ".to_string()),
                ("day", day.to_string()),
                ("literal", ", ".to_string()),
                ("year", year.to_string()),
            ],
            "full" => vec![
                ("weekday", WEEKDAY_FULL[wi].to_string()),
                ("literal", ", ".to_string()),
                ("month", MONTH_FULL[mi].to_string()),
                ("literal", " ".to_string()),
                ("day", day.to_string()),
                ("literal", ", ".to_string()),
                ("year", year.to_string()),
            ],
            _ => vec![
                ("month", month.to_string()),
                ("literal", "/".to_string()),
                ("day", day.to_string()),
                ("literal", "/".to_string()),
                ("year", year.to_string()),
            ],
        }
    };

    let time_parts = |ts: &str| -> Vec<(&'static str, String)> {
        let inc_secs = ts != "short";
        if use_24h {
            if inc_secs {
                vec![
                    ("hour", format!("{:02}", hour)),
                    ("literal", ":".to_string()),
                    ("minute", format!("{:02}", minute)),
                    ("literal", ":".to_string()),
                    ("second", format!("{:02}", second)),
                ]
            } else {
                vec![
                    ("hour", format!("{:02}", hour)),
                    ("literal", ":".to_string()),
                    ("minute", format!("{:02}", minute)),
                ]
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
                vec![
                    ("hour", h.to_string()),
                    ("literal", ":".to_string()),
                    ("minute", format!("{:02}", minute)),
                    ("literal", ":".to_string()),
                    ("second", format!("{:02}", second)),
                    ("literal", " ".to_string()),
                    ("dayPeriod", ampm.to_string()),
                ]
            } else {
                vec![
                    ("hour", h.to_string()),
                    ("literal", ":".to_string()),
                    ("minute", format!("{:02}", minute)),
                    ("literal", " ".to_string()),
                    ("dayPeriod", ampm.to_string()),
                ]
            }
        }
    };

    match (eff_date_style, eff_time_style) {
        (Some(ds), Some(ts)) => {
            let mut parts = date_parts(ds);
            parts.push(("literal", ", ".to_string()));
            parts.extend(time_parts(ts));
            parts
        }
        (Some(ds), None) => match temporal_kind {
            Some(PlainYearMonth) => vec![
                (
                    "month",
                    match ds {
                        "medium" => MONTH_ABBR[mi].to_string(),
                        "long" | "full" => MONTH_FULL[mi].to_string(),
                        _ => month.to_string(),
                    },
                ),
                (
                    "literal",
                    if matches!(ds, "long" | "medium" | "full") {
                        " ".to_string()
                    } else {
                        "/".to_string()
                    },
                ),
                (
                    "year",
                    if ds == "short" {
                        format!("{:02}", year.rem_euclid(100))
                    } else {
                        year.to_string()
                    },
                ),
            ],
            Some(PlainMonthDay) => vec![
                (
                    "month",
                    match ds {
                        "medium" => MONTH_ABBR[mi].to_string(),
                        "long" | "full" => MONTH_FULL[mi].to_string(),
                        _ => month.to_string(),
                    },
                ),
                (
                    "literal",
                    if matches!(ds, "long" | "medium" | "full") {
                        " ".to_string()
                    } else {
                        "/".to_string()
                    },
                ),
                ("day", day.to_string()),
            ],
            _ => date_parts(ds),
        },
        (None, Some(ts)) => time_parts(ts),
        (None, None) => {
            let is_default = get_field(obj, KEY_DT_IS_DEFAULT).to_bits() == crate::value::TAG_TRUE;
            let no_primary = dtf_primary_mask(obj) == 0;
            let ampm_for = |h: u32| -> (u32, &'static str) {
                if h == 0 {
                    (12, "AM")
                } else if h < 12 {
                    (h, "AM")
                } else if h == 12 {
                    (12, "PM")
                } else {
                    (h - 12, "PM")
                }
            };
            // When default DTF or only supplementary options are used with a
            // Temporal type, emit Temporal-type-appropriate default parts.
            let era_opt_for_default = get_string_field(obj, KEY_ERA);
            if is_default || no_primary {
                let append_era = |mut v: Vec<(&'static str, String)>, has_year: bool| {
                    if let Some(ref era_s) = era_opt_for_default {
                        if has_year {
                            v.push(("literal", " ".to_string()));
                            v.push(("era", era_string(year, era_s.as_str()).to_string()));
                        }
                    }
                    v
                };
                match temporal_kind {
                    Some(PlainDateTime) => {
                        let (h, ampm) = ampm_for(hour);
                        return append_era(
                            vec![
                                ("month", month.to_string()),
                                ("literal", "/".to_string()),
                                ("day", day.to_string()),
                                ("literal", "/".to_string()),
                                ("year", year.to_string()),
                                ("literal", ", ".to_string()),
                                ("hour", h.to_string()),
                                ("literal", ":".to_string()),
                                ("minute", format!("{:02}", minute)),
                                ("literal", ":".to_string()),
                                ("second", format!("{:02}", second)),
                                ("literal", " ".to_string()),
                                ("dayPeriod", ampm.to_string()),
                            ],
                            true,
                        );
                    }
                    Some(PlainTime) => {
                        let (h, ampm) = ampm_for(hour);
                        return vec![
                            ("hour", h.to_string()),
                            ("literal", ":".to_string()),
                            ("minute", format!("{:02}", minute)),
                            ("literal", ":".to_string()),
                            ("second", format!("{:02}", second)),
                            ("literal", " ".to_string()),
                            ("dayPeriod", ampm.to_string()),
                        ];
                    }
                    Some(PlainMonthDay) => {
                        return vec![
                            ("month", month.to_string()),
                            ("literal", "/".to_string()),
                            ("day", day.to_string()),
                        ];
                    }
                    Some(PlainYearMonth) => {
                        return append_era(
                            vec![
                                ("month", month.to_string()),
                                ("literal", "/".to_string()),
                                ("year", year.to_string()),
                            ],
                            true,
                        );
                    }
                    _ => {}
                }
            }
            // Build parts from individual component options.
            let year_opt = get_string_field(obj, KEY_YEAR);
            let month_opt = get_string_field(obj, KEY_MONTH);
            let day_opt = get_string_field(obj, KEY_DAY);
            let hour_opt = get_string_field(obj, KEY_HOUR);
            let minute_opt = get_string_field(obj, KEY_MINUTE);
            let second_opt = get_string_field(obj, KEY_SECOND);
            let weekday_opt = get_string_field(obj, KEY_WEEKDAY);
            let era_opt = get_string_field(obj, KEY_ERA);
            build_parts_from_components(
                year,
                month,
                day,
                hour,
                minute,
                second,
                secs,
                mi,
                year_opt.as_deref(),
                month_opt.as_deref(),
                day_opt.as_deref(),
                hour_opt.as_deref(),
                minute_opt.as_deref(),
                second_opt.as_deref(),
                weekday_opt.as_deref(),
                era_opt.as_deref(),
                use_24h,
            )
        }
    }
}

/// Build `formatToParts` parts from individual component options (no dateStyle/timeStyle).
fn build_parts_from_components(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    secs: i64,
    mi: usize,
    year_opt: Option<&str>,
    month_opt: Option<&str>,
    day_opt: Option<&str>,
    hour_opt: Option<&str>,
    minute_opt: Option<&str>,
    second_opt: Option<&str>,
    weekday_opt: Option<&str>,
    era_opt: Option<&str>,
    use_24h: bool,
) -> Vec<(&'static str, String)> {
    let mut parts: Vec<(&'static str, String)> = Vec::new();
    let has_date = year_opt.is_some() || month_opt.is_some() || day_opt.is_some();
    let has_time = hour_opt.is_some() || minute_opt.is_some() || second_opt.is_some();

    // Weekday prepended before date parts.
    if let Some(wk_s) = weekday_opt {
        parts.push(("weekday", weekday_name(secs, wk_s)));
        if has_date || has_time {
            parts.push(("literal", ", ".to_string()));
        }
    }

    if has_date {
        if let Some(month_s) = month_opt {
            let month_str = match month_s {
                "long" => MONTH_FULL[mi].to_string(),
                "short" | "narrow" => MONTH_ABBR[mi].to_string(),
                "2-digit" => format!("{:02}", month),
                _ => month.to_string(),
            };
            let is_named = matches!(month_s, "long" | "short" | "narrow");
            if day_opt.is_some() || year_opt.is_some() {
                parts.push(("month", month_str));
            } else {
                parts.push(("month", month_str));
            }
            // emit day (if requested) after month
            if let Some(day_s) = day_opt {
                let sep = if is_named { " " } else { "/" };
                parts.push(("literal", sep.to_string()));
                let day_str = if day_s == "2-digit" {
                    format!("{:02}", day)
                } else {
                    day.to_string()
                };
                parts.push(("day", day_str));
            }
            if let Some(year_s) = year_opt {
                let sep = if is_named { ", " } else { "/" };
                if day_opt.is_some() {
                    parts.push(("literal", sep.to_string()));
                } else {
                    parts.push(("literal", sep.to_string()));
                }
                let year_str = if year_s == "2-digit" {
                    format!("{:02}", year.rem_euclid(100))
                } else {
                    year.to_string()
                };
                parts.push(("year", year_str));
            }
        } else {
            // No month in the options
            if let Some(day_s) = day_opt {
                let day_str = if day_s == "2-digit" {
                    format!("{:02}", day)
                } else {
                    day.to_string()
                };
                parts.push(("day", day_str));
                if year_opt.is_some() {
                    parts.push(("literal", "/".to_string()));
                }
            }
            if let Some(year_s) = year_opt {
                let year_str = if year_s == "2-digit" {
                    format!("{:02}", year.rem_euclid(100))
                } else {
                    year.to_string()
                };
                parts.push(("year", year_str));
            }
        }
    }

    if has_date && has_time {
        parts.push(("literal", ", ".to_string()));
    }

    if has_time {
        let inc_secs = second_opt.is_some();
        let inc_mins = minute_opt.is_some() || inc_secs;
        if use_24h {
            if let Some(h_s) = hour_opt {
                let h_str = if h_s == "2-digit" {
                    format!("{:02}", hour)
                } else {
                    hour.to_string()
                };
                parts.push(("hour", h_str));
            }
            if inc_mins {
                if hour_opt.is_some() {
                    parts.push(("literal", ":".to_string()));
                }
                parts.push(("minute", format!("{:02}", minute)));
            }
            if inc_secs {
                parts.push(("literal", ":".to_string()));
                parts.push(("second", format!("{:02}", second)));
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
            if let Some(h_s) = hour_opt {
                let h_str = if h_s == "2-digit" {
                    format!("{:02}", h)
                } else {
                    h.to_string()
                };
                parts.push(("hour", h_str));
            }
            if inc_mins {
                if hour_opt.is_some() {
                    parts.push(("literal", ":".to_string()));
                }
                parts.push(("minute", format!("{:02}", minute)));
            }
            if inc_secs {
                parts.push(("literal", ":".to_string()));
                parts.push(("second", format!("{:02}", second)));
            }
            if hour_opt.is_some() || inc_mins {
                parts.push(("literal", " ".to_string()));
                parts.push(("dayPeriod", ampm.to_string()));
            }
        }
    }

    // Era appended when requested and the DTF has date content, or when it is
    // the only option (era-only DTF: just emit the era tag so callers like
    // formatToParts can detect its presence via part.type === "era").
    if let Some(era_s) = era_opt {
        let has_date_content =
            year_opt.is_some() || month_opt.is_some() || day_opt.is_some() || weekday_opt.is_some();
        if has_date_content || (!has_date && !has_time) {
            if !parts.is_empty() {
                parts.push(("literal", " ".to_string()));
            }
            parts.push(("era", era_string(year, era_s).to_string()));
        }
    }

    if parts.is_empty() {
        // Absolute fallback.
        return date_range_parts_from_ms((secs * 1000) as f64);
    }
    parts
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

/// ECMA-402 §11.1.3: field-presence bitmask for the DTF object's *primary*
/// date/time components. Only fields that directly indicate a date or time
/// dimension contribute; supplementary fields (`era`, `timeZoneName`) do not
/// participate in the no-overlap check.
///
///   bit 0 (0x01) — year dimension  (year, dateStyle)
///   bit 1 (0x02) — month dimension (month, dateStyle)
///   bit 2 (0x04) — day dimension   (day, weekday, dateStyle)
///   bit 3 (0x08) — time dimension  (hour, minute, second, fractional, timeStyle)
fn dtf_primary_mask(obj: *const ObjectHeader) -> u8 {
    const BIT_YEAR: u8 = 0x01;
    const BIT_MONTH: u8 = 0x02;
    const BIT_DAY: u8 = 0x04;
    const BIT_TIME: u8 = 0x08;
    let mut mask = 0u8;
    if get_string_field(obj, KEY_DATE_STYLE).is_some() {
        mask |= BIT_YEAR | BIT_MONTH | BIT_DAY;
    }
    if get_string_field(obj, KEY_TIME_STYLE).is_some() {
        mask |= BIT_TIME;
    }
    if get_string_field(obj, KEY_YEAR).is_some() {
        mask |= BIT_YEAR;
    }
    if get_string_field(obj, KEY_MONTH).is_some() {
        mask |= BIT_MONTH;
    }
    if get_string_field(obj, KEY_DAY).is_some() {
        mask |= BIT_DAY;
    }
    if get_string_field(obj, KEY_WEEKDAY).is_some() {
        mask |= BIT_DAY;
    }
    if get_string_field(obj, KEY_HOUR).is_some()
        || get_string_field(obj, KEY_MINUTE).is_some()
        || get_string_field(obj, KEY_SECOND).is_some()
        || get_number_field(obj, KEY_FRACTIONAL).is_some()
    {
        mask |= BIT_TIME;
    }
    mask
}

/// Field-presence bitmask for a Temporal type's data model (same bit layout as
/// `dtf_primary_mask`). Used to check whether the DTF's requested fields overlap
/// with the fields the Temporal value actually carries.
fn temporal_primary_mask(kind: crate::temporal::TemporalKind) -> u8 {
    use crate::temporal::TemporalKind::*;
    const BIT_YEAR: u8 = 0x01;
    const BIT_MONTH: u8 = 0x02;
    const BIT_DAY: u8 = 0x04;
    const BIT_TIME: u8 = 0x08;
    match kind {
        PlainDate => BIT_YEAR | BIT_MONTH | BIT_DAY,
        PlainTime => BIT_TIME,
        PlainDateTime => BIT_YEAR | BIT_MONTH | BIT_DAY | BIT_TIME,
        PlainYearMonth => BIT_YEAR | BIT_MONTH,
        PlainMonthDay => BIT_MONTH | BIT_DAY,
        Instant | ZonedDateTime => BIT_YEAR | BIT_MONTH | BIT_DAY | BIT_TIME,
        Duration => 0,
    }
}

/// ECMA-402 §11.5.5 / HandleDateTimeValue: throw a TypeError when the DTF's
/// explicit options have no field in common with the Temporal type's data model.
/// DTFs created with *no* options (defaults applied) skip the check — the spec
/// only applies to explicitly-constructed option sets.
fn validate_temporal_dtf_overlap(kind: crate::temporal::TemporalKind, obj: *const ObjectHeader) {
    let is_default = get_field(obj, KEY_DT_IS_DEFAULT).to_bits() == crate::value::TAG_TRUE;
    if is_default {
        return;
    }
    let dtf_mask = dtf_primary_mask(obj);
    if dtf_mask == 0 {
        return;
    }
    let type_mask = temporal_primary_mask(kind);
    if dtf_mask & type_mask == 0 {
        throw_type_error(
            "Intl.DateTimeFormat: the requested options have no overlap \
             with the Temporal type's data model",
        );
    }
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

const WEEKDAY_ABBR: &[&str] = &["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAY_NARROW: &[&str] = &["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

fn weekday_name(secs: i64, style: &str) -> String {
    let wi = weekday_index(secs);
    match style {
        "long" => WEEKDAY_FULL[wi].to_string(),
        "short" => WEEKDAY_ABBR[wi].to_string(),
        "narrow" => WEEKDAY_NARROW[wi].to_string(),
        _ => WEEKDAY_ABBR[wi].to_string(),
    }
}

fn era_string(year: i32, style: &str) -> &'static str {
    // Proleptic Gregorian: year > 0 → AD, year <= 0 → BC.
    // "narrow": "A"/"B", "short": "AD"/"BC", "long": "Anno Domini"/"Before Christ"
    let is_ad = year > 0;
    match style {
        "narrow" => {
            if is_ad {
                "A"
            } else {
                "B"
            }
        }
        "long" => {
            if is_ad {
                "Anno Domini"
            } else {
                "Before Christ"
            }
        }
        _ => {
            if is_ad {
                "AD"
            } else {
                "BC"
            }
        }
    }
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

/// Like `format_date_style` but for PlainYearMonth: omits the day field.
fn format_year_month_style(year: i32, month: u32, style: &str) -> String {
    let mi = month.saturating_sub(1).min(11) as usize;
    match style {
        "short" => format!("{}/{:02}", month, year.rem_euclid(100)),
        "medium" => format!("{} {}", MONTH_ABBR[mi], year),
        "long" | "full" => format!("{} {}", MONTH_FULL[mi], year),
        _ => format!("{}/{}", month, year),
    }
}

/// Like `format_date_style` but for PlainMonthDay: omits the year field.
fn format_month_day_style(month: u32, day: u32, style: &str) -> String {
    let mi = month.saturating_sub(1).min(11) as usize;
    match style {
        "short" => format!("{}/{}", month, day),
        "medium" => format!("{} {}", MONTH_ABBR[mi], day),
        "long" | "full" => format!("{} {}", MONTH_FULL[mi], day),
        _ => format!("{}/{}", month, day),
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
    secs: i64,
    year_opt: Option<&str>,
    month_opt: Option<&str>,
    day_opt: Option<&str>,
    hour_opt: Option<&str>,
    minute_opt: Option<&str>,
    second_opt: Option<&str>,
    weekday_opt: Option<&str>,
    era_opt: Option<&str>,
    use_24h: bool,
) -> String {
    let has_date = year_opt.is_some() || month_opt.is_some() || day_opt.is_some();
    let has_time = hour_opt.is_some() || minute_opt.is_some() || second_opt.is_some();

    let date_part = if has_date {
        let has_m = month_opt.is_some();
        let has_d = day_opt.is_some();
        let has_y = year_opt.is_some();
        let fmt_month = match month_opt {
            Some("long") => MONTH_FULL[month.saturating_sub(1).min(11) as usize].to_string(),
            Some("short") | Some("narrow") => {
                MONTH_ABBR[month.saturating_sub(1).min(11) as usize].to_string()
            }
            Some("2-digit") => format!("{:02}", month),
            Some(_) => month.to_string(), // "numeric" or unrecognised
            None => String::new(),        // absent — do NOT leak the raw month
        };
        let fmt_day = match day_opt {
            Some("2-digit") => format!("{:02}", day),
            Some(_) => day.to_string(),
            None => String::new(),
        };
        let fmt_year = match year_opt {
            Some("2-digit") => format!("{:02}", year.rem_euclid(100)),
            Some(_) => year.to_string(),
            None => String::new(),
        };
        // Named-month styles (long/short/narrow) use word-first layout.
        // Numeric styles assemble the present fields with "/" separators —
        // only fields whose option is Some appear in the output.
        match month_opt {
            Some("long") | Some("short") | Some("narrow") => Some(match (has_d, has_y) {
                (true, true) => format!("{} {}, {}", fmt_month, fmt_day, fmt_year),
                (true, false) => format!("{} {}", fmt_month, fmt_day),
                (false, true) => format!("{} {}", fmt_month, fmt_year),
                (false, false) => fmt_month,
            }),
            _ => {
                // Build "M/D/YYYY" using only the fields that are present.
                let parts: Vec<&str> = [
                    if has_m { fmt_month.as_str() } else { "" },
                    if has_d { fmt_day.as_str() } else { "" },
                    if has_y { fmt_year.as_str() } else { "" },
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
                Some(parts.join("/"))
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

    // Weekday-only (no year/month/day/time fields): suppress the M/D/YYYY
    // fallback so the caller only sees the weekday name. For era-only or any
    // other no-field combination, keep the fallback so there's date context.
    let core = match (date_part, time_part) {
        (Some(d), Some(t)) => format!("{}, {}", d, t),
        (Some(d), None) => d,
        (None, Some(t)) => t,
        (None, None) => {
            if weekday_opt.is_some() {
                String::new()
            } else {
                format!("{}/{}/{}", month, day, year)
            }
        }
    };
    // Prepend weekday; if core is empty the weekday stands alone.
    let with_weekday = if let Some(wk_s) = weekday_opt {
        let wk = weekday_name(secs, wk_s);
        if core.is_empty() {
            wk
        } else {
            format!("{}, {}", wk, core)
        }
    } else {
        core
    };
    // Append era when: (a) there are date/weekday fields, or (b) no fields at
    // all (era-only DTF — the fallback date string already has date context).
    if let Some(era_s) = era_opt {
        if year_opt.is_some()
            || month_opt.is_some()
            || day_opt.is_some()
            || weekday_opt.is_some()
            || (!has_date && !has_time && weekday_opt.is_none())
        {
            format!("{} {}", with_weekday, era_string(year, era_s))
        } else {
            with_weekday
        }
    } else {
        with_weekday
    }
}

/// Format a millisecond timestamp using the options stored on a DTF instance.
fn format_ms_with_dtf_obj(
    obj: *const ObjectHeader,
    ms: f64,
    temporal_kind: Option<crate::temporal::TemporalKind>,
) -> String {
    use crate::temporal::TemporalKind::*;
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

    // When both dateStyle and timeStyle are set for a date-only or time-only
    // Temporal value, the spec says the inapplicable style is silently ignored.
    let (eff_date_style, eff_time_style) = match temporal_kind {
        Some(PlainDate | PlainYearMonth | PlainMonthDay) => {
            // Date-only: ignore timeStyle when dateStyle also set.
            if date_style.is_some() {
                (date_style.as_deref(), None)
            } else {
                (date_style.as_deref(), time_style.as_deref())
            }
        }
        Some(PlainTime) => {
            // Time-only: ignore dateStyle when timeStyle also set.
            if time_style.is_some() {
                (None, time_style.as_deref())
            } else {
                (date_style.as_deref(), time_style.as_deref())
            }
        }
        _ => (date_style.as_deref(), time_style.as_deref()),
    };

    match (eff_date_style, eff_time_style) {
        (Some(ds), Some(ts)) => format!(
            "{}, {}",
            format_date_style(year, month, day, secs, ds),
            format_time_style(hour, minute, second, ts, use_24h),
        ),
        (Some(ds), None) => match temporal_kind {
            Some(PlainYearMonth) => format_year_month_style(year, month, ds),
            Some(PlainMonthDay) => format_month_day_style(month, day, ds),
            _ => format_date_style(year, month, day, secs, ds),
        },
        (None, Some(ts)) => format_time_style(hour, minute, second, ts, use_24h),
        (None, None) => {
            let is_default = get_field(obj, KEY_DT_IS_DEFAULT).to_bits() == crate::value::TAG_TRUE;
            // Also treat DTFs that only have supplementary options (era, timeZoneName)
            // — no primary date/time fields — as needing Temporal-type-driven defaults.
            let no_primary = dtf_primary_mask(obj) == 0;
            let (year_opt, month_opt, day_opt, hour_opt, minute_opt, second_opt) =
                if is_default || no_primary {
                    match temporal_kind {
                        Some(PlainDateTime) => (
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                        ),
                        Some(PlainTime) => (
                            None,
                            None,
                            None,
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                        ),
                        Some(PlainMonthDay) => (
                            None,
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                            None,
                            None,
                            None,
                        ),
                        Some(PlainYearMonth) => (
                            Some("numeric".to_string()),
                            Some("numeric".to_string()),
                            None,
                            None,
                            None,
                            None,
                        ),
                        _ => (
                            get_string_field(obj, KEY_YEAR),
                            get_string_field(obj, KEY_MONTH),
                            get_string_field(obj, KEY_DAY),
                            get_string_field(obj, KEY_HOUR),
                            get_string_field(obj, KEY_MINUTE),
                            get_string_field(obj, KEY_SECOND),
                        ),
                    }
                } else {
                    (
                        get_string_field(obj, KEY_YEAR),
                        get_string_field(obj, KEY_MONTH),
                        get_string_field(obj, KEY_DAY),
                        get_string_field(obj, KEY_HOUR),
                        get_string_field(obj, KEY_MINUTE),
                        get_string_field(obj, KEY_SECOND),
                    )
                };
            let weekday_opt = get_string_field(obj, KEY_WEEKDAY);
            let era_opt = get_string_field(obj, KEY_ERA);
            format_components(
                year,
                month,
                day,
                hour,
                minute,
                second,
                secs,
                year_opt.as_deref(),
                month_opt.as_deref(),
                day_opt.as_deref(),
                hour_opt.as_deref(),
                minute_opt.as_deref(),
                second_opt.as_deref(),
                weekday_opt.as_deref(),
                era_opt.as_deref(),
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

    let mut date_style = get_opt("dateStyle");
    let mut time_style = get_opt("timeStyle");
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
            // ECMA-402: timeStyle is invalid when there is no date component
            // overlap. But when dateStyle is ALSO present the spec says the
            // timeStyle is silently ignored (the date-only value is formatted
            // using dateStyle alone). Only throw when timeStyle is the sole
            // style selector (no dateStyle to fall back on).
            if time_style.is_some() && date_style.is_none() {
                throw_type_error(
                    "timeStyle option is not valid for this Temporal type (no time component)",
                );
            }
            // Silence timeStyle so downstream formatting uses date-only logic.
            if time_style.is_some() && date_style.is_some() {
                time_style = None;
            }
        }
        TemporalLocaleCtx::PlainTime => {
            // Symmetric: dateStyle alone throws; combined → drop dateStyle.
            if date_style.is_some() && time_style.is_none() {
                throw_type_error(
                    "dateStyle option is not valid for Temporal.PlainTime (no date component)",
                );
            }
            if date_style.is_some() && time_style.is_some() {
                date_style = None;
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
            // No options given — apply per-type ECMA-402 / Temporal-spec
            // defaults.  `ToDateTimeOptions(options, required, defaults)` sets
            // the default fields differently per type:
            //   PlainDate         → required="date",  defaults="date"
            //   PlainDateTime     → required="any",   defaults="any"   (date+time)
            //   PlainTime         → required="time",  defaults="time"
            //   PlainYearMonth    → required="year month", defaults="year month"
            //   PlainMonthDay     → required="month day",  defaults="month day"
            //   Instant           → required="any",   defaults="all"   (date+time)
            //   ZonedDateTime     → required="any",   defaults="all"   (date+time)
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
            year,
            month,
            day,
            hour,
            minute,
            second,
            secs,
            eff_year,
            eff_month,
            eff_day,
            eff_hour,
            eff_min,
            eff_sec,
            weekday_opt.as_deref(),
            era_opt.as_deref(),
            use_24h,
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
    // ECMA-402 PartitionDateTimeRangePattern: if both endpoints carry a
    // calendar and those calendars differ, throw a RangeError.
    let cal_s = crate::temporal::temporal_calendar_id(start);
    let cal_e = crate::temporal::temporal_calendar_id(end);
    if let (Some(cs), Some(ce)) = (cal_s, cal_e) {
        if cs != ce {
            throw_range_error(&format!(
                "Intl.DateTimeFormat.prototype.{method}: both values must use the same calendar"
            ));
        }
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

pub(crate) fn date_time_format_range_value(
    obj: *const ObjectHeader,
    method: &str,
    start: f64,
    end: f64,
) -> f64 {
    let temporal_kind = crate::temporal::temporal_kind(start);
    let (x, y) = date_time_range_clip(method, start, end);
    let sx = format_ms_with_dtf_obj(obj, x, temporal_kind);
    let sy = format_ms_with_dtf_obj(obj, y, temporal_kind);
    if sx == sy {
        string_value(&sx)
    } else {
        string_value(&format!("{sx} \u{2013} {sy}"))
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

pub(crate) fn date_time_format_range_parts_value(
    obj: *const ObjectHeader,
    method: &str,
    start: f64,
    end: f64,
) -> f64 {
    let temporal_kind = crate::temporal::temporal_kind(start);
    let (x, y) = date_time_range_clip(method, start, end);
    let sx = format_ms_with_dtf_obj(obj, x, temporal_kind);
    let sy = format_ms_with_dtf_obj(obj, y, temporal_kind);
    let tag = |parts: Vec<(&'static str, String)>, source: &'static str| {
        parts.into_iter().map(move |(t, v)| (t, v, source))
    };
    if sx == sy {
        let shared: Vec<_> =
            tag(format_parts_with_dtf_obj(obj, x, temporal_kind), "shared").collect();
        return range_parts_to_js_array(&shared);
    }
    let mut parts: Vec<(&'static str, String, &'static str)> = tag(
        format_parts_with_dtf_obj(obj, x, temporal_kind),
        "startRange",
    )
    .collect();
    parts.push(("literal", " \u{2013} ".to_string(), "shared"));
    parts.extend(tag(
        format_parts_with_dtf_obj(obj, y, temporal_kind),
        "endRange",
    ));
    range_parts_to_js_array(&parts)
}

pub(crate) extern "C" fn date_time_format_range_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = this_intl_object("formatRange", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(start) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_range_value(obj, "formatRange", start, end)
}

pub(crate) extern "C" fn date_time_format_bound_range_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "formatRange", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(start) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_range_value(obj, "formatRange", start, end)
}

pub(crate) extern "C" fn date_time_format_range_to_parts_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = this_intl_object("formatRangeToParts", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(start) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_range_parts_value(obj, "formatRangeToParts", start, end)
}

pub(crate) extern "C" fn date_time_format_bound_range_to_parts_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "formatRangeToParts", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(start) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_range_parts_value(obj, "formatRangeToParts", start, end)
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
