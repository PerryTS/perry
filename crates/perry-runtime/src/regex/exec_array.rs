//! Match-result array decoration — the `index` / `input` / `groups` /
//! `indices` own properties ECMA-262 RegExpBuiltinExec attaches to the
//! array returned by `regex.exec(s)` / `s.match(regex)`. The `indices`
//! builders (`d` flag, hasIndices, #4930) cover both the `regex`-crate
//! fast path and the `fancy_regex` fallback (lookbehind/backreferences).
//!
//! Split out of `regex.rs` under the 2000-line CI cap.

pub(super) use super::utf16::{byte_index_to_utf16_index, utf16_index_to_byte};
use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
use crate::string::{StringHeader, STRING_FLAG_HAS_LONE_SURROGATES};
use crate::value::js_nanbox_string;

use super::js_string_from_str;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// A capture snapshotted while the subject payload is borrowed. Runtime result
/// materialization uses only these scalar offsets after the borrow is dropped.
#[derive(Clone, Copy)]
pub(super) struct OwnedCapture {
    byte_start: usize,
    byte_len: u32,
    utf16_len: u32,
    flags: u32,
    utf16_range: Option<(f64, f64)>,
}

impl OwnedCapture {
    pub(super) fn from_range(str_data: &str, byte_start: usize, byte_end: usize) -> Self {
        Self::from_range_with_indices(str_data, byte_start, byte_end, false)
    }

    fn from_range_with_indices(
        str_data: &str,
        byte_start: usize,
        byte_end: usize,
        with_indices: bool,
    ) -> Self {
        let bytes = &str_data.as_bytes()[byte_start..byte_end];
        let utf16_len = crate::string::compute_utf16_len_wtf8(bytes);
        let flags = if crate::string::bytes_have_lone_surrogate(bytes) {
            STRING_FLAG_HAS_LONE_SURROGATES
        } else {
            0
        };
        Self {
            byte_start,
            byte_len: bytes.len() as u32,
            utf16_len,
            flags,
            utf16_range: with_indices.then(|| {
                (
                    byte_index_to_utf16_index(str_data, byte_start) as f64,
                    byte_index_to_utf16_index(str_data, byte_end) as f64,
                )
            }),
        }
    }
}

/// All subject-derived state needed to build one RegExp match result. This is
/// deliberately owned/scalar-only: no `&str`, `regex::Match`, or `Captures`
/// may survive into the allocation phase (#8449).
///
/// Every offset here is absolute in `str_data`. The constructors used to take
/// a `search_start_byte` and re-base onto it, because `exec` searched a
/// `&str_data[lastIndex..]` slice; they no longer do, because `exec` searches
/// the whole subject from a position (#9429). Reintroducing a base offset
/// would mean the engine had been handed a slice again.
pub(super) struct OwnedExecMatch {
    captures: Vec<Option<OwnedCapture>>,
    named: Vec<(String, usize)>,
    pub(super) match_index: f64,
}

impl OwnedExecMatch {
    pub(super) fn from_standard(
        str_data: &str,
        regex: &regex::Regex,
        caps: &regex::Captures,
        has_indices: bool,
    ) -> Self {
        let captures: Vec<Option<OwnedCapture>> = caps
            .iter()
            .map(|capture| {
                capture.map(|m| {
                    OwnedCapture::from_range_with_indices(str_data, m.start(), m.end(), has_indices)
                })
            })
            .collect();
        let named = regex
            .capture_names()
            .enumerate()
            .filter_map(|(index, name)| name.map(|name| (name.to_string(), index)))
            .collect();
        let match_index = captures
            .first()
            .and_then(|capture| capture.as_ref())
            .map(|capture| {
                capture.utf16_range.map(|range| range.0).unwrap_or_else(|| {
                    byte_index_to_utf16_index(str_data, capture.byte_start) as f64
                })
            })
            .unwrap_or(0.0);
        Self {
            captures,
            named,
            match_index,
        }
    }

    pub(super) fn from_fancy(
        str_data: &str,
        regex: &fancy_regex::Regex,
        caps: &fancy_regex::Captures,
        has_indices: bool,
    ) -> Self {
        let captures: Vec<Option<OwnedCapture>> = (0..caps.len())
            .map(|index| {
                caps.get(index).map(|m| {
                    OwnedCapture::from_range_with_indices(str_data, m.start(), m.end(), has_indices)
                })
            })
            .collect();
        let named = regex
            .capture_names()
            .enumerate()
            .filter_map(|(index, name)| name.map(|name| (name.to_string(), index)))
            .collect();
        let match_index = captures
            .first()
            .and_then(|capture| capture.as_ref())
            .map(|capture| {
                capture.utf16_range.map(|range| range.0).unwrap_or_else(|| {
                    byte_index_to_utf16_index(str_data, capture.byte_start) as f64
                })
            })
            .unwrap_or(0.0);
        Self {
            captures,
            named,
            match_index,
        }
    }

    pub(super) fn from_repeat_matcher(
        str_data: &str,
        regex: &super::repeat_matcher::RepeatMatcherRegex,
        matched: &regress::Match,
        has_indices: bool,
    ) -> Self {
        let captures: Vec<Option<OwnedCapture>> = matched
            .groups()
            .map(|capture| {
                capture.map(|range| {
                    OwnedCapture::from_range_with_indices(
                        str_data,
                        range.start,
                        range.end,
                        has_indices,
                    )
                })
            })
            .collect();
        let named = regex
            .capture_names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| name.as_ref().map(|name| (name.clone(), index + 1)))
            .collect();
        let match_index = byte_index_to_utf16_index(str_data, matched.start()) as f64;
        Self {
            captures,
            named,
            match_index,
        }
    }
}

/// Match-result metadata helper taking the `input` property as an already-boxed
/// string VALUE (typically the rooted original subject) instead of a `&str` to
/// copy. Both the array and the input value are rooted across the internal
/// key-string allocations, which can trigger a (potentially moving) minor GC.
pub(super) fn set_exec_array_metadata_value(arr: *mut ArrayHeader, input_value: f64, index: f64) {
    if arr.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let input_handle = scope.root_nanbox_f64(input_value);
    let index_key = js_string_from_str("index");
    crate::array::js_array_set_string_key(
        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
        index_key,
        index,
    );

    let input_key = js_string_from_str("input");
    crate::array::js_array_set_string_key(
        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
        input_key,
        input_handle.get_nanbox_f64(),
    );
}

/// Combined `index`/`input`/`groups` decoration for a FRESHLY built
/// match-result array (#6386).
///
/// * `input` re-boxes the already-heap-allocated subject `StringHeader`
///   instead of copying the whole subject per match (the string is demoted
///   to shared so a later in-place `s += x` on the source local can't edit
///   the stored property).
/// * all three properties land in the named-props side table with ONE probe
///   and no key-string allocations
///   (`crate::array::array_named_props_install_fresh`).
///
/// Sound only because the array was allocated moments ago in the same
/// helper: it has no descriptors, no freeze/seal state, and no existing
/// named props, so the generic `js_array_set_string_key` ladder is
/// observationally skipped. Performs no GC allocation, so no rooting needed.
pub(super) fn set_exec_array_metadata_groups_fresh(
    arr: *mut ArrayHeader,
    input: *const crate::string::StringHeader,
    index: f64,
    groups_obj: *mut ObjectHeader,
) {
    if arr.is_null() {
        return;
    }
    let input_value = js_nanbox_string(input as i64);
    crate::string::js_string_addref_if_heap_string(input_value);
    let groups_value = if groups_obj.is_null() {
        f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
    } else {
        crate::value::js_nanbox_pointer(groups_obj as i64)
    };
    unsafe {
        crate::array::array_named_props_install_fresh(
            arr,
            &[
                ("index", index),
                ("input", input_value),
                ("groups", groups_value),
            ],
        );
    }
}

fn copy_owned_capture(
    source: &crate::gc::RuntimeHandle<'_>,
    capture: OwnedCapture,
) -> *mut StringHeader {
    source.with_const_ptr::<StringHeader, _>(|source_now| {
        crate::string::string_copy_range(
            source_now,
            capture.byte_start,
            capture.byte_len,
            capture.utf16_len,
            capture.flags,
        )
    })
}

unsafe fn attach_owned_indices(
    result_handle: &crate::gc::RuntimeHandle<'_>,
    data: &OwnedExecMatch,
    scope: &crate::gc::RuntimeHandleScope,
) {
    let indices = crate::array::js_array_alloc(data.captures.len() as u32);
    let indices_handle = scope.root_raw_mut_ptr(indices);
    (*indices_handle.get_raw_mut_ptr::<ArrayHeader>()).length = data.captures.len() as u32;

    for (index, capture) in data.captures.iter().enumerate() {
        let value = if let Some(capture) = capture {
            let pair = crate::array::js_array_alloc(2);
            let pair_handle = scope.root_raw_mut_ptr(pair);
            (*pair_handle.get_raw_mut_ptr::<ArrayHeader>()).length = 2;
            let (start, end) = capture
                .utf16_range
                .expect("hasIndices snapshots every UTF-16 capture range");
            crate::array::store_array_slot(
                pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                0,
                start.to_bits(),
            );
            crate::array::store_array_slot(
                pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                1,
                end.to_bits(),
            );
            crate::value::js_nanbox_pointer(pair_handle.get_raw_mut_ptr::<ArrayHeader>() as i64)
        } else {
            f64::from_bits(TAG_UNDEFINED)
        };
        crate::array::store_array_slot(
            indices_handle.get_raw_mut_ptr::<ArrayHeader>(),
            index,
            value.to_bits(),
        );
    }

    if !data.named.is_empty() {
        let groups = crate::object::js_object_alloc(0, 0);
        let groups_handle = scope.root_raw_mut_ptr(groups);
        for (name, capture_index) in &data.named {
            // ECMAScript exposes the same index-pair object at `indices[i]`
            // and `indices.groups.name`; re-read it from the rooted array.
            let value = crate::array::js_array_get_f64(
                indices_handle.get_raw_mut_ptr::<ArrayHeader>(),
                *capture_index as u32,
            );
            let value_handle = scope.root_nanbox_f64(value);
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            groups_handle.with_mut_ptr::<ObjectHeader, _>(|groups_now| {
                crate::object::js_object_set_field_by_name(
                    groups_now,
                    key,
                    value_handle.get_nanbox_f64(),
                )
            });
        }

        let (groups_key, groups_now) =
            groups_handle.across_mut::<ObjectHeader, _>(|| js_string_from_str("groups"));
        let groups_value = crate::value::js_nanbox_pointer(groups_now as i64);
        crate::array::js_array_set_string_key(
            indices_handle.get_raw_mut_ptr::<ArrayHeader>(),
            groups_key,
            groups_value,
        );
    }

    let (indices_key, indices_now) =
        indices_handle.across_mut::<ArrayHeader, _>(|| js_string_from_str("indices"));
    let indices_value = crate::value::js_nanbox_pointer(indices_now as i64);
    crate::array::js_array_set_string_key(
        result_handle.get_raw_mut_ptr::<ArrayHeader>(),
        indices_key,
        indices_value,
    );
}

/// Phase 2 for `RegExp#exec` and non-global `String#match`: allocate solely
/// from an owned byte-range snapshot. `source` is rooted before the first JS
/// allocation and each capture copy re-reads it after allocating its result.
pub(super) unsafe fn materialize_exec_match(
    source: *const StringHeader,
    data: &OwnedExecMatch,
    has_indices: bool,
) -> (*mut ArrayHeader, *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source_handle = scope.root_string_ptr(source);

    let result = crate::array::js_array_alloc(data.captures.len() as u32);
    let result_handle = scope.root_raw_mut_ptr(result);
    (*result_handle.get_raw_mut_ptr::<ArrayHeader>()).length = data.captures.len() as u32;

    for (index, capture) in data.captures.iter().enumerate() {
        if let Some(capture) = capture {
            let (string, result_now) = result_handle
                .across_mut::<ArrayHeader, _>(|| copy_owned_capture(&source_handle, *capture));
            let value = js_nanbox_string(string as i64);
            crate::array::store_array_slot(result_now, index, value.to_bits());
        } else {
            crate::array::store_array_slot(
                result_handle.get_raw_mut_ptr::<ArrayHeader>(),
                index,
                TAG_UNDEFINED,
            );
        }
    }

    let groups_handle = if data.named.is_empty() {
        None
    } else {
        let groups = crate::object::js_object_alloc(0, 0);
        let groups_handle = scope.root_raw_mut_ptr(groups);
        for (name, capture_index) in &data.named {
            let value = if let Some(capture) = data.captures[*capture_index] {
                js_nanbox_string(copy_owned_capture(&source_handle, capture) as i64)
            } else {
                f64::from_bits(TAG_UNDEFINED)
            };
            let value_handle = scope.root_nanbox_f64(value);
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            groups_handle.with_mut_ptr::<ObjectHeader, _>(|groups_now| {
                crate::object::js_object_set_field_by_name(
                    groups_now,
                    key,
                    value_handle.get_nanbox_f64(),
                )
            });
        }
        Some(groups_handle)
    };

    let groups = groups_handle
        .as_ref()
        .map(|handle| handle.get_raw_mut_ptr::<ObjectHeader>())
        .unwrap_or(std::ptr::null_mut());
    source_handle.with_const_ptr::<StringHeader, _>(|source_now| {
        set_exec_array_metadata_groups_fresh(
            result_handle.get_raw_mut_ptr::<ArrayHeader>(),
            source_now,
            data.match_index,
            groups,
        )
    });

    if has_indices {
        attach_owned_indices(&result_handle, data, &scope);
    }

    let groups = groups_handle
        .as_ref()
        .map(|handle| handle.get_raw_mut_ptr::<ObjectHeader>())
        .unwrap_or(std::ptr::null_mut());
    (result_handle.get_raw_mut_ptr::<ArrayHeader>(), groups)
}

/// Phase 2 for global `String#match`, which returns only the full-match strings.
pub(super) unsafe fn materialize_match_list(
    source: *const StringHeader,
    matches: &[OwnedCapture],
) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source_handle = scope.root_string_ptr(source);
    let result = crate::array::js_array_alloc(matches.len() as u32);
    let result_handle = scope.root_raw_mut_ptr(result);
    (*result_handle.get_raw_mut_ptr::<ArrayHeader>()).length = matches.len() as u32;

    for (index, capture) in matches.iter().enumerate() {
        let (string, result_now) = result_handle
            .across_mut::<ArrayHeader, _>(|| copy_owned_capture(&source_handle, *capture));
        let value = js_nanbox_string(string as i64);
        crate::array::store_array_slot(result_now, index, value.to_bits());
    }
    result_handle.get_raw_mut_ptr::<ArrayHeader>()
}
