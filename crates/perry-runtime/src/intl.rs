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

const KIND_NUMBER: &str = "NumberFormat";
const KIND_DATE_TIME: &str = "DateTimeFormat";
const KIND_COLLATOR: &str = "Collator";
const KIND_SEGMENTER: &str = "Segmenter";

const KEY_KIND: &str = "__intlKind";
const KEY_LOCALE: &str = "__intlLocale";
const KEY_STYLE: &str = "__intlStyle";
const KEY_CURRENCY: &str = "__intlCurrency";
const KEY_MAX_FRACTION_DIGITS: &str = "__intlMaxFractionDigits";
const KEY_DATE_STYLE: &str = "__intlDateStyle";
const KEY_TIME_ZONE: &str = "__intlTimeZone";
const KEY_GRANULARITY: &str = "__intlGranularity";

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

fn format_number_instance(obj: *const ObjectHeader, value: f64) -> String {
    let locale = get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string());
    let style = get_string_field(obj, KEY_STYLE).unwrap_or_else(|| "decimal".to_string());
    let currency = get_string_field(obj, KEY_CURRENCY);
    if style == "currency" {
        let mut formatted = format_number_parts(value, &locale, Some(2), None);
        match currency.as_deref() {
            Some("EUR") if locale.starts_with("de") => formatted.push_str("\u{00a0}\u{20ac}"),
            Some("EUR") => formatted = format!("\u{20ac}{formatted}"),
            Some("USD") => formatted = format!("${formatted}"),
            Some(code) => {
                formatted.push(' ');
                formatted.push_str(code);
            }
            None => {}
        }
        formatted
    } else {
        let max_digits = get_number_field(obj, KEY_MAX_FRACTION_DIGITS)
            .filter(|n| *n >= 0.0)
            .map(|n| n as usize);
        format_number_parts(value, &locale, None, max_digits)
    }
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
    set_field(out, "calendar", string_value("gregory"));
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
                "resolvedOptions",
                number_format_bound_resolved_options_thunk as *const u8,
                0,
            );
        }
        KIND_DATE_TIME => {
            let date_style =
                get_option_string(options, "dateStyle").unwrap_or_else(|| "short".to_string());
            let time_zone =
                get_option_string(options, "timeZone").unwrap_or_else(|| "UTC".to_string());
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

fn install_constructor(
    ns_obj: *mut ObjectHeader,
    name: &str,
    ctor_ptr: *const u8,
    methods: &[(&str, *const u8, u32)],
) {
    let ctor = crate::closure::js_closure_alloc(ctor_ptr, 0);
    if ctor.is_null() {
        return;
    }
    crate::closure::js_register_closure_rest(ctor_ptr, 0);
    crate::object::set_bound_native_closure_name(ctor, name);
    crate::object::set_builtin_closure_length(ctor as usize, 0);
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
    install_constructor(
        ns_obj,
        "NumberFormat",
        number_format_constructor_thunk as *const u8,
        &[
            ("format", number_format_format_thunk as *const u8, 1),
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
        &[
            ("format", date_time_format_format_thunk as *const u8, 1),
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
        &[
            ("segment", segmenter_segment_thunk as *const u8, 1),
            (
                "resolvedOptions",
                segmenter_resolved_options_thunk as *const u8,
                0,
            ),
        ],
    );
}
