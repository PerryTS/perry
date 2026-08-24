//! ResolveLocale support for `Intl.DateTimeFormat`'s `ca`, `hc`, and `nu`
//! relevant extension keys.

use super::*;

/// The calendars Perry exposes through `Intl.supportedValuesOf("calendar")`.
/// CreateDateTimeFormat may only retain one of these values; well-formed future
/// or implementation-specific identifiers fall back through ResolveLocale.
const SUPPORTED_CALENDARS: &[&str] = &[
    "buddhist",
    "chinese",
    "coptic",
    "dangi",
    "ethioaa",
    "ethiopic",
    "gregory",
    "hebrew",
    "indian",
    "islamic",
    "islamic-civil",
    "islamic-rgsa",
    "islamic-tbla",
    "islamic-umalqura",
    "iso8601",
    "japanese",
    "persian",
    "roc",
];

pub(super) struct DateTimeLocaleResolution {
    pub(super) locale: String,
    pub(super) calendar: String,
    pub(super) numbering_system: String,
    /// Effective `hc` override from the extension or options. Locale defaults
    /// remain absent so ICU can select its own CLDR preference.
    pub(super) hour_cycle: Option<String>,
}

fn base_locale(locale: &str) -> String {
    locale
        .split('-')
        .take_while(|part| part.len() != 1)
        .collect::<Vec<_>>()
        .join("-")
}

fn supported_calendar(value: &str) -> Option<String> {
    let canonical = canonicalize_calendar_id(value)?;
    SUPPORTED_CALENDARS
        .contains(&canonical.as_str())
        .then_some(canonical)
}

fn supported_hour_cycle(value: &str) -> Option<String> {
    ["h11", "h12", "h23", "h24"]
        .contains(&value)
        .then(|| value.to_string())
}

fn push_keyword(locale: &mut String, started: &mut bool, key: &str, value: &str) {
    if !*started {
        locale.push_str("-u");
        *started = true;
    }
    locale.push('-');
    locale.push_str(key);
    locale.push('-');
    locale.push_str(value);
}

/// Apply ResolveLocale to DateTimeFormat's relevant extension keys. Only a
/// supported `ca`, `hc`, or `nu` keyword may survive in the resolved locale;
/// unrelated keys and unsupported values are removed. A supported explicit
/// option wins, while an unsupported option leaves a supported extension value
/// in place. `hour12` suppresses both the `hourCycle` option and the `hc`
/// extension, as required by CreateDateTimeFormat.
pub(super) fn resolve_date_time_locale(
    requested: &str,
    calendar_option: Option<&str>,
    numbering_option: Option<&str>,
    hour12: Option<bool>,
    hour_cycle_option: Option<&str>,
) -> DateTimeLocaleResolution {
    let ext_calendar = unicode_extension_keyword(requested, "ca")
        .as_deref()
        .and_then(supported_calendar);
    let opt_calendar = calendar_option.and_then(supported_calendar);
    let calendar = opt_calendar
        .or_else(|| ext_calendar.clone())
        .unwrap_or_else(|| "gregory".to_string());

    let ext_numbering = unicode_extension_keyword(requested, "nu")
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| numbering_system::is_supported_numbering_system(value));
    let opt_numbering = numbering_option
        .map(|value| value.to_ascii_lowercase())
        .filter(|value| numbering_system::is_supported_numbering_system(value));
    let numbering_system = opt_numbering
        .or_else(|| ext_numbering.clone())
        .unwrap_or_else(|| "latn".to_string());

    let ext_hour_cycle = unicode_extension_keyword(requested, "hc")
        .as_deref()
        .and_then(supported_hour_cycle);
    let hour_cycle = if hour12.is_some() {
        None
    } else {
        hour_cycle_option
            .and_then(supported_hour_cycle)
            .or_else(|| ext_hour_cycle.clone())
    };

    let mut locale = base_locale(requested);
    let mut started = false;
    if ext_calendar.as_deref() == Some(calendar.as_str()) {
        push_keyword(&mut locale, &mut started, "ca", &calendar);
    }
    if hour12.is_none() && ext_hour_cycle.as_deref() == hour_cycle.as_deref() {
        if let Some(ref hc) = hour_cycle {
            push_keyword(&mut locale, &mut started, "hc", hc);
        }
    }
    if ext_numbering.as_deref() == Some(numbering_system.as_str()) {
        push_keyword(&mut locale, &mut started, "nu", &numbering_system);
    }

    DateTimeLocaleResolution {
        locale,
        calendar,
        numbering_system,
        hour_cycle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_supported_and_unsupported_calendar_values() {
        let kept = resolve_date_time_locale("en-u-ca-iso8601", Some("invalid"), None, None, None);
        assert_eq!(kept.locale, "en-u-ca-iso8601");
        assert_eq!(kept.calendar, "iso8601");

        let replaced =
            resolve_date_time_locale("en-u-ca-gregory", Some("iso8601"), None, None, None);
        assert_eq!(replaced.locale, "en");
        assert_eq!(replaced.calendar, "iso8601");

        let future = resolve_date_time_locale("en-u-ca-bangla", Some("vikram"), None, None, None);
        assert_eq!(future.locale, "en");
        assert_eq!(future.calendar, "gregory");

        let alias = resolve_date_time_locale("en", Some("ethiopic-amete-alem"), None, None, None);
        assert_eq!(alias.calendar, "ethioaa");

        let existing = resolve_date_time_locale("en", Some("islamic"), None, None, None);
        assert_eq!(existing.calendar, "islamic");
    }

    #[test]
    fn removes_irrelevant_extensions_and_resolves_hc_and_nu() {
        let irrelevant =
            resolve_date_time_locale("ja-JP-u-cu-usd-tz-usnyc", None, None, None, None);
        assert_eq!(irrelevant.locale, "ja-JP");

        let overridden =
            resolve_date_time_locale("en-u-hc-h23-nu-arab", None, None, None, Some("h11"));
        assert_eq!(overridden.locale, "en-u-nu-arab");
        assert_eq!(overridden.hour_cycle.as_deref(), Some("h11"));
        assert_eq!(overridden.numbering_system, "arab");

        let hour12 = resolve_date_time_locale("en-u-hc-h11", None, None, Some(false), Some("h23"));
        assert_eq!(hour12.locale, "en");
        assert_eq!(hour12.hour_cycle, None);
    }
}
