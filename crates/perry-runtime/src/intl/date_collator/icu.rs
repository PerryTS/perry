//! Thin feature-gated adapters from DateTimeFormat state to `icu_dtf`.
//!
//! Kept separate from `date_collator.rs` so the primary implementation stays
//! below the repository's 2,000-line limit.

/// Format a `dateStyle`/`timeStyle` combination via icu4x (CLDR patterns).
/// Returns `None` when the icu feature is off, the caller opted out (`enabled`
/// = false, e.g. a Temporal partial), or the option combination is unmapped.
#[cfg(feature = "intl-datetime")]
#[allow(clippy::too_many_arguments)]
pub(super) fn icu_style(
    enabled: bool,
    locale: &str,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    date_style: Option<&str>,
    time_style: Option<&str>,
    hour_cycle: Option<&str>,
    hour12: Option<bool>,
) -> Option<String> {
    use super::super::icu_dtf::{self, Len, Req};
    if !enabled {
        return None;
    }
    icu_dtf::format(&Req {
        locale,
        year,
        month: month as u8,
        day: day as u8,
        hour: hour as u8,
        minute: minute as u8,
        second: second as u8,
        date_style: date_style.and_then(Len::parse),
        time_style: time_style.and_then(Len::parse),
        hour_cycle,
        hour12,
    })
}

#[cfg(not(feature = "intl-datetime"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn icu_style(
    _enabled: bool,
    _locale: &str,
    _year: i32,
    _month: u32,
    _day: u32,
    _hour: u32,
    _minute: u32,
    _second: u32,
    _date_style: Option<&str>,
    _time_style: Option<&str>,
    _hour_cycle: Option<&str>,
    _hour12: Option<bool>,
) -> Option<String> {
    None
}

/// Format a date-only component set via icu4x. `None` when the feature is off
/// or icu cannot reproduce the combination.
#[cfg(feature = "intl-datetime")]
#[allow(clippy::too_many_arguments)]
pub(super) fn icu_components(
    locale: &str,
    year: i32,
    month: u32,
    day: u32,
    year_opt: Option<&str>,
    month_opt: Option<&str>,
    day_opt: Option<&str>,
    weekday_opt: Option<&str>,
) -> Option<String> {
    use super::super::icu_dtf::{self, CompReq};
    icu_dtf::format_components(&CompReq {
        locale,
        year,
        month: month as u8,
        day: day as u8,
        hour: 0,
        minute: 0,
        second: 0,
        has_year: year_opt.is_some(),
        has_month: month_opt.is_some(),
        has_day: day_opt.is_some(),
        year_style: year_opt,
        month_style: month_opt,
        day_style: day_opt,
        weekday_style: weekday_opt,
        has_hour: false,
        has_minute: false,
        has_second: false,
        hour_cycle: None,
        hour12: None,
    })
}

#[cfg(not(feature = "intl-datetime"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn icu_components(
    _locale: &str,
    _year: i32,
    _month: u32,
    _day: u32,
    _year_opt: Option<&str>,
    _month_opt: Option<&str>,
    _day_opt: Option<&str>,
    _weekday_opt: Option<&str>,
) -> Option<String> {
    None
}

/// Semantic counterpart of `icu_components`, used by `formatToParts` for the
/// default numeric Y/M/D field set so its order and punctuation match CLDR.
#[cfg(feature = "intl-datetime")]
#[allow(clippy::too_many_arguments)]
pub(super) fn icu_component_parts(
    locale: &str,
    year: i32,
    month: u32,
    day: u32,
    year_opt: Option<&str>,
    month_opt: Option<&str>,
    day_opt: Option<&str>,
    weekday_opt: Option<&str>,
) -> Option<Vec<(&'static str, String)>> {
    use super::super::icu_dtf::{self, CompReq};
    icu_dtf::format_components_parts(&CompReq {
        locale,
        year,
        month: month as u8,
        day: day as u8,
        hour: 0,
        minute: 0,
        second: 0,
        has_year: year_opt.is_some(),
        has_month: month_opt.is_some(),
        has_day: day_opt.is_some(),
        year_style: year_opt,
        month_style: month_opt,
        day_style: day_opt,
        weekday_style: weekday_opt,
        has_hour: false,
        has_minute: false,
        has_second: false,
        hour_cycle: None,
        hour12: None,
    })
}

#[cfg(not(feature = "intl-datetime"))]
#[allow(clippy::too_many_arguments)]
pub(super) fn icu_component_parts(
    _locale: &str,
    _year: i32,
    _month: u32,
    _day: u32,
    _year_opt: Option<&str>,
    _month_opt: Option<&str>,
    _day_opt: Option<&str>,
    _weekday_opt: Option<&str>,
) -> Option<Vec<(&'static str, String)>> {
    None
}
