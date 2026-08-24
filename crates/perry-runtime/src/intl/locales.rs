//! `Intl.getCanonicalLocales` and `Intl.supportedValuesOf` — the locale-list
//! services of the `Intl` namespace, split out of `intl.rs` to keep that file
//! under the per-file LOC ceiling. Canonicalization itself lives in
//! [`super::canonicalize_language_tag`].

use super::{locales_from_value, string_value, throw_range_error, value_to_string};
use crate::array::{js_array_alloc, js_array_push_f64};
use crate::closure::ClosureHeader;
use crate::value::js_nanbox_pointer;

pub(super) fn canonical_locales_array(list: &[String]) -> f64 {
    let mut arr = js_array_alloc(list.len() as u32);
    for locale in list {
        arr = js_array_push_f64(arr, string_value(locale));
    }
    js_nanbox_pointer(arr as i64)
}

/// `Intl.getCanonicalLocales(locales)` — CanonicalizeLocaleList then
/// CreateArrayFromList. `undefined` → `[]`; a String → a single-element list;
/// `null` → `TypeError`; an Array (or array-like Object) → its elements
/// canonicalized and de-duplicated, in order. Other primitives are ToObject-
/// wrapped, so inherited array-like properties remain observable.
pub(super) fn get_canonical_locales(locales: f64) -> f64 {
    canonical_locales_array(&locales_from_value(locales))
}

pub(super) extern "C" fn get_canonical_locales_thunk(
    _closure: *const ClosureHeader,
    locales: f64,
) -> f64 {
    get_canonical_locales(locales)
}

// `Intl.supportedValuesOf(key)` data tables. The spec only requires each list to
// be sorted, duplicate-free, and to match the value `type` production for its
// key (test262 self-checks these; it does not compare the set against the host's
// own list). The "-accepted-by-<Ctor>" cross-checks pass because Perry's
// formatters don't reject these option values. Lists are kept in JS
// (code-unit) sort order so a caller's `.sort()` round-trips unchanged.
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
const SUPPORTED_COLLATIONS: &[&str] = &[
    "compat", "dict", "emoji", "eor", "phonebk", "pinyin", "searchjl", "stroke", "trad", "unihan",
    "zhuyin",
];
const SUPPORTED_CURRENCIES: &[&str] = &[
    "AED", "AFN", "ALL", "AMD", "ANG", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT",
    "BGN", "BHD", "BIF", "BMD", "BND", "BOB", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD", "CAD",
    "CDF", "CHF", "CLP", "CNY", "COP", "CRC", "CUP", "CVE", "CZK", "DJF", "DKK", "DOP", "DZD",
    "EGP", "ERN", "ETB", "EUR", "FJD", "GBP", "GEL", "GHS", "GMD", "GNF", "GTQ", "GYD", "HKD",
    "HNL", "HRK", "HTG", "HUF", "IDR", "ILS", "INR", "IQD", "IRR", "ISK", "JMD", "JOD", "JPY",
    "KES", "KGS", "KHR", "KMF", "KPW", "KRW", "KWD", "KYD", "KZT", "LAK", "LBP", "LKR", "LRD",
    "LSL", "LYD", "MAD", "MDL", "MGA", "MKD", "MMK", "MNT", "MOP", "MRU", "MUR", "MVR", "MWK",
    "MXN", "MYR", "MZN", "NAD", "NGN", "NIO", "NOK", "NPR", "NZD", "OMR", "PAB", "PEN", "PGK",
    "PHP", "PKR", "PLN", "PYG", "QAR", "RON", "RSD", "RUB", "RWF", "SAR", "SBD", "SCR", "SDG",
    "SEK", "SGD", "SHP", "SLE", "SOS", "SRD", "SSP", "STN", "SVC", "SYP", "SZL", "THB", "TJS",
    "TMT", "TND", "TOP", "TRY", "TTD", "TWD", "TZS", "UAH", "UGX", "USD", "UYU", "UZS", "VES",
    "VND", "VUV", "WST", "XAF", "XCD", "XOF", "XPF", "YER", "ZAR", "ZMW", "ZWG",
];
const SUPPORTED_NUMBERING_SYSTEMS: &[&str] = &[
    "arab", "arabext", "beng", "deva", "fullwide", "gujr", "guru", "hanidec", "khmr", "knda",
    "laoo", "latn", "mlym", "mong", "mymr", "orya", "tamldec", "telu", "thai", "tibt",
];
const SUPPORTED_TIME_ZONES: &[&str] = &[
    "Africa/Cairo",
    "America/New_York",
    "Asia/Tokyo",
    "Australia/Sydney",
    "Europe/London",
    "Pacific/Auckland",
    "UTC",
];
const SUPPORTED_UNITS: &[&str] = &[
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

fn supported_values_list(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "calendar" => Some(SUPPORTED_CALENDARS),
        "collation" => Some(SUPPORTED_COLLATIONS),
        "currency" => Some(SUPPORTED_CURRENCIES),
        "numberingSystem" => Some(SUPPORTED_NUMBERING_SYSTEMS),
        "timeZone" => Some(SUPPORTED_TIME_ZONES),
        "unit" => Some(SUPPORTED_UNITS),
        _ => None,
    }
}

pub(super) extern "C" fn supported_values_of_thunk(
    _closure: *const ClosureHeader,
    key: f64,
) -> f64 {
    // Coerce `key` to String first (the spec's GetOption-like step), then a
    // non-key string raises RangeError.
    let key_str = value_to_string(key);
    match supported_values_list(&key_str) {
        Some(list) => {
            canonical_locales_array(&list.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
        }
        None => throw_range_error(&format!(
            "Invalid key : {key_str}. Wanted calendar, collation, currency, \
             numberingSystem, timeZone, or unit"
        )),
    }
}
