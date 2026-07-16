//! Shared `toLocaleString` implementation for the Temporal types.
//! Split out of `date_collator.rs` to keep it under the 2000-line cap.
//! A child module of `date_collator`, so `use super::*` reaches the
//! parent's private date-formatting helpers (`format_components`,
//! `format_date_style`, `format_time_style`, `opt_string`, …).

use super::*;

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
    let day_period_opt = get_opt("dayPeriod");
    let tz_name_opt = get_opt("timeZoneName");
    let tz_opt = get_opt("timeZone");
    let fractional_digits = opts_obj.and_then(|o| {
        let raw = get_field(o, "fractionalSecondDigits");
        let v = JSValue::from_bits(raw.to_bits());
        if v.is_undefined() {
            None
        } else {
            let n = v.to_number();
            (n.is_finite() && (1.0..=3.0).contains(&n)).then_some(n as u8)
        }
    });

    let has_style = date_style.is_some() || time_style.is_some();
    let has_component = year_opt.is_some()
        || month_opt.is_some()
        || day_opt.is_some()
        || hour_opt.is_some()
        || minute_opt.is_some()
        || second_opt.is_some()
        || weekday_opt.is_some()
        || era_opt.is_some()
        || day_period_opt.is_some()
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
            // Date-only Temporal types run `ToDateTimeOptions(options, "date",
            // "date")`, whose `required = "date"` step throws a TypeError if ANY
            // time-only style is present — `timeStyle` is rejected even when
            // `dateStyle` is also supplied (test262
            // .../toLocaleString/datestyle-and-timestyle for
            // PlainDate/PlainYearMonth/PlainMonthDay).
            if time_style.is_some() {
                throw_type_error(
                    "timeStyle option is not valid for this Temporal type (no time component)",
                );
            }
        }
        TemporalLocaleCtx::PlainTime => {
            // Symmetric: `ToDateTimeOptions(options, "time", "time")` rejects any
            // `dateStyle` on a time-only value, combined with `timeStyle` or not.
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
    let locale = crate::intl::locale_or_default(locale_arg);

    let result = match (eff_date_style, eff_time_style) {
        (Some(ds), Some(ts)) => format!(
            "{}, {}",
            format_date_style(year, month, day, secs, ds),
            format_time_style(hour, minute, second, ts, use_24h),
        ),
        (Some(ds), None) => format_date_style(year, month, day, secs, ds),
        (None, Some(ts)) => format_time_style(hour, minute, second, ts, use_24h),
        (None, None) => format_components(
            &locale,
            year,
            month,
            day,
            hour,
            minute,
            second,
            secs,
            epoch_ms,
            eff_year,
            eff_month,
            eff_day,
            eff_hour,
            eff_min,
            eff_sec,
            weekday_opt.as_deref(),
            era_opt.as_deref(),
            day_period_opt.as_deref(),
            fractional_digits,
            use_24h,
        ),
    };
    string_value(&result)
}
