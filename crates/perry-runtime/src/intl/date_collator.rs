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

/// ECMA-402 FormatDateTime / HandleDateTimeValue step 1: coerce the
/// `format`/`formatToParts` argument to a TimeClip'd integer-millisecond value.
/// `undefined` means "now". Every other value goes through ToNumber — a Date
/// object's ToNumber is its timestamp; a string is *parsed*, never fed to the
/// `Date` constructor; a Symbol throws TypeError; an object's abrupt
/// valueOf/toString propagates. A non-finite or out-of-range (|t| > 8.64e15)
/// result is a RangeError, per TimeClip.
fn date_arg_to_clipped_ms(value: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    // A Temporal argument dispatches on its brand in the spec — it is never fed
    // to ToNumber — so it must not raise the "Cannot convert a Temporal value to
    // a number" TypeError here. Perry has no Temporal/calendar formatting engine
    // (out of scope, see CLAUDE.md), so this is a best-effort fallthrough: the
    // raw cell value decodes to epoch in the deterministic formatter rather than
    // throwing, keeping `format`/`formatToParts` non-throwing for these inputs.
    if crate::temporal::is_temporal_value(value) {
        return crate::date::date_cell_timestamp(value);
    }
    let ms = if js.is_undefined() {
        crate::date::js_date_now()
    } else {
        // Date cells fast-path to their stored timestamp (identical to
        // ToNumber(date)); `date_cell_timestamp` returns its argument unchanged
        // for non-cells, so everything else is routed through ToNumber.
        let ts = crate::date::date_cell_timestamp(value);
        if ts.to_bits() == value.to_bits() {
            crate::builtins::js_number_coerce(value)
        } else {
            ts
        }
    };
    const TIME_CLIP_LIMIT_MS: f64 = 8.64e15;
    if !ms.is_finite() || ms.abs() > TIME_CLIP_LIMIT_MS {
        throw_range_error("Invalid time value");
    }
    ms.trunc()
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
    let ms = date_arg_to_clipped_ms(value);
    string_value(&date_short_utc_from_ms(ms))
}

pub(crate) extern "C" fn date_time_format_to_parts_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let _obj = this_intl_object("formatToParts", KIND_DATE_TIME);
    let ms = date_arg_to_clipped_ms(value);
    parts_to_js_array(&date_range_parts_from_ms(ms))
}

pub(crate) extern "C" fn date_time_format_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let _obj = captured_intl_object(closure, "formatToParts", KIND_DATE_TIME);
    let ms = date_arg_to_clipped_ms(value);
    parts_to_js_array(&date_range_parts_from_ms(ms))
}

/// `M/D/YYYY` short form rendered directly from an integer-millisecond
/// timestamp. Shared by `format`, `formatToParts`, and both range variants so
/// all four stay byte-for-byte consistent.
pub(crate) fn date_short_utc_from_ms(ms: f64) -> String {
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    format!("{}/{}/{}", month, day, year)
}

pub(crate) fn date_range_parts_from_ms(ms: f64) -> Vec<(&'static str, String)> {
    let secs = (ms as i64).div_euclid(1000);
    let (year, month, day, _, _, _) = crate::date::timestamp_to_components(secs);
    vec![
        ("month", month.to_string()),
        ("literal", "/".to_string()),
        ("day", day.to_string()),
        ("literal", "/".to_string()),
        ("year", year.to_string()),
    ]
}

/// Shared steps 4–7 of `Intl.DateTimeFormat.prototype.formatRange` /
/// `formatRangeToParts`: reject `undefined` endpoints (TypeError), coerce each
/// via ToNumber (propagating abrupt completions and the Symbol TypeError), and
/// reject any non-finite (TimeClip → NaN) endpoint (RangeError). The current
/// ECMA-402 PartitionDateTimeRangePattern does **not** reject `x > y` — it just
/// formats the range as given — so no such check is made here. Returns the
/// clipped `(x, y)` millisecond pair.
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
    let out = js_object_alloc(0, 16);
    // Properties are inserted in ECMA-402 resolvedOptions order
    // (resolvedOptions/order*.js asserts this): locale, calendar,
    // numberingSystem, timeZone, [hourCycle, hour12], the date/time components,
    // then [dateStyle, timeStyle]. Only requested components are emitted, so an
    // absent option is reported as a missing own property.
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
    set_field(
        out,
        "numberingSystem",
        string_value(
            &get_string_field(obj, KEY_NUMBERING_SYSTEM).unwrap_or_else(|| "latn".to_string()),
        ),
    );
    set_field(
        out,
        "timeZone",
        string_value(&get_string_field(obj, KEY_TIME_ZONE).unwrap_or_else(|| "UTC".to_string())),
    );
    // hourCycle / hour12 surface only when an hour field is present. With no tz
    // /CLDR data, the default cycle is the 12-hour clock (`h11` for `ja`, else
    // `h12`); an explicit hour12 overrides hourCycle, and `hour12: false` is the
    // 24-hour `h23`.
    if get_string_field(obj, KEY_HOUR).is_some() {
        let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_default();
        let is_ja = locale == "ja" || locale.starts_with("ja-");
        let raw_h12 = {
            let v = JSValue::from_bits(get_field(obj, KEY_HOUR12).to_bits());
            if v.is_bool() {
                Some(v.as_bool())
            } else {
                None
            }
        };
        let raw_hc = get_string_field(obj, KEY_HOUR_CYCLE);
        let default_12h = if is_ja { "h11" } else { "h12" };
        let (hc, h12): (&str, bool) = if let Some(h12) = raw_h12 {
            if h12 {
                (default_12h, true)
            } else {
                ("h23", false)
            }
        } else if let Some(ref hc) = raw_hc {
            (hc.as_str(), hc == "h11" || hc == "h12")
        } else {
            (default_12h, true)
        };
        set_field(out, "hourCycle", string_value(hc));
        set_field(out, "hour12", bool_value(h12));
    }
    for (key, name) in [
        (KEY_WEEKDAY, "weekday"),
        (KEY_ERA, "era"),
        (KEY_YEAR, "year"),
        (KEY_MONTH, "month"),
        (KEY_DAY, "day"),
        (KEY_DAY_PERIOD, "dayPeriod"),
        (KEY_HOUR, "hour"),
        (KEY_MINUTE, "minute"),
        (KEY_SECOND, "second"),
    ] {
        if let Some(value) = get_string_field(obj, key) {
            set_field(out, name, string_value(&value));
        }
    }
    if let Some(n) = get_number_field(obj, KEY_FRACTIONAL) {
        set_field(out, "fractionalSecondDigits", n);
    }
    if let Some(value) = get_string_field(obj, KEY_TIME_ZONE_NAME) {
        set_field(out, "timeZoneName", string_value(&value));
    }
    if let Some(value) = get_string_field(obj, KEY_DATE_STYLE) {
        set_field(out, "dateStyle", string_value(&value));
    }
    if let Some(value) = get_string_field(obj, KEY_TIME_STYLE) {
        set_field(out, "timeStyle", string_value(&value));
    }
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

/// Normalize to NFD so canonically-equivalent strings (e.g. `"ö"` precomposed
/// vs. `"ö"` decomposed) collate equal — the ECMA-402 requirement that
/// `Collator.compare` treats canonical equivalents as 0 (canonically-equivalent
/// -strings.js). Without `string-normalize` this is an identity passthrough, so
/// the precomposed/decomposed pair still compares unequal (best effort).
#[cfg(feature = "string-normalize")]
fn collation_normalize(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    // NFC (composition), not NFD: it makes canonical equivalents equal while
    // keeping precomposed `å/ä/ö` intact for the Swedish fast path below.
    s.nfc().collect()
}
#[cfg(not(feature = "string-normalize"))]
fn collation_normalize(s: &str) -> String {
    s.to_string()
}

pub(crate) fn compare_strings(locale: &str, left: &str, right: &str) -> f64 {
    let left = collation_normalize(left);
    let right = collation_normalize(right);
    let (left, right) = (left.as_str(), right.as_str());
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

/// Strip the code points a UCA `ignorePunctuation` collator treats as ignorable
/// — whitespace and punctuation — so e.g. `compare("", " ")` and
/// `compare("", "*")` are 0 (compare/ignorePunctuation.js).
fn strip_ignorable_punctuation(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !is_punctuation(*c))
        .collect()
}

fn is_punctuation(c: char) -> bool {
    // ASCII punctuation plus an explicit set of Unicode punctuation code points,
    // deliberately NOT whole Latin-1 ranges — those contain letters/numbers
    // (`ª` U+00AA, `µ` U+00B5, `º` U+00BA, the `¹²³` superscripts, `¼½¾`
    // fractions) that must not be stripped or distinct strings would compare
    // equal. The General Punctuation block (U+2000–U+206F) and CJK punctuation
    // (U+3000–U+303F) are all punctuation/spaces and are safe as ranges.
    c.is_ascii_punctuation()
        || matches!(c,
            '\u{00A1}' | '\u{00A7}' | '\u{00AB}' | '\u{00B6}' | '\u{00B7}'
            | '\u{00BB}' | '\u{00BF}'
            | '\u{2000}'..='\u{206F}'
            | '\u{3000}'..='\u{303F}')
}

pub(crate) fn collator_compare_object(obj: *const ObjectHeader, left: f64, right: f64) -> f64 {
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    let ignore_punct = get_field(obj, KEY_COL_IGNORE_PUNCT).to_bits() == crate::value::TAG_TRUE;
    let (mut l, mut r) = (value_to_string(left), value_to_string(right));
    if ignore_punct {
        l = strip_ignorable_punctuation(&l);
        r = strip_ignorable_punctuation(&r);
    }
    compare_strings(&locale, &l, &r)
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
    let out = js_object_alloc(0, 7);
    // Property insertion order matches ECMA-402 (resolvedOptions/order.js):
    // locale, usage, sensitivity, ignorePunctuation, collation, numeric, caseFirst.
    set_field(
        out,
        "locale",
        string_value(&get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string())),
    );
    set_field(
        out,
        "usage",
        string_value(&get_string_field(obj, KEY_COL_USAGE).unwrap_or_else(|| "sort".to_string())),
    );
    set_field(
        out,
        "sensitivity",
        string_value(
            &get_string_field(obj, KEY_COL_SENSITIVITY).unwrap_or_else(|| "variant".to_string()),
        ),
    );
    set_field(
        out,
        "ignorePunctuation",
        bool_value(get_field(obj, KEY_COL_IGNORE_PUNCT).to_bits() == crate::value::TAG_TRUE),
    );
    set_field(
        out,
        "collation",
        string_value(
            &get_string_field(obj, KEY_COL_COLLATION).unwrap_or_else(|| "default".to_string()),
        ),
    );
    set_field(
        out,
        "numeric",
        bool_value(get_field(obj, KEY_COL_NUMERIC).to_bits() == crate::value::TAG_TRUE),
    );
    set_field(
        out,
        "caseFirst",
        string_value(
            &get_string_field(obj, KEY_COL_CASE_FIRST).unwrap_or_else(|| "false".to_string()),
        ),
    );
    js_nanbox_pointer(out as i64)
}
