//! `Intl.getCanonicalLocales` and `Intl.supportedValuesOf` — the locale-list
//! services of the `Intl` namespace, split out of `intl.rs` to keep that file
//! under the per-file LOC ceiling. Canonicalization itself lives in
//! [`super::canonicalize_language_tag`].

use super::{
    array_ptr_from_value, canonicalize_language_tag, get_field, get_number_field,
    locale_instance_tag, object_ptr_from_value, string_from_string_value, string_value,
    throw_invalid_language_tag, throw_range_error, throw_type_error, value_to_string,
};
use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::closure::ClosureHeader;
use crate::value::{js_nanbox_pointer, JSValue};

/// The ECMA-402 element-type guard inside CanonicalizeLocaleList: each element
/// must be a String or an Object, else `TypeError`. A Locale/other object is
/// coerced via `ToString` (an `Intl.Locale` stringifies to its canonical id).
fn locale_list_element_tag(value: f64) -> String {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_any_string() {
        return string_from_string_value(value).unwrap_or_default();
    }
    // An `Intl.Locale` (or `class X extends Intl.Locale` subclass) element:
    // CanonicalizeLocaleList reads its `[[Locale]]` slot directly, WITHOUT
    // calling the (user-overridable) `toString` — checked before the generic
    // ToString path below (test262 canonicalize-locale-list-take-locale.js).
    if let Some(tag) = locale_instance_tag(value) {
        return tag;
    }
    // Object (but not a Symbol, which is pointer-shaped yet a primitive).
    if js.is_pointer() && unsafe { crate::symbol::js_is_symbol(value) } == 0 {
        return value_to_string(value);
    }
    throw_type_error("locale must be a String or Object");
}

fn push_canonical_locale(seen: &mut Vec<String>, tag: &str) {
    let Some(canonical) = canonicalize_language_tag(tag) else {
        throw_invalid_language_tag(tag);
    };
    if !seen.iter().any(|existing| existing == &canonical) {
        seen.push(canonical);
    }
}

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
/// canonicalized and de-duplicated, in order; any other primitive → `[]`
/// (`ToObject` yields a wrapper with no integer-indexed entries).
fn get_canonical_locales(locales: f64) -> f64 {
    let js = JSValue::from_bits(locales.to_bits());
    let mut seen: Vec<String> = Vec::new();

    if js.is_undefined() {
        return canonical_locales_array(&seen);
    }
    if js.is_null() {
        throw_type_error("Cannot convert undefined or null to object");
    }
    if js.is_any_string() {
        let tag = string_from_string_value(locales).unwrap_or_default();
        push_canonical_locale(&mut seen, &tag);
        return canonical_locales_array(&seen);
    }
    // CanonicalizeLocaleList step 2: a value with an `[[InitializedLocale]]`
    // slot (an `Intl.Locale` or a subclass instance) is the single-element list
    // « locale », read from its `[[Locale]]` slot — never iterated as an
    // array-like nor stringified via `toString`.
    if let Some(tag) = locale_instance_tag(locales) {
        push_canonical_locale(&mut seen, &tag);
        return canonical_locales_array(&seen);
    }
    if let Some(arr) = array_ptr_from_value(locales) {
        let len = js_array_length(arr);
        for i in 0..len {
            let tag = locale_list_element_tag(js_array_get_f64(arr, i));
            push_canonical_locale(&mut seen, &tag);
        }
        return canonical_locales_array(&seen);
    }
    if let Some(obj) = object_ptr_from_value(locales) {
        // Generic array-like: iterate `O[0..length]`.
        let len = get_number_field(obj, "length")
            .filter(|n| n.is_finite() && *n > 0.0)
            .map(|n| n as u32)
            .unwrap_or(0);
        for i in 0..len {
            let tag = locale_list_element_tag(get_field(obj, &i.to_string()));
            push_canonical_locale(&mut seen, &tag);
        }
        return canonical_locales_array(&seen);
    }
    // Other primitives (number/boolean/symbol/bigint): ToObject succeeds but the
    // wrapper has length 0 — an empty list, no throw.
    canonical_locales_array(&seen)
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
