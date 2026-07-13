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
use crate::value::js_nanbox_string;

use super::js_string_from_str;

pub(super) fn set_exec_array_metadata(arr: *mut ArrayHeader, input: &str, index: f64) {
    if arr.is_null() {
        return;
    }
    let index_key = js_string_from_str("index");
    crate::array::js_array_set_string_key(arr, index_key, index);

    let input_key = js_string_from_str("input");
    let input_str = js_string_from_str(input);
    let input_value = js_nanbox_string(input_str as i64);
    crate::array::js_array_set_string_key(arr, input_key, input_value);
}

/// [`set_exec_array_metadata`] variant taking the `input` property as an
/// already-boxed string VALUE (typically the rooted original subject) instead
/// of a `&str` to copy. Both the array and the input value are rooted across
/// the internal key-string allocations, which can trigger a (potentially
/// moving) minor GC.
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

/// Attach the `groups` own property to a regex match-result array.
///
/// Mirrors `set_exec_array_metadata` for `index`/`input`: the result of
/// `regex.exec(s)` / `s.match(regex)` carries `groups` as a real own property
/// so reads stay correct under aliasing and interleaved matches — a stored
/// `m.groups` survives a later `re2.exec(...)`, instead of resolving through a
/// single most-recent-match thread-local (`LAST_EXEC_GROUPS`). Per ECMA-262
/// RegExpBuiltinExec, `groups` is the named-capture object when the pattern
/// has named groups, else `undefined`.
pub(super) fn set_exec_array_groups(arr: *mut ArrayHeader, groups_obj: *mut ObjectHeader) {
    if arr.is_null() {
        return;
    }
    let groups_key = js_string_from_str("groups");
    let value = if groups_obj.is_null() {
        f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
    } else {
        crate::value::js_nanbox_pointer(groups_obj as i64)
    };
    crate::array::js_array_set_string_key(arr, groups_key, value);
}

/// Attach the `indices` own property to a regex match-result array when the
/// `d` flag (hasIndices) is set. Per ECMA-262 RegExpBuiltinExec, `indices` is
/// an array where each element is `[start, end]` for the corresponding capture
/// group. Element 0 is the full match, elements 1..N are capture groups.
/// Unmatched groups are `undefined`. The `indices` array also has a `.groups`
/// property with named captures mapping to `[start, end]` pairs.
pub(super) fn set_exec_array_indices(
    arr: *mut ArrayHeader,
    str_data: &str,
    search_start_byte: usize,
    caps: &regex::Captures,
    regex: &regex::Regex,
) {
    if arr.is_null() {
        return;
    }

    let scope = crate::gc::RuntimeHandleScope::new();

    // Build the indices array: [[start, end], [start, end], ...]
    let indices_arr = crate::array::js_array_alloc(caps.len() as u32);
    let indices_handle = scope.root_raw_mut_ptr(indices_arr);
    unsafe {
        (*indices_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;
    }

    for (i, cap) in caps.iter().enumerate() {
        let indices_arr_ptr = indices_handle.get_raw_mut_ptr::<ArrayHeader>();
        if let Some(m) = cap {
            // Convert byte offsets to JS string indices (UTF-16 code units),
            // consistent with `.index` / `lastIndex` / `str.length`.
            let start_byte = m.start() + search_start_byte;
            let end_byte = m.end() + search_start_byte;
            let start_char = byte_index_to_utf16_index(str_data, start_byte) as f64;
            let end_char = byte_index_to_utf16_index(str_data, end_byte) as f64;

            // Create [start, end] pair
            let pair = crate::array::js_array_alloc(2);
            let pair_handle = scope.root_raw_mut_ptr(pair);
            unsafe {
                (*pair_handle.get_raw_mut_ptr::<ArrayHeader>()).length = 2;
                crate::array::store_array_slot(
                    pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                    0,
                    start_char.to_bits(),
                );
                crate::array::store_array_slot(
                    pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                    1,
                    end_char.to_bits(),
                );
            }

            let pair_ptr = pair_handle.get_raw_mut_ptr::<ArrayHeader>();
            let nanboxed = crate::value::js_nanbox_pointer(pair_ptr as i64);
            unsafe {
                crate::array::store_array_slot(indices_arr_ptr, i, nanboxed.to_bits());
            }
        } else {
            // Unmatched capture group -> undefined
            let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
            unsafe {
                crate::array::store_array_slot(indices_arr_ptr, i, undefined.to_bits());
            }
        }
    }

    // If there are named groups, attach .groups property to indices array
    let has_named_groups = regex.capture_names().any(|n| n.is_some());
    if has_named_groups {
        let groups_obj = crate::object::js_object_alloc(0, 0);
        let groups_handle = scope.root_raw_mut_ptr(groups_obj);

        for (name, m) in regex
            .capture_names()
            .enumerate()
            .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
        {
            let val = if let Some(m) = m {
                let start_byte = m.start() + search_start_byte;
                let end_byte = m.end() + search_start_byte;
                let start_char = byte_index_to_utf16_index(str_data, start_byte) as f64;
                let end_char = byte_index_to_utf16_index(str_data, end_byte) as f64;

                // Create [start, end] pair for named group
                let pair = crate::array::js_array_alloc(2);
                let pair_handle = scope.root_raw_mut_ptr(pair);
                unsafe {
                    (*pair_handle.get_raw_mut_ptr::<ArrayHeader>()).length = 2;
                    crate::array::store_array_slot(
                        pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        0,
                        start_char.to_bits(),
                    );
                    crate::array::store_array_slot(
                        pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        1,
                        end_char.to_bits(),
                    );
                }
                let pair_ptr = pair_handle.get_raw_mut_ptr::<ArrayHeader>();
                crate::value::js_nanbox_pointer(pair_ptr as i64)
            } else {
                f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
            };

            let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let groups_obj_ptr = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
            crate::object::js_object_set_field_by_name(groups_obj_ptr, key_ptr, val);
        }

        let groups_ptr = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
        let groups_nanboxed = crate::value::js_nanbox_pointer(groups_ptr as i64);
        let indices_key = js_string_from_str("groups");
        crate::array::js_array_set_string_key(
            indices_handle.get_raw_mut_ptr::<ArrayHeader>(),
            indices_key,
            f64::from_bits(groups_nanboxed.to_bits()),
        );
    }

    // Attach indices array to the match result
    let indices_ptr = indices_handle.get_raw_mut_ptr::<ArrayHeader>();
    let indices_nanboxed = crate::value::js_nanbox_pointer(indices_ptr as i64);
    let indices_key = js_string_from_str("indices");
    crate::array::js_array_set_string_key(
        arr,
        indices_key,
        f64::from_bits(indices_nanboxed.to_bits()),
    );
}

/// Build and attach the `indices` property for fancy-regex captures (lookbehind/backreference fallback).
pub(super) unsafe fn set_exec_array_indices_fancy(
    arr: *mut ArrayHeader,
    str_data: &str,
    search_start_byte: usize,
    fre: &fancy_regex::Regex,
    caps: &fancy_regex::Captures,
) {
    if arr.is_null() {
        return;
    }

    let scope = crate::gc::RuntimeHandleScope::new();

    // Build the indices array: [[start, end], [start, end], ...]
    let indices_arr = crate::array::js_array_alloc(caps.len() as u32);
    let indices_handle = scope.root_raw_mut_ptr(indices_arr);
    (*indices_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;

    for i in 0..caps.len() {
        let indices_arr_ptr = indices_handle.get_raw_mut_ptr::<ArrayHeader>();
        if let Some(m) = caps.get(i) {
            let start_byte = m.start() + search_start_byte;
            let end_byte = m.end() + search_start_byte;
            let start_char = byte_index_to_utf16_index(str_data, start_byte) as f64;
            let end_char = byte_index_to_utf16_index(str_data, end_byte) as f64;

            // Create [start, end] pair
            let pair = crate::array::js_array_alloc(2);
            let pair_handle = scope.root_raw_mut_ptr(pair);
            (*pair_handle.get_raw_mut_ptr::<ArrayHeader>()).length = 2;
            crate::array::store_array_slot(
                pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                0,
                start_char.to_bits(),
            );
            crate::array::store_array_slot(
                pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                1,
                end_char.to_bits(),
            );

            let pair_ptr = pair_handle.get_raw_mut_ptr::<ArrayHeader>();
            let nanboxed = crate::value::js_nanbox_pointer(pair_ptr as i64);
            crate::array::store_array_slot(indices_arr_ptr, i, nanboxed.to_bits());
        } else {
            // Unmatched capture group -> undefined
            let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
            crate::array::store_array_slot(indices_arr_ptr, i, undefined.to_bits());
        }
    }

    // If there are named groups, attach .groups property to indices array
    let has_named_groups = fre.capture_names().any(|n| n.is_some());
    if has_named_groups {
        let groups_obj = crate::object::js_object_alloc(0, 0);
        let groups_handle = scope.root_raw_mut_ptr(groups_obj);

        for (name, m) in fre
            .capture_names()
            .enumerate()
            .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
        {
            let val = if let Some(m) = m {
                let start_byte = m.start() + search_start_byte;
                let end_byte = m.end() + search_start_byte;
                let start_char = byte_index_to_utf16_index(str_data, start_byte) as f64;
                let end_char = byte_index_to_utf16_index(str_data, end_byte) as f64;

                // Create [start, end] pair for named group
                let pair = crate::array::js_array_alloc(2);
                let pair_handle = scope.root_raw_mut_ptr(pair);
                (*pair_handle.get_raw_mut_ptr::<ArrayHeader>()).length = 2;
                crate::array::store_array_slot(
                    pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                    0,
                    start_char.to_bits(),
                );
                crate::array::store_array_slot(
                    pair_handle.get_raw_mut_ptr::<ArrayHeader>(),
                    1,
                    end_char.to_bits(),
                );
                let pair_ptr = pair_handle.get_raw_mut_ptr::<ArrayHeader>();
                crate::value::js_nanbox_pointer(pair_ptr as i64)
            } else {
                f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
            };

            let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let groups_obj_ptr = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
            crate::object::js_object_set_field_by_name(groups_obj_ptr, key_ptr, val);
        }

        let groups_ptr = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
        let groups_nanboxed = crate::value::js_nanbox_pointer(groups_ptr as i64);
        let indices_key = js_string_from_str("groups");
        crate::array::js_array_set_string_key(
            indices_handle.get_raw_mut_ptr::<ArrayHeader>(),
            indices_key,
            f64::from_bits(groups_nanboxed.to_bits()),
        );
    }

    // Attach indices array to the match result
    let indices_ptr = indices_handle.get_raw_mut_ptr::<ArrayHeader>();
    let indices_nanboxed = crate::value::js_nanbox_pointer(indices_ptr as i64);
    let indices_key = js_string_from_str("indices");
    crate::array::js_array_set_string_key(
        arr,
        indices_key,
        f64::from_bits(indices_nanboxed.to_bits()),
    );
}
