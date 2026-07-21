use super::*;

use crate::array::{js_array_alloc, js_array_push_f64};
use crate::closure::ClosureHeader;
use crate::object::{js_object_alloc, ObjectHeader};
use crate::value::js_nanbox_pointer;
#[cfg(feature = "intl-segmenter")]
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn normalize_granularity(value: Option<String>) -> String {
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
pub(crate) fn segment_is_word_like(segment: &str) -> bool {
    segment.chars().any(|c| c.is_alphanumeric())
}

pub(crate) fn utf16_len(segment: &str) -> u32 {
    segment.chars().map(|c| c.len_utf16() as u32).sum()
}

pub(crate) fn make_segment_record(
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
pub(crate) fn build_segments(granularity: &str, value: f64) -> f64 {
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

pub(crate) extern "C" fn segmenter_segment_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = this_intl_object("segment", KIND_SEGMENTER);
    segmenter_segment_object(obj, value)
}

pub(crate) extern "C" fn segmenter_bound_segment_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "segment", KIND_SEGMENTER);
    segmenter_segment_object(obj, value)
}

pub(crate) fn segmenter_segment_object(obj: *const ObjectHeader, value: f64) -> f64 {
    let granularity =
        get_string_field(obj, KEY_GRANULARITY).unwrap_or_else(|| "grapheme".to_string());
    build_segments(&granularity, value)
}

pub(crate) extern "C" fn segmenter_resolved_options_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_SEGMENTER);
    segmenter_resolved_options_object(obj)
}

pub(crate) extern "C" fn segmenter_bound_resolved_options_thunk(
    closure: *const ClosureHeader,
) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_SEGMENTER);
    segmenter_resolved_options_object(obj)
}

pub(crate) fn segmenter_resolved_options_object(obj: *const ObjectHeader) -> f64 {
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
