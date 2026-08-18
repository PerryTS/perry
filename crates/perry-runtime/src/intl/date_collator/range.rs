use super::*;

type Part = (&'static str, String);
type RangePart = (&'static str, String, &'static str);

const RANGE_SEPARATOR: &str = " \u{2013} ";

/// Shared steps of `formatRange` / `formatRangeToParts`: require matching
/// endpoint brands and calendars, then convert each endpoint to clipped epoch
/// milliseconds. A descending range is valid and is deliberately not rejected.
pub(crate) fn date_time_range_clip(method: &str, start: f64, end: f64) -> (f64, f64) {
    let sj = JSValue::from_bits(start.to_bits());
    let ej = JSValue::from_bits(end.to_bits());
    if sj.is_undefined() || ej.is_undefined() {
        throw_type_error(&format!(
            "Intl.DateTimeFormat.prototype.{method} called with undefined startDate or endDate"
        ));
    }
    if range_type_tag(start) != range_type_tag(end) {
        throw_type_error(&format!(
            "Intl.DateTimeFormat.prototype.{method} called with values of different types"
        ));
    }
    if let (Some(cs), Some(ce)) = (
        crate::temporal::temporal_calendar_id(start),
        crate::temporal::temporal_calendar_id(end),
    ) {
        if cs != ce {
            throw_range_error(&format!(
                "Intl.DateTimeFormat.prototype.{method}: both values must use the same calendar"
            ));
        }
    }
    (date_arg_to_clipped_ms(start), date_arg_to_clipped_ms(end))
}

fn range_type_tag(value: f64) -> u8 {
    crate::temporal::temporal_kind(value).map_or(0xFF, |kind| kind as u8)
}

fn tagged(parts: impl IntoIterator<Item = Part>, source: &'static str) -> Vec<RangePart> {
    parts
        .into_iter()
        .map(|(ty, value)| (ty, value, source))
        .collect()
}

fn append_tagged(out: &mut Vec<RangePart>, parts: &[Part], source: &'static str) {
    out.extend(parts.iter().map(|(ty, value)| (*ty, value.clone(), source)));
}

fn full_range_parts(start: Vec<Part>, end: Vec<Part>) -> Vec<RangePart> {
    let mut out = tagged(start, "startRange");
    out.push(("literal", RANGE_SEPARATOR.to_string(), "shared"));
    out.extend(tagged(end, "endRange"));
    out
}

fn is_named_date(parts: &[Part]) -> bool {
    parts.len() == 5
        && parts[0].0 == "month"
        && parts[1] == ("literal", " ".to_string())
        && parts[2].0 == "day"
        && parts[3] == ("literal", ", ".to_string())
        && parts[4].0 == "year"
}

/// Return a CLDR-style collapsed interval when Perry has a deterministic
/// English pattern for it. Other patterns retain both complete endpoints.
fn collapsed_english_range(start: &[Part], end: &[Part]) -> Option<Vec<RangePart>> {
    if is_named_date(start) && is_named_date(end) && start[4] == end[4] {
        let same_month = start[0] == end[0];
        let mut out = Vec::new();
        if same_month {
            append_tagged(&mut out, &start[..2], "shared");
            append_tagged(&mut out, &start[2..3], "startRange");
        } else {
            append_tagged(&mut out, &start[..3], "startRange");
        }
        out.push(("literal", RANGE_SEPARATOR.to_string(), "shared"));
        if same_month {
            append_tagged(&mut out, &end[2..3], "endRange");
        } else {
            append_tagged(&mut out, &end[..3], "endRange");
        }
        append_tagged(&mut out, &start[3..], "shared");
        return Some(out);
    }

    // A same-day date-time interval shares its complete date prefix and varies
    // only the time suffix: `8/4/2021, 12:30 AM – 11:30 PM`.
    let prefix_len = start.iter().zip(end).take_while(|(a, b)| a == b).count();
    let prefix = &start[..prefix_len];
    let has_full_date = ["month", "day", "year"]
        .iter()
        .all(|ty| prefix.iter().any(|part| part.0 == *ty));
    let has_time_suffix = start[prefix_len..]
        .iter()
        .chain(&end[prefix_len..])
        .any(|part| matches!(part.0, "hour" | "minute" | "second" | "fractionalSecond"));
    if has_full_date && has_time_suffix {
        let mut out = Vec::new();
        append_tagged(&mut out, prefix, "shared");
        append_tagged(&mut out, &start[prefix_len..], "startRange");
        out.push(("literal", RANGE_SEPARATOR.to_string(), "shared"));
        append_tagged(&mut out, &end[prefix_len..], "endRange");
        return Some(out);
    }
    None
}

fn range_endpoint_parts(
    obj: *const ObjectHeader,
    x: f64,
    y: f64,
    temporal_kind: Option<crate::temporal::TemporalKind>,
) -> (Vec<Part>, Vec<Part>) {
    (
        format_parts_with_dtf_obj(obj, x, temporal_kind),
        format_parts_with_dtf_obj(obj, y, temporal_kind),
    )
}

fn is_english(obj: *const ObjectHeader) -> bool {
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    locale == "en" || locale.starts_with("en-")
}

pub(crate) fn date_time_format_range_value(
    obj: *const ObjectHeader,
    method: &str,
    start: f64,
    end: f64,
) -> f64 {
    let temporal_kind = crate::temporal::temporal_kind(start);
    let (x, y) = date_time_range_clip(method, start, end);
    let sx = format_ms_with_dtf_obj(obj, x, temporal_kind);
    let sy = format_ms_with_dtf_obj(obj, y, temporal_kind);
    if sx == sy {
        return string_value(&sx);
    }
    if is_english(obj) {
        let (px, py) = range_endpoint_parts(obj, x, y, temporal_kind);
        if let Some(parts) = collapsed_english_range(&px, &py) {
            return string_value(&parts.into_iter().map(|part| part.1).collect::<String>());
        }
    }
    string_value(&format!("{sx}{RANGE_SEPARATOR}{sy}"))
}

/// Convert range parts to JS objects carrying the ECMA-402 `source` field.
pub(crate) fn range_parts_to_js_array(parts: &[RangePart]) -> f64 {
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

pub(crate) fn date_time_format_range_parts_value(
    obj: *const ObjectHeader,
    method: &str,
    start: f64,
    end: f64,
) -> f64 {
    let temporal_kind = crate::temporal::temporal_kind(start);
    let (x, y) = date_time_range_clip(method, start, end);
    let (px, py) = range_endpoint_parts(obj, x, y, temporal_kind);
    let parts = if px == py {
        tagged(px, "shared")
    } else if is_english(obj) {
        collapsed_english_range(&px, &py).unwrap_or_else(|| full_range_parts(px, py))
    } else {
        full_range_parts(px, py)
    };
    range_parts_to_js_array(&parts)
}

pub(crate) extern "C" fn date_time_format_range_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = this_intl_object("formatRange", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(start) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_range_value(obj, "formatRange", start, end)
}

pub(crate) extern "C" fn date_time_format_bound_range_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "formatRange", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(start) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_range_value(obj, "formatRange", start, end)
}

pub(crate) extern "C" fn date_time_format_range_to_parts_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = this_intl_object("formatRangeToParts", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(start) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_range_parts_value(obj, "formatRangeToParts", start, end)
}

pub(crate) extern "C" fn date_time_format_bound_range_to_parts_thunk(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "formatRangeToParts", KIND_DATE_TIME);
    if let Some(kind) = crate::temporal::temporal_kind(start) {
        validate_temporal_dtf_overlap(kind, obj);
    }
    date_time_format_range_parts_value(obj, "formatRangeToParts", start, end)
}
