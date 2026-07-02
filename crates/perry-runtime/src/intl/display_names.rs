//! `Intl.DisplayNames` — ECMA-402 localized display names.
//!
//! A focused implementation covering the constructor (required-options +
//! `type`/`style`/`fallback`/`languageDisplay` validation in spec order),
//! `resolvedOptions`, and `of`. `of` performs the full
//! `CanonicalCodeForDisplayNames` structural validation per `type` (region /
//! script / language / currency / calendar / dateTimeField grammars, throwing
//! `RangeError` on a malformed code) and returns the canonicalized code as its
//! display string. The localized name tables themselves need CLDR data Perry
//! doesn't carry, so — matching the `fallback: "code"` default — `of` returns
//! the canonical code; with `fallback: "none"` it returns `undefined`.

use super::*;

const KEY_DN_FALLBACK: &str = "__dnFallback";
const KEY_DN_LANG_DISPLAY: &str = "__dnLanguageDisplay";

/// The twelve `dateTimeField` codes accepted by `of` (case-sensitive).
const DATE_TIME_FIELDS: &[&str] = &[
    "era",
    "year",
    "quarter",
    "month",
    "weekOfYear",
    "weekday",
    "day",
    "dayPeriod",
    "hour",
    "minute",
    "second",
    "timeZoneName",
];

/// `GetOption(options, key, "string", ...)`: only `undefined` selects the
/// default; every other value (including `null`) is coerced via `ToString`.
/// `super::get_option_string` instead treats `null` as absent, which `GetOption`
/// does not — the option-validation tests that pass `null` expect a RangeError,
/// not silent defaulting.
fn dn_get_option_string(options: f64, key: &str) -> Option<String> {
    let raw = get_option_value(options, key);
    let jv = JSValue::from_bits(raw.to_bits());
    if jv.is_undefined() {
        None
    } else if jv.is_any_string() {
        string_from_string_value(raw)
    } else if unsafe { crate::symbol::js_is_symbol(raw) } != 0 {
        // ToString(symbol) throws a TypeError — it must not fall through to the
        // value-list check (which would surface a RangeError instead).
        throw_type_error(&format!(
            "Cannot convert a Symbol value to a string for Intl.DisplayNames options property {key}"
        ));
    } else {
        Some(value_to_string(raw))
    }
}

/// `GetOption` with a fixed value list (RangeError on an out-of-range value),
/// treating only `undefined` as absent (see [`dn_get_option_string`]).
fn dn_enum_option(options: f64, key: &str, allowed: &[&str], default: &str) -> String {
    match dn_get_option_string(options, key) {
        None => default.to_string(),
        Some(value) => {
            if allowed.contains(&value.as_str()) {
                value
            } else {
                throw_range_error(&format!(
                    "Value {value} out of range for Intl.DisplayNames options property {key}"
                ))
            }
        }
    }
}

/// `GetOptionsObject(options)`: `undefined` yields an empty bag (reported here as
/// `undefined`, which the option readers treat as "every key absent"); an Object
/// passes through; any other value (including `null`) throws `TypeError`.
fn get_options_object(options: f64) -> f64 {
    let jv = JSValue::from_bits(options.to_bits());
    if jv.is_undefined() {
        return options;
    }
    if object_ptr_from_value(options).is_some() {
        return options;
    }
    throw_type_error("Intl.DisplayNames: options must be an object");
}

/// Configure a freshly-allocated `Intl.DisplayNames` instance: validate the
/// options bag (in spec order) and install the bound instance methods.
pub(super) fn configure(obj: *mut ObjectHeader, options_arg: f64) {
    let options = get_options_object(options_arg);

    // localeMatcher, then style, then type (required), then fallback, then
    // languageDisplay — the order the resolvedOptions / option-* tests rely on.
    let _matcher = dn_enum_option(
        options,
        "localeMatcher",
        &["lookup", "best fit"],
        "best fit",
    );
    let style = dn_enum_option(options, "style", &["narrow", "short", "long"], "long");
    let type_ = match dn_get_option_string(options, "type") {
        Some(v) => {
            if ![
                "language",
                "region",
                "script",
                "currency",
                "calendar",
                "dateTimeField",
            ]
            .contains(&v.as_str())
            {
                throw_range_error(&format!(
                    "Value {v} out of range for Intl.DisplayNames options property type"
                ));
            }
            v
        }
        None => throw_type_error("Intl.DisplayNames: options.type is required"),
    };
    let fallback = dn_enum_option(options, "fallback", &["code", "none"], "code");
    // languageDisplay is read + validated unconditionally, but only applies to —
    // and is reported by resolvedOptions for — `type: "language"`.
    let language_display = dn_enum_option(
        options,
        "languageDisplay",
        &["dialect", "standard"],
        "dialect",
    );

    set_internal_field(obj, KEY_STYLE, string_value(&style));
    set_internal_field(obj, KEY_TYPE, string_value(&type_));
    set_internal_field(obj, KEY_DN_FALLBACK, string_value(&fallback));
    if type_ == "language" {
        set_internal_field(obj, KEY_DN_LANG_DISPLAY, string_value(&language_display));
    }

    install_bound_instance_function(obj, "of", bound_of_thunk as *const u8, 1);
    install_bound_instance_function(
        obj,
        "resolvedOptions",
        bound_resolved_options_thunk as *const u8,
        0,
    );
}

// ---- resolvedOptions -------------------------------------------------------

fn resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 6);
    // Order: locale, style, type, fallback, [languageDisplay].
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(
        out,
        "style",
        string_value(&get_string_field(obj, KEY_STYLE).unwrap_or_else(|| "long".to_string())),
    );
    set_field(
        out,
        "type",
        string_value(&get_string_field(obj, KEY_TYPE).unwrap_or_else(|| "language".to_string())),
    );
    set_field(
        out,
        "fallback",
        string_value(&get_string_field(obj, KEY_DN_FALLBACK).unwrap_or_else(|| "code".to_string())),
    );
    if let Some(language_display) = get_string_field(obj, KEY_DN_LANG_DISPLAY) {
        set_field(out, "languageDisplay", string_value(&language_display));
    }
    js_nanbox_pointer(out as i64)
}

extern "C" fn bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", super::KIND_DISPLAY_NAMES);
    resolved_options_object(obj)
}

pub(super) extern "C" fn resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", super::KIND_DISPLAY_NAMES);
    resolved_options_object(obj)
}

// ---- of + CanonicalCodeForDisplayNames -------------------------------------

fn is_alpha(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}
fn is_digit(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}
fn is_alphanum(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// `unicode_region_subtag`: `alpha{2}` or `digit{3}`.
fn valid_region(s: &str) -> bool {
    (s.len() == 2 && is_alpha(s)) || (s.len() == 3 && is_digit(s))
}

/// `unicode_language_id` (no extensions/`root`): a language subtag
/// (`alpha{2,3}` | `alpha{5,8}`) plus an optional script (`alpha{4}`), optional
/// region, and any number of unique variant subtags (`alphanum{5,8}` | a
/// 4-char `digit alphanum{3}`).
fn valid_language_id(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    let mut idx = 0usize;
    let lang = parts[idx];
    if !(is_alpha(lang) && matches!(lang.len(), 2 | 3 | 5 | 6 | 7 | 8)) {
        return false;
    }
    idx += 1;
    // optional script: alpha{4}
    if idx < parts.len() && parts[idx].len() == 4 && is_alpha(parts[idx]) {
        idx += 1;
    }
    // optional region
    if idx < parts.len() && valid_region(parts[idx]) {
        idx += 1;
    }
    // variants (must be unique)
    let mut seen: Vec<String> = Vec::new();
    while idx < parts.len() {
        let v = parts[idx];
        let is_variant = (matches!(v.len(), 5..=8) && is_alphanum(v))
            || (v.len() == 4 && v.as_bytes()[0].is_ascii_digit() && is_alphanum(v));
        if !is_variant {
            return false;
        }
        let lower = v.to_ascii_lowercase();
        if seen.contains(&lower) {
            return false;
        }
        seen.push(lower);
        idx += 1;
    }
    true
}

/// A `<type id>` value (`numberingSystem`/`calendar` form): one or more
/// `alphanum{3,8}` subtags joined by `-`.
fn valid_type_id(s: &str) -> bool {
    !s.is_empty()
        && s.split('-')
            .all(|seg| (3..=8).contains(&seg.len()) && is_alphanum(seg))
}

/// Title-case a 4-letter script subtag (`abcd` → `Abcd`).
fn titlecase_script(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, ch) in s.chars().enumerate() {
        if i == 0 {
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// `CanonicalCodeForDisplayNames(type, code)`: structural validation +
/// canonicalization, throwing `RangeError` on a malformed code.
fn canonical_code(type_: &str, code: &str) -> String {
    match type_ {
        "region" => {
            if !valid_region(code) {
                throw_range_error(&format!("Invalid region code: {code}"));
            }
            code.to_ascii_uppercase()
        }
        "script" => {
            if code.len() != 4 || !is_alpha(code) {
                throw_range_error(&format!("Invalid script code: {code}"));
            }
            titlecase_script(code)
        }
        "language" => {
            if !valid_language_id(code) {
                throw_range_error(&format!("Invalid language code: {code}"));
            }
            canonicalize_language_tag(code).unwrap_or_else(|| code.to_string())
        }
        "currency" => {
            if code.len() != 3 || !is_alpha(code) {
                throw_range_error(&format!("Invalid currency code: {code}"));
            }
            code.to_ascii_uppercase()
        }
        "calendar" => {
            if !valid_type_id(code) {
                throw_range_error(&format!("Invalid calendar code: {code}"));
            }
            code.to_ascii_lowercase()
        }
        "dateTimeField" => {
            if !DATE_TIME_FIELDS.contains(&code) {
                throw_range_error(&format!("Invalid dateTimeField code: {code}"));
            }
            code.to_string()
        }
        _ => code.to_string(),
    }
}

fn of_value(obj: *const ObjectHeader, code_value: f64) -> f64 {
    let type_ = get_string_field(obj, KEY_TYPE).unwrap_or_else(|| "language".to_string());
    let code = value_to_string(code_value);
    let canonical = canonical_code(&type_, &code);
    // No CLDR name table is carried, so honour `fallback`: `"code"` returns the
    // canonical code, `"none"` returns undefined.
    let fallback = get_string_field(obj, KEY_DN_FALLBACK).unwrap_or_else(|| "code".to_string());
    if fallback == "none" {
        undefined()
    } else {
        string_value(&canonical)
    }
}

extern "C" fn bound_of_thunk(closure: *const ClosureHeader, code: f64) -> f64 {
    let obj = captured_intl_object(closure, "of", super::KIND_DISPLAY_NAMES);
    of_value(obj, code)
}

pub(super) extern "C" fn of_thunk(_closure: *const ClosureHeader, code: f64) -> f64 {
    let obj = this_intl_object("of", super::KIND_DISPLAY_NAMES);
    of_value(obj, code)
}

pub(super) extern "C" fn constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    super::make_instance(
        closure,
        super::KIND_DISPLAY_NAMES,
        super::rest_arg(rest, 0),
        super::rest_arg(rest, 1),
    )
}
