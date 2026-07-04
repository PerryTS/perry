//! `Intl.DurationFormat` — ECMA-402 duration formatting.
//!
//! The constructor resolves `style` + the ten per-unit style/display slots via the
//! spec's `GetDurationUnitOptions` (including the `numeric`→`fractional` promotion
//! for sub-second units and the `prevStyle`-driven `2-digit` propagation), validates
//! the options bag, and exposes `format` / `formatToParts` / `resolvedOptions`.
//!
//! `format`/`formatToParts` are a faithful port of `PartitionDurationFormatPattern`:
//! each unit value is rendered through the *same* `Intl.NumberFormat` code path that
//! a nested `new Intl.NumberFormat(...)` would use (see [`super::number_parts_from_resolved`])
//! and the per-unit strings are joined with [`super::list_format_parts`]. Routing
//! through the shared formatters keeps the output byte-identical to the value test262
//! computes from `Intl.NumberFormat`/`Intl.ListFormat`, regardless of how much CLDR
//! data those carry. The argument is validated per `ToDurationRecord` + `IsValidDuration`
//! (objects, and ISO-8601 duration strings).

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

/// Singular `Intl.NumberFormat` unit identifier for each duration unit, in `UNITS`
/// order (`"years"` → `"year"`). Used as the `unit` option when rendering through
/// the shared NumberFormat path.
const UNIT_SINGULAR: &[&str] = &[
    "year",
    "month",
    "week",
    "day",
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
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
/// the options bag (in spec order).
pub(super) fn configure(obj: *mut ObjectHeader, options: f64) {
    // GetOptionsObject: `undefined` → empty options; any other non-object
    // (notably `null` and primitives) is a TypeError. Object-like values —
    // including arrays, functions, and Proxies (all pointer-tagged) — are
    // accepted, so a property-bag Proxy still has its traps observed. Symbols
    // are also pointer-tagged, so `is_pointer()` alone would wrongly admit them;
    // exclude registered symbols explicitly.
    let opts_jv = JSValue::from_bits(options.to_bits());
    let is_symbol = opts_jv.is_pointer()
        && crate::symbol::is_registered_symbol(
            (options.to_bits() & crate::value::POINTER_MASK) as usize,
        );
    if !opts_jv.is_undefined() && (!opts_jv.is_pointer() || is_symbol) {
        throw_type_error("Intl.DurationFormat: options must be an object");
    }

    // Order (constructor-options-order): localeMatcher, numberingSystem, style,
    // then each unit + unitDisplay, then fractionalDigits.
    let _matcher = df_enum_option(
        options,
        "localeMatcher",
        &["lookup", "best fit"],
        "best fit",
    );

    let opt_ns = match df_get_option_string(options, "numberingSystem") {
        Some(ns) => {
            if !valid_numbering_system(&ns) {
                throw_range_error(&format!(
                    "Value {ns} out of range for Intl.DurationFormat options property numberingSystem"
                ));
            }
            Some(ns.to_ascii_lowercase())
        }
        None => None,
    };
    // ResolveLocale for `nu`: reconcile the option with the requested locale's
    // `-u-nu-` keyword (stored in KEY_LOCALE at construction) and update both the
    // resolved locale and numbering system.
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    let (resolved_locale, numbering) = super::resolve_numbering_system(&locale, opt_ns.as_deref());
    set_internal_field(obj, KEY_LOCALE, string_value(&resolved_locale));
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

    // Unlike `Intl.NumberFormat.prototype.format` (a bound getter), the
    // `Intl.DurationFormat` methods are plain prototype methods that read their
    // receiver from `this` — so a detached `const f = df.format; f(d)` must throw.
    // We install them as own instance properties (Perry's method dispatch resolves
    // own properties) but back them with the implicit-`this` thunks, so a
    // detached call lands on an undefined receiver and `RequireInternalSlot` throws.
    super::install_function(obj, "format", format_thunk as *const u8, 1, 1, false);
    super::install_function(
        obj,
        "formatToParts",
        to_parts_thunk as *const u8,
        1,
        1,
        false,
    );
    super::install_function(
        obj,
        "resolvedOptions",
        resolved_options_thunk as *const u8,
        0,
        0,
        false,
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
/// `DURATION_UNITS` order. A String is parsed as an ISO-8601 duration; a non-object
/// non-string is a `TypeError`; out-of-range / non-integral / mixed-sign records are
/// a `RangeError`.
fn to_duration_record(value: f64) -> Vec<f64> {
    // `ToDurationRecord` first branch: a `Temporal.Duration` (or subclass) copies
    // its internal slots directly — no prototype getters observed, no field-order
    // side effects. Only reachable when the Temporal engine is compiled in.
    #[cfg(feature = "temporal")]
    if let Some(vals) = crate::temporal::duration_unit_values(value) {
        let vals = vals.to_vec();
        validate_duration(&vals);
        return vals;
    }
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        let s = string_from_string_value(value).unwrap_or_default();
        let Some(vals) = parse_iso_duration(&s) else {
            throw_range_error("Intl.DurationFormat.format: invalid duration string");
        };
        validate_duration(&vals);
        return vals;
    }
    let Some(input) = object_ptr_from_value(value) else {
        throw_type_error("Intl.DurationFormat.format: duration must be an object");
    };
    // ToDurationRecord reads the fields in alphabetical order (days, hours,
    // microseconds, milliseconds, minutes, months, nanoseconds, seconds, weeks,
    // years), which is observable through Proxy/getter side effects — distinct
    // from the DURATION_UNITS storage order. The second tuple element is the
    // index into the returned `vals` (DURATION_UNITS order).
    const FIELD_ORDER: &[(&str, usize)] = &[
        ("days", 3),
        ("hours", 4),
        ("microseconds", 8),
        ("milliseconds", 7),
        ("minutes", 5),
        ("months", 1),
        ("nanoseconds", 9),
        ("seconds", 6),
        ("weeks", 2),
        ("years", 0),
    ];
    let mut vals = vec![0.0; DURATION_UNITS.len()];
    let mut any = false;
    for (unit, idx) in FIELD_ORDER.iter().copied() {
        let raw = get_field(input, unit);
        let jv = JSValue::from_bits(raw.to_bits());
        if jv.is_undefined() {
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
        vals[idx] = n;
    }
    if !any {
        throw_type_error("Intl.DurationFormat.format: duration must have at least one field");
    }
    validate_duration(&vals);
    vals
}

/// `IsValidDuration`: a single overall sign, years/months/weeks bounded by 2^32-1,
/// and the calendar/time units' combined magnitude (in seconds) below 2^53.
fn validate_duration(vals: &[f64]) {
    let mut sign = 0i32;
    for (i, unit) in DURATION_UNITS.iter().copied().enumerate() {
        let n = vals[i];
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
        const U32_MAX: f64 = 4_294_967_295.0;
        if matches!(unit, "years" | "months" | "weeks") && n.abs() > U32_MAX {
            throw_range_error(&format!("Intl.DurationFormat.format: {unit} out of range"));
        }
    }
    // `IsValidDurationRecord` step 16–17:
    //   normalizedSeconds = days×86400 + hours×3600 + minutes×60 + seconds
    //                       + ms×10⁻³ + µs×10⁻⁶ + ns×10⁻⁹
    //   reject when abs(normalizedSeconds) ≥ 2⁵³.
    // The naive f64 sum above loses ULPs at these magnitudes (days near
    // MAX_SAFE_INTEGER/86400), spuriously rounding a maximal-but-valid duration
    // past 2⁵³ — test262 `duration-out-of-range-3/-4` pin this exact edge. Do it
    // in exact integer nanoseconds instead: each field is a validated integral
    // f64, so `abs(normalizedSeconds) ≥ 2⁵³` ⇔ `abs(totalNs) ≥ 2⁵³ × 10⁹`, an
    // exact i128 comparison.
    const NS_PER: [i128; 7] = [
        86_400_000_000_000, // days
        3_600_000_000_000,  // hours
        60_000_000_000,     // minutes
        1_000_000_000,      // seconds
        1_000_000,          // milliseconds
        1_000,              // microseconds
        1,                  // nanoseconds
    ];
    const LIMIT_NS: i128 = (1i128 << 53) * 1_000_000_000;
    let mut total_ns: i128 = 0;
    let mut overflow = false;
    for (k, &scale) in NS_PER.iter().enumerate() {
        let v = vals[3 + k];
        // A non-integral / non-finite field is impossible here (fields are
        // validated in `to_duration_record`), but stay defensive: treat it as
        // out-of-range rather than panicking on the cast.
        if !v.is_finite() || v.fract() != 0.0 {
            overflow = true;
            break;
        }
        match (v as i128)
            .checked_mul(scale)
            .and_then(|prod| total_ns.checked_add(prod))
        {
            Some(sum) => total_ns = sum,
            None => {
                overflow = true;
                break;
            }
        }
    }
    if overflow || total_ns.unsigned_abs() >= LIMIT_NS as u128 {
        throw_range_error("Intl.DurationFormat.format: duration out of range");
    }
}

/// Parse an ISO-8601 / `Temporal.Duration` string (`±P[nY][nM][nW][nD][T[nH][nM][nS]]`,
/// designators case-insensitive, fraction allowed on the final time component) into
/// the ten unit values. Returns `None` for any structural deviation. Fractional
/// seconds split into milli/micro/nanoseconds.
fn parse_iso_duration(s: &str) -> Option<Vec<f64>> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let sign = match bytes.first() {
        Some(b'+') => {
            i += 1;
            1.0
        }
        Some(b'-') => {
            i += 1;
            -1.0
        }
        _ => 1.0,
    };
    if bytes.get(i).map(|b| b.to_ascii_uppercase()) != Some(b'P') {
        return None;
    }
    i += 1;

    // years, months, weeks, days, hours, minutes, seconds, ms, us, ns
    let mut vals = [0.0f64; 10];
    let mut any = false;
    let mut in_time = false;
    // Track which designators are allowed to appear (monotonic order).
    let mut date_idx = 0usize; // 0=Y,1=M,2=W,3=D
    let mut time_idx = 0usize; // 0=H,1=M,2=S

    while i < bytes.len() {
        let c = bytes[i].to_ascii_uppercase();
        if c == b'T' {
            if in_time {
                return None;
            }
            in_time = true;
            i += 1;
            // T must be followed by at least one time component.
            if i >= bytes.len() {
                return None;
            }
            continue;
        }
        // Read a number (digits, optionally a fraction for the final time component).
        let num_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let int_str = &s[num_start..i];
        let mut frac_str: &str = "";
        if i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b',') {
            i += 1;
            let frac_start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            frac_str = &s[frac_start..i];
            if frac_str.is_empty() {
                return None;
            }
        }
        if int_str.is_empty() {
            return None;
        }
        let int_val: f64 = int_str.parse().ok()?;
        let designator = *bytes.get(i)?;
        i += 1;
        let has_frac = !frac_str.is_empty();
        any = true;

        if !in_time {
            // Date designators in strict order: Y < M < W < D.
            let target = match designator {
                b'Y' | b'y' => 0,
                b'M' | b'm' => 1,
                b'W' | b'w' => 2,
                b'D' | b'd' => 3,
                _ => return None,
            };
            if target < date_idx || has_frac {
                return None;
            }
            date_idx = target + 1;
            vals[target] = int_val;
        } else {
            // Time designators in strict order: H < M < S. Fraction only on the last.
            match designator {
                b'H' | b'h' => {
                    if time_idx > 0 || has_frac {
                        return None;
                    }
                    time_idx = 1;
                    vals[4] = int_val;
                }
                b'M' | b'm' => {
                    if time_idx > 1 || has_frac {
                        return None;
                    }
                    time_idx = 2;
                    vals[5] = int_val;
                }
                b'S' | b's' => {
                    if time_idx > 2 {
                        return None;
                    }
                    time_idx = 3;
                    vals[6] = int_val;
                    if has_frac {
                        // Pad/truncate fraction to 9 digits, split 3/3/3.
                        let mut frac = frac_str.to_string();
                        frac.truncate(9);
                        while frac.len() < 9 {
                            frac.push('0');
                        }
                        vals[7] = frac[0..3].parse::<f64>().ok()?;
                        vals[8] = frac[3..6].parse::<f64>().ok()?;
                        vals[9] = frac[6..9].parse::<f64>().ok()?;
                    }
                }
                _ => return None,
            }
        }
    }
    // A lone `T` with nothing after, or `P` with no components, is invalid.
    if !any {
        return None;
    }
    for v in vals.iter_mut() {
        *v *= sign;
    }
    Some(vals.to_vec())
}

// ---- format / formatToParts (PartitionDurationFormatPattern) ---------------

/// One emitted part: a NumberFormat/ListFormat segment plus the singular unit it
/// belongs to (for `formatToParts`; list separators carry no unit).
struct DfPart {
    ty: &'static str,
    value: String,
    unit: Option<&'static str>,
}

/// `durationToFractional(duration, exponent)` from the spec helper: combine the
/// sub-second units at or below `exponent` into a single decimal seconds/ms/µs value.
/// Mirrors the reference's integer-division string construction (so a zero integer
/// part drops the sign, exactly as `${q}.${r}` does).
fn duration_to_fractional(vals: &[f64], exponent: u32) -> f64 {
    let (sec, ms, us, ns) = (vals[6], vals[7], vals[8], vals[9]);
    match exponent {
        9 if ms == 0.0 && us == 0.0 && ns == 0.0 => return sec,
        6 if us == 0.0 && ns == 0.0 => return ms,
        3 if ns == 0.0 => return us,
        _ => {}
    }
    let mut total: i128 = ns as i128;
    if exponent == 9 {
        total += (sec as i128) * 1_000_000_000;
    }
    if exponent >= 6 {
        total += (ms as i128) * 1_000_000;
    }
    if exponent >= 3 {
        total += (us as i128) * 1_000;
    }
    let e: i128 = 10i128.pow(exponent);
    let q = total / e;
    let r = (total % e).unsigned_abs();
    let frac = format!("{:0width$}", r, width = exponent as usize);
    format!("{q}.{frac}").parse::<f64>().unwrap_or(0.0)
}

/// Index of `unit`'s successor in `UNITS`, used to peek the next unit's resolved
/// style for the sub-second fractional combination.
fn next_style(obj: *const ObjectHeader, idx: usize) -> String {
    UNITS
        .get(idx + 1)
        .and_then(|(u, _, _)| get_string_field(obj, &style_key(u)))
        .unwrap_or_default()
}

/// `PartitionDurationFormatPattern`: render the validated duration to a flat part
/// list. Routes every numeric value through [`super::number_parts_from_resolved`] and
/// joins the per-unit strings with [`super::list_format_parts`].
fn partition(obj: *const ObjectHeader, vals: &[f64]) -> Vec<DfPart> {
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    let numbering = get_string_field(obj, KEY_DF_NUMBERING).unwrap_or_else(|| "latn".to_string());
    let base_style = get_string_field(obj, KEY_DF_STYLE).unwrap_or_else(|| "short".to_string());
    let fractional_digits = get_number_field(obj, KEY_DF_FRACTIONAL).map(|n| n as u32);

    let mut result: Vec<Vec<DfPart>> = Vec::new();
    let mut need_separator = false;
    let mut display_negative_sign = true;

    for (idx, (unit, _, _)) in UNITS.iter().copied().enumerate() {
        let mut value = vals[idx];
        let style = get_string_field(obj, &style_key(unit)).unwrap_or_else(|| "short".to_string());
        let display =
            get_string_field(obj, &display_key(unit)).unwrap_or_else(|| "auto".to_string());
        let nf_unit = UNIT_SINGULAR[idx];

        let mut r = super::nf_resolved_default(&locale);
        r.numbering_system = numbering.clone();

        // Numeric seconds and sub-seconds combine into one fractional value.
        let mut done = false;
        if matches!(unit, "seconds" | "milliseconds" | "microseconds")
            && next_style(obj, idx) == "numeric"
        {
            let exponent = match unit {
                "seconds" => 9,
                "milliseconds" => 6,
                _ => 3,
            };
            value = duration_to_fractional(vals, exponent);
            r.max_frac = fractional_digits.unwrap_or(9);
            r.min_frac = fractional_digits.unwrap_or(0);
            r.rounding_mode = "trunc".to_string();
            done = true;
        }

        // Display zero numeric minutes when seconds will be displayed.
        let mut display_required = false;
        if unit == "minutes" && need_separator {
            let seconds_display = get_string_field(obj, &display_key("seconds"))
                .unwrap_or_else(|| "auto".to_string());
            display_required = seconds_display == "always"
                || vals[6] != 0.0
                || vals[7] != 0.0
                || vals[8] != 0.0
                || vals[9] != 0.0;
        }

        if value != 0.0 || display != "auto" || display_required {
            // Only the first displayed value shows the duration sign.
            if display_negative_sign {
                display_negative_sign = false;
                if value == 0.0 && vals.iter().any(|v| *v < 0.0) {
                    value = -0.0;
                }
            } else {
                r.sign_display = "never".to_string();
            }

            if style == "2-digit" {
                r.min_int = 2;
            }
            if style != "numeric" && style != "2-digit" {
                r.style = "unit".to_string();
                r.unit = Some(nf_unit.to_string());
                r.unit_display = style.clone();
            } else {
                r.use_grouping = "false".to_string();
            }

            let nf_parts = super::number_parts_from_resolved(&r, value);

            if !need_separator {
                let list: Vec<DfPart> = nf_parts
                    .into_iter()
                    .map(|(ty, v)| DfPart {
                        ty,
                        value: v,
                        unit: Some(nf_unit),
                    })
                    .collect();
                if style == "2-digit" || style == "numeric" {
                    need_separator = true;
                }
                result.push(list);
            } else if let Some(list) = result.last_mut() {
                list.push(DfPart {
                    ty: "literal",
                    value: ":".to_string(),
                    unit: None,
                });
                for (ty, v) in nf_parts {
                    list.push(DfPart {
                        ty,
                        value: v,
                        unit: Some(nf_unit),
                    });
                }
            }
        }

        if done {
            break;
        }
    }

    let mut list_style = base_style;
    if list_style == "digital" {
        list_style = "short".to_string();
    }
    let strings: Vec<String> = result
        .iter()
        .map(|parts| parts.iter().map(|p| p.value.as_str()).collect())
        .collect();
    // DurationFormat historically used the base (en-US) list separators; pass
    // "en-US" explicitly to preserve that output (locale-specific duration list
    // patterns are out of scope here).
    let lf_parts = super::list_format_parts("en-US", &strings, "unit", &list_style);

    let mut flattened: Vec<DfPart> = Vec::new();
    let mut elem = 0usize;
    for (ty, val) in lf_parts {
        if ty == "element" {
            if let Some(parts) = result.get_mut(elem) {
                flattened.append(parts);
            }
            elem += 1;
        } else {
            flattened.push(DfPart {
                ty,
                value: val,
                unit: None,
            });
        }
    }
    flattened
}

/// Convert duration parts into the `formatToParts` JS array (`{ type, value, unit? }`).
fn df_parts_to_js_array(parts: &[DfPart]) -> f64 {
    let mut arr = js_array_alloc(parts.len() as u32);
    for part in parts {
        let obj = js_object_alloc(0, 3);
        set_field(obj, "type", string_value(part.ty));
        set_field(obj, "value", string_value(&part.value));
        if let Some(unit) = part.unit {
            set_field(obj, "unit", string_value(unit));
        }
        arr = js_array_push_f64(arr, js_nanbox_pointer(obj as i64));
    }
    js_nanbox_pointer(arr as i64)
}

fn format_value(obj: *const ObjectHeader, duration: f64) -> f64 {
    let vals = to_duration_record(duration);
    let parts = partition(obj, &vals);
    string_value(&parts.iter().map(|p| p.value.as_str()).collect::<String>())
}

pub(super) extern "C" fn format_thunk(_closure: *const ClosureHeader, duration: f64) -> f64 {
    let obj = this_intl_object("format", super::KIND_DURATION_FORMAT);
    format_value(obj, duration)
}

pub(super) extern "C" fn to_parts_thunk(_closure: *const ClosureHeader, duration: f64) -> f64 {
    let obj = this_intl_object("formatToParts", super::KIND_DURATION_FORMAT);
    let vals = to_duration_record(duration);
    df_parts_to_js_array(&partition(obj, &vals))
}

pub(super) extern "C" fn constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    // `Intl.DurationFormat` is `[[Construct]]`-only: a bare call is a TypeError.
    if crate::object::js_new_target_get().to_bits() == crate::value::TAG_UNDEFINED {
        throw_type_error("Constructor Intl.DurationFormat requires 'new'");
    }
    super::make_instance(
        closure,
        super::KIND_DURATION_FORMAT,
        super::rest_arg(rest, 0),
        super::rest_arg(rest, 1),
    )
}
