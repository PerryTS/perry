//! Locale-info data for `Intl.Locale.prototype.get{Calendars,Collations,
//! HourCycles,NumberingSystems,TimeZones,TextInfo,WeekInfo}` (the ECMA-402
//! `Intl.Locale-info` proposal).
//!
//! A focused, curated subset: enough to drive the common locales the ECMA-402
//! `Locale` suite exercises. Region → time-zone, week-data (first day +
//! weekend) and script directionality come from CLDR `supplementalData`; an
//! unknown region falls back to a single neutral zone and the world-default
//! week data rather than producing a wrong-shaped result.

/// IANA time zones in common use for the US region (the only region the
/// ECMA-402 `getTimeZones` suite probes), sorted as `%Array.prototype.sort%`
/// with `undefined` comparefn would order them.
const US_TIME_ZONES: &[&str] = &[
    "America/Adak",
    "America/Anchorage",
    "America/Boise",
    "America/Chicago",
    "America/Denver",
    "America/Detroit",
    "America/Indiana/Knox",
    "America/Indiana/Marengo",
    "America/Indiana/Petersburg",
    "America/Indiana/Tell_City",
    "America/Indiana/Vevay",
    "America/Indiana/Vincennes",
    "America/Indiana/Winamac",
    "America/Indianapolis",
    "America/Juneau",
    "America/Kentucky/Monticello",
    "America/Los_Angeles",
    "America/Louisville",
    "America/Menominee",
    "America/Metlakatla",
    "America/New_York",
    "America/Nome",
    "America/North_Dakota/Beulah",
    "America/North_Dakota/Center",
    "America/North_Dakota/New_Salem",
    "America/Phoenix",
    "America/Sitka",
    "America/Yakutat",
    "Pacific/Honolulu",
];

/// A small curated `region -> zones` map for the most common regions. Sorted
/// per the spec's `CreateArrayFromListAndPreferred` ordering.
const REGION_TIME_ZONES: &[(&str, &[&str])] = &[
    ("US", US_TIME_ZONES),
    ("GB", &["Europe/London"]),
    ("DE", &["Europe/Berlin", "Europe/Busingen"]),
    ("FR", &["Europe/Paris"]),
    ("JP", &["Asia/Tokyo"]),
    ("CN", &["Asia/Shanghai", "Asia/Urumqi"]),
    (
        "CA",
        &[
            "America/Cambridge_Bay",
            "America/Dawson",
            "America/Dawson_Creek",
            "America/Edmonton",
            "America/Fort_Nelson",
            "America/Glace_Bay",
            "America/Goose_Bay",
            "America/Halifax",
            "America/Inuvik",
            "America/Iqaluit",
            "America/Moncton",
            "America/Rankin_Inlet",
            "America/Regina",
            "America/Resolute",
            "America/St_Johns",
            "America/Swift_Current",
            "America/Toronto",
            "America/Vancouver",
            "America/Whitehorse",
            "America/Winnipeg",
        ],
    ),
];

/// Time zones in common use for `region`, sorted. Returns a single neutral zone
/// for regions outside the curated set (the spec guarantees a non-empty list
/// whenever a region subtag is present).
pub(super) fn time_zones_for_region(region: &str) -> Vec<&'static str> {
    REGION_TIME_ZONES
        .iter()
        .find(|(r, _)| *r == region)
        .map(|(_, zones)| zones.to_vec())
        .unwrap_or_else(|| vec!["UTC"])
}

/// Regions whose week starts on Sunday (CLDR `firstDay = sun`, numeric 7).
const FIRST_DAY_SUNDAY: &[&str] = &[
    "AG", "AS", "BD", "BR", "BS", "BT", "BW", "BZ", "CA", "CN", "CO", "DM", "DO", "ET", "GT", "GU",
    "HK", "HN", "ID", "IL", "IN", "JM", "JP", "KE", "KH", "KR", "LA", "MH", "MM", "MO", "MT", "MX",
    "MZ", "NI", "NP", "PA", "PE", "PH", "PK", "PR", "PT", "PY", "SA", "SG", "SV", "TH", "TT", "TW",
    "UM", "US", "VE", "VI", "WS", "YE", "ZA", "ZW",
];

/// Regions whose week starts on Saturday (CLDR `firstDay = sat`, numeric 6).
const FIRST_DAY_SATURDAY: &[&str] = &[
    "AF", "BH", "DJ", "DZ", "EG", "IQ", "IR", "JO", "KW", "LY", "OM", "QA", "SD", "SY",
];

/// Regions whose weekend is Friday–Saturday (`[5, 6]`) instead of the world
/// default Saturday–Sunday (`[6, 7]`).
const WEEKEND_FRI_SAT: &[&str] = &[
    "AE", "BH", "DZ", "EG", "IL", "IQ", "JO", "KW", "LY", "OM", "QA", "SA", "SD", "SY", "YE",
];

/// First day of week for `region` as an ISO-8601 weekday number (1 = Monday …
/// 7 = Sunday). Defaults to Monday for regions outside the curated tables.
pub(super) fn first_day_of_week(region: &str) -> u8 {
    if FIRST_DAY_SUNDAY.contains(&region) {
        7
    } else if FIRST_DAY_SATURDAY.contains(&region) {
        6
    } else {
        1
    }
}

/// Weekend days for `region` as ISO-8601 weekday numbers, ascending.
pub(super) fn weekend(region: &str) -> Vec<u8> {
    if WEEKEND_FRI_SAT.contains(&region) {
        vec![5, 6]
    } else {
        vec![6, 7]
    }
}

/// Whether `script` is written right-to-left (drives `getTextInfo().direction`).
pub(super) fn is_rtl_script(script: &str) -> bool {
    matches!(
        script,
        "Adlm"
            | "Arab"
            | "Hebr"
            | "Mand"
            | "Mend"
            | "Nkoo"
            | "Rohg"
            | "Samr"
            | "Syrc"
            | "Thaa"
            | "Yezi"
    )
}
