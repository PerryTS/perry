use super::*;

use crate::array::{js_array_alloc, js_array_push_f64};
use crate::closure::ClosureHeader;
use crate::object::{js_object_alloc, ObjectHeader};
use crate::value::js_nanbox_pointer;
#[cfg(feature = "intl-segmenter")]
use unicode_segmentation::UnicodeSegmentation;

const KEY_SEGMENTS_BRAND: &str = "__intlSegmentsBrand";
const KEY_SEGMENTS_LENGTH: &str = "__intlSegmentsLength";
const SEGMENTS_BRAND: &str = "Segments";

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

fn string_pointer_value(ptr: *const StringHeader) -> f64 {
    f64::from_bits(JSValue::string_ptr(ptr as *mut StringHeader).bits())
}

unsafe fn segmenter_input_text(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let data = unsafe { (ptr as *const u8).add(std::mem::size_of::<StringHeader>()) };
    let len = unsafe { (*ptr).byte_len as usize };
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }

    // `unicode-segmentation` requires valid UTF-8. Represent each WTF-8 lone
    // surrogate as one U+FFFD while computing boundaries; both occupy exactly
    // one UTF-16 code unit, so offsets still map back to the original string.
    let mut text = String::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let (advance, _, code_point) = crate::string::wtf8_step(bytes, offset);
        match char::from_u32(code_point) {
            Some(ch) if !(0xD800..=0xDFFF).contains(&code_point) => text.push(ch),
            _ => text.push('\u{FFFD}'),
        }
        offset = (offset + advance).min(bytes.len());
    }
    text
}

pub(crate) fn make_segment_record(
    segment_value: f64,
    index: u32,
    input_value: f64,
    word_like: Option<bool>,
) -> f64 {
    let obj = js_object_alloc(0, 4);
    set_field(obj, "segment", segment_value);
    // `index` is a plain Number (UTF-16 code-unit offset into the input).
    set_field(obj, "index", index as f64);
    set_field(obj, "input", input_value);
    if let Some(word_like) = word_like {
        set_field(obj, "isWordLike", bool_value(word_like));
    }
    js_nanbox_pointer(obj as i64)
}

/// Build the segment list for `input` under `granularity`. The backing array
/// keeps the existing iterable / spreadable representation while exposing the
/// `Segments.prototype.containing()` surface required by ECMA-402.
pub(crate) fn build_segments(granularity: &str, value: f64) -> f64 {
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        throw_type_error("Cannot convert a Symbol value to a string");
    }
    let input_ptr = js_jsvalue_to_string(value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let input_handle = scope.root_string_ptr(input_ptr);
    let input =
        unsafe { input_handle.with_const_ptr::<StringHeader, _>(|ptr| segmenter_input_text(ptr)) };
    let mut arr = js_array_alloc(0);
    let mut index = 0u32;
    #[cfg(feature = "intl-segmenter")]
    match granularity {
        "word" => {
            for segment in input.split_word_bounds() {
                let end = index + utf16_len(segment);
                let segment_ptr = input_handle.with_const_ptr::<StringHeader, _>(|ptr| {
                    crate::string::js_string_slice(ptr, index as i32, end as i32)
                });
                let record = make_segment_record(
                    string_pointer_value(segment_ptr),
                    index,
                    input_handle.with_const_ptr::<StringHeader, _>(|ptr| string_pointer_value(ptr)),
                    Some(segment_is_word_like(segment)),
                );
                arr = js_array_push_f64(arr, record);
                index = end;
            }
        }
        "sentence" => {
            for segment in input.split_sentence_bounds() {
                let end = index + utf16_len(segment);
                let segment_ptr = input_handle.with_const_ptr::<StringHeader, _>(|ptr| {
                    crate::string::js_string_slice(ptr, index as i32, end as i32)
                });
                let record = make_segment_record(
                    string_pointer_value(segment_ptr),
                    index,
                    input_handle.with_const_ptr::<StringHeader, _>(|ptr| string_pointer_value(ptr)),
                    None,
                );
                arr = js_array_push_f64(arr, record);
                index = end;
            }
        }
        // "grapheme" (default): extended grapheme clusters (emoji ZWJ
        // sequences, combining marks, regional-indicator flags).
        _ => {
            for segment in input.graphemes(true) {
                let end = index + utf16_len(segment);
                let segment_ptr = input_handle.with_const_ptr::<StringHeader, _>(|ptr| {
                    crate::string::js_string_slice(ptr, index as i32, end as i32)
                });
                let record = make_segment_record(
                    string_pointer_value(segment_ptr),
                    index,
                    input_handle.with_const_ptr::<StringHeader, _>(|ptr| string_pointer_value(ptr)),
                    None,
                );
                arr = js_array_push_f64(arr, record);
                index = end;
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
            let end = index + utf16_len(&segment);
            let segment_ptr = input_handle.with_const_ptr::<StringHeader, _>(|ptr| {
                crate::string::js_string_slice(ptr, index as i32, end as i32)
            });
            let word_like = if is_word {
                Some(segment.chars().any(|c| c.is_alphanumeric()))
            } else {
                None
            };
            let record = make_segment_record(
                string_pointer_value(segment_ptr),
                index,
                input_handle.with_const_ptr::<StringHeader, _>(|ptr| string_pointer_value(ptr)),
                word_like,
            );
            arr = js_array_push_f64(arr, record);
            index = end;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let segments = scope.root_raw_mut_ptr(arr as *mut ObjectHeader);
    let brand = string_value(SEGMENTS_BRAND);
    segments.with_mut_ptr(|segments| set_internal_field(segments, KEY_SEGMENTS_BRAND, brand));
    segments
        .with_mut_ptr(|segments| set_internal_field(segments, KEY_SEGMENTS_LENGTH, index as f64));
    segments.with_mut_ptr(|segments| {
        install_function(
            segments,
            "containing",
            segmenter_containing_thunk as *const u8,
            1,
            1,
            false,
        )
    });
    install_segments_iterator(&segments);
    segments.with_mut_ptr(|segments: *mut ObjectHeader| js_nanbox_pointer(segments as i64))
}

fn install_segments_iterator(segments: &crate::gc::RuntimeHandle<'_>) {
    let symbol = crate::symbol::well_known_symbol("iterator");
    if symbol.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let symbol = scope.root_raw_mut_ptr(symbol);
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        segments_iterator_thunk as *const u8,
        0,
    ));
    if closure.with_mut_ptr(|closure: *mut ClosureHeader| closure.is_null()) {
        return;
    }
    crate::closure::js_register_closure_arity(segments_iterator_thunk as *const u8, 0);
    closure.with_mut_ptr::<ClosureHeader, _>(|ptr| {
        crate::object::set_bound_native_closure_name(ptr, "[Symbol.iterator]")
    });
    closure.with_mut_ptr::<ClosureHeader, _>(|ptr| {
        crate::object::set_builtin_closure_length(ptr as usize, 0)
    });
    let value = closure.with_mut_ptr::<ClosureHeader, _>(|ptr| js_nanbox_pointer(ptr as i64));
    unsafe {
        segments.with_mut_ptr(|segments: *mut ObjectHeader| {
            symbol.with_const_ptr(|symbol: *const u8| {
                crate::symbol::js_object_set_symbol_property(
                    js_nanbox_pointer(segments as i64),
                    f64::from_bits(JSValue::pointer(symbol).bits()),
                    value,
                )
            })
        });
    }
    segments.with_mut_ptr(|segments: *mut ObjectHeader| {
        symbol.with_const_ptr(|symbol: *const u8| {
            crate::symbol::set_symbol_property_attrs(
                segments as usize,
                symbol as usize,
                PropertyAttrs::new(true, false, true),
            )
        })
    });
}

extern "C" fn segments_iterator_thunk(_closure: *const ClosureHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let segments = scope.root_raw_const_ptr(segments_from_this());
    segments.with_const_ptr(|segments: *const crate::ArrayHeader| {
        crate::array::array_values_iter(js_nanbox_pointer(segments as i64))
    })
}

fn segments_from_this() -> *const crate::ArrayHeader {
    let this_value = crate::object::js_implicit_this_get();
    let Some(segments) = array_ptr_from_value(this_value) else {
        throw_type_error("Intl.Segments.prototype.containing called on incompatible receiver");
    };
    let brand = get_string_field(segments as *const ObjectHeader, KEY_SEGMENTS_BRAND);
    if brand.as_deref() != Some(SEGMENTS_BRAND) {
        throw_type_error("Intl.Segments.prototype.containing called on incompatible receiver");
    }
    segments
}

pub(crate) extern "C" fn segmenter_containing_thunk(
    _closure: *const ClosureHeader,
    index: f64,
) -> f64 {
    let segments = segments_from_this();
    let input_len =
        get_number_field(segments as *const ObjectHeader, KEY_SEGMENTS_LENGTH).unwrap_or(0.0);

    // ToIntegerOrInfinity may invoke user code, so keep the Segments backing
    // array rooted while coercing the index.
    let scope = crate::gc::RuntimeHandleScope::new();
    let segments_handle = scope.root_raw_const_ptr(segments);
    let (number, segments) = segments_handle.across_const::<crate::ArrayHeader, _>(|| {
        list_relative_plural::to_number_reject_bigint(index)
    });
    let integer = if number.is_nan() { 0.0 } else { number.trunc() };
    if integer < 0.0 || integer >= input_len {
        return undefined();
    }

    let count = js_array_length(segments);
    for i in 0..count {
        let record_value = js_array_get_f64(segments, i);
        let Some(record) = object_ptr_from_value(record_value) else {
            continue;
        };
        let start = get_number_field(record, "index").unwrap_or(0.0);
        let end = if i + 1 < count {
            let next_value = js_array_get_f64(segments, i + 1);
            object_ptr_from_value(next_value)
                .and_then(|next| get_number_field(next, "index"))
                .unwrap_or(input_len)
        } else {
            input_len
        };
        if integer >= start && integer < end {
            let segment_value = get_field(record, "segment");
            let input_value = get_field(record, "input");
            let word_like_value = get_field(record, "isWordLike");
            let word_like = if JSValue::from_bits(word_like_value.to_bits()).is_undefined() {
                None
            } else {
                Some(crate::value::js_is_truthy(word_like_value) != 0)
            };
            return make_segment_record(segment_value, start as u32, input_value, word_like);
        }
    }
    undefined()
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
