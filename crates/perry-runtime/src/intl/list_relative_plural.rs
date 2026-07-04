use super::*;

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

/// Validate and canonicalize a `calendar` option per the Unicode Locale
/// Identifier `type` nonterminal: one or more `-`-joined segments, each 3–8
/// ASCII alphanumerics. Returns the lowercased + alias-resolved calendar ID, or
/// `None` if the input is malformed (the caller throws RangeError). Non-ASCII
/// input (e.g. capital dotted `İ`) fails the `is_ascii_alphanumeric` test, so it
/// is rejected rather than silently lowercased.
pub(crate) fn canonicalize_calendar_id(raw: &str) -> Option<String> {
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
pub(crate) fn is_valid_offset_time_zone(tz: &str) -> bool {
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
pub(crate) fn canonicalize_offset_time_zone(tz: &str) -> String {
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
pub(crate) fn collect_string_list(value: f64) -> Vec<String> {
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
pub(crate) fn list_separators(
    list_type: &str,
    style: &str,
) -> (&'static str, &'static str, &'static str) {
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

pub(crate) fn list_format_parts(
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

pub(crate) fn list_format_instance_parts(
    obj: *const ObjectHeader,
    value: f64,
) -> Vec<(&'static str, String)> {
    let items = collect_string_list(value);
    let list_type = get_string_field(obj, KEY_TYPE).unwrap_or_else(|| "conjunction".to_string());
    let style = get_string_field(obj, KEY_LF_STYLE).unwrap_or_else(|| "long".to_string());
    list_format_parts(&items, &list_type, &style)
}

pub(crate) extern "C" fn list_format_format_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = this_intl_object("format", KIND_LIST_FORMAT);
    string_value(
        &list_format_instance_parts(obj, value)
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<String>(),
    )
}

pub(crate) extern "C" fn list_format_bound_format_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "format", KIND_LIST_FORMAT);
    string_value(
        &list_format_instance_parts(obj, value)
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<String>(),
    )
}

pub(crate) extern "C" fn list_format_to_parts_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = this_intl_object("formatToParts", KIND_LIST_FORMAT);
    parts_to_js_array(&list_format_instance_parts(obj, value))
}

pub(crate) extern "C" fn list_format_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "formatToParts", KIND_LIST_FORMAT);
    parts_to_js_array(&list_format_instance_parts(obj, value))
}

pub(crate) fn list_format_resolved_options_object(obj: *const ObjectHeader) -> f64 {
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

pub(crate) extern "C" fn list_format_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_LIST_FORMAT);
    list_format_resolved_options_object(obj)
}

pub(crate) extern "C" fn list_format_bound_resolved_options_thunk(
    closure: *const ClosureHeader,
) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_LIST_FORMAT);
    list_format_resolved_options_object(obj)
}

// ---- Intl.RelativeTimeFormat ----------------------------------------------

const RTF_SINGULAR_UNITS: &[&str] = &[
    "second", "minute", "hour", "day", "week", "month", "quarter", "year",
];

/// Normalize a RelativeTimeFormat unit argument (singular or plural) to its
/// singular sanctioned form, or `None` if unrecognized (caller raises RangeError).
pub(crate) fn rtf_singular_unit(unit: &str) -> Option<&'static str> {
    // The sanctioned units are case-sensitive (ECMA-402 IsSanctionedSingularUnit):
    // `"second"`/`"seconds"` are accepted, `"SECOND"` is not (format/unit-invalid.js).
    let candidate = unit.strip_suffix('s').unwrap_or(unit);
    RTF_SINGULAR_UNITS.iter().copied().find(|u| *u == candidate)
}

/// Build the long-form, `numeric: "always"` en-US relative-time parts for
/// `value` in `unit`. (`short`/`narrow` abbreviations and the `numeric: "auto"`
/// special words — "tomorrow"/"yesterday" — need CLDR data and fall back to the
/// long numeric form here.) Returns `(leading, number, trailing)` literal/number
/// fragments so `format` and `formatToParts` stay consistent.
pub(crate) fn rtf_parts(value: f64, unit: &str) -> Vec<(&'static str, String)> {
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

/// `ToNumber(value)` that rejects BigInt with a TypeError, matching the
/// ECMA-262 abstract operation. `js_number_coerce` alone converts `1n` → `1`
/// (for `Number(1n)`), but `Intl` `format`/`select*` go through ToNumber, so
/// `format(1n, "day")` must throw. A Symbol still throws inside `js_number_coerce`,
/// and an object's `valueOf` is honoured there.
pub(crate) fn to_number_reject_bigint(value: f64) -> f64 {
    if JSValue::from_bits(value.to_bits()).is_bigint() {
        throw_type_error("Cannot convert a BigInt value to a number");
    }
    crate::builtins::js_number_coerce(value)
}

/// Shared steps of `format`/`formatToParts`: `value = ? ToNumber(value)` (a
/// Symbol or BigInt throws TypeError; an object's `valueOf` is honoured), then
/// `unit = ? ToString(unit)`, then the RangeError guards for a non-finite value
/// or an unsanctioned unit. Returns the rendered parts together with the
/// resolved singular `unit` (the `[[Unit]]` field formatToParts attaches).
pub(crate) fn rtf_instance_parts_and_unit(
    value: f64,
    unit_arg: f64,
) -> (Vec<(&'static str, String)>, &'static str) {
    // ToNumber: a Symbol/BigInt value throws TypeError *before* the finite-ness
    // RangeError (format/value-symbol.js); an object's valueOf is invoked.
    let number = to_number_reject_bigint(value);
    // ToString(unit): a Symbol throws TypeError (before the RangeError enum
    // guard), matching ECMA-262 ToString — format/unit-invalid.js.
    if unsafe { crate::symbol::js_is_symbol(unit_arg) != 0 } {
        throw_type_error("Cannot convert a Symbol value to a string");
    }
    let unit_str = value_to_string(unit_arg);
    if !number.is_finite() {
        throw_range_error("Value need to be finite number for Intl.RelativeTimeFormat.format()");
    }
    let Some(unit) = rtf_singular_unit(&unit_str) else {
        throw_range_error(&format!(
            "Value {unit_str} out of range for Intl.RelativeTimeFormat.format() unit"
        ));
    };
    (rtf_parts(number, unit), unit)
}

pub(crate) fn rtf_instance_parts(value: f64, unit_arg: f64) -> Vec<(&'static str, String)> {
    rtf_instance_parts_and_unit(value, unit_arg).0
}

/// Build the `formatToParts` array, attaching the `[[Unit]]` field to every part
/// derived from the formatted number (i.e. every non-`"literal"` part) per
/// FormatRelativeTimeToParts (formatToParts/result-type.js).
fn rtf_parts_to_js_array(parts: &[(&'static str, String)], unit: &str) -> f64 {
    let mut arr = js_array_alloc(parts.len() as u32);
    for (ty, val) in parts {
        let obj = js_object_alloc(0, 3);
        set_field(obj, "type", string_value(ty));
        set_field(obj, "value", string_value(val));
        if *ty != "literal" {
            set_field(obj, "unit", string_value(unit));
        }
        arr = js_array_push_f64(arr, js_nanbox_pointer(obj as i64));
    }
    js_nanbox_pointer(arr as i64)
}

pub(crate) extern "C" fn rtf_format_thunk(
    _closure: *const ClosureHeader,
    value: f64,
    unit: f64,
) -> f64 {
    let _obj = this_intl_object("format", KIND_RELATIVE_TIME);
    string_value(
        &rtf_instance_parts(value, unit)
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<String>(),
    )
}

pub(crate) extern "C" fn rtf_bound_format_thunk(
    closure: *const ClosureHeader,
    value: f64,
    unit: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "format", KIND_RELATIVE_TIME);
    string_value(
        &rtf_instance_parts(value, unit)
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<String>(),
    )
}

pub(crate) extern "C" fn rtf_to_parts_thunk(
    _closure: *const ClosureHeader,
    value: f64,
    unit: f64,
) -> f64 {
    let _obj = this_intl_object("formatToParts", KIND_RELATIVE_TIME);
    let (parts, unit) = rtf_instance_parts_and_unit(value, unit);
    rtf_parts_to_js_array(&parts, unit)
}

pub(crate) extern "C" fn rtf_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
    unit: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatToParts", KIND_RELATIVE_TIME);
    let (parts, unit) = rtf_instance_parts_and_unit(value, unit);
    rtf_parts_to_js_array(&parts, unit)
}

pub(crate) fn rtf_resolved_options_object(obj: *const ObjectHeader) -> f64 {
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

pub(crate) extern "C" fn rtf_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_RELATIVE_TIME);
    rtf_resolved_options_object(obj)
}

pub(crate) extern "C" fn rtf_bound_resolved_options_thunk(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_RELATIVE_TIME);
    rtf_resolved_options_object(obj)
}

// ---- Intl.PluralRules ------------------------------------------------------

/// en plural-category selection. Cardinal: `i == 1 && v == 0` → "one". Ordinal
/// (UTS #35 en ordinal rules): 1st→"one", 2nd→"two", 3rd→"few", else "other".
pub(crate) fn plural_select_en(n: f64, is_ordinal: bool) -> &'static str {
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

pub(crate) fn plural_categories(is_ordinal: bool) -> &'static [&'static str] {
    if is_ordinal {
        &["one", "two", "few", "other"]
    } else {
        &["one", "other"]
    }
}

pub(crate) fn plural_rules_select(obj: *const ObjectHeader, value: f64) -> f64 {
    let n = JSValue::from_bits(value.to_bits()).to_number();
    let is_ordinal = get_string_field(obj, KEY_TYPE).as_deref() == Some("ordinal");
    string_value(plural_select_en(n, is_ordinal))
}

pub(crate) extern "C" fn plural_rules_select_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = this_intl_object("select", KIND_PLURAL_RULES);
    plural_rules_select(obj, value)
}

pub(crate) extern "C" fn plural_rules_bound_select_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "select", KIND_PLURAL_RULES);
    plural_rules_select(obj, value)
}

pub(crate) extern "C" fn plural_rules_select_range_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = this_intl_object("selectRange", KIND_PLURAL_RULES);
    plural_select_range(start, end)
}

pub(crate) extern "C" fn plural_rules_bound_select_range_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "selectRange", KIND_PLURAL_RULES);
    plural_select_range(start, end)
}

pub(crate) fn plural_select_range(start: f64, end: f64) -> f64 {
    // PluralRules.prototype.selectRange(start, end): a `undefined` endpoint is a
    // TypeError (step 3), evaluated *before* the `? ToNumber` coercions — and
    // ToNumber itself throws TypeError for a Symbol (selectRange/
    // undefined-arguments-throws.js, argument-tonumber-throws.js).
    if JSValue::from_bits(start.to_bits()).is_undefined()
        || JSValue::from_bits(end.to_bits()).is_undefined()
    {
        throw_type_error("Intl.PluralRules.prototype.selectRange: start and end must be defined");
    }
    let s = to_number_reject_bigint(start);
    let e = to_number_reject_bigint(end);
    if s.is_nan() || e.is_nan() {
        throw_range_error("Invalid values for Intl.PluralRules.selectRange()");
    }
    // en range plural is "other" for all but trivial cases; report "other".
    string_value("other")
}

pub(crate) fn plural_rules_resolved_options_object(obj: *const ObjectHeader) -> f64 {
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
    let notation = get_string_field(obj, KEY_PR_NOTATION).unwrap_or_else(|| "standard".to_string());
    set_field(out, "notation", string_value(&notation));
    // `compactDisplay` surfaces only when notation is "compact".
    if notation == "compact" {
        set_field(
            out,
            "compactDisplay",
            string_value(
                &get_string_field(obj, KEY_PR_COMPACT_DISPLAY)
                    .unwrap_or_else(|| "short".to_string()),
            ),
        );
    }
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

pub(crate) extern "C" fn plural_rules_resolved_options_thunk(
    _closure: *const ClosureHeader,
) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_PLURAL_RULES);
    plural_rules_resolved_options_object(obj)
}

pub(crate) extern "C" fn plural_rules_bound_resolved_options_thunk(
    closure: *const ClosureHeader,
) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_PLURAL_RULES);
    plural_rules_resolved_options_object(obj)
}
