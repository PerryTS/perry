//! Minimal `Intl` namespace support for Node compatibility.
//!
//! This is intentionally a focused ECMA-402 subset: it exposes the standard
//! namespace and the core constructor/prototype shape for NumberFormat,
//! DateTimeFormat, and Collator, with deterministic formatting for the common
//! explicit locale/options combinations used by Perry's Node parity suite.

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

mod display_names;
mod duration_format;
mod locale;
mod locales;
use locales::{get_canonical_locales_thunk, supported_values_of_thunk};

const KIND_NUMBER: &str = "NumberFormat";
const KIND_DATE_TIME: &str = "DateTimeFormat";
const KIND_COLLATOR: &str = "Collator";
const KIND_SEGMENTER: &str = "Segmenter";
const KIND_LIST_FORMAT: &str = "ListFormat";
const KIND_PLURAL_RULES: &str = "PluralRules";
const KIND_RELATIVE_TIME: &str = "RelativeTimeFormat";
const KIND_DURATION_FORMAT: &str = "DurationFormat";
const KIND_DISPLAY_NAMES: &str = "DisplayNames";

const KEY_KIND: &str = "__intlKind";
const KEY_LOCALE: &str = "__intlLocale";
const KEY_STYLE: &str = "__intlStyle";
const KEY_CURRENCY: &str = "__intlCurrency";
const KEY_MAX_FRACTION_DIGITS: &str = "__intlMaxFractionDigits";
const KEY_DATE_STYLE: &str = "__intlDateStyle";
const KEY_TIME_ZONE: &str = "__intlTimeZone";
const KEY_CALENDAR: &str = "__intlCalendar";
const KEY_GRANULARITY: &str = "__intlGranularity";
const KEY_TYPE: &str = "__intlType";
const KEY_LF_STYLE: &str = "__intlListStyle";
const KEY_NUMERIC: &str = "__intlNumeric";
const KEY_RTF_STYLE: &str = "__intlRtfStyle";
const KEY_PR_MIN_INT: &str = "__intlMinInt";
const KEY_PR_MIN_FRAC: &str = "__intlMinFrac";
const KEY_PR_MAX_FRAC: &str = "__intlMaxFrac";
const KEY_PR_MIN_SIG: &str = "__intlMinSig";
const KEY_PR_MAX_SIG: &str = "__intlMaxSig";
const KEY_PR_USE_SIG: &str = "__intlUseSig";

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

unsafe fn string_header_to_owned(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let len = (*ptr).byte_len as usize;
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

fn string_from_string_value(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = str_bytes_from_jsvalue(value, &mut scratch)?;
    if ptr.is_null() || len == 0 {
        return Some(String::new());
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn value_to_string(value: f64) -> String {
    unsafe { string_header_to_owned(js_jsvalue_to_string(value)) }
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let ptr = js.as_pointer::<u8>();
    if ptr.is_null() || !crate::object::is_valid_obj_ptr(ptr as *const u8) {
        return None;
    }
    unsafe {
        let gc = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(ptr as *mut ObjectHeader)
}

fn array_ptr_from_value(value: f64) -> Option<*const crate::ArrayHeader> {
    let is_array = JSValue::from_bits(crate::array::js_array_is_array(value).to_bits());
    if !is_array.is_bool() || !is_array.as_bool() {
        return None;
    }
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let ptr = js.as_pointer::<crate::ArrayHeader>();
    (!ptr.is_null()).then_some(ptr)
}

fn get_field(value: *const ObjectHeader, key: &str) -> f64 {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_get_field_by_name_f64(value, key_ptr)
}

fn set_field(obj: *mut ObjectHeader, key: &str, value: f64) {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(obj, key_ptr, value);
}

fn set_builtin_attrs(obj: *mut ObjectHeader, key: &str, attrs: PropertyAttrs) {
    set_builtin_property_attrs(obj as usize, key.to_string(), attrs);
}

fn set_internal_field(obj: *mut ObjectHeader, key: &str, value: f64) {
    set_field(obj, key, value);
    set_builtin_attrs(obj, key, PropertyAttrs::new(true, false, true));
}

fn get_string_field(obj: *const ObjectHeader, key: &str) -> Option<String> {
    string_from_string_value(get_field(obj, key))
}

fn get_number_field(obj: *const ObjectHeader, key: &str) -> Option<f64> {
    let value = get_field(obj, key);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        None
    } else {
        Some(js.to_number())
    }
}

fn get_option_value(options: f64, key: &str) -> f64 {
    let Some(obj) = object_ptr_from_value(options) else {
        return undefined();
    };
    get_field(obj, key)
}

fn get_option_string(options: f64, key: &str) -> Option<String> {
    let value = get_option_value(options, key);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        None
    } else if js.is_any_string() {
        string_from_string_value(value)
    } else {
        Some(value_to_string(value))
    }
}

fn get_option_number(options: f64, key: &str) -> Option<f64> {
    let value = get_option_value(options, key);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() || js.is_null() {
        None
    } else {
        let n = js.to_number();
        n.is_finite().then_some(n)
    }
}

#[cold]
fn throw_type_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

#[cold]
fn throw_invalid_language_tag(tag: &str) -> ! {
    let message = format!("Invalid language tag: {tag}");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

fn canonical_locale(tag: &str) -> Option<String> {
    if tag.is_empty() {
        return None;
    }
    let mut out = String::new();
    for (i, subtag) in tag.split('-').enumerate() {
        if subtag.is_empty()
            || subtag.len() > 8
            || !subtag.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return None;
        }
        if i == 0 && !subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        if i > 0 {
            out.push('-');
        }
        if i == 0 {
            out.push_str(&subtag.to_ascii_lowercase());
        } else if subtag.len() == 2 && subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
            out.push_str(&subtag.to_ascii_uppercase());
        } else {
            out.push_str(subtag);
        }
    }
    Some(out)
}

/// CanonicalizeLanguageTag (ECMA-402): structural validity check + UTS #35
/// canonicalization. Returns `None` when the tag is not a structurally valid
/// `unicode_locale_id` (the caller raises `RangeError`).
///
/// With the `intl-locale` feature this delegates to `icu_locale_core`'s data-free
/// structural parser, which gives correct case normalization, variant ordering,
/// extension well-formedness, and UTS #35 rejection of extlang / grandfathered /
/// duplicate-singleton tags. (Deep CLDR alias replacement —
/// grandfathered→preferred, complex subtag replacement, unicode-extension value
/// aliases — needs `icu_locale` + its CLDR data and is out of scope.) The
/// fallback path uses the lighter hand-rolled `canonical_locale`.
fn canonicalize_language_tag(tag: &str) -> Option<String> {
    #[cfg(feature = "intl-locale")]
    {
        match icu_locale_core::Locale::normalize(tag) {
            Ok(canonical) => Some(canonical.into_owned()),
            Err(_) => None,
        }
    }
    #[cfg(not(feature = "intl-locale"))]
    {
        canonical_locale(tag)
    }
}

fn locales_from_value(locales: f64) -> Vec<String> {
    let js = JSValue::from_bits(locales.to_bits());
    if js.is_undefined() || js.is_null() {
        return Vec::new();
    }
    if let Some(arr) = array_ptr_from_value(locales) {
        let len = js_array_length(arr);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let value = js_array_get_f64(arr, i);
            if let Some(tag) = string_from_string_value(value) {
                let Some(canonical) = canonical_locale(&tag) else {
                    throw_invalid_language_tag(&tag);
                };
                out.push(canonical);
            }
        }
        return out;
    }
    if let Some(tag) = string_from_string_value(locales) {
        let Some(canonical) = canonical_locale(&tag) else {
            throw_invalid_language_tag(&tag);
        };
        return vec![canonical];
    }
    Vec::new()
}

fn locale_or_default(locales: f64) -> String {
    locales_from_value(locales)
        .into_iter()
        .next()
        .unwrap_or_else(|| "en-US".to_string())
}

fn rest_arg(rest: f64, index: u32) -> f64 {
    let Some(arr) = array_ptr_from_value(rest) else {
        return undefined();
    };
    if js_array_length(arr) <= index {
        undefined()
    } else {
        js_array_get_f64(arr, index)
    }
}

fn group_integer_digits(digits: &str, separator: char) -> String {
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    let len = digits.len();
    for (i, ch) in digits.chars().enumerate() {
        let from_end = len - i;
        grouped.push(ch);
        if from_end > 1 && from_end % 3 == 1 {
            grouped.push(separator);
        }
    }
    grouped
}

fn format_number_parts(
    value: f64,
    locale: &str,
    fixed_fraction_digits: Option<usize>,
    max_fraction_digits: Option<usize>,
) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }

    let negative = value.is_sign_negative() && value != 0.0;
    let abs = value.abs();
    let raw = if let Some(digits) = fixed_fraction_digits {
        format!("{:.*}", digits, abs)
    } else {
        let digits = max_fraction_digits.unwrap_or(3);
        let mut s = format!("{:.*}", digits, abs);
        if let Some(dot) = s.find('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.len() == dot + 1 {
                s.pop();
            }
        }
        s
    };

    let (int_part, frac_part) = raw.split_once('.').unwrap_or((&raw, ""));
    let de_style = locale.eq_ignore_ascii_case("de") || locale.starts_with("de-");
    let group_sep = if de_style { '.' } else { ',' };
    let decimal_sep = if de_style { ',' } else { '.' };
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&group_integer_digits(int_part, group_sep));
    if !frac_part.is_empty() {
        out.push(decimal_sep);
        out.push_str(frac_part);
    }
    out
}

/// Split an already-formatted numeric string (e.g. `-1,234.50`, `Infinity`,
/// `NaN`) into typed `formatToParts` segments under `locale`. The concatenation
/// of the segment values reproduces the input string exactly, so `format()` and
/// `formatToParts()` stay byte-consistent (the invariant the spec's own
/// `formatToParts` main test asserts: `format(x) === parts.map(p=>p.value).join('')`).
fn split_numeric_parts(s: &str, locale: &str, parts: &mut Vec<(&'static str, String)>) {
    let de_style = locale.eq_ignore_ascii_case("de") || locale.starts_with("de-");
    let group_sep = if de_style { '.' } else { ',' };
    let decimal_sep = if de_style { ',' } else { '.' };

    let mut rest = s;
    if let Some(stripped) = rest.strip_prefix('-') {
        parts.push(("minusSign", "-".to_string()));
        rest = stripped;
    }
    if rest == "Infinity" {
        parts.push(("infinity", rest.to_string()));
        return;
    }
    if rest == "NaN" {
        parts.push(("nan", rest.to_string()));
        return;
    }

    let (int_part, frac_part) = match rest.split_once(decimal_sep) {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let mut cur = String::new();
    for ch in int_part.chars() {
        if ch == group_sep {
            if !cur.is_empty() {
                parts.push(("integer", std::mem::take(&mut cur)));
            }
            parts.push(("group", ch.to_string()));
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        parts.push(("integer", cur));
    }
    if let Some(frac) = frac_part {
        parts.push(("decimal", decimal_sep.to_string()));
        parts.push(("fraction", frac.to_string()));
    }
}

/// Build the typed `formatToParts` segment list for a NumberFormat instance.
/// `format()` is defined as the concatenation of these segments' values.
fn number_instance_parts(obj: *const ObjectHeader, value: f64) -> Vec<(&'static str, String)> {
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    let style = get_string_field(obj, KEY_STYLE).unwrap_or_else(|| "decimal".to_string());
    let mut parts: Vec<(&'static str, String)> = Vec::new();
    if style == "currency" {
        let digits = format_number_parts(value, &locale, Some(2), None);
        let currency = get_string_field(obj, KEY_CURRENCY);
        let mut numeric: Vec<(&'static str, String)> = Vec::new();
        split_numeric_parts(&digits, &locale, &mut numeric);
        match currency.as_deref() {
            Some("EUR") if locale.starts_with("de") => {
                parts = numeric;
                parts.push(("literal", "\u{00a0}".to_string()));
                parts.push(("currency", "\u{20ac}".to_string()));
            }
            Some("EUR") => {
                parts.push(("currency", "\u{20ac}".to_string()));
                parts.extend(numeric);
            }
            Some("USD") => {
                parts.push(("currency", "$".to_string()));
                parts.extend(numeric);
            }
            Some(code) => {
                parts = numeric;
                parts.push(("literal", " ".to_string()));
                parts.push(("currency", code.to_string()));
            }
            None => parts = numeric,
        }
    } else {
        let max_digits = get_number_field(obj, KEY_MAX_FRACTION_DIGITS)
            .filter(|n| *n >= 0.0)
            .map(|n| n as usize);
        let digits = format_number_parts(value, &locale, None, max_digits);
        split_numeric_parts(&digits, &locale, &mut parts);
    }
    parts
}

fn format_number_instance(obj: *const ObjectHeader, value: f64) -> String {
    number_instance_parts(obj, value)
        .iter()
        .map(|(_, v)| v.as_str())
        .collect()
}

/// Convert a typed-parts list into a JS array of `{ type, value }` objects —
/// the `Intl.*.prototype.formatToParts` return shape.
fn parts_to_js_array(parts: &[(&'static str, String)]) -> f64 {
    let mut arr = js_array_alloc(parts.len() as u32);
    for (ty, val) in parts {
        let obj = js_object_alloc(0, 2);
        set_field(obj, "type", string_value(ty));
        set_field(obj, "value", string_value(val));
        arr = js_array_push_f64(arr, js_nanbox_pointer(obj as i64));
    }
    js_nanbox_pointer(arr as i64)
}

fn this_intl_object(method: &str, expected_kind: &str) -> *mut ObjectHeader {
    let this_value = crate::object::js_implicit_this_get();
    intl_object_from_value(this_value, method, expected_kind)
}

fn captured_intl_object(
    closure: *const ClosureHeader,
    method: &str,
    expected_kind: &str,
) -> *mut ObjectHeader {
    let this_value = crate::closure::js_closure_get_capture_f64(closure, 0);
    intl_object_from_value(this_value, method, expected_kind)
}

fn intl_object_from_value(value: f64, method: &str, expected_kind: &str) -> *mut ObjectHeader {
    let Some(obj) = object_ptr_from_value(value) else {
        throw_type_error(&format!(
            "Intl.{expected_kind}.prototype.{method} called on incompatible receiver"
        ));
    };
    let kind = get_string_field(obj, KEY_KIND);
    if kind.as_deref() != Some(expected_kind) {
        throw_type_error(&format!(
            "Intl.{expected_kind}.prototype.{method} called on incompatible receiver"
        ));
    }
    obj
}

extern "C" fn number_format_format_thunk(_closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = this_intl_object("format", KIND_NUMBER);
    number_format_format_object(obj, value)
}

extern "C" fn number_format_bound_format_thunk(closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = captured_intl_object(closure, "format", KIND_NUMBER);
    number_format_format_object(obj, value)
}

fn number_format_format_object(obj: *const ObjectHeader, value: f64) -> f64 {
    let number = JSValue::from_bits(value.to_bits()).to_number();
    string_value(&format_number_instance(obj, number))
}

extern "C" fn number_format_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_NUMBER);
    number_format_resolved_options_object(obj)
}

extern "C" fn number_format_bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_NUMBER);
    number_format_resolved_options_object(obj)
}

extern "C" fn number_format_to_parts_thunk(_closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = this_intl_object("formatToParts", KIND_NUMBER);
    let number = JSValue::from_bits(value.to_bits()).to_number();
    parts_to_js_array(&number_instance_parts(obj, number))
}

extern "C" fn number_format_bound_to_parts_thunk(closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = captured_intl_object(closure, "formatToParts", KIND_NUMBER);
    let number = JSValue::from_bits(value.to_bits()).to_number();
    parts_to_js_array(&number_instance_parts(obj, number))
}

fn number_format_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 6);
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(out, "numberingSystem", string_value("latn"));
    let style = get_string_field(obj, KEY_STYLE).unwrap_or_else(|| "decimal".to_string());
    set_field(out, "style", string_value(&style));
    if let Some(currency) = get_string_field(obj, KEY_CURRENCY) {
        set_field(out, "currency", string_value(&currency));
    }
    js_nanbox_pointer(out as i64)
}

fn date_short_utc(value: f64) -> String {
    let timestamp = crate::date::date_cell_timestamp(value);
    if timestamp.is_nan() {
        return "Invalid Date".to_string();
    }
    let secs = (timestamp as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    format!("{}/{}/{:02}", month, day, year.rem_euclid(100))
}

extern "C" fn date_time_format_format_thunk(_closure: *const ClosureHeader, value: f64) -> f64 {
    let _obj = this_intl_object("format", KIND_DATE_TIME);
    date_time_format_format_value(value)
}

extern "C" fn date_time_format_bound_format_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "format", KIND_DATE_TIME);
    date_time_format_format_value(value)
}

fn date_time_format_format_value(value: f64) -> f64 {
    string_value(&date_short_utc(value))
}

/// Typed `formatToParts` segments for the default short DateTimeFormat. The
/// concatenation reproduces `date_short_utc` (`M/D/YY`), keeping `format()` and
/// `formatToParts()` consistent.
fn date_instance_parts(value: f64) -> Vec<(&'static str, String)> {
    let timestamp = crate::date::date_cell_timestamp(value);
    if timestamp.is_nan() {
        return vec![("literal", "Invalid Date".to_string())];
    }
    let secs = (timestamp as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    vec![
        ("month", month.to_string()),
        ("literal", "/".to_string()),
        ("day", day.to_string()),
        ("literal", "/".to_string()),
        ("year", format!("{:02}", year.rem_euclid(100))),
    ]
}

extern "C" fn date_time_format_to_parts_thunk(_closure: *const ClosureHeader, value: f64) -> f64 {
    let _obj = this_intl_object("formatToParts", KIND_DATE_TIME);
    parts_to_js_array(&date_instance_parts(value))
}

extern "C" fn date_time_format_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatToParts", KIND_DATE_TIME);
    parts_to_js_array(&date_instance_parts(value))
}

/// `M/D/YY` short form rendered directly from a millisecond timestamp (the
/// `formatRange` arguments arrive as already-coerced ToNumber values, not Date
/// cells, so they bypass `date_short_utc`'s `date_cell_timestamp` decode).
fn date_short_utc_from_ms(ms: f64) -> String {
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    format!("{}/{}/{:02}", month, day, year.rem_euclid(100))
}

fn date_range_parts_from_ms(ms: f64) -> Vec<(&'static str, String)> {
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    vec![
        ("month", month.to_string()),
        ("literal", "/".to_string()),
        ("day", day.to_string()),
        ("literal", "/".to_string()),
        ("year", format!("{:02}", year.rem_euclid(100))),
    ]
}

/// Shared steps 4–7 of `Intl.DateTimeFormat.prototype.formatRange` /
/// `formatRangeToParts`: reject `undefined` endpoints (TypeError), coerce each
/// via ToNumber (propagating abrupt completions and the Symbol TypeError),
/// reject `x > y` and any non-finite (TimeClip → NaN) endpoint (RangeError).
/// Returns the clipped `(x, y)` millisecond pair.
fn date_time_range_clip(method: &str, start: f64, end: f64) -> (f64, f64) {
    let sj = JSValue::from_bits(start.to_bits());
    let ej = JSValue::from_bits(end.to_bits());
    if sj.is_undefined() || ej.is_undefined() {
        throw_type_error(&format!(
            "Intl.DateTimeFormat.prototype.{method} called with undefined startDate or endDate"
        ));
    }
    let x = crate::builtins::js_number_coerce(start);
    let y = crate::builtins::js_number_coerce(end);
    if x > y {
        throw_range_error("startDate is greater than endDate in formatRange");
    }
    // TimeClip (ECMA-262): a non-finite endpoint, or one whose magnitude exceeds
    // the maximum representable time (±8.64e15 ms), is NaN → RangeError.
    // Otherwise truncate toward zero to integer milliseconds, so sub-millisecond
    // equivalents collapse to the same formatted date.
    const TIME_CLIP_LIMIT_MS: f64 = 8.64e15;
    if !x.is_finite()
        || !y.is_finite()
        || x.abs() > TIME_CLIP_LIMIT_MS
        || y.abs() > TIME_CLIP_LIMIT_MS
    {
        throw_range_error("Invalid time value");
    }
    (x.trunc(), y.trunc())
}

fn date_time_format_range_value(method: &str, start: f64, end: f64) -> f64 {
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
fn range_parts_to_js_array(parts: &[(&'static str, String, &'static str)]) -> f64 {
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

fn date_time_format_range_parts_value(method: &str, start: f64, end: f64) -> f64 {
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

extern "C" fn date_time_format_range_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = this_intl_object("formatRange", KIND_DATE_TIME);
    date_time_format_range_value("formatRange", start, end)
}

extern "C" fn date_time_format_bound_range_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatRange", KIND_DATE_TIME);
    date_time_format_range_value("formatRange", start, end)
}

extern "C" fn date_time_format_range_to_parts_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = this_intl_object("formatRangeToParts", KIND_DATE_TIME);
    date_time_format_range_parts_value("formatRangeToParts", start, end)
}

extern "C" fn date_time_format_bound_range_to_parts_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatRangeToParts", KIND_DATE_TIME);
    date_time_format_range_parts_value("formatRangeToParts", start, end)
}

extern "C" fn date_time_format_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_DATE_TIME);
    date_time_format_resolved_options_object(obj)
}

extern "C" fn date_time_format_bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_DATE_TIME);
    date_time_format_resolved_options_object(obj)
}

fn date_time_format_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 6);
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
    set_field(out, "numberingSystem", string_value("latn"));
    set_field(
        out,
        "dateStyle",
        string_value(&get_string_field(obj, KEY_DATE_STYLE).unwrap_or_else(|| "short".to_string())),
    );
    set_field(
        out,
        "timeZone",
        string_value(&get_string_field(obj, KEY_TIME_ZONE).unwrap_or_else(|| "UTC".to_string())),
    );
    js_nanbox_pointer(out as i64)
}

fn swedish_collation_key(s: &str) -> Vec<u32> {
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

fn compare_strings(locale: &str, left: &str, right: &str) -> f64 {
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

extern "C" fn collator_compare_thunk(_closure: *const ClosureHeader, left: f64, right: f64) -> f64 {
    let obj = this_intl_object("compare", KIND_COLLATOR);
    collator_compare_object(obj, left, right)
}

extern "C" fn collator_bound_compare_thunk(
    closure: *const ClosureHeader,
    left: f64,
    right: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "compare", KIND_COLLATOR);
    collator_compare_object(obj, left, right)
}

fn collator_compare_object(obj: *const ObjectHeader, left: f64, right: f64) -> f64 {
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    compare_strings(&locale, &value_to_string(left), &value_to_string(right))
}

extern "C" fn collator_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_COLLATOR);
    collator_resolved_options_object(obj)
}

extern "C" fn collator_bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_COLLATOR);
    collator_resolved_options_object(obj)
}

fn collator_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 6);
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(out, "usage", string_value("sort"));
    set_field(out, "sensitivity", string_value("variant"));
    set_field(out, "ignorePunctuation", bool_value(false));
    set_field(out, "numeric", bool_value(false));
    set_field(out, "caseFirst", string_value("false"));
    js_nanbox_pointer(out as i64)
}

#[cold]
fn throw_range_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
}

fn normalize_granularity(value: Option<String>) -> String {
    match value.as_deref() {
        None | Some("grapheme") => "grapheme".to_string(),
        Some("word") => "word".to_string(),
        Some("sentence") => "sentence".to_string(),
        Some(other) => throw_range_error(&format!(
            "Value {other} out of range for Intl.Segmenter options property granularity"
        )),
    }
}

/// A segment is "word-like" when it contains at least one alphanumeric
/// character — i.e. it is not pure whitespace/punctuation. This mirrors the
/// `isWordLike` flag the spec attaches to word-granularity segments.
#[cfg(feature = "intl-segmenter")]
fn segment_is_word_like(segment: &str) -> bool {
    segment.chars().any(|c| c.is_alphanumeric())
}

fn utf16_len(segment: &str) -> u32 {
    segment.chars().map(|c| c.len_utf16() as u32).sum()
}

fn make_segment_record(
    segment: &str,
    index: u32,
    input_value: f64,
    word_like: Option<bool>,
) -> f64 {
    let obj = js_object_alloc(0, 4);
    set_field(obj, "segment", string_value(segment));
    // `index` is a plain Number (UTF-16 code-unit offset into the input).
    set_field(obj, "index", index as f64);
    set_field(obj, "input", input_value);
    if let Some(word_like) = word_like {
        set_field(obj, "isWordLike", bool_value(word_like));
    }
    js_nanbox_pointer(obj as i64)
}

/// Build the segment list for `input` under `granularity`. We return a plain
/// JS array of segment records, which is iterable / spreadable — enough for
/// `[...seg.segment(s)]` and `for (const {segment} of seg.segment(s))`, the
/// shapes `string-width` / `wrap-ansi` actually use. (The spec's `Segments`
/// object additionally exposes `.containing()`; that is not yet needed.)
fn build_segments(granularity: &str, value: f64) -> f64 {
    let input = value_to_string(value);
    let input_value = string_value(&input);
    let mut arr = js_array_alloc(0);
    let mut index = 0u32;
    #[cfg(feature = "intl-segmenter")]
    match granularity {
        "word" => {
            for segment in input.split_word_bounds() {
                let record = make_segment_record(
                    segment,
                    index,
                    input_value,
                    Some(segment_is_word_like(segment)),
                );
                arr = js_array_push_f64(arr, record);
                index += utf16_len(segment);
            }
        }
        "sentence" => {
            for segment in input.split_sentence_bounds() {
                let record = make_segment_record(segment, index, input_value, None);
                arr = js_array_push_f64(arr, record);
                index += utf16_len(segment);
            }
        }
        // "grapheme" (default): extended grapheme clusters (emoji ZWJ
        // sequences, combining marks, regional-indicator flags).
        _ => {
            for segment in input.graphemes(true) {
                let record = make_segment_record(segment, index, input_value, None);
                arr = js_array_push_f64(arr, record);
                index += utf16_len(segment);
            }
        }
    }
    // Segmenter engine gated off: no UAX #29 tables. Fall back to per-code-point
    // segmentation (one segment per `char`) for every granularity — enough to
    // keep iteration / spread working without the segmentation crate.
    #[cfg(not(feature = "intl-segmenter"))]
    {
        // Preserve the `isWordLike` field for word granularity so the record
        // shape matches the engine-enabled path (this block is dead in practice
        // — the compiler enables `intl-segmenter` on any `Intl.Segmenter` use).
        let is_word = granularity == "word";
        for segment in input.chars().map(|c| c.to_string()).collect::<Vec<_>>() {
            let word_like = if is_word {
                Some(segment.chars().any(|c| c.is_alphanumeric()))
            } else {
                None
            };
            let record = make_segment_record(&segment, index, input_value, word_like);
            arr = js_array_push_f64(arr, record);
            index += utf16_len(&segment);
        }
    }
    js_nanbox_pointer(arr as i64)
}

extern "C" fn segmenter_segment_thunk(_closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = this_intl_object("segment", KIND_SEGMENTER);
    segmenter_segment_object(obj, value)
}

extern "C" fn segmenter_bound_segment_thunk(closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = captured_intl_object(closure, "segment", KIND_SEGMENTER);
    segmenter_segment_object(obj, value)
}

fn segmenter_segment_object(obj: *const ObjectHeader, value: f64) -> f64 {
    let granularity =
        get_string_field(obj, KEY_GRANULARITY).unwrap_or_else(|| "grapheme".to_string());
    build_segments(&granularity, value)
}

extern "C" fn segmenter_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_SEGMENTER);
    segmenter_resolved_options_object(obj)
}

extern "C" fn segmenter_bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_SEGMENTER);
    segmenter_resolved_options_object(obj)
}

fn segmenter_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 2);
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(
        out,
        "granularity",
        string_value(
            &get_string_field(obj, KEY_GRANULARITY).unwrap_or_else(|| "grapheme".to_string()),
        ),
    );
    js_nanbox_pointer(out as i64)
}

/// GetOption with an enumerated value set: coerce `options[key]` to a string and
/// require it to be one of `allowed`, else `RangeError`. Absent/`undefined`
/// yields `default`.
fn enum_option(options: f64, key: &str, allowed: &[&str], default: &str) -> String {
    match get_option_string(options, key) {
        None => default.to_string(),
        Some(value) => {
            if allowed.contains(&value.as_str()) {
                value
            } else {
                throw_range_error(&format!(
                    "Value {value} out of range for Intl options property {key}"
                ))
            }
        }
    }
}

/// Validate and canonicalize a `calendar` option per the Unicode Locale
/// Identifier `type` nonterminal: one or more `-`-joined segments, each 3–8
/// ASCII alphanumerics. Returns the lowercased + alias-resolved calendar ID, or
/// `None` if the input is malformed (the caller throws RangeError). Non-ASCII
/// input (e.g. capital dotted `İ`) fails the `is_ascii_alphanumeric` test, so it
/// is rejected rather than silently lowercased.
fn canonicalize_calendar_id(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    for segment in raw.split('-') {
        if segment.len() < 3
            || segment.len() > 8
            || !segment.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return None;
        }
    }
    let lower = raw.to_ascii_lowercase();
    // BCP-47 `-u-ca-` type aliases (TR35): a handful of legacy IDs canonicalize
    // to their preferred form. Everything else passes through lowercased.
    let canonical = match lower.as_str() {
        "islamicc" => "islamic-civil",
        "ethioaa" => "ethiopic-amete-alem",
        other => other,
    };
    Some(canonical.to_string())
}

/// True when `tz` is a syntactically valid UTC-offset time-zone identifier for
/// `Intl.DateTimeFormat`: `±HH`, `±HHmm`, or `±HH:mm` with hour 00–23 and
/// minute 00–59. Sub-minute precision (seconds / fractions) is rejected, as are
/// 1-digit fields and mixed separators. Named zones (no leading sign) are not
/// the caller's concern — this is only consulted when `tz` begins with `+`/`-`.
fn is_valid_offset_time_zone(tz: &str) -> bool {
    let bytes = tz.as_bytes();
    if bytes.len() < 2 || (bytes[0] != b'+' && bytes[0] != b'-') {
        return false;
    }
    let rest = &bytes[1..];
    let hour_ok = |h: &[u8]| -> bool {
        h.len() == 2 && h.iter().all(|b| b.is_ascii_digit()) && {
            let v = (h[0] - b'0') * 10 + (h[1] - b'0');
            v <= 23
        }
    };
    let minute_ok = |m: &[u8]| -> bool {
        m.len() == 2 && m.iter().all(|b| b.is_ascii_digit()) && {
            let v = (m[0] - b'0') * 10 + (m[1] - b'0');
            v <= 59
        }
    };
    match rest.len() {
        2 => hour_ok(rest),
        4 => hour_ok(&rest[..2]) && minute_ok(&rest[2..]),
        5 => rest[2] == b':' && hour_ok(&rest[..2]) && minute_ok(&rest[3..]),
        _ => false,
    }
}

/// Canonicalize a *validated* offset time zone (`±HH`, `±HHmm`, `±HH:mm`) to the
/// `±HH:mm` form ECMA-402's FormatOffsetTimeZoneIdentifier emits. A zero offset
/// always normalizes to `+00:00` (the sign is forced positive, so `-00:00`
/// becomes `+00:00`). Assumes `is_valid_offset_time_zone(tz)` already passed.
fn canonicalize_offset_time_zone(tz: &str) -> String {
    let bytes = tz.as_bytes();
    let digits: Vec<u8> = bytes[1..]
        .iter()
        .copied()
        .filter(|b| b.is_ascii_digit())
        .collect();
    let hh = (digits[0] - b'0') * 10 + (digits[1] - b'0');
    let mm = if digits.len() == 4 {
        (digits[2] - b'0') * 10 + (digits[3] - b'0')
    } else {
        0
    };
    let sign = if hh == 0 && mm == 0 {
        '+'
    } else {
        bytes[0] as char
    };
    format!("{sign}{hh:02}:{mm:02}")
}

/// Drain any JS iterable into a `Vec<String>`, throwing `TypeError` if an
/// element is not a String (the ECMA-402 StringListFromIterable contract).
fn collect_string_list(value: f64) -> Vec<String> {
    use crate::collection_iter::{classify_init, InitIter};
    let arr_ptr = match classify_init(value) {
        InitIter::Empty => return Vec::new(),
        InitIter::Values(p) => p as *const crate::ArrayHeader,
    };
    if arr_ptr.is_null() {
        return Vec::new();
    }
    let len = js_array_length(arr_ptr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let element = js_array_get_f64(arr_ptr, i);
        if !JSValue::from_bits(element.to_bits()).is_any_string() {
            throw_type_error("Iterable yielded a non-string value for Intl.ListFormat");
        }
        out.push(string_from_string_value(element).unwrap_or_default());
    }
    out
}

/// en-US `listPattern` connectors as `(pair, middle, last)` separators, where
/// `pair` joins a 2-element list, `middle` joins all but the final boundary of a
/// 3+-element list, and `last` joins the final boundary.
fn list_separators(list_type: &str, style: &str) -> (&'static str, &'static str, &'static str) {
    match list_type {
        "unit" => {
            if style == "narrow" {
                (" ", " ", " ")
            } else {
                (", ", ", ", ", ")
            }
        }
        "disjunction" => (" or ", ", ", ", or "),
        // conjunction (default)
        _ => match style {
            "short" => (" & ", ", ", ", & "),
            "narrow" => (", ", ", ", ", "),
            _ => (" and ", ", ", ", and "),
        },
    }
}

fn list_format_parts(
    items: &[String],
    list_type: &str,
    style: &str,
) -> Vec<(&'static str, String)> {
    let (pair, middle, last) = list_separators(list_type, style);
    let mut parts: Vec<(&'static str, String)> = Vec::new();
    let n = items.len();
    if n == 0 {
        return parts;
    }
    if n == 1 {
        parts.push(("element", items[0].clone()));
        return parts;
    }
    if n == 2 {
        parts.push(("element", items[0].clone()));
        parts.push(("literal", pair.to_string()));
        parts.push(("element", items[1].clone()));
        return parts;
    }
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            let sep = if i == n - 1 { last } else { middle };
            parts.push(("literal", sep.to_string()));
        }
        parts.push(("element", item.clone()));
    }
    parts
}

fn list_format_instance_parts(obj: *const ObjectHeader, value: f64) -> Vec<(&'static str, String)> {
    let items = collect_string_list(value);
    let list_type = get_string_field(obj, KEY_TYPE).unwrap_or_else(|| "conjunction".to_string());
    let style = get_string_field(obj, KEY_LF_STYLE).unwrap_or_else(|| "long".to_string());
    list_format_parts(&items, &list_type, &style)
}

extern "C" fn list_format_format_thunk(_closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = this_intl_object("format", KIND_LIST_FORMAT);
    string_value(
        &list_format_instance_parts(obj, value)
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<String>(),
    )
}

extern "C" fn list_format_bound_format_thunk(closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = captured_intl_object(closure, "format", KIND_LIST_FORMAT);
    string_value(
        &list_format_instance_parts(obj, value)
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<String>(),
    )
}

extern "C" fn list_format_to_parts_thunk(_closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = this_intl_object("formatToParts", KIND_LIST_FORMAT);
    parts_to_js_array(&list_format_instance_parts(obj, value))
}

extern "C" fn list_format_bound_to_parts_thunk(closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = captured_intl_object(closure, "formatToParts", KIND_LIST_FORMAT);
    parts_to_js_array(&list_format_instance_parts(obj, value))
}

fn list_format_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 3);
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(
        out,
        "type",
        string_value(&get_string_field(obj, KEY_TYPE).unwrap_or_else(|| "conjunction".to_string())),
    );
    set_field(
        out,
        "style",
        string_value(&get_string_field(obj, KEY_LF_STYLE).unwrap_or_else(|| "long".to_string())),
    );
    js_nanbox_pointer(out as i64)
}

extern "C" fn list_format_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_LIST_FORMAT);
    list_format_resolved_options_object(obj)
}

extern "C" fn list_format_bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_LIST_FORMAT);
    list_format_resolved_options_object(obj)
}

// ---- Intl.RelativeTimeFormat ----------------------------------------------

const RTF_SINGULAR_UNITS: &[&str] = &[
    "second", "minute", "hour", "day", "week", "month", "quarter", "year",
];

/// Normalize a RelativeTimeFormat unit argument (singular or plural) to its
/// singular sanctioned form, or `None` if unrecognized (caller raises RangeError).
fn rtf_singular_unit(unit: &str) -> Option<&'static str> {
    let lower = unit.to_ascii_lowercase();
    let candidate = lower.strip_suffix('s').unwrap_or(&lower);
    RTF_SINGULAR_UNITS.iter().copied().find(|u| *u == candidate)
}

/// Build the long-form, `numeric: "always"` en-US relative-time parts for
/// `value` in `unit`. (`short`/`narrow` abbreviations and the `numeric: "auto"`
/// special words — "tomorrow"/"yesterday" — need CLDR data and fall back to the
/// long numeric form here.) Returns `(leading, number, trailing)` literal/number
/// fragments so `format` and `formatToParts` stay consistent.
fn rtf_parts(value: f64, unit: &str) -> Vec<(&'static str, String)> {
    let abs = value.abs();
    let num_str = format_number_parts(abs, "en-US", None, None);
    let unit_display = if abs == 1.0 {
        unit.to_string()
    } else {
        format!("{unit}s")
    };
    let past = value.is_sign_negative();
    let mut parts: Vec<(&'static str, String)> = Vec::new();
    if past {
        split_numeric_parts(&num_str, "en-US", &mut parts);
        parts.push(("literal", format!(" {unit_display} ago")));
    } else {
        parts.push(("literal", "in ".to_string()));
        split_numeric_parts(&num_str, "en-US", &mut parts);
        parts.push(("literal", format!(" {unit_display}")));
    }
    parts
}

fn rtf_instance_parts(value: f64, unit_arg: f64) -> Vec<(&'static str, String)> {
    let number = JSValue::from_bits(value.to_bits()).to_number();
    if !number.is_finite() {
        throw_range_error("Value need to be finite number for Intl.RelativeTimeFormat.format()");
    }
    let unit_str = value_to_string(unit_arg);
    let Some(unit) = rtf_singular_unit(&unit_str) else {
        throw_range_error(&format!(
            "Value {unit_str} out of range for Intl.RelativeTimeFormat.format() unit"
        ));
    };
    rtf_parts(number, unit)
}

extern "C" fn rtf_format_thunk(_closure: *const ClosureHeader, value: f64, unit: f64) -> f64 {
    let _obj = this_intl_object("format", KIND_RELATIVE_TIME);
    string_value(
        &rtf_instance_parts(value, unit)
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<String>(),
    )
}

extern "C" fn rtf_bound_format_thunk(closure: *const ClosureHeader, value: f64, unit: f64) -> f64 {
    let _obj = captured_intl_object(closure, "format", KIND_RELATIVE_TIME);
    string_value(
        &rtf_instance_parts(value, unit)
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<String>(),
    )
}

extern "C" fn rtf_to_parts_thunk(_closure: *const ClosureHeader, value: f64, unit: f64) -> f64 {
    let _obj = this_intl_object("formatToParts", KIND_RELATIVE_TIME);
    parts_to_js_array(&rtf_instance_parts(value, unit))
}

extern "C" fn rtf_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
    unit: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatToParts", KIND_RELATIVE_TIME);
    parts_to_js_array(&rtf_instance_parts(value, unit))
}

fn rtf_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 4);
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(
        out,
        "style",
        string_value(&get_string_field(obj, KEY_RTF_STYLE).unwrap_or_else(|| "long".to_string())),
    );
    set_field(
        out,
        "numeric",
        string_value(&get_string_field(obj, KEY_NUMERIC).unwrap_or_else(|| "always".to_string())),
    );
    set_field(out, "numberingSystem", string_value("latn"));
    js_nanbox_pointer(out as i64)
}

extern "C" fn rtf_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_RELATIVE_TIME);
    rtf_resolved_options_object(obj)
}

extern "C" fn rtf_bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_RELATIVE_TIME);
    rtf_resolved_options_object(obj)
}

// ---- Intl.PluralRules ------------------------------------------------------

/// en plural-category selection. Cardinal: `i == 1 && v == 0` → "one". Ordinal
/// (UTS #35 en ordinal rules): 1st→"one", 2nd→"two", 3rd→"few", else "other".
fn plural_select_en(n: f64, is_ordinal: bool) -> &'static str {
    if !n.is_finite() {
        return "other";
    }
    let abs = n.abs();
    if !is_ordinal {
        return if abs == 1.0 { "one" } else { "other" };
    }
    if abs.fract() != 0.0 {
        return "other";
    }
    let i = abs as u64;
    let m10 = i % 10;
    let m100 = i % 100;
    if m10 == 1 && m100 != 11 {
        "one"
    } else if m10 == 2 && m100 != 12 {
        "two"
    } else if m10 == 3 && m100 != 13 {
        "few"
    } else {
        "other"
    }
}

fn plural_categories(is_ordinal: bool) -> &'static [&'static str] {
    if is_ordinal {
        &["one", "two", "few", "other"]
    } else {
        &["one", "other"]
    }
}

fn plural_rules_select(obj: *const ObjectHeader, value: f64) -> f64 {
    let n = JSValue::from_bits(value.to_bits()).to_number();
    let is_ordinal = get_string_field(obj, KEY_TYPE).as_deref() == Some("ordinal");
    string_value(plural_select_en(n, is_ordinal))
}

extern "C" fn plural_rules_select_thunk(_closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = this_intl_object("select", KIND_PLURAL_RULES);
    plural_rules_select(obj, value)
}

extern "C" fn plural_rules_bound_select_thunk(closure: *const ClosureHeader, value: f64) -> f64 {
    let obj = captured_intl_object(closure, "select", KIND_PLURAL_RULES);
    plural_rules_select(obj, value)
}

extern "C" fn plural_rules_select_range_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = this_intl_object("selectRange", KIND_PLURAL_RULES);
    plural_select_range(start, end)
}

extern "C" fn plural_rules_bound_select_range_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "selectRange", KIND_PLURAL_RULES);
    plural_select_range(start, end)
}

fn plural_select_range(start: f64, end: f64) -> f64 {
    let s = JSValue::from_bits(start.to_bits()).to_number();
    let e = JSValue::from_bits(end.to_bits()).to_number();
    if s.is_nan() || e.is_nan() {
        throw_range_error("Invalid values for Intl.PluralRules.selectRange()");
    }
    // en range plural is "other" for all but trivial cases; report "other".
    string_value("other")
}

fn plural_rules_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 11);
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    let is_ordinal = get_string_field(obj, KEY_TYPE).as_deref() == Some("ordinal");
    set_field(
        out,
        "type",
        string_value(if is_ordinal { "ordinal" } else { "cardinal" }),
    );
    set_field(out, "notation", string_value("standard"));
    set_field(
        out,
        "minimumIntegerDigits",
        get_number_field(obj, KEY_PR_MIN_INT).unwrap_or(1.0),
    );
    let use_sig = get_field(obj, KEY_PR_USE_SIG).to_bits() == crate::value::TAG_TRUE;
    if use_sig {
        set_field(
            out,
            "minimumSignificantDigits",
            get_number_field(obj, KEY_PR_MIN_SIG).unwrap_or(1.0),
        );
        set_field(
            out,
            "maximumSignificantDigits",
            get_number_field(obj, KEY_PR_MAX_SIG).unwrap_or(21.0),
        );
    } else {
        set_field(
            out,
            "minimumFractionDigits",
            get_number_field(obj, KEY_PR_MIN_FRAC).unwrap_or(0.0),
        );
        set_field(
            out,
            "maximumFractionDigits",
            get_number_field(obj, KEY_PR_MAX_FRAC).unwrap_or(3.0),
        );
    }
    let mut categories = js_array_alloc(0);
    for cat in plural_categories(is_ordinal) {
        categories = js_array_push_f64(categories, string_value(cat));
    }
    set_field(
        out,
        "pluralCategories",
        js_nanbox_pointer(categories as i64),
    );
    set_field(out, "roundingIncrement", 1.0);
    set_field(out, "roundingMode", string_value("halfExpand"));
    set_field(out, "roundingPriority", string_value("auto"));
    set_field(out, "trailingZeroDisplay", string_value("auto"));
    js_nanbox_pointer(out as i64)
}

extern "C" fn plural_rules_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_PLURAL_RULES);
    plural_rules_resolved_options_object(obj)
}

extern "C" fn plural_rules_bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_PLURAL_RULES);
    plural_rules_resolved_options_object(obj)
}

fn make_instance(closure: *const ClosureHeader, kind: &str, locales: f64, options: f64) -> f64 {
    let locale = locale_or_default(locales);
    let obj = js_object_alloc(0, 8);
    set_internal_field(obj, KEY_KIND, string_value(kind));
    set_internal_field(obj, KEY_LOCALE, string_value(&locale));

    match kind {
        KIND_NUMBER => {
            let style =
                get_option_string(options, "style").unwrap_or_else(|| "decimal".to_string());
            set_internal_field(obj, KEY_STYLE, string_value(&style));
            if let Some(currency) = get_option_string(options, "currency") {
                set_internal_field(
                    obj,
                    KEY_CURRENCY,
                    string_value(&currency.to_ascii_uppercase()),
                );
            }
            if let Some(max) = get_option_number(options, "maximumFractionDigits") {
                set_internal_field(obj, KEY_MAX_FRACTION_DIGITS, max);
            }
            install_bound_instance_function(
                obj,
                "format",
                number_format_bound_format_thunk as *const u8,
                1,
            );
            install_bound_instance_function(
                obj,
                "formatToParts",
                number_format_bound_to_parts_thunk as *const u8,
                1,
            );
            install_bound_instance_function(
                obj,
                "resolvedOptions",
                number_format_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_DATE_TIME => {
            // `dateStyle` / `timeStyle` are GetOption string enums — an
            // out-of-range value is a RangeError (ECMA-402 CreateDateTimeFormat
            // steps 39/40), not a silent fallthrough.
            let date_style = enum_option(
                options,
                "dateStyle",
                &["full", "long", "medium", "short"],
                "short",
            );
            if let Some(time_style) = get_option_string(options, "timeStyle") {
                if !["full", "long", "medium", "short"].contains(&time_style.as_str()) {
                    throw_range_error(&format!(
                        "Value {time_style} out of range for Intl options property timeStyle"
                    ));
                }
            }
            // `calendar` must match the Unicode locale `type` nonterminal; store
            // the canonicalized ID so `resolvedOptions().calendar` reflects it.
            if let Some(calendar) = get_option_string(options, "calendar") {
                match canonicalize_calendar_id(&calendar) {
                    Some(canonical) => {
                        set_internal_field(obj, KEY_CALENDAR, string_value(&canonical))
                    }
                    None => throw_range_error(&format!(
                        "Value {calendar} out of range for Intl options property calendar"
                    )),
                }
            }
            let mut time_zone =
                get_option_string(options, "timeZone").unwrap_or_else(|| "UTC".to_string());
            // A timeZone that begins with a sign is an offset identifier: it must
            // be syntactically valid (ECMA-402 rejects malformed offsets with a
            // RangeError), and is then canonicalized to `±HH:mm` so
            // `resolvedOptions().timeZone` matches FormatOffsetTimeZoneIdentifier.
            if matches!(time_zone.as_bytes().first(), Some(b'+') | Some(b'-')) {
                if !is_valid_offset_time_zone(&time_zone) {
                    throw_range_error(&format!("Invalid time zone specified: {time_zone}"));
                }
                time_zone = canonicalize_offset_time_zone(&time_zone);
            }
            set_internal_field(obj, KEY_DATE_STYLE, string_value(&date_style));
            set_internal_field(obj, KEY_TIME_ZONE, string_value(&time_zone));
            install_bound_instance_function(
                obj,
                "format",
                date_time_format_bound_format_thunk as *const u8,
                1,
            );
            install_bound_instance_function(
                obj,
                "formatToParts",
                date_time_format_bound_to_parts_thunk as *const u8,
                1,
            );
            install_bound_instance_function(
                obj,
                "formatRange",
                date_time_format_bound_range_thunk as *const u8,
                2,
            );
            install_bound_instance_function(
                obj,
                "formatRangeToParts",
                date_time_format_bound_range_to_parts_thunk as *const u8,
                2,
            );
            install_bound_instance_function(
                obj,
                "resolvedOptions",
                date_time_format_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_COLLATOR => {
            install_bound_instance_function(
                obj,
                "compare",
                collator_bound_compare_thunk as *const u8,
                2,
            );
            install_bound_instance_function(
                obj,
                "resolvedOptions",
                collator_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_SEGMENTER => {
            let granularity = normalize_granularity(get_option_string(options, "granularity"));
            set_internal_field(obj, KEY_GRANULARITY, string_value(&granularity));
            install_bound_instance_function(
                obj,
                "segment",
                segmenter_bound_segment_thunk as *const u8,
                1,
            );
            install_bound_instance_function(
                obj,
                "resolvedOptions",
                segmenter_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_LIST_FORMAT => {
            let list_type = enum_option(
                options,
                "type",
                &["conjunction", "disjunction", "unit"],
                "conjunction",
            );
            let style = enum_option(options, "style", &["long", "short", "narrow"], "long");
            set_internal_field(obj, KEY_TYPE, string_value(&list_type));
            set_internal_field(obj, KEY_LF_STYLE, string_value(&style));
            install_bound_instance_function(
                obj,
                "format",
                list_format_bound_format_thunk as *const u8,
                1,
            );
            install_bound_instance_function(
                obj,
                "formatToParts",
                list_format_bound_to_parts_thunk as *const u8,
                1,
            );
            install_bound_instance_function(
                obj,
                "resolvedOptions",
                list_format_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_RELATIVE_TIME => {
            let style = enum_option(options, "style", &["long", "short", "narrow"], "long");
            let numeric = enum_option(options, "numeric", &["always", "auto"], "always");
            set_internal_field(obj, KEY_RTF_STYLE, string_value(&style));
            set_internal_field(obj, KEY_NUMERIC, string_value(&numeric));
            install_bound_instance_function(obj, "format", rtf_bound_format_thunk as *const u8, 2);
            install_bound_instance_function(
                obj,
                "formatToParts",
                rtf_bound_to_parts_thunk as *const u8,
                2,
            );
            install_bound_instance_function(
                obj,
                "resolvedOptions",
                rtf_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_PLURAL_RULES => {
            let pr_type = enum_option(options, "type", &["cardinal", "ordinal"], "cardinal");
            set_internal_field(obj, KEY_TYPE, string_value(&pr_type));
            let min_int = get_option_number(options, "minimumIntegerDigits").unwrap_or(1.0);
            set_internal_field(obj, KEY_PR_MIN_INT, min_int);
            let min_sig = get_option_number(options, "minimumSignificantDigits");
            let max_sig = get_option_number(options, "maximumSignificantDigits");
            if min_sig.is_some() || max_sig.is_some() {
                set_internal_field(obj, KEY_PR_USE_SIG, bool_value(true));
                set_internal_field(obj, KEY_PR_MIN_SIG, min_sig.unwrap_or(1.0));
                set_internal_field(obj, KEY_PR_MAX_SIG, max_sig.unwrap_or(21.0));
            } else {
                set_internal_field(obj, KEY_PR_USE_SIG, bool_value(false));
                let min_frac = get_option_number(options, "minimumFractionDigits").unwrap_or(0.0);
                let max_frac = get_option_number(options, "maximumFractionDigits")
                    .unwrap_or_else(|| min_frac.max(3.0));
                set_internal_field(obj, KEY_PR_MIN_FRAC, min_frac);
                set_internal_field(obj, KEY_PR_MAX_FRAC, max_frac);
            }
            install_bound_instance_function(
                obj,
                "select",
                plural_rules_bound_select_thunk as *const u8,
                1,
            );
            install_bound_instance_function(
                obj,
                "selectRange",
                plural_rules_bound_select_range_thunk as *const u8,
                2,
            );
            install_bound_instance_function(
                obj,
                "resolvedOptions",
                plural_rules_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_DURATION_FORMAT => duration_format::configure(obj, options),
        KIND_DISPLAY_NAMES => display_names::configure(obj, options),
        _ => {}
    }

    let proto = crate::closure::closure_get_dynamic_prop(closure as usize, "prototype");
    if JSValue::from_bits(proto.to_bits()).is_pointer() {
        crate::object::prototype_chain::object_set_static_prototype(obj as usize, proto.to_bits());
    }
    js_nanbox_pointer(obj as i64)
}

fn install_bound_instance_function(
    obj: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    let closure = crate::closure::js_closure_alloc(func_ptr, 1);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    crate::closure::js_closure_set_capture_f64(closure, 0, js_nanbox_pointer(obj as i64));
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    set_field(obj, name, js_nanbox_pointer(closure as i64));
    set_builtin_attrs(obj, name, PropertyAttrs::new(true, false, true));
}

extern "C" fn number_format_constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    make_instance(closure, KIND_NUMBER, rest_arg(rest, 0), rest_arg(rest, 1))
}

extern "C" fn date_time_format_constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    make_instance(
        closure,
        KIND_DATE_TIME,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

extern "C" fn collator_constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    make_instance(closure, KIND_COLLATOR, rest_arg(rest, 0), rest_arg(rest, 1))
}

extern "C" fn segmenter_constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    make_instance(
        closure,
        KIND_SEGMENTER,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

extern "C" fn list_format_constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    make_instance(
        closure,
        KIND_LIST_FORMAT,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

extern "C" fn relative_time_format_constructor_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    make_instance(
        closure,
        KIND_RELATIVE_TIME,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

extern "C" fn plural_rules_constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    make_instance(
        closure,
        KIND_PLURAL_RULES,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

fn supported_locales_array(locales: f64) -> f64 {
    let locales = locales_from_value(locales);
    let mut arr = js_array_alloc(locales.len() as u32);
    for locale in locales {
        arr = js_array_push_f64(arr, string_value(&locale));
    }
    js_nanbox_pointer(arr as i64)
}

extern "C" fn supported_locales_of_thunk(_closure: *const ClosureHeader, locales: f64) -> f64 {
    supported_locales_array(locales)
}

fn install_function(
    owner: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
    call_arity: u32,
    length: u32,
    has_rest: bool,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return undefined();
    }
    if has_rest {
        crate::closure::js_register_closure_rest(func_ptr, call_arity);
    } else {
        crate::closure::js_register_closure_arity(func_ptr, call_arity);
    }
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    let value = js_nanbox_pointer(closure as i64);
    set_field(owner, name, value);
    set_builtin_attrs(owner, name, PropertyAttrs::new(true, false, true));
    value
}

/// Set `proto[Symbol.toStringTag]` to `tag` (non-writable, non-enumerable,
/// configurable) so `Object.prototype.toString.call(instance)` yields
/// `[object <tag>]` — the ECMA-402 default for every `Intl.*` prototype.
fn set_proto_to_string_tag(proto: *mut ObjectHeader, tag: &str) {
    let sym = crate::symbol::well_known_symbol("toStringTag");
    if sym.is_null() {
        return;
    }
    let tag_str = js_string_from_bytes(tag.as_ptr(), tag.len() as u32);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            js_nanbox_pointer(proto as i64),
            f64::from_bits(JSValue::pointer(sym as *const u8).bits()),
            f64::from_bits(crate::js_nanbox_string(tag_str as i64).to_bits()),
        );
    }
    crate::symbol::set_symbol_property_attrs(
        proto as usize,
        sym as usize,
        PropertyAttrs::new(false, false, true),
    );
}

fn install_constructor(
    ns_obj: *mut ObjectHeader,
    name: &str,
    ctor_ptr: *const u8,
    ctor_length: u32,
    methods: &[(&str, *const u8, u32)],
) {
    let ctor = crate::closure::js_closure_alloc(ctor_ptr, 0);
    if ctor.is_null() {
        return;
    }
    crate::closure::js_register_closure_rest(ctor_ptr, 0);
    crate::object::set_bound_native_closure_name(ctor, name);
    crate::object::set_builtin_closure_length(ctor as usize, ctor_length);
    crate::object::set_builtin_property_attrs(
        ctor as usize,
        "name".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    crate::object::set_builtin_property_attrs(
        ctor as usize,
        "length".to_string(),
        PropertyAttrs::new(false, false, true),
    );

    let ctor_value = js_nanbox_pointer(ctor as i64);
    let proto = js_object_alloc(0, 4);
    set_field(proto, "constructor", ctor_value);
    set_builtin_attrs(proto, "constructor", PropertyAttrs::new(true, false, true));
    for (method, ptr, arity) in methods.iter().copied() {
        install_function(proto, method, ptr, arity, arity, false);
    }
    set_proto_to_string_tag(proto, &format!("Intl.{name}"));
    let proto_value = js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(ctor as usize, "prototype", proto_value);
    crate::object::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        PropertyAttrs::new(false, false, false),
    );

    let supported = install_function(
        ctor as *mut ObjectHeader,
        "supportedLocalesOf",
        supported_locales_of_thunk as *const u8,
        1,
        1,
        false,
    );
    crate::closure::closure_set_dynamic_prop(ctor as usize, "supportedLocalesOf", supported);

    set_field(ns_obj, name, ctor_value);
    set_builtin_attrs(ns_obj, name, PropertyAttrs::new(true, false, true));
}

pub fn install_intl_namespace(ns_obj: *mut ObjectHeader) {
    if ns_obj.is_null() {
        return;
    }
    // `Intl.getCanonicalLocales` / `Intl.supportedValuesOf` — plain namespace
    // functions (length 1 each).
    install_function(
        ns_obj,
        "getCanonicalLocales",
        get_canonical_locales_thunk as *const u8,
        1,
        1,
        false,
    );
    install_function(
        ns_obj,
        "supportedValuesOf",
        supported_values_of_thunk as *const u8,
        1,
        1,
        false,
    );
    locale::install_locale(ns_obj);
    install_constructor(
        ns_obj,
        "NumberFormat",
        number_format_constructor_thunk as *const u8,
        0,
        &[
            ("format", number_format_format_thunk as *const u8, 1),
            (
                "formatToParts",
                number_format_to_parts_thunk as *const u8,
                1,
            ),
            (
                "resolvedOptions",
                number_format_resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
    install_constructor(
        ns_obj,
        "DateTimeFormat",
        date_time_format_constructor_thunk as *const u8,
        0,
        &[
            ("format", date_time_format_format_thunk as *const u8, 1),
            (
                "formatToParts",
                date_time_format_to_parts_thunk as *const u8,
                1,
            ),
            ("formatRange", date_time_format_range_thunk as *const u8, 2),
            (
                "formatRangeToParts",
                date_time_format_range_to_parts_thunk as *const u8,
                2,
            ),
            (
                "resolvedOptions",
                date_time_format_resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
    install_constructor(
        ns_obj,
        "Collator",
        collator_constructor_thunk as *const u8,
        0,
        &[
            ("compare", collator_compare_thunk as *const u8, 2),
            (
                "resolvedOptions",
                collator_resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
    install_constructor(
        ns_obj,
        "Segmenter",
        segmenter_constructor_thunk as *const u8,
        0,
        &[
            ("segment", segmenter_segment_thunk as *const u8, 1),
            (
                "resolvedOptions",
                segmenter_resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
    install_constructor(
        ns_obj,
        "ListFormat",
        list_format_constructor_thunk as *const u8,
        0,
        &[
            ("format", list_format_format_thunk as *const u8, 1),
            ("formatToParts", list_format_to_parts_thunk as *const u8, 1),
            (
                "resolvedOptions",
                list_format_resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
    install_constructor(
        ns_obj,
        "RelativeTimeFormat",
        relative_time_format_constructor_thunk as *const u8,
        0,
        &[
            ("format", rtf_format_thunk as *const u8, 2),
            ("formatToParts", rtf_to_parts_thunk as *const u8, 2),
            (
                "resolvedOptions",
                rtf_resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
    install_constructor(
        ns_obj,
        "PluralRules",
        plural_rules_constructor_thunk as *const u8,
        0,
        &[
            ("select", plural_rules_select_thunk as *const u8, 1),
            (
                "selectRange",
                plural_rules_select_range_thunk as *const u8,
                2,
            ),
            (
                "resolvedOptions",
                plural_rules_resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
    install_constructor(
        ns_obj,
        "DurationFormat",
        duration_format::constructor_thunk as *const u8,
        0,
        &[
            ("format", duration_format::format_thunk as *const u8, 1),
            (
                "formatToParts",
                duration_format::to_parts_thunk as *const u8,
                1,
            ),
            (
                "resolvedOptions",
                duration_format::resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
    install_constructor(
        ns_obj,
        "DisplayNames",
        display_names::constructor_thunk as *const u8,
        2,
        &[
            ("of", display_names::of_thunk as *const u8, 1),
            (
                "resolvedOptions",
                display_names::resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
}
