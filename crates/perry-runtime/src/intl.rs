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
mod date_collator;
mod install;
use install::install_constructor;
mod list_relative_plural;
mod number_format;
mod number_format_options;
mod segmenter;

pub(crate) use date_collator::{
    collator_bound_compare_thunk, collator_bound_resolved_options_thunk, collator_compare_object,
    collator_compare_thunk, collator_resolved_options_object, collator_resolved_options_thunk,
    compare_strings, date_range_parts_from_ms, date_short_utc_from_ms,
    date_time_format_bound_format_thunk, date_time_format_bound_range_thunk,
    date_time_format_bound_range_to_parts_thunk, date_time_format_bound_resolved_options_thunk,
    date_time_format_bound_to_parts_thunk, date_time_format_format_getter_thunk,
    date_time_format_format_thunk, date_time_format_format_value,
    date_time_format_range_parts_value, date_time_format_range_thunk,
    date_time_format_range_to_parts_thunk, date_time_format_range_value,
    date_time_format_resolved_options_object, date_time_format_resolved_options_thunk,
    date_time_format_to_parts_thunk, date_time_range_clip, range_parts_to_js_array,
    swedish_collation_key, temporal_locale_string, TemporalLocaleCtx,
};
pub(crate) use list_relative_plural::{
    canonicalize_calendar_id, canonicalize_offset_time_zone, collect_string_list,
    is_valid_offset_time_zone, list_format_bound_format_thunk,
    list_format_bound_resolved_options_thunk, list_format_bound_to_parts_thunk,
    list_format_format_thunk, list_format_instance_parts, list_format_parts,
    list_format_resolved_options_object, list_format_resolved_options_thunk,
    list_format_to_parts_thunk, list_separators, plural_categories,
    plural_rules_bound_resolved_options_thunk, plural_rules_bound_select_range_thunk,
    plural_rules_bound_select_thunk, plural_rules_resolved_options_object,
    plural_rules_resolved_options_thunk, plural_rules_select, plural_rules_select_range_thunk,
    plural_rules_select_thunk, plural_select_en, plural_select_range, rtf_bound_format_thunk,
    rtf_bound_resolved_options_thunk, rtf_bound_to_parts_thunk, rtf_format_thunk,
    rtf_instance_parts, rtf_parts, rtf_resolved_options_object, rtf_resolved_options_thunk,
    rtf_singular_unit, rtf_to_parts_thunk,
};
pub(crate) use number_format::{
    captured_intl_object, compact_round, compact_suffix, currency_instance_parts,
    decimal_msd_exponent, format_number_instance, grouping_enabled, increment_decimal,
    intl_object_from_value, nf_coerce_number, nf_load, nf_resolved_default,
    number_format_bound_format_thunk, number_format_bound_resolved_options_thunk,
    number_format_bound_to_parts_thunk, number_format_format_getter_thunk,
    number_format_format_object, number_format_range_thunk, number_format_range_to_parts_thunk,
    number_format_resolved_options_object, number_format_resolved_options_thunk,
    number_format_to_parts_thunk, number_instance_parts, number_parts_from_resolved,
    parts_to_js_array, push_grouped_integer, push_sign, push_style_suffix, round_integer_to_place,
    round_mode_code, round_to_fraction, round_to_significant, rounding_up, set_round_ctx,
    significant_count, strip_leading_zeros, this_intl_object, trim_fraction, NfResolved,
};
pub(crate) use number_format_options::{
    configure_number_format, is_well_formed_currency_code, is_well_formed_unit_identifier,
};
#[cfg(feature = "intl-segmenter")]
pub(crate) use segmenter::segment_is_word_like;
pub(crate) use segmenter::{
    build_segments, make_segment_record, normalize_granularity,
    segmenter_bound_resolved_options_thunk, segmenter_bound_segment_thunk,
    segmenter_resolved_options_object, segmenter_resolved_options_thunk, segmenter_segment_object,
    segmenter_segment_thunk, utf16_len,
};

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
// DateTimeFormat option storage (ECMA-402 CreateDateTimeFormat). Each option is
// read+validated once in the constructor and reproduced by `resolvedOptions`.
// Absent fields are simply never written, so `resolvedOptions` can omit them.
const KEY_NUMBERING_SYSTEM: &str = "__intlDtNumbering";
const KEY_HOUR_CYCLE: &str = "__intlDtHourCycle";
const KEY_HOUR12: &str = "__intlDtHour12";
const KEY_WEEKDAY: &str = "__intlDtWeekday";
const KEY_ERA: &str = "__intlDtEra";
const KEY_YEAR: &str = "__intlDtYear";
const KEY_MONTH: &str = "__intlDtMonth";
const KEY_DAY: &str = "__intlDtDay";
const KEY_DAY_PERIOD: &str = "__intlDtDayPeriod";
const KEY_HOUR: &str = "__intlDtHour";
const KEY_MINUTE: &str = "__intlDtMinute";
const KEY_SECOND: &str = "__intlDtSecond";
const KEY_FRACTIONAL: &str = "__intlDtFractional";
const KEY_TIME_ZONE_NAME: &str = "__intlDtTimeZoneName";
const KEY_TIME_STYLE: &str = "__intlDtTimeStyle";
const KEY_DT_IS_DEFAULT: &str = "__intlDtIsDefault";
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

// NumberFormat option storage (ECMA-402 §15). Read once in the constructor and
// reproduced by `resolvedOptions` / the formatter.
const KEY_NF_NUMBERING: &str = "__intlNfNumbering";
const KEY_NF_CURRENCY_DISPLAY: &str = "__intlNfCurrencyDisplay";
const KEY_NF_CURRENCY_SIGN: &str = "__intlNfCurrencySign";
const KEY_NF_UNIT: &str = "__intlNfUnit";
const KEY_NF_UNIT_DISPLAY: &str = "__intlNfUnitDisplay";
const KEY_NF_NOTATION: &str = "__intlNfNotation";
const KEY_NF_COMPACT_DISPLAY: &str = "__intlNfCompactDisplay";
const KEY_NF_SIGN_DISPLAY: &str = "__intlNfSignDisplay";
const KEY_NF_USE_GROUPING: &str = "__intlNfUseGrouping";
const KEY_NF_MIN_INT: &str = "__intlNfMinInt";
const KEY_NF_MIN_FRAC: &str = "__intlNfMinFrac";
const KEY_NF_USE_SIG: &str = "__intlNfUseSig";
const KEY_NF_MIN_SIG: &str = "__intlNfMinSig";
const KEY_NF_MAX_SIG: &str = "__intlNfMaxSig";
const KEY_NF_ROUNDING_INCREMENT: &str = "__intlNfRoundingIncrement";
const KEY_NF_ROUNDING_MODE: &str = "__intlNfRoundingMode";
const KEY_NF_ROUNDING_PRIORITY: &str = "__intlNfRoundingPriority";
const KEY_NF_TRAILING_ZERO: &str = "__intlNfTrailingZero";
// Hidden [[BoundFormat]] slots. The bound format function is also installed as an
// own `format` property for the native dispatch fast path, but the prototype
// `format` getter reads it from here so user mutation/deletion of the public
// property can't corrupt what the accessor returns.
const KEY_NF_BOUND_FORMAT: &str = "__intlNfBoundFormat";
const KEY_DTF_BOUND_FORMAT: &str = "__intlDtfBoundFormat";
const KEY_COL_USAGE: &str = "__intlColUsage";
const KEY_COL_SENSITIVITY: &str = "__intlColSensitivity";
const KEY_COL_IGNORE_PUNCT: &str = "__intlColIgnorePunct";
const KEY_COL_COLLATION: &str = "__intlColCollation";
const KEY_COL_NUMERIC: &str = "__intlColNumeric";
const KEY_COL_CASE_FIRST: &str = "__intlColCaseFirst";
const KEY_PR_NOTATION: &str = "__intlPrNotation";
const KEY_PR_COMPACT_DISPLAY: &str = "__intlPrCompactDisplay";

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

/// Coerce an already-fetched option value to its GetOption string form. ECMA-402
/// GetOption treats ONLY `undefined` as "absent → fallback"; every other value —
/// `null` included — is coerced with ToString and then checked against the
/// allow-list, so `{ localeMatcher: null }` must surface as the string "null"
/// (which no enum accepts) and raise a RangeError, not be silently ignored.
/// Kept separate from the property read so callers that must observe the option
/// getter exactly once (the GetOption call-order tests) can reuse the value.
fn coerce_option_string(value: f64) -> Option<String> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        None
    } else if js.is_null() {
        Some("null".to_string())
    } else if js.is_any_string() {
        string_from_string_value(value)
    } else if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        throw_type_error("Cannot convert a Symbol value to a string")
    } else {
        Some(value_to_string(value))
    }
}

fn get_option_string(options: f64, key: &str) -> Option<String> {
    coerce_option_string(get_option_value(options, key))
}

/// As `get_option_string`, but for the Unicode locale-extension keys (`calendar`,
/// `numberingSystem`) whose value is validated for *well-formedness* rather than
/// against a closed enum. ECMA-402 coerces `null` to the string `"null"` — a
/// well-formed `type` subtag that names no supported calendar / numbering system,
/// so ResolveLocale drops it and `resolvedOptions` reports the locale default
/// (`gregory` / `latn`). Perry models no per-locale extension negotiation and
/// otherwise echoes the requested value verbatim, so it mirrors that observable
/// outcome by treating `null` as "absent" (leaving the field at its default)
/// rather than reporting a literal `"null"`. A non-null unsupported value is
/// still echoed, matching Perry's existing behaviour. The option getter is read
/// exactly once so the GetOption call-order is preserved.
fn get_locale_extension_option(options: f64, key: &str) -> Option<String> {
    let value = get_option_value(options, key);
    if JSValue::from_bits(value.to_bits()).is_null() {
        return None;
    }
    coerce_option_string(value)
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

/// GetOption(options, key, "string", «allowed», default) — coerce to string,
/// require membership in `allowed`, else `RangeError`. Absent → `default`.
fn get_string_option_enum(options: f64, key: &str, allowed: &[&str], default: &str) -> String {
    match get_option_string(options, key) {
        None => default.to_string(),
        Some(value) => {
            if allowed.contains(&value.as_str()) {
                value
            } else {
                throw_range_error(&format!(
                    "Value {value} out of range for Intl.NumberFormat options property {key}"
                ))
            }
        }
    }
}

/// GetOption(options, key, "boolean"/"string", …) for `useGrouping`: returns the
/// resolved value as a string — `"false"` for a falsy boolean, otherwise one of
/// `"auto"`/`"always"`/`"min2"`. `true` maps to `"always"`, absent → `default`.
fn get_use_grouping_option(options: f64, default: &str) -> String {
    let value = get_option_value(options, "useGrouping");
    let js = JSValue::from_bits(value.to_bits());
    // GetStringOrBooleanOption(options, "useGrouping",
    //   «"min2","auto","always"», "always", false, fallback):
    // 2. undefined → fallback.
    if js.is_undefined() {
        return default.to_string();
    }
    // 3. The boolean `true` → trueValue ("always").
    if js.is_bool() && js.as_bool() {
        return "always".to_string();
    }
    // 4. Any value whose ToBoolean is false (false, 0, null, "") → falseValue,
    //    stored as the sentinel "false" (resolvedOptions surfaces it as `false`).
    if crate::value::js_is_truthy(value) == 0 {
        return "false".to_string();
    }
    // 5-8. ToString the (truthy) value. The strings "true"/"false" map back to
    //    the fallback; only the sanctioned grouping strings are otherwise valid.
    let s = if js.is_any_string() {
        string_from_string_value(value).unwrap_or_default()
    } else {
        value_to_string(value)
    };
    match s.as_str() {
        "true" | "false" => default.to_string(),
        "min2" | "auto" | "always" => s,
        other => throw_range_error(&format!(
            "Value {other} out of range for Intl.NumberFormat options property useGrouping"
        )),
    }
}

/// GetNumberOption(options, key, min, max, fallback) with integer truncation and
/// `RangeError` when out of `[min, max]`. Returns `None` when absent.
fn get_int_option_in_range(options: f64, key: &str, min: f64, max: f64) -> Option<f64> {
    let value = get_option_value(options, key);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        return None;
    }
    let n = js.to_number();
    if n.is_nan() || n < min || n > max {
        throw_range_error(&format!(
            "Value {n} out of range for Intl.NumberFormat options property {key}"
        ));
    }
    Some(n.floor())
}

/// GetNumberOption(options, key, min, max, undefined) using a full ToNumber
/// (`js_number_coerce`) so string and object option values coerce correctly
/// (`JSValue::to_number` returns NaN for non-primitives). Out of `[min, max]`,
/// NaN, or a non-numeric value is a `RangeError`; the result is floored.
fn get_number_option_coerced(options: f64, key: &str, min: f64, max: f64) -> Option<f64> {
    let value = get_option_value(options, key);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return None;
    }
    let n = crate::builtins::js_number_coerce(value);
    if n.is_nan() || n < min || n > max {
        throw_range_error(&format!(
            "Value {n} out of range for Intl options property {key}"
        ));
    }
    Some(n.floor())
}

/// Default fraction-digit count for a currency code (CLDR `currencyDigits`). Most
/// currencies use 2; this covers the common zero/three-digit exceptions enough
/// for the parity matrix. Unknown codes fall back to 2.
fn currency_fraction_digits(code: &str) -> u32 {
    match code {
        "JPY" | "KRW" | "CLP" | "ISK" | "HUF" | "TWD" | "VND" => 0,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        _ => 2,
    }
}

/// A `numberingSystem` value is structurally valid when it is one or more
/// hyphen-separated subtags of 3–8 alphanumerics (the `type` Unicode nonterminal).
fn is_well_formed_numbering_system(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|sub| {
            (3..=8).contains(&sub.len()) && sub.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// Extract the `-u-nu-<value>` numbering system from a (canonicalized) locale
/// string, lower-cased. Returns `None` when no `nu` keyword is present.
fn numbering_system_from_locale(locale: &str) -> Option<String> {
    let lower = locale.to_ascii_lowercase();
    let subtags: Vec<&str> = lower.split('-').collect();
    let u = subtags.iter().position(|s| *s == "u")?;
    let mut i = u + 1;
    while i < subtags.len() {
        let key = subtags[i];
        // A keyword key is exactly two chars; everything up to the next key is its value.
        if key.len() == 2 {
            if key == "nu" {
                let mut value = String::new();
                let mut j = i + 1;
                while j < subtags.len() && subtags[j].len() != 2 {
                    if !value.is_empty() {
                        value.push('-');
                    }
                    value.push_str(subtags[j]);
                    j += 1;
                }
                return (!value.is_empty()).then_some(value);
            }
            i += 1;
            while i < subtags.len() && subtags[i].len() != 2 {
                i += 1;
            }
        } else {
            // Hit another singleton extension (e.g. `-t-`); `nu` lives only under `u`.
            break;
        }
    }
    None
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
    // Subtags after a singleton (length-1 `u`/`t`/`x`/…) belong to an extension
    // or private-use sequence and are canonicalized to lower case (UTS #35) — the
    // core-tag region rule (uppercase 2-letter subtags) must not apply there, or
    // `en-US-u-nu-latn` would mis-canonicalize the `nu` keyword to `NU`.
    let mut in_extension = false;
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
        if i == 0 || in_extension {
            out.push_str(&subtag.to_ascii_lowercase());
        } else if subtag.len() == 2 && subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
            out.push_str(&subtag.to_ascii_uppercase());
        } else {
            out.push_str(subtag);
        }
        if subtag.len() == 1 {
            in_extension = true;
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

/// `HasProperty(O, ToString(index))` — true when the integer-indexed property is
/// present (own or inherited). Used to skip holes/absent indices in
/// CanonicalizeLocaleList's array/array-like walk.
fn js_has_index(obj: f64, index: u32) -> bool {
    let key = string_value(&index.to_string());
    crate::object::js_object_has_property(obj, key).to_bits() == crate::value::TAG_TRUE
}

/// CanonicalizeLocaleList element handler: a present element must be a String or
/// an Object (an `Intl.Locale` or anything ToString-able), else `TypeError`; the
/// resulting tag is canonicalized (`RangeError` if structurally invalid) and
/// pushed if not already present.
fn push_locale_element(out: &mut Vec<String>, value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    let tag = if jv.is_any_string() {
        string_from_string_value(value).unwrap_or_default()
    } else if object_ptr_from_value(value).is_some() {
        value_to_string(value)
    } else {
        // undefined / null / boolean / number / Symbol element → TypeError.
        throw_type_error("locale must be a String or Object");
    };
    let Some(canonical) = canonicalize_language_tag(&tag) else {
        throw_invalid_language_tag(&tag);
    };
    if !out.iter().any(|existing| existing == &canonical) {
        out.push(canonical);
    }
}

fn locales_from_value(locales: f64) -> Vec<String> {
    let js = JSValue::from_bits(locales.to_bits());
    // CanonicalizeLocaleList(undefined) is the empty list; `null` fails ToObject
    // with a TypeError (everything else is a String or coerces via ToObject).
    if js.is_undefined() {
        return Vec::new();
    }
    if js.is_null() {
        throw_type_error("Cannot convert undefined or null to object");
    }
    // A String argument is treated as a single-element list (not iterated by char).
    if js.is_any_string() {
        let tag = string_from_string_value(locales).unwrap_or_default();
        let Some(canonical) = canonicalize_language_tag(&tag) else {
            throw_invalid_language_tag(&tag);
        };
        return vec![canonical];
    }
    if let Some(arr) = array_ptr_from_value(locales) {
        let len = js_array_length(arr);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            push_locale_element(&mut out, js_array_get_f64(arr, i));
        }
        return out;
    }
    // CanonicalizeLocaleList on a generic array-like Object: iterate `O[0..length]`
    // (e.g. `{ 0: "DE", length: 1 }` → `["de"]`).
    if let Some(obj) = object_ptr_from_value(locales) {
        // `length = ? ToLength(? Get(O, "length"))`: a throwing `length` getter or
        // ToNumber step (Symbol / abrupt valueOf/toString) propagates here.
        let len_raw = get_field(obj, "length");
        let len_num = crate::builtins::js_number_coerce(len_raw);
        let len = if len_num.is_finite() && len_num > 0.0 {
            len_num as u32
        } else {
            0
        };
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            // Skip absent indices (`HasProperty` is false) — e.g.
            // `{ length: 3, 0: "en" }` yields just `["en"]`, never `undefined`.
            if !js_has_index(locales, i) {
                continue;
            }
            push_locale_element(&mut out, get_field(obj, &i.to_string()));
        }
        return out;
    }
    // Other primitives (number/boolean/Symbol/BigInt): ToObject yields a wrapper
    // with length 0 — an empty list, no throw.
    Vec::new()
}

/// BestAvailableLocale (lookup) — a requested canonical locale is "supported"
/// when its primary language subtag is one Perry's deterministic formatters can
/// service. Perry carries no CLDR locale database, so this is a curated set of
/// common CLDR languages rather than a data lookup: it is enough to distinguish
/// real languages (`en`, `de`, `zh`, …) from the "no linguistic content" tag
/// `zxx` and other unsupported primaries that `supportedLocalesOf` must drop.
fn is_available_locale(canonical: &str) -> bool {
    let primary = canonical.split(['-', '_']).next().unwrap_or(canonical);
    const AVAILABLE_LANGUAGES: &[&str] = &[
        "af", "am", "ar", "az", "be", "bg", "bn", "bs", "ca", "cs", "cy", "da", "de", "el", "en",
        "es", "et", "eu", "fa", "fi", "fil", "fr", "ga", "gl", "gu", "he", "hi", "hr", "hu", "hy",
        "id", "is", "it", "ja", "ka", "kk", "km", "kn", "ko", "ky", "lo", "lt", "lv", "mk", "ml",
        "mn", "mr", "ms", "my", "nb", "ne", "nl", "no", "pa", "pl", "pt", "ro", "ru", "si", "sk",
        "sl", "sq", "sr", "sv", "sw", "ta", "te", "th", "tr", "uk", "ur", "uz", "vi", "zh", "zu",
    ];
    AVAILABLE_LANGUAGES.contains(&primary)
}

fn locale_or_default(locales: f64) -> String {
    locales_from_value(locales)
        .into_iter()
        .next()
        .unwrap_or_else(|| "en-US".to_string())
}

/// Look up a Unicode (`-u-`) extension keyword's value in a BCP-47 tag. Returns
/// `Some(value)` if the 2-letter `key` is present (the value is the `-`-joined
/// run of type subtags after it, or `""` for a value-less boolean key like
/// `-u-kn`), else `None`. Case-insensitive. Used to resolve `kn`/`kf`/`co` for
/// Collator when the corresponding option is absent (numeric-and-caseFirst.js).
fn unicode_extension_keyword(locale: &str, key: &str) -> Option<String> {
    let lower = locale.to_ascii_lowercase();
    let key = key.to_ascii_lowercase();
    let mut iter = lower.split('-');
    // Advance to the `u` singleton. A `x` singleton starts the private-use
    // sequence (which must come last); a `u` inside it — e.g. `en-x-u-kn` — is
    // private data, not a Unicode extension, so stop scanning there.
    let mut in_u = false;
    for p in iter.by_ref() {
        if p == "x" {
            return None;
        }
        if p == "u" {
            in_u = true;
            break;
        }
    }
    if !in_u {
        return None;
    }
    let mut found = false;
    let mut value: Vec<&str> = Vec::new();
    for p in iter {
        if p.len() == 1 {
            // Next singleton ends the `u` extension.
            break;
        }
        if p.len() == 2 && p.chars().all(|c| c.is_ascii_alphanumeric()) {
            if found {
                break; // reached the next keyword
            }
            if p == key {
                found = true;
            }
        } else if found {
            value.push(p);
        }
    }
    found.then(|| value.join("-"))
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

#[cold]
fn throw_range_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(js_nanbox_pointer(err as i64))
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

/// `GetOption(options, key, "string", ...)` with full `ToString` coercion: only
/// `undefined` selects the default. `null`, numbers, booleans, etc. are coerced
/// via `ToString` (so `null` → `"null"`, never the absent path), and a Symbol
/// throws `TypeError` (ToString of a Symbol is a TypeError). This is the strict
/// spec behavior; `get_option_string` instead treats `null` as absent, which the
/// `options-*-invalid` value-validation tests reject.
fn get_option_string_coerced(options: f64, key: &str) -> Option<String> {
    let raw = get_option_value(options, key);
    let jv = JSValue::from_bits(raw.to_bits());
    if jv.is_undefined() {
        None
    } else if jv.is_any_string() {
        string_from_string_value(raw)
    } else if unsafe { crate::symbol::js_is_symbol(raw) } != 0 {
        throw_type_error(&format!(
            "Cannot convert a Symbol value to a string for Intl options property {key}"
        ));
    } else {
        Some(value_to_string(raw))
    }
}

/// `GetOption` with an enumerated value set, using strict `ToString` coercion
/// (see [`get_option_string_coerced`]): an out-of-range value (including a
/// `ToString`-coerced `null` / number) is a `RangeError`; absent → `default`.
fn enum_option_strict(options: f64, key: &str, allowed: &[&str], default: &str) -> String {
    match get_option_string_coerced(options, key) {
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

/// `GetOptionsObject(options)`: `undefined` yields an empty bag (reported as
/// `undefined`, which the option readers treat as "every key absent"); an Object
/// passes through unchanged; any other value (including `null`, primitives, and
/// BigInt) throws `TypeError`. Used by the constructors whose spec step is
/// `GetOptionsObject` (ListFormat, Segmenter, PluralRules, …).
fn get_options_object(options: f64) -> f64 {
    let jv = JSValue::from_bits(options.to_bits());
    if jv.is_undefined() {
        return options;
    }
    if object_ptr_from_value(options).is_some() {
        return options;
    }
    throw_type_error("Cannot convert undefined or null to object");
}

/// `CoerceOptionsToObject(options)` partial: `undefined` stays an empty bag and
/// `null` throws `TypeError` (`ToObject(null)`). Primitives are *not* boxed here
/// — Perry reads option keys directly off Objects, so a primitive simply yields
/// every-key-absent — but `null` must still reject. Used by the constructors
/// whose spec step is `ToObject` (RelativeTimeFormat, Collator, …).
fn coerce_options_reject_null(options: f64) -> f64 {
    if JSValue::from_bits(options.to_bits()).is_null() {
        throw_type_error("Cannot convert undefined or null to object");
    }
    options
}

/// GetBooleanOption(options, key): `undefined` → `None`, otherwise ToBoolean.
fn get_bool_option(options: f64, key: &str) -> Option<bool> {
    let value = get_option_value(options, key);
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        None
    } else {
        Some(crate::value::js_is_truthy(value) != 0)
    }
}

/// Read a `DateTimeFormat` component option (Table 7): a GetOption string enum
/// that, when present, must be one of `allowed` (else `RangeError`) and is then
/// stored in `store_key`. Returns whether the option was supplied.
fn dt_component_option(
    obj: *mut ObjectHeader,
    options: f64,
    key: &str,
    allowed: &[&str],
    store_key: &str,
) -> bool {
    match get_option_string(options, key) {
        None => false,
        Some(value) => {
            if !allowed.contains(&value.as_str()) {
                throw_range_error(&format!(
                    "Value {value} out of range for Intl options property {key}"
                ));
            }
            set_internal_field(obj, store_key, string_value(&value));
            true
        }
    }
}

/// Validate a *named* (non-offset) `timeZone` identifier. Perry ships no tz
/// database (see `date.rs`), so this is a structural check rather than a lookup:
/// the case-insensitive UTC aliases normalize to `"UTC"`, the legacy
/// single-component zone names are accepted from a fixed list, and any other
/// identifier must be an all-ASCII, space-free `Area/Location[/…]` form. Real
/// IANA zone identifiers pass; the malformed names ECMA-402 rejects
/// (`"MEZ"`, `"invalid"`, `"Europe/İstanbul"`, …) do not. Returns the (best
/// effort, un-recased) canonical identifier, or `None` to signal `RangeError`.
fn canonicalize_named_time_zone(tz: &str) -> Option<String> {
    if tz.eq_ignore_ascii_case("UTC") || tz.eq_ignore_ascii_case("Etc/UTC") {
        return Some("UTC".to_string());
    }
    if !tz.is_ascii() {
        return None;
    }
    // Legacy single-component IANA zones / links that carry no '/'.
    const SINGLE_WORD_ZONES: &[&str] = &[
        "GMT",
        "GMT0",
        "Zulu",
        "Universal",
        "UCT",
        "Greenwich",
        "Navajo",
        "Eire",
        "Iceland",
        "Cuba",
        "Egypt",
        "Hongkong",
        "Iran",
        "Israel",
        "Japan",
        "Jamaica",
        "Libya",
        "Poland",
        "Portugal",
        "PRC",
        "Singapore",
        "Turkey",
        "ROC",
        "ROK",
        "W-SU",
        "Factory",
        "EST",
        "MST",
        "HST",
        "EST5EDT",
        "CST6CDT",
        "MST7MDT",
        "PST8PDT",
    ];
    if SINGLE_WORD_ZONES.iter().any(|z| z.eq_ignore_ascii_case(tz)) {
        return Some(tz.to_string());
    }
    let segments: Vec<&str> = tz.split('/').collect();
    if segments.len() < 2 {
        return None;
    }
    let mut has_alpha = false;
    for seg in &segments {
        if seg.is_empty() {
            return None;
        }
        for b in seg.bytes() {
            if b.is_ascii_alphabetic() {
                has_alpha = true;
            } else if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'+' || b == b'-') {
                return None;
            }
        }
    }
    has_alpha.then(|| tz.to_string())
}
fn make_instance(closure: *const ClosureHeader, kind: &str, locales: f64, options: f64) -> f64 {
    let locale = locale_or_default(locales);
    let obj = js_object_alloc(0, 8);
    set_internal_field(obj, KEY_KIND, string_value(kind));
    set_internal_field(obj, KEY_LOCALE, string_value(&locale));

    match kind {
        KIND_NUMBER => {
            configure_number_format(obj, &locale, options);
            // The bound format function is the [[BoundFormat]] slot: ECMA-402
            // gives it an empty `name` ("") and length 1. It is installed as an
            // own `format` property so `nf.format(x)` dispatches without the
            // prototype accessor (native objects resolve methods from own
            // props), and is also stashed in the hidden KEY_NF_BOUND_FORMAT slot
            // that the prototype `format` getter reads — so mutating or deleting
            // the public property can't corrupt what the accessor returns.
            let format_fn = install_bound_instance_function(
                obj,
                "format",
                number_format_bound_format_thunk as *const u8,
                1,
            );
            if !format_fn.is_null() {
                crate::object::set_bound_native_closure_name(format_fn, "");
                set_internal_field(
                    obj,
                    KEY_NF_BOUND_FORMAT,
                    js_nanbox_pointer(format_fn as i64),
                );
            }
            install_bound_instance_function(
                obj,
                "formatToParts",
                number_format_bound_to_parts_thunk as *const u8,
                1,
            );
            // `formatRange`/`formatRangeToParts` are installed as own instance
            // properties (native Intl method dispatch resolves from own props,
            // not the static prototype) but with a *this-based* closure rather
            // than a bound one: a detached `nf.formatRange` reference therefore
            // loses `this` and the `this_intl_object` guard throws a TypeError
            // (formatRange/invoked-as-func.js), matching the non-bound prototype
            // method these shadow.
            install_function(
                obj,
                "formatRange",
                number_format_range_thunk as *const u8,
                2,
                2,
                false,
            );
            install_function(
                obj,
                "formatRangeToParts",
                number_format_range_to_parts_thunk as *const u8,
                2,
                2,
                false,
            );
            install_bound_instance_function(
                obj,
                "resolvedOptions",
                number_format_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_DATE_TIME => {
            // CoerceOptionsToObject: `undefined` behaves as an empty (null-proto)
            // options object, but `null` (and other ToObject-rejected primitives)
            // is a TypeError. Primitives that DO coerce become wrapper objects
            // with no DateTimeFormat-relevant properties, i.e. behave as empty —
            // `object_ptr_from_value` already returns `None` for them, so option
            // reads simply see `undefined`.
            if JSValue::from_bits(options.to_bits()).is_null() {
                throw_type_error("Cannot convert undefined or null to object");
            }
            // GetOption reads run in the exact ECMA-402 CreateDateTimeFormat
            // order (constructor-options-order.js asserts this sequence).
            // localeMatcher / formatMatcher are validated but don't affect the
            // deterministic formatter, so their resolved value is discarded.
            let _ = enum_option(
                options,
                "localeMatcher",
                &["lookup", "best fit"],
                "best fit",
            );
            // `calendar` must match the Unicode locale `type` nonterminal; store
            // the canonicalized ID so `resolvedOptions().calendar` reflects it.
            if let Some(calendar) = get_locale_extension_option(options, "calendar") {
                match canonicalize_calendar_id(&calendar) {
                    Some(canonical) => {
                        set_internal_field(obj, KEY_CALENDAR, string_value(&canonical))
                    }
                    None => throw_range_error(&format!(
                        "Value {calendar} out of range for Intl options property calendar"
                    )),
                }
            }
            // `numberingSystem` must be a well-formed `type` nonterminal.
            if let Some(ns) = get_locale_extension_option(options, "numberingSystem") {
                if !is_well_formed_numbering_system(&ns) {
                    throw_range_error(&format!(
                        "Value {ns} out of range for Intl options property numberingSystem"
                    ));
                }
                set_internal_field(obj, KEY_NUMBERING_SYSTEM, string_value(&ns));
            }
            // hour12 (boolean) then hourCycle (enum) — both only surface in
            // `resolvedOptions` when the resolved pattern has an hour field.
            if let Some(h12) = get_bool_option(options, "hour12") {
                set_internal_field(obj, KEY_HOUR12, bool_value(h12));
            }
            if let Some(hc) = get_option_string(options, "hourCycle") {
                if !["h11", "h12", "h23", "h24"].contains(&hc.as_str()) {
                    throw_range_error(&format!(
                        "Value {hc} out of range for Intl options property hourCycle"
                    ));
                }
                set_internal_field(obj, KEY_HOUR_CYCLE, string_value(&hc));
            }
            let mut time_zone =
                get_option_string(options, "timeZone").unwrap_or_else(|| "UTC".to_string());
            // A timeZone that begins with a sign is an offset identifier: it must
            // be syntactically valid (ECMA-402 rejects malformed offsets with a
            // RangeError), and is then canonicalized to `±HH:mm` so
            // `resolvedOptions().timeZone` matches FormatOffsetTimeZoneIdentifier.
            // Named zones are validated structurally (Perry has no tz database).
            if matches!(time_zone.as_bytes().first(), Some(b'+') | Some(b'-')) {
                if !is_valid_offset_time_zone(&time_zone) {
                    throw_range_error(&format!("Invalid time zone specified: {time_zone}"));
                }
                time_zone = canonicalize_offset_time_zone(&time_zone);
            } else {
                match canonicalize_named_time_zone(&time_zone) {
                    Some(canonical) => time_zone = canonical,
                    None => throw_range_error(&format!("Invalid time zone specified: {time_zone}")),
                }
            }
            set_internal_field(obj, KEY_TIME_ZONE, string_value(&time_zone));
            // Date/time component options (ECMA-402 Table 7), read in order. Each
            // out-of-range value is a RangeError.
            //
            // Two separate flags are tracked:
            //
            // • `any_component` — the ECMA-402 §11.1.2 `needDefaults` flag.
            //   Only the fields listed in steps 38a/38b count:
            //     date fields: weekday, year, month, day
            //     time fields: dayPeriod, hour, minute, second, fractionalSecondDigits
            //   `era` and `timeZoneName` are read and stored but do NOT affect
            //   this flag — an era-only or timeZoneName-only DTF still gets
            //   year/month/day defaults applied (spec step 40).
            //
            // • `has_explicit_component` — set by ALL of the above INCLUDING
            //   `era` and `timeZoneName`.  Used for the dateStyle/timeStyle
            //   conflict check (step 35.b: throw if style + any component option).
            let mut any_component = false;
            let mut has_explicit_component = false;
            let has_weekday = dt_component_option(
                obj,
                options,
                "weekday",
                &["narrow", "short", "long"],
                KEY_WEEKDAY,
            );
            any_component |= has_weekday;
            has_explicit_component |= has_weekday;
            // era counts toward the style-conflict check but NOT toward needDefaults.
            has_explicit_component |=
                dt_component_option(obj, options, "era", &["narrow", "short", "long"], KEY_ERA);
            let has_year =
                dt_component_option(obj, options, "year", &["2-digit", "numeric"], KEY_YEAR);
            any_component |= has_year;
            has_explicit_component |= has_year;
            let has_month = dt_component_option(
                obj,
                options,
                "month",
                &["2-digit", "numeric", "narrow", "short", "long"],
                KEY_MONTH,
            );
            any_component |= has_month;
            has_explicit_component |= has_month;
            let has_day =
                dt_component_option(obj, options, "day", &["2-digit", "numeric"], KEY_DAY);
            any_component |= has_day;
            has_explicit_component |= has_day;
            let has_day_period = dt_component_option(
                obj,
                options,
                "dayPeriod",
                &["narrow", "short", "long"],
                KEY_DAY_PERIOD,
            );
            any_component |= has_day_period;
            has_explicit_component |= has_day_period;
            let has_hour =
                dt_component_option(obj, options, "hour", &["2-digit", "numeric"], KEY_HOUR);
            any_component |= has_hour;
            has_explicit_component |= has_hour;
            let has_minute =
                dt_component_option(obj, options, "minute", &["2-digit", "numeric"], KEY_MINUTE);
            any_component |= has_minute;
            has_explicit_component |= has_minute;
            let has_second =
                dt_component_option(obj, options, "second", &["2-digit", "numeric"], KEY_SECOND);
            any_component |= has_second;
            has_explicit_component |= has_second;
            // fractionalSecondDigits is GetNumberOption(1, 3) — out of range or
            // non-numeric is a RangeError.
            if let Some(n) = get_number_option_coerced(options, "fractionalSecondDigits", 1.0, 3.0)
            {
                set_internal_field(obj, KEY_FRACTIONAL, n);
                any_component = true;
                has_explicit_component = true;
            }
            // timeZoneName counts toward the style-conflict check but NOT toward needDefaults.
            has_explicit_component |= dt_component_option(
                obj,
                options,
                "timeZoneName",
                &[
                    "short",
                    "long",
                    "shortOffset",
                    "longOffset",
                    "shortGeneric",
                    "longGeneric",
                ],
                KEY_TIME_ZONE_NAME,
            );
            let _ = enum_option(options, "formatMatcher", &["basic", "best fit"], "best fit");
            // dateStyle / timeStyle have no default (an absent style stays absent
            // in `resolvedOptions`); an out-of-range value is a RangeError.
            let date_style = get_option_string(options, "dateStyle");
            if let Some(ref ds) = date_style {
                if !["full", "long", "medium", "short"].contains(&ds.as_str()) {
                    throw_range_error(&format!(
                        "Value {ds} out of range for Intl options property dateStyle"
                    ));
                }
            }
            let time_style = get_option_string(options, "timeStyle");
            if let Some(ref ts) = time_style {
                if !["full", "long", "medium", "short"].contains(&ts.as_str()) {
                    throw_range_error(&format!(
                        "Value {ts} out of range for Intl options property timeStyle"
                    ));
                }
            }
            let has_style = date_style.is_some() || time_style.is_some();
            // ECMA-402 §11.1.2 step 35.b: combining a style with any explicit
            // component option (including era and timeZoneName) is a TypeError.
            if has_style && has_explicit_component {
                throw_type_error(
                    "Intl.DateTimeFormat: dateStyle/timeStyle cannot be used with explicit date-time component options",
                );
            }
            if let Some(ds) = date_style {
                set_internal_field(obj, KEY_DATE_STYLE, string_value(&ds));
            }
            if let Some(ts) = time_style {
                set_internal_field(obj, KEY_TIME_STYLE, string_value(&ts));
            }
            // ToDateTimeOptions(required="any", defaults="date"): when neither a
            // style nor any component was requested, fall back to numeric
            // year/month/day so `resolvedOptions` reports the default date shape.
            if !has_style && !any_component {
                set_internal_field(obj, KEY_YEAR, string_value("numeric"));
                set_internal_field(obj, KEY_MONTH, string_value("numeric"));
                set_internal_field(obj, KEY_DAY, string_value("numeric"));
                set_internal_field(obj, KEY_DT_IS_DEFAULT, bool_value(true));
            }
            let format_fn = install_bound_instance_function(
                obj,
                "format",
                date_time_format_bound_format_thunk as *const u8,
                1,
            );
            if !format_fn.is_null() {
                crate::object::set_bound_native_closure_name(format_fn, "");
                set_internal_field(
                    obj,
                    KEY_DTF_BOUND_FORMAT,
                    js_nanbox_pointer(format_fn as i64),
                );
            }
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
            // InitializeCollator reads options via `? ToObject(options)` (null →
            // TypeError) then GetOption in this exact order: usage, localeMatcher,
            // collation, numeric, caseFirst, sensitivity, ignorePunctuation
            // (constructor-options-throwing-getters / resolvedOptions order.js).
            let options = coerce_options_reject_null(options);
            let usage = enum_option_strict(options, "usage", &["sort", "search"], "sort");
            let _ = enum_option_strict(
                options,
                "localeMatcher",
                &["lookup", "best fit"],
                "best fit",
            );
            // `collation` is a `type` string: malformed, or the reserved `standard`
            // /`search` values, are a RangeError (the latter are only valid as a
            // `usage` selector, never an explicit collation). A valid value wins
            // over any `-u-co-` keyword; absent ⇒ fall back to the extension.
            let collation_opt = get_option_string_coerced(options, "collation").map(|v| {
                if !is_well_formed_numbering_system(&v) || v == "standard" || v == "search" {
                    throw_range_error(&format!(
                        "Value {v} out of range for Intl options property collation"
                    ));
                }
                v
            });
            let numeric_opt = get_bool_option(options, "numeric");
            let case_first_opt = get_option_string_coerced(options, "caseFirst").map(|v| {
                if ["upper", "lower", "false"].contains(&v.as_str()) {
                    v
                } else {
                    throw_range_error(&format!(
                        "Value {v} out of range for Intl options property caseFirst"
                    ))
                }
            });
            let sensitivity = enum_option_strict(
                options,
                "sensitivity",
                &["base", "accent", "case", "variant"],
                "variant",
            );
            let ignore_punct = get_bool_option(options, "ignorePunctuation").unwrap_or(false);
            // ResolveLocale: when an option is absent, fall back to the matching
            // Unicode (`-u-`) extension keyword in the resolved locale — `kn`
            // (numeric, value-less ⇒ true) and `kf` (caseFirst).
            let numeric =
                numeric_opt.unwrap_or_else(|| match unicode_extension_keyword(&locale, "kn") {
                    Some(v) => v != "false",
                    None => false,
                });
            let case_first = case_first_opt.unwrap_or_else(|| {
                unicode_extension_keyword(&locale, "kf")
                    .filter(|v| ["upper", "lower", "false"].contains(&v.as_str()))
                    .unwrap_or_else(|| "false".to_string())
            });
            let collation = collation_opt.unwrap_or_else(|| {
                unicode_extension_keyword(&locale, "co")
                    .filter(|v| !v.is_empty() && v != "standard" && v != "search")
                    .unwrap_or_else(|| "default".to_string())
            });
            set_internal_field(obj, KEY_COL_USAGE, string_value(&usage));
            set_internal_field(obj, KEY_COL_SENSITIVITY, string_value(&sensitivity));
            set_internal_field(obj, KEY_COL_IGNORE_PUNCT, bool_value(ignore_punct));
            set_internal_field(obj, KEY_COL_COLLATION, string_value(&collation));
            set_internal_field(obj, KEY_COL_NUMERIC, bool_value(numeric));
            set_internal_field(obj, KEY_COL_CASE_FIRST, string_value(&case_first));
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
            // `? ToObject(options)` (null → TypeError), then GetOption in order:
            // localeMatcher, granularity (options-order.js / options-null.js).
            let options = coerce_options_reject_null(options);
            let _ = enum_option_strict(
                options,
                "localeMatcher",
                &["lookup", "best fit"],
                "best fit",
            );
            let granularity =
                normalize_granularity(get_option_string_coerced(options, "granularity"));
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
            // `? GetOptionsObject(options)` (any non-Object, non-undefined →
            // TypeError), then GetOption: localeMatcher, type, style
            // (options-getoptionsobject.js / options-order.js).
            let options = get_options_object(options);
            let _ = enum_option_strict(
                options,
                "localeMatcher",
                &["lookup", "best fit"],
                "best fit",
            );
            let list_type = enum_option_strict(
                options,
                "type",
                &["conjunction", "disjunction", "unit"],
                "conjunction",
            );
            let style = enum_option_strict(options, "style", &["long", "short", "narrow"], "long");
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
            // `? ToObject(options)` (null → TypeError), then GetOption in order:
            // localeMatcher, numberingSystem, style, numeric (options-order.js).
            let options = coerce_options_reject_null(options);
            let _ = enum_option_strict(
                options,
                "localeMatcher",
                &["lookup", "best fit"],
                "best fit",
            );
            if let Some(ns) = get_option_string_coerced(options, "numberingSystem") {
                if !is_well_formed_numbering_system(&ns) {
                    throw_range_error(&format!(
                        "Value {ns} out of range for Intl options property numberingSystem"
                    ));
                }
            }
            let style = enum_option_strict(options, "style", &["long", "short", "narrow"], "long");
            let numeric = enum_option_strict(options, "numeric", &["always", "auto"], "always");
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
            // `? GetOptionsObject(options)`, then GetOption in the exact order
            // constructor-option-read-order.js asserts: localeMatcher, type,
            // notation, compactDisplay, then SetNumberFormatDigitOptions
            // (minimumIntegerDigits, min/maxFractionDigits, min/maxSignificantDigits,
            // roundingIncrement, roundingMode, roundingPriority, trailingZeroDisplay).
            let options = get_options_object(options);
            let _ = enum_option_strict(
                options,
                "localeMatcher",
                &["lookup", "best fit"],
                "best fit",
            );
            let pr_type = enum_option_strict(options, "type", &["cardinal", "ordinal"], "cardinal");
            set_internal_field(obj, KEY_TYPE, string_value(&pr_type));
            let notation = enum_option_strict(
                options,
                "notation",
                &["standard", "scientific", "engineering", "compact"],
                "standard",
            );
            let compact_display =
                enum_option_strict(options, "compactDisplay", &["short", "long"], "short");
            set_internal_field(obj, KEY_PR_NOTATION, string_value(&notation));
            if notation == "compact" {
                set_internal_field(obj, KEY_PR_COMPACT_DISPLAY, string_value(&compact_display));
            }
            let min_int = get_option_number(options, "minimumIntegerDigits").unwrap_or(1.0);
            set_internal_field(obj, KEY_PR_MIN_INT, min_int);
            let min_frac_read = get_option_number(options, "minimumFractionDigits");
            let max_frac_read = get_option_number(options, "maximumFractionDigits");
            let min_sig = get_option_number(options, "minimumSignificantDigits");
            let max_sig = get_option_number(options, "maximumSignificantDigits");
            // Trailing SetNumberFormatDigitOptions reads — observed for read-order
            // parity even though Perry's plural selection ignores their values.
            let _ = get_option_value(options, "roundingIncrement");
            let _ = get_option_value(options, "roundingMode");
            let _ = get_option_value(options, "roundingPriority");
            let _ = get_option_value(options, "trailingZeroDisplay");
            if min_sig.is_some() || max_sig.is_some() {
                set_internal_field(obj, KEY_PR_USE_SIG, bool_value(true));
                set_internal_field(obj, KEY_PR_MIN_SIG, min_sig.unwrap_or(1.0));
                set_internal_field(obj, KEY_PR_MAX_SIG, max_sig.unwrap_or(21.0));
            } else {
                set_internal_field(obj, KEY_PR_USE_SIG, bool_value(false));
                // Reuse the values read above (in spec order) — re-reading would
                // double-invoke the option getters and break read-order parity.
                let min_frac = min_frac_read.unwrap_or(0.0);
                let max_frac = max_frac_read.unwrap_or_else(|| min_frac.max(3.0));
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
) -> *mut ClosureHeader {
    let closure = crate::closure::js_closure_alloc(func_ptr, 1);
    if closure.is_null() {
        return closure;
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
    closure
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

fn supported_locales_array(locales: f64, options: f64) -> f64 {
    // `supportedLocalesOf(locales, options)`:
    //   1. requestedLocales = ? CanonicalizeLocaleList(locales)   ← runs FIRST,
    //      so a malformed locale errors before `options` is touched.
    //   2. SupportedLocales(..., options): when `options` is not undefined,
    //      `? ToObject(options)` (null → TypeError) then
    //      `? GetOption(options, "localeMatcher", …)` — an invalid localeMatcher
    //      is a RangeError even though the matcher choice does not affect Perry's
    //      lookup result.
    let requested = locales_from_value(locales);
    if !JSValue::from_bits(options.to_bits()).is_undefined() {
        let options = coerce_options_reject_null(options);
        let _ = enum_option_strict(
            options,
            "localeMatcher",
            &["lookup", "best fit"],
            "best fit",
        );
    }
    // BestAvailableLocale-filter the canonicalized request list: drop tags whose
    // primary language Perry can't service (e.g. `zxx`), keeping order + dedup.
    let mut arr = js_array_alloc(0);
    for locale in requested {
        if is_available_locale(&locale) {
            arr = js_array_push_f64(arr, string_value(&locale));
        }
    }
    js_nanbox_pointer(arr as i64)
}

extern "C" fn supported_locales_of_thunk(_closure: *const ClosureHeader, rest: f64) -> f64 {
    supported_locales_array(rest_arg(rest, 0), rest_arg(rest, 1))
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
            (
                "formatToParts",
                number_format_to_parts_thunk as *const u8,
                1,
            ),
            // `formatRange`/`formatRangeToParts` are plain (non-bound) prototype
            // methods (Intl.NumberFormat-v3): a detached reference loses `this`
            // and the `this_intl_object` guard throws, so they are installed on
            // the prototype only — never as own bound instance functions.
            ("formatRange", number_format_range_thunk as *const u8, 2),
            (
                "formatRangeToParts",
                number_format_range_to_parts_thunk as *const u8,
                2,
            ),
            (
                "resolvedOptions",
                number_format_resolved_options_thunk as *const u8,
                0,
            ),
        ],
        // `format` is an accessor (getter) per ECMA-402, not a plain method.
        &[("format", number_format_format_getter_thunk as *const u8)],
    );
    install_constructor(
        ns_obj,
        "DateTimeFormat",
        date_time_format_constructor_thunk as *const u8,
        0,
        &[
            // `format` is an accessor (getter) per ECMA-402 §11.4.3 — see below.
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
        &[("format", date_time_format_format_getter_thunk as *const u8)],
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
        &[],
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
        &[],
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
        &[],
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
        &[],
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
        &[],
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
        &[],
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
        &[],
    );
}
