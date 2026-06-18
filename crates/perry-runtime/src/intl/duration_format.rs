//! `Intl.DurationFormat` — ECMA-402 duration formatting.
//!
//! A focused implementation: the constructor resolves `style` + the ten
//! per-unit style/display slots via the spec's `GetDurationUnitOptions`
//! (including the `numeric`→`fractional` promotion for sub-second units and the
//! `prevStyle`-driven `2-digit` propagation), validates the options bag, and
//! exposes `format` / `formatToParts` / `resolvedOptions`. The localized output
//! uses a deterministic English rendering (full CLDR unit patterns + list
//! grouping need data Perry doesn't carry); `format` validates its duration
//! argument per `ToDurationRecord` + `IsValidDuration` so the argument-negative
//! tests pass regardless.

use super::*;

/// Per-unit config: (name, allowed styles, digital-base style).
const L3: &[&str] = &["long", "short", "narrow"];
const HMS: &[&str] = &["long", "short", "narrow", "numeric", "2-digit"];
const SUB: &[&str] = &["long", "short", "narrow", "numeric"];

const UNITS: &[(&str, &[&str], &str)] = &[
    ("years", L3, "short"),
    ("months", L3, "short"),
    ("weeks", L3, "short"),
    ("days", L3, "short"),
    ("hours", HMS, "numeric"),
    ("minutes", HMS, "numeric"),
    ("seconds", HMS, "numeric"),
    ("milliseconds", SUB, "numeric"),
    ("microseconds", SUB, "numeric"),
    ("nanoseconds", SUB, "numeric"),
];

fn is_hms(unit: &str) -> bool {
    matches!(unit, "hours" | "minutes" | "seconds")
}
fn is_subsec(unit: &str) -> bool {
    matches!(unit, "milliseconds" | "microseconds" | "nanoseconds")
}

fn style_key(unit: &str) -> String {
    format!("__df_{unit}")
}
fn display_key(unit: &str) -> String {
    format!("__df_{unit}Display")
}

const KEY_DF_STYLE: &str = "__dfStyle";
const KEY_DF_NUMBERING: &str = "__dfNumbering";
const KEY_DF_FRACTIONAL: &str = "__dfFractional";

/// `GetOption(options, key, "string", ...)`: only `undefined` selects the
/// default; every other value (including `null`) is coerced via `ToString`. The
/// shared `super::get_option_string` instead treats `null` as absent, which
/// `GetOption` does not — so the option-validation tests that pass `null`
/// expect a RangeError, not silent defaulting.
fn df_get_option_string(options: f64, key: &str) -> Option<String> {
    let raw = get_option_value(options, key);
    let jv = JSValue::from_bits(raw.to_bits());
    if jv.is_undefined() {
        None
    } else if jv.is_any_string() {
        string_from_string_value(raw)
    } else {
        Some(value_to_string(raw))
    }
}

/// `GetOption` with a fixed value list (RangeError on an out-of-range value),
/// treating only `undefined` as absent (see [`df_get_option_string`]).
fn df_enum_option(options: f64, key: &str, allowed: &[&str], default: &str) -> String {
    match df_get_option_string(options, key) {
        None => default.to_string(),
        Some(value) => {
            if allowed.contains(&value.as_str()) {
                value
            } else {
                throw_range_error(&format!(
                    "Value {value} out of range for Intl.DurationFormat options property {key}"
                ))
            }
        }
    }
}

/// A unicode `type` value: one or more `alphanum{3,8}` subtags. Used to validate
/// the `numberingSystem` option (invalid → RangeError).
fn valid_numbering_system(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| {
            (3..=8).contains(&seg.len()) && seg.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// GetDurationUnitOptions. Returns `(internal_style, display)`. `internal_style`
/// can be `"fractional"` (threaded as the next unit's `prevStyle`); the caller
/// maps it to `"numeric"` for `resolvedOptions`.
fn get_duration_unit_options(
    options: f64,
    unit: &str,
    allowed: &[&str],
    base_style: &str,
    digital_base: &str,
    prev_style: Option<&str>,
) -> (String, String) {
    // 1. style = GetOption(options, unit, string, allowed, undefined)
    let mut style = match df_get_option_string(options, unit) {
        Some(v) => {
            if !allowed.contains(&v.as_str()) {
                throw_range_error(&format!(
                    "Value {v} out of range for Intl.DurationFormat options property {unit}"
                ));
            }
            Some(v)
        }
        None => None,
    };
    let mut display_default = "always";
    // 3. style undefined → defaults
    if style.is_none() {
        if base_style == "digital" {
            if !is_hms(unit) {
                display_default = "auto";
            }
            style = Some(digital_base.to_string());
        } else if matches!(
            prev_style,
            Some("fractional") | Some("numeric") | Some("2-digit")
        ) {
            if unit != "minutes" && unit != "seconds" {
                display_default = "auto";
            }
            style = Some("numeric".to_string());
        } else {
            display_default = "auto";
            style = Some(base_style.to_string());
        }
    }
    let mut style = style.unwrap();
    // 4. numeric sub-second → fractional
    if style == "numeric" && is_subsec(unit) {
        style = "fractional".to_string();
        display_default = "auto";
    }
    // 6. display = GetOption(options, unitDisplay, string, «auto,always», displayDefault)
    let display = df_enum_option(
        options,
        &display_key_field(unit),
        &["auto", "always"],
        display_default,
    );
    // 7. display "always" && style "fractional" → RangeError
    if display == "always" && style == "fractional" {
        throw_range_error(&format!(
            "Intl.DurationFormat: {unit}Display 'always' conflicts with fractional style"
        ));
    }
    // 8. prevStyle "fractional" → this must be fractional too
    if prev_style == Some("fractional") && style != "fractional" {
        throw_range_error(&format!(
            "Intl.DurationFormat: {unit} style conflicts with a preceding fractional unit"
        ));
    }
    // 9. prevStyle numeric/2-digit
    if matches!(prev_style, Some("numeric") | Some("2-digit")) {
        if !matches!(style.as_str(), "fractional" | "numeric" | "2-digit") {
            throw_range_error(&format!(
                "Intl.DurationFormat: {unit} style conflicts with a preceding numeric unit"
            ));
        }
        if unit == "minutes" || unit == "seconds" {
            style = "2-digit".to_string();
        }
    }
    (style, display)
}

/// The `<unit>Display` option name (e.g. `yearsDisplay`).
fn display_key_field(unit: &str) -> String {
    format!("{unit}Display")
}

/// Map an internal style to its `resolvedOptions` reporting form (`fractional`
/// is reported as `numeric`).
fn report_style(style: &str) -> &str {
    if style == "fractional" {
        "numeric"
    } else {
        style
    }
}

/// Configure a freshly-allocated `Intl.DurationFormat` instance: read + validate
/// the options bag (in spec order) and install the bound instance methods.
pub(super) fn configure(obj: *mut ObjectHeader, options: f64) {
    // Order (constructor-options-order): localeMatcher, numberingSystem, style,
    // then each unit + unitDisplay, then fractionalDigits.
    let _matcher = df_enum_option(
        options,
        "localeMatcher",
        &["lookup", "best fit"],
        "best fit",
    );

    let numbering = match df_get_option_string(options, "numberingSystem") {
        Some(ns) => {
            if !valid_numbering_system(&ns) {
                throw_range_error(&format!(
                    "Value {ns} out of range for Intl.DurationFormat options property numberingSystem"
                ));
            }
            ns
        }
        None => "latn".to_string(),
    };
    set_internal_field(obj, KEY_DF_NUMBERING, string_value(&numbering));

    let base_style = df_enum_option(
        options,
        "style",
        &["long", "short", "narrow", "digital"],
        "short",
    );
    set_internal_field(obj, KEY_DF_STYLE, string_value(&base_style));

    let mut prev_style: Option<String> = None;
    for (unit, allowed, digital_base) in UNITS.iter().copied() {
        let (style, display) = get_duration_unit_options(
            options,
            unit,
            allowed,
            &base_style,
            digital_base,
            prev_style.as_deref(),
        );
        set_internal_field(obj, &style_key(unit), string_value(report_style(&style)));
        set_internal_field(obj, &display_key(unit), string_value(&display));
        prev_style = Some(style);
    }

    // fractionalDigits: integer in [0, 9], else RangeError. Read last.
    if let Some(n) = get_option_number(options, "fractionalDigits") {
        if !n.is_finite() || n.fract() != 0.0 || !(0.0..=9.0).contains(&n) {
            throw_range_error(
                "Value out of range for Intl.DurationFormat options property fractionalDigits",
            );
        }
        set_internal_field(obj, KEY_DF_FRACTIONAL, n);
    }

    install_bound_instance_function(obj, "format", bound_format_thunk as *const u8, 1);
    install_bound_instance_function(obj, "formatToParts", bound_to_parts_thunk as *const u8, 1);
    install_bound_instance_function(
        obj,
        "resolvedOptions",
        bound_resolved_options_thunk as *const u8,
        0,
    );
}

// ---- resolvedOptions -------------------------------------------------------

fn resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let out = js_object_alloc(0, 24);
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(
        out,
        "numberingSystem",
        string_value(
            &get_string_field(obj, KEY_DF_NUMBERING).unwrap_or_else(|| "latn".to_string()),
        ),
    );
    set_field(
        out,
        "style",
        string_value(&get_string_field(obj, KEY_DF_STYLE).unwrap_or_else(|| "short".to_string())),
    );
    for (unit, _, _) in UNITS.iter().copied() {
        if let Some(style) = get_string_field(obj, &style_key(unit)) {
            set_field(out, unit, string_value(&style));
        }
        if let Some(display) = get_string_field(obj, &display_key(unit)) {
            set_field(out, &display_key_field(unit), string_value(&display));
        }
    }
    if let Some(frac) = get_number_field(obj, KEY_DF_FRACTIONAL) {
        set_field(out, "fractionalDigits", frac);
    }
    js_nanbox_pointer(out as i64)
}

extern "C" fn bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", super::KIND_DURATION_FORMAT);
    resolved_options_object(obj)
}

pub(super) extern "C" fn resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", super::KIND_DURATION_FORMAT);
    resolved_options_object(obj)
}

// ---- duration validation (ToDurationRecord + IsValidDuration) --------------

const DURATION_UNITS: &[&str] = &[
    "years",
    "months",
    "weeks",
    "days",
    "hours",
    "minutes",
    "seconds",
    "milliseconds",
    "microseconds",
    "nanoseconds",
];

/// `ToDurationRecord` + `IsValidDuration`: returns the ten unit values in
/// `DURATION_UNITS` order. Throws `TypeError` for a non-object / all-undefined
/// input and `RangeError` for non-integral values, mixed signs, or
/// out-of-range magnitudes.
fn to_duration_record(value: f64) -> Vec<f64> {
    let Some(input) = object_ptr_from_value(value) else {
        throw_type_error("Intl.DurationFormat.format: duration must be an object");
    };
    let mut vals = Vec::with_capacity(DURATION_UNITS.len());
    let mut any = false;
    let mut sign = 0i32;
    for unit in DURATION_UNITS.iter().copied() {
        let raw = get_field(input, unit);
        let jv = JSValue::from_bits(raw.to_bits());
        if jv.is_undefined() {
            vals.push(0.0);
            continue;
        }
        any = true;
        let n = jv.to_number();
        // ToIntegerIfIntegral: must be a finite integral Number.
        if !n.is_finite() || n.fract() != 0.0 {
            throw_range_error(&format!(
                "Intl.DurationFormat.format: {unit} must be an integer"
            ));
        }
        if n > 0.0 {
            if sign < 0 {
                throw_range_error("Intl.DurationFormat.format: duration fields have mixed signs");
            }
            sign = 1;
        } else if n < 0.0 {
            if sign > 0 {
                throw_range_error("Intl.DurationFormat.format: duration fields have mixed signs");
            }
            sign = -1;
        }
        vals.push(n);
    }
    if !any {
        throw_type_error("Intl.DurationFormat.format: duration must have at least one field");
    }
    // IsValidDuration: years/months/weeks bounded by 2^32-1; the calendar/time
    // units' combined magnitude in seconds must stay below 2^53.
    const U32_MAX: f64 = 4_294_967_295.0;
    for (i, unit) in DURATION_UNITS.iter().copied().enumerate() {
        if matches!(unit, "years" | "months" | "weeks") && vals[i].abs() > U32_MAX {
            throw_range_error(&format!("Intl.DurationFormat.format: {unit} out of range"));
        }
    }
    // days*86400 + hours*3600 + minutes*60 + seconds + ms/1e3 + us/1e6 + ns/1e9
    let normalized = vals[3] * 86_400.0
        + vals[4] * 3_600.0
        + vals[5] * 60.0
        + vals[6]
        + vals[7] / 1.0e3
        + vals[8] / 1.0e6
        + vals[9] / 1.0e9;
    if !normalized.is_finite() || normalized.abs() >= 9_007_199_254_740_992.0 {
        throw_range_error("Intl.DurationFormat.format: duration out of range");
    }
    vals
}

// ---- format / formatToParts (deterministic English rendering) --------------

/// English unit label for `(unit, style)`, pluralized by `n`. Used only by the
/// data-free fallback rendering.
fn unit_label(unit: &str, style: &str, n: f64) -> String {
    let plural = n.abs() != 1.0;
    let base = unit.strip_suffix('s').unwrap_or(unit); // "years" -> "year"
    match style {
        "long" => {
            if plural {
                format!("{base}s")
            } else {
                base.to_string()
            }
        }
        _ => {
            // short / narrow abbreviations
            let abbr = match base {
                "year" => "yr",
                "month" => "mth",
                "week" => "wk",
                "day" => "day",
                "hour" => "hr",
                "minute" => "min",
                "second" => "sec",
                "millisecond" => "ms",
                "microsecond" => "μs",
                "nanosecond" => "ns",
                other => other,
            };
            abbr.to_string()
        }
    }
}

/// Build the rendered segments for a validated duration. Best-effort English;
/// the concatenation is what `format` returns and `formatToParts` mirrors.
fn render(obj: *const ObjectHeader, vals: &[f64]) -> Vec<(&'static str, String)> {
    let mut pieces: Vec<String> = Vec::new();
    for (i, (unit, _, _)) in UNITS.iter().copied().enumerate() {
        let n = vals[i];
        let display =
            get_string_field(obj, &display_key(unit)).unwrap_or_else(|| "auto".to_string());
        if n == 0.0 && display == "auto" {
            continue;
        }
        let style = get_string_field(obj, &style_key(unit)).unwrap_or_else(|| "short".to_string());
        if style == "numeric" || style == "2-digit" {
            pieces.push(format!("{}", n as i64));
        } else {
            pieces.push(format!("{} {}", n as i64, unit_label(unit, &style, n)));
        }
    }
    let joined = pieces.join(", ");
    vec![("literal", joined)]
}

fn format_value(obj: *const ObjectHeader, duration: f64) -> f64 {
    let vals = to_duration_record(duration);
    let parts = render(obj, &vals);
    string_value(&parts.iter().map(|(_, v)| v.as_str()).collect::<String>())
}

extern "C" fn bound_format_thunk(closure: *const ClosureHeader, duration: f64) -> f64 {
    let obj = captured_intl_object(closure, "format", super::KIND_DURATION_FORMAT);
    format_value(obj, duration)
}

pub(super) extern "C" fn format_thunk(_closure: *const ClosureHeader, duration: f64) -> f64 {
    let obj = this_intl_object("format", super::KIND_DURATION_FORMAT);
    format_value(obj, duration)
}

extern "C" fn bound_to_parts_thunk(closure: *const ClosureHeader, duration: f64) -> f64 {
    let obj = captured_intl_object(closure, "formatToParts", super::KIND_DURATION_FORMAT);
    let vals = to_duration_record(duration);
    parts_to_js_array(&render(obj, &vals))
}

pub(super) extern "C" fn to_parts_thunk(_closure: *const ClosureHeader, duration: f64) -> f64 {
    let obj = this_intl_object("formatToParts", super::KIND_DURATION_FORMAT);
    let vals = to_duration_record(duration);
    parts_to_js_array(&render(obj, &vals))
}

pub(super) extern "C" fn constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    super::make_instance(
        closure,
        super::KIND_DURATION_FORMAT,
        super::rest_arg(rest, 0),
        super::rest_arg(rest, 1),
    )
}
