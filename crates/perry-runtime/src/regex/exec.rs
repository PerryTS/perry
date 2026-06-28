use super::*;

#[cfg(feature = "regex-engine")]
use regex::Regex;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ptr;
#[cfg(feature = "regex-engine")]
use std::sync::Arc;

#[cfg(feature = "regex-engine")]
use crate::array::ArrayHeader;
use crate::string::StringHeader;
#[cfg(feature = "regex-engine")]
use crate::value::js_nanbox_string;

use crate::object::ObjectHeader;

/// regex.exec(string) -> match array (like string.match) with thread-local index/groups
/// For global regexes, starts matching at lastIndex and updates it.
/// Returns *mut ArrayHeader (null for no match). Stores .index and .groups
/// in thread-locals, retrieved via js_regexp_exec_get_index / js_regexp_exec_get_groups.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_exec(
    re: *mut RegExpHeader,
    s: *const StringHeader,
) -> *mut crate::array::ArrayHeader {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    // #854: POINTER_TAG / POINTER_MASK kept co-located with the NaN-box
    // tag contract even when this exec helper only reads TAG_UNDEFINED.
    // Codegen and sibling helpers in regex.rs use the same values.
    #[allow(dead_code)]
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    #[allow(dead_code)]
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    if !is_valid_regex_ptr(re) || !is_valid_ptr(s) {
        LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
        return ptr::null_mut();
    }

    let str_data = string_as_str(s);

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;
        let sticky = (*re).sticky;
        // Per spec RegExpBuiltinExec, `lastIndex` drives the search start for
        // BOTH global and sticky regexes (and lastIndex is reset/updated for
        // either). A sticky match must additionally *anchor* at lastIndex.
        let use_last_index = global || sticky;
        // Spec RegExpBuiltinExec step 4 reads `lastIndex` (Get → ToLength) once,
        // up front and *before* the global/sticky branch (step 8). So the read
        // — and any `valueOf`/`toString` side effect of a coercible lastIndex —
        // is observed exactly once even for a non-global/non-sticky regex
        // (test262 prototype/exec/{success,failure}-lastindex-access).
        let last_index_read = regex_last_index_offset(re);
        // Step 8: a non-global/non-sticky search always starts at 0; the value
        // read above only drives the search start for a stateful regex.
        let last_index = if use_last_index { last_index_read } else { 0 };

        let search_start_byte = if use_last_index && last_index > 0 {
            let mut byte_off = 0;
            let mut char_count = 0;
            for ch in str_data.chars() {
                if char_count >= last_index {
                    break;
                }
                byte_off += ch.len_utf8();
                char_count += 1;
            }
            byte_off
        } else {
            0
        };

        if search_start_byte > str_data.len() {
            if use_last_index {
                set_last_index_throwing(re, 0);
            }
            LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
            LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
            return ptr::null_mut();
        }

        let search_str = &str_data[search_start_byte..];

        // Check if this regex has a fancy-regex fallback (lookbehind/lookahead).
        let fancy_captures = FANCY_CACHE.with(|fc| {
            let fc = fc.borrow();
            let pat = string_as_str((*re).pattern_ptr);
            let flags_str = string_as_str((*re).flags_ptr);
            if let Some(fre) = fc.get(&(pat.to_string(), flags_str.to_string())) {
                if let Ok(Some(caps)) = fre.captures(search_str) {
                    let full = caps.get(0).unwrap();
                    // Sticky (`y`) requires the match to start exactly at
                    // lastIndex — i.e. offset 0 of the sliced search string.
                    if sticky && full.start() != 0 {
                        return Some(ptr::null_mut());
                    }
                    let match_byte_offset = full.start() + search_start_byte;
                    let match_char_offset = str_data[..match_byte_offset].chars().count();
                    let arr = crate::array::js_array_alloc(caps.len() as u32);
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let arr_handle = scope.root_raw_mut_ptr(arr);
                    (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;
                    for i in 0..caps.len() {
                        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                        if let Some(m) = caps.get(i) {
                            let str_ptr = js_string_from_str(m.as_str());
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            // GC_STORE_AUDIT(BARRIERED): regex exec fancy capture slot uses the shared array slot-store helper.
                            crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                        } else {
                            let undefined = f64::from_bits(TAG_UNDEFINED);
                            // GC_STORE_AUDIT(BARRIERED): regex exec fancy unmatched capture slot uses the shared array slot-store helper.
                            crate::array::store_array_slot(arr, i, undefined.to_bits());
                        }
                    }
                    if use_last_index {
                        let match_str = full.as_str();
                        set_last_index_throwing(re, match_char_offset + match_str.chars().count());
                    }
                    set_exec_array_metadata(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        str_data,
                        match_char_offset as f64,
                    );
                    LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = match_char_offset as f64);
                    // Extract named-capture groups through the fancy path so
                    // `/(?<=x)(?<y>\d+)/.exec(s).groups` works for patterns the
                    // `regex` crate can't compile.
                    let groups_obj = build_fancy_groups(fre, &caps, &scope);
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = groups_obj);
                    set_exec_array_groups(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), groups_obj);
                    // Build indices array if `d` flag (hasIndices) is set
                    if (*re).has_indices {
                        set_exec_array_indices_fancy(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            str_data,
                            search_start_byte,
                            fre,
                            &caps,
                        );
                    }
                    return Some(arr_handle.get_raw_mut_ptr::<ArrayHeader>());
                }
                return Some(ptr::null_mut()); // fancy-regex tried but no match
            }
            None // no fancy fallback — use standard regex
        });
        if let Some(result) = fancy_captures {
            if result.is_null() {
                if use_last_index {
                    set_last_index_throwing(re, 0);
                }
                LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
                LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                return ptr::null_mut();
            }
            return result;
        }

        let standard_caps = regex.captures(search_str).filter(|caps| {
            // Sticky (`y`) requires the match to start at lastIndex (offset 0 of
            // the slice); a leftmost match further in does not count.
            !sticky || caps.get(0).map(|m| m.start() == 0).unwrap_or(false)
        });
        match standard_caps {
            Some(caps) => {
                let match_byte_offset = caps.get(0).unwrap().start() + search_start_byte;
                let match_char_offset = str_data[..match_byte_offset].chars().count();

                if use_last_index {
                    let match_end_byte = caps.get(0).unwrap().end() + search_start_byte;
                    let match_end_char = str_data[..match_end_byte].chars().count();
                    set_last_index_throwing(re, match_end_char);
                }

                // Create match array: [fullMatch, group1, group2, ...]
                let arr = crate::array::js_array_alloc(caps.len() as u32);
                let scope = crate::gc::RuntimeHandleScope::new();
                let arr_handle = scope.root_raw_mut_ptr(arr);
                (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;

                for (i, cap) in caps.iter().enumerate() {
                    if let Some(m) = cap {
                        let str_ptr = js_string_from_str(m.as_str());
                        let nanboxed = js_nanbox_string(str_ptr as i64);
                        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                        // GC_STORE_AUDIT(BARRIERED): regex exec capture slot uses the shared array slot-store helper.
                        crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                    } else {
                        let undefined = f64::from_bits(TAG_UNDEFINED);
                        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                        // GC_STORE_AUDIT(BARRIERED): regex exec unmatched capture slot uses the shared array slot-store helper.
                        crate::array::store_array_slot(arr, i, undefined.to_bits());
                    }
                }

                // Store .index in thread-local
                LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = match_char_offset as f64);
                set_exec_array_metadata(
                    arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                    str_data,
                    match_char_offset as f64,
                );

                // Build groups object if named captures exist
                let group_names: Vec<(&str, Option<regex::Match>)> = regex
                    .capture_names()
                    .enumerate()
                    .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
                    .collect();

                if !group_names.is_empty() {
                    // Allocate a fresh per-result object (and shape) via
                    // `js_object_alloc(0, 0)` + by-name setters, NOT a shared
                    // `js_object_alloc_with_shape(const_id)`. A fixed interned
                    // shape id makes a later match with different named captures
                    // inherit the prior call's key names (e.g. `(?<x>…)` then
                    // `(?<z>…)` exposing `.x` on the second result). This mirrors
                    // the fix already applied to the `js_string_match` path.
                    let groups_obj = crate::object::js_object_alloc(0, 0);
                    let groups_handle = scope.root_raw_mut_ptr(groups_obj);
                    for (name, m) in &group_names {
                        let val = if let Some(m) = m {
                            let str_ptr = js_string_from_str(m.as_str());
                            js_nanbox_string(str_ptr as i64)
                        } else {
                            f64::from_bits(TAG_UNDEFINED)
                        };
                        let key_ptr =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        let groups_obj =
                            groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                        crate::object::js_object_set_field_by_name(groups_obj, key_ptr, val);
                    }
                    LAST_EXEC_GROUPS.with(|g| {
                        *g.borrow_mut() =
                            groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>()
                    });
                    set_exec_array_groups(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
                    );
                } else {
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                    set_exec_array_groups(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        ptr::null_mut(),
                    );
                }

                // Build indices array if `d` flag (hasIndices) is set
                if (*re).has_indices {
                    set_exec_array_indices(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        str_data,
                        search_start_byte,
                        &caps,
                        regex,
                    );
                }

                arr_handle.get_raw_mut_ptr::<ArrayHeader>()
            }
            None => {
                if use_last_index {
                    set_last_index_throwing(re, 0);
                }
                LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
                LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                ptr::null_mut()
            }
        }
    }
}
