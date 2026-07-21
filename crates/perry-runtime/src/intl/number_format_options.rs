use super::*;

use crate::object::ObjectHeader;
use crate::value::JSValue;

/// Read, validate, and store the NumberFormat option slots (ECMA-402
/// CreateNumberFormat / SetNumberFormatUnitOptions / SetNumberFormatDigitOptions).
pub(crate) fn configure_number_format(obj: *mut ObjectHeader, locale: &str, options: f64) {
    // CoerceOptionsToObject: `null` throws; `undefined` behaves as an empty
    // null-prototype object (our readers already treat non-objects as empty).
    if JSValue::from_bits(options.to_bits()).is_null() {
        throw_type_error("Cannot convert undefined or null to object");
    }

    // localeMatcher is the first option read (ResolveLocale step) and is
    // validated, but the resolved value doesn't affect our deterministic locale
    // lookup. Reading it here keeps the GetOption sequence that
    // constructor-option-read-order.js asserts (localeMatcher before
    // numberingSystem) and propagates a throwing localeMatcher getter.
    let _ = get_string_option_enum(
        options,
        "localeMatcher",
        &["lookup", "best fit"],
        "best fit",
    );

    // numberingSystem: validate the option (well-formed `type` nonterminal),
    // then run ResolveLocale for the `nu` key — reconciling the option with the
    // requested locale's `-u-nu-` keyword and updating the resolved locale so
    // `resolvedOptions().locale` reflects only the supported value actually used.
    let opt_ns = match get_option_string(options, "numberingSystem") {
        Some(value) => {
            let lower = value.to_ascii_lowercase();
            if !is_well_formed_numbering_system(&lower) {
                throw_range_error(&format!(
                    "Value {value} out of range for Intl.NumberFormat options property numberingSystem"
                ));
            }
            Some(lower)
        }
        None => None,
    };
    let (resolved_locale, numbering) = resolve_numbering_system(locale, opt_ns.as_deref());
    set_internal_field(obj, KEY_LOCALE, string_value(&resolved_locale));
    set_internal_field(obj, KEY_NF_NUMBERING, string_value(&numbering));

    // SetNumberFormatUnitOptions.
    let style = get_string_option_enum(
        options,
        "style",
        &["decimal", "percent", "currency", "unit"],
        "decimal",
    );
    set_internal_field(obj, KEY_STYLE, string_value(&style));

    let currency = get_option_string(options, "currency");
    if let Some(code) = &currency {
        if !is_well_formed_currency_code(code) {
            throw_range_error(&format!("Invalid currency code : {code}"));
        }
        set_internal_field(obj, KEY_CURRENCY, string_value(&code.to_ascii_uppercase()));
    }
    // Throw TypeError for missing currency BEFORE reading currencyDisplay /
    // currencySign — so proxy-get traps on those keys are never triggered when
    // currency is missing, matching the spec-observable option-read order.
    if style == "currency" && currency.is_none() {
        throw_type_error("Currency code is required with currency style.");
    }
    let currency_display = get_string_option_enum(
        options,
        "currencyDisplay",
        &["code", "symbol", "narrowSymbol", "name"],
        "symbol",
    );
    let currency_sign = get_string_option_enum(
        options,
        "currencySign",
        &["standard", "accounting"],
        "standard",
    );
    set_internal_field(
        obj,
        KEY_NF_CURRENCY_DISPLAY,
        string_value(&currency_display),
    );
    set_internal_field(obj, KEY_NF_CURRENCY_SIGN, string_value(&currency_sign));

    let unit = get_option_string(options, "unit");
    if let Some(u) = &unit {
        if !is_well_formed_unit_identifier(u) {
            throw_range_error(&format!(
                "Value {u} out of range for Intl.NumberFormat options property unit"
            ));
        }
        set_internal_field(obj, KEY_NF_UNIT, string_value(u));
    }
    let unit_display = get_string_option_enum(
        options,
        "unitDisplay",
        &["short", "narrow", "long"],
        "short",
    );
    set_internal_field(obj, KEY_NF_UNIT_DISPLAY, string_value(&unit_display));

    if style == "unit" && unit.is_none() {
        throw_type_error("unit is required with unit style.");
    }

    // notation (read before the digit options per the spec order).
    let notation = get_string_option_enum(
        options,
        "notation",
        &["standard", "scientific", "engineering", "compact"],
        "standard",
    );
    set_internal_field(obj, KEY_NF_NOTATION, string_value(&notation));

    // SetNumberFormatDigitOptions — the GetOption reads run in the exact
    // ECMA-402 order asserted by constructor-option-read-order.js:
    // minimumIntegerDigits, minimumFractionDigits, maximumFractionDigits,
    // minimumSignificantDigits, maximumSignificantDigits, roundingIncrement,
    // roundingMode, roundingPriority, trailingZeroDisplay.
    let min_int =
        get_int_option_in_range(options, "minimumIntegerDigits", 1.0, 21.0).unwrap_or(1.0);
    let min_frac_opt = get_int_option_in_range(options, "minimumFractionDigits", 0.0, 100.0);
    let max_frac_opt = get_int_option_in_range(options, "maximumFractionDigits", 0.0, 100.0);
    let min_sig_opt = get_int_option_in_range(options, "minimumSignificantDigits", 1.0, 21.0);
    let max_sig_opt = get_int_option_in_range(options, "maximumSignificantDigits", 1.0, 21.0);

    // roundingIncrement is read before roundingMode/roundingPriority and is
    // ToNumber-coerced (so `{ valueOf }` objects work) then checked against the
    // sanctioned increment set — a [1, 5000] range alone would wrongly admit
    // values like 3 or 5000.1.
    let rounding_increment = read_rounding_increment(options);

    let rounding_mode = enum_option_strict(
        options,
        "roundingMode",
        &[
            "ceil",
            "floor",
            "expand",
            "trunc",
            "halfCeil",
            "halfFloor",
            "halfExpand",
            "halfTrunc",
            "halfEven",
        ],
        "halfExpand",
    );
    let mut rounding_priority = get_string_option_enum(
        options,
        "roundingPriority",
        &["auto", "morePrecision", "lessPrecision"],
        "auto",
    );
    let trailing_zero = get_string_option_enum(
        options,
        "trailingZeroDisplay",
        &["auto", "stripIfInteger"],
        "auto",
    );

    set_internal_field(obj, KEY_NF_MIN_INT, min_int);

    // The currency-specific digit defaults only apply to "standard" notation
    // (ECMA-402 SetNumberFormatDigitOptions step 19-20) — compact/engineering/
    // scientific currency values fall back to the generic 0/3 (or 0/0 for
    // percent) defaults, same as any other style.
    let (default_min_frac, default_max_frac) = if style == "currency" && notation == "standard" {
        // `currency` is the raw (possibly lowercase) option value — the
        // internal field is uppercased separately above — but
        // `currency_fraction_digits` only matches uppercase ISO codes.
        let d = currency
            .as_deref()
            .map_or(2, |c| currency_fraction_digits(&c.to_ascii_uppercase()));
        (d, d)
    } else if style == "percent" {
        (0, 0)
    } else {
        (0, 3)
    };

    let has_sd = min_sig_opt.is_some() || max_sig_opt.is_some();
    let has_fd = min_frac_opt.is_some() || max_frac_opt.is_some();

    // Same FractionDigitDefaults-style resolution as fraction digits below: an
    // explicit maximumSignificantDigits below an explicit minimum is a
    // RangeError, not a silent widen.
    let (min_sig, max_sig) = if has_sd {
        match (min_sig_opt, max_sig_opt) {
            (Some(mn), Some(mx)) => {
                let (mn, mx) = (mn as u32, mx as u32);
                if mn > mx {
                    throw_range_error(&format!(
                        "Value {mx} is out of range for Intl.NumberFormat options property maximumSignificantDigits"
                    ));
                }
                (mn, mx)
            }
            (Some(mn), None) => {
                let mn = mn as u32;
                (mn, mn.max(21))
            }
            (None, Some(mx)) => (1, mx as u32),
            (None, None) => unreachable!("has_sd implies at least one is Some"),
        }
    } else {
        (1, 21)
    };
    // Resolve minimumFractionDigits/maximumFractionDigits per FractionDigitDefaults:
    // an explicit value on one side is never widened by the *other* side's
    // default — only a missing side falls back, clamped against the side that
    // was actually given (e.g. `{currency: "USD", maximumFractionDigits: 1}`
    // must resolve to `1`, not be pulled back up to USD's default of `2`).
    let (min_frac, max_frac) = if has_fd {
        match (min_frac_opt, max_frac_opt) {
            (Some(mn), Some(mx)) => {
                let (mn, mx) = (mn as u32, mx as u32);
                if mn > mx {
                    throw_range_error(&format!(
                        "Value {mx} is out of range for Intl.NumberFormat options property maximumFractionDigits"
                    ));
                }
                (mn, mx)
            }
            (Some(mn), None) => {
                let mn = mn as u32;
                (mn, mn.max(default_max_frac))
            }
            (None, Some(mx)) => {
                let mx = mx as u32;
                (default_min_frac.min(mx), mx)
            }
            (None, None) => unreachable!("has_fd implies at least one is Some"),
        }
    } else {
        (default_min_frac, default_max_frac)
    };

    // A roundingIncrement other than 1 constrains the rounding type to fraction
    // digits with a fixed fraction width (ECMA-402 SetNumberFormatDigitOptions):
    // significant digits or a non-auto roundingPriority is a TypeError, and the
    // resolved maximum/minimum fraction digits must be equal.
    if rounding_increment != 1.0 {
        if has_sd || rounding_priority != "auto" {
            throw_type_error(
                "roundingIncrement is only valid with the default fraction-digit rounding type",
            );
        }
        if max_frac != min_frac {
            throw_range_error(
                "With roundingIncrement, maximumFractionDigits must equal minimumFractionDigits",
            );
        }
    }

    set_internal_field(obj, KEY_NF_MIN_SIG, min_sig as f64);
    set_internal_field(obj, KEY_NF_MAX_SIG, max_sig as f64);
    set_internal_field(obj, KEY_NF_MIN_FRAC, min_frac as f64);
    set_internal_field(obj, KEY_MAX_FRACTION_DIGITS, max_frac as f64);

    // Digit display mode: "fraction" | "significant" | "both" (compact default).
    let digit_mode = if has_sd && !has_fd {
        "significant"
    } else if !has_sd && !has_fd && notation == "compact" {
        // Compact with no explicit digit options rounds by 1–2 significant
        // digits with morePrecision priority, surfacing both slots.
        rounding_priority = "morePrecision".to_string();
        "both"
    } else if has_sd && has_fd {
        if rounding_priority == "lessPrecision" {
            "fraction"
        } else {
            "significant"
        }
    } else {
        "fraction"
    };
    // Compact's significant defaults are 1–2 when not explicitly given.
    if digit_mode == "both" {
        set_internal_field(obj, KEY_NF_MIN_SIG, 1.0);
        set_internal_field(obj, KEY_NF_MAX_SIG, 2.0);
        set_internal_field(obj, KEY_NF_MIN_FRAC, 0.0);
        set_internal_field(obj, KEY_MAX_FRACTION_DIGITS, 0.0);
    }
    set_internal_field(obj, KEY_NF_USE_SIG, string_value(digit_mode));

    set_internal_field(obj, KEY_NF_ROUNDING_INCREMENT, rounding_increment);
    set_internal_field(obj, KEY_NF_ROUNDING_MODE, string_value(&rounding_mode));
    set_internal_field(
        obj,
        KEY_NF_ROUNDING_PRIORITY,
        string_value(&rounding_priority),
    );
    set_internal_field(obj, KEY_NF_TRAILING_ZERO, string_value(&trailing_zero));

    // compactDisplay, useGrouping, signDisplay.
    let compact_display =
        get_string_option_enum(options, "compactDisplay", &["short", "long"], "short");
    set_internal_field(obj, KEY_NF_COMPACT_DISPLAY, string_value(&compact_display));

    let default_grouping = if notation == "compact" {
        "min2"
    } else {
        "auto"
    };
    let use_grouping = get_use_grouping_option(options, default_grouping);
    set_internal_field(obj, KEY_NF_USE_GROUPING, string_value(&use_grouping));

    let sign_display = get_string_option_enum(
        options,
        "signDisplay",
        &["auto", "never", "always", "exceptZero", "negative"],
        "auto",
    );
    set_internal_field(obj, KEY_NF_SIGN_DISPLAY, string_value(&sign_display));
}

/// GetNumberOption(options, "roundingIncrement", 1, 5000, 1) followed by the
/// sanctioned-increment membership check (ECMA-402 SetNumberFormatDigitOptions).
/// The value is ToNumber-coerced (so `{ valueOf }` and string options work) but
/// NOT floored: `5000.1` is in range yet absent from the set, so it must throw.
fn read_rounding_increment(options: f64) -> f64 {
    const VALID: &[f64] = &[
        1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0,
        5000.0,
    ];
    let value = get_option_value(options, "roundingIncrement");
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        return 1.0;
    }
    let n = crate::builtins::js_number_coerce(value);
    if n.is_nan() || n < 1.0 || n > 5000.0 || !VALID.contains(&n) {
        throw_range_error(&format!(
            "Value {n} out of range for Intl.NumberFormat options property roundingIncrement"
        ));
    }
    n
}

/// A currency code is well-formed when it is exactly three ASCII letters
/// (ISO 4217 alphabetic). Validity (vs. an actual currency) is not checked.
pub(crate) fn is_well_formed_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphabetic())
}

/// ECMA-402 Table 2 — sanctioned single unit identifiers (includes hyphenated
/// atoms like "fluid-ounce" and "mile-scandinavian").
const SANCTIONED_UNITS: &[&str] = &[
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

fn is_sanctioned_single_unit(unit: &str) -> bool {
    SANCTIONED_UNITS.contains(&unit)
}

/// ECMA-402 IsWellFormedUnitIdentifier: a simple sanctioned unit, or a
/// compound `<sanctioned>-per-<sanctioned>` with exactly one `-per-` separator.
pub(crate) fn is_well_formed_unit_identifier(unit: &str) -> bool {
    if is_sanctioned_single_unit(unit) {
        return true;
    }
    match unit.split_once("-per-") {
        Some((numerator, denominator)) => {
            !denominator.contains("-per-")
                && is_sanctioned_single_unit(numerator)
                && is_sanctioned_single_unit(denominator)
        }
        None => false,
    }
}
