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

pub(crate) fn date_short_utc(value: f64) -> String {
    let timestamp = crate::date::date_cell_timestamp(value);
    if timestamp.is_nan() {
        return "Invalid Date".to_string();
    }
    let secs = (timestamp as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    format!("{}/{}/{:02}", month, day, year.rem_euclid(100))
}

pub(crate) extern "C" fn date_time_format_format_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let _obj = this_intl_object("format", KIND_DATE_TIME);
    date_time_format_format_value(value)
}

pub(crate) extern "C" fn date_time_format_bound_format_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "format", KIND_DATE_TIME);
    date_time_format_format_value(value)
}

pub(crate) fn date_time_format_format_value(value: f64) -> f64 {
    string_value(&date_short_utc(value))
}

/// Typed `formatToParts` segments for the default short DateTimeFormat. The
/// concatenation reproduces `date_short_utc` (`M/D/YY`), keeping `format()` and
/// `formatToParts()` consistent.
pub(crate) fn date_instance_parts(value: f64) -> Vec<(&'static str, String)> {
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

pub(crate) extern "C" fn date_time_format_to_parts_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let _obj = this_intl_object("formatToParts", KIND_DATE_TIME);
    parts_to_js_array(&date_instance_parts(value))
}

pub(crate) extern "C" fn date_time_format_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatToParts", KIND_DATE_TIME);
    parts_to_js_array(&date_instance_parts(value))
}

/// `M/D/YY` short form rendered directly from a millisecond timestamp (the
/// `formatRange` arguments arrive as already-coerced ToNumber values, not Date
/// cells, so they bypass `date_short_utc`'s `date_cell_timestamp` decode).
pub(crate) fn date_short_utc_from_ms(ms: f64) -> String {
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    format!("{}/{}/{:02}", month, day, year.rem_euclid(100))
}

pub(crate) fn date_range_parts_from_ms(ms: f64) -> Vec<(&'static str, String)> {
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
pub(crate) fn date_time_range_clip(method: &str, start: f64, end: f64) -> (f64, f64) {
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

pub(crate) fn date_time_format_range_value(method: &str, start: f64, end: f64) -> f64 {
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
pub(crate) fn range_parts_to_js_array(parts: &[(&'static str, String, &'static str)]) -> f64 {
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

pub(crate) fn date_time_format_range_parts_value(method: &str, start: f64, end: f64) -> f64 {
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

pub(crate) extern "C" fn date_time_format_range_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = this_intl_object("formatRange", KIND_DATE_TIME);
    date_time_format_range_value("formatRange", start, end)
}

pub(crate) extern "C" fn date_time_format_bound_range_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatRange", KIND_DATE_TIME);
    date_time_format_range_value("formatRange", start, end)
}

pub(crate) extern "C" fn date_time_format_range_to_parts_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = this_intl_object("formatRangeToParts", KIND_DATE_TIME);
    date_time_format_range_parts_value("formatRangeToParts", start, end)
}

pub(crate) extern "C" fn date_time_format_bound_range_to_parts_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatRangeToParts", KIND_DATE_TIME);
    date_time_format_range_parts_value("formatRangeToParts", start, end)
}

pub(crate) extern "C" fn date_time_format_resolved_options_thunk(
    _closure: *const ClosureHeader,
) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_DATE_TIME);
    date_time_format_resolved_options_object(obj)
}

pub(crate) extern "C" fn date_time_format_bound_resolved_options_thunk(
    closure: *const ClosureHeader,
) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_DATE_TIME);
    date_time_format_resolved_options_object(obj)
}

pub(crate) fn date_time_format_resolved_options_object(obj: *const ObjectHeader) -> f64 {
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

pub(crate) fn swedish_collation_key(s: &str) -> Vec<u32> {
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

pub(crate) fn compare_strings(locale: &str, left: &str, right: &str) -> f64 {
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

pub(crate) extern "C" fn collator_compare_thunk(
    _closure: *const ClosureHeader,
    left: f64,
    right: f64,
) -> f64 {
    let obj = this_intl_object("compare", KIND_COLLATOR);
    collator_compare_object(obj, left, right)
}

pub(crate) extern "C" fn collator_bound_compare_thunk(
    closure: *const ClosureHeader,
    left: f64,
    right: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "compare", KIND_COLLATOR);
    collator_compare_object(obj, left, right)
}

pub(crate) fn collator_compare_object(obj: *const ObjectHeader, left: f64, right: f64) -> f64 {
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    compare_strings(&locale, &value_to_string(left), &value_to_string(right))
}

pub(crate) extern "C" fn collator_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_COLLATOR);
    collator_resolved_options_object(obj)
}

pub(crate) extern "C" fn collator_bound_resolved_options_thunk(
    closure: *const ClosureHeader,
) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_COLLATOR);
    collator_resolved_options_object(obj)
}

pub(crate) fn collator_resolved_options_object(obj: *const ObjectHeader) -> f64 {
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
