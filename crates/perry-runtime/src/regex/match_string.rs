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

/// Coerce a `String.prototype.search`/`match` argument into a RegExp
/// (ECMA-262 §22.1.3.12 / §22.1.3.20 → `RegExpCreate`). A RegExp value passes
/// through unchanged; anything else builds a fresh regex whose source pattern
/// is `ToString(arg)` (running user `toString`/`valueOf`, which may throw),
/// with `undefined` mapped to the empty pattern (the `/(?:)/` regex that
/// matches at index 0). Flags default to none.
#[cfg(feature = "regex-engine")]
fn coerce_search_arg_to_regex(arg: f64) -> *const RegExpHeader {
    let jv = crate::value::JSValue::from_bits(arg.to_bits());
    if jv.is_pointer() {
        let p = crate::value::js_nanbox_get_pointer(arg) as *const u8;
        if is_regex_pointer(p) {
            return p as *const RegExpHeader;
        }
    }
    // `undefined` → empty pattern. Build a real empty `StringHeader` (NOT a
    // null pointer): the resulting RegExp header's `pattern_ptr` is later
    // dereferenced by `js_string_match`'s `lookup_fancy_regex`
    // (`string_as_str((*re).pattern_ptr)`), which would SIGSEGV on null.
    let src: *const StringHeader = if jv.is_undefined() {
        crate::string::js_string_from_str("") as *const StringHeader
    } else {
        crate::builtins::js_string_coerce(arg) as *const StringHeader
    };
    // `flags` may be read the same way; pass an empty header rather than null.
    let flags = crate::string::js_string_from_str("") as *const StringHeader;
    js_regexp_new(src, flags)
}

/// `String.prototype.search(regexp)` (ECMA-262 §22.1.3.12) with full argument
/// coercion: a non-RegExp arg is turned into `RegExpCreate(ToString(arg))`
/// (so `"x".search("pat")`, `.search(undefined)`, and `.search({toString})`
/// all work). `s` is the already-`ToString`-coerced `this`.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_search_value(s: *const StringHeader, arg: f64) -> i32 {
    // Root the receiver across the (possibly allocating / GC-triggering)
    // argument coercion so a moving collector can't dangle `s`.
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    let re = coerce_search_arg_to_regex(arg);
    let s = s_handle.get_raw_const_ptr::<StringHeader>();
    js_string_search_regex(s, re)
}

/// `String.prototype.match(regexp)` (ECMA-262 §22.1.3.11) with full argument
/// coercion (see [`js_string_search_value`]). Returns the match array pointer,
/// or null on no match.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_match_value(s: *const StringHeader, arg: f64) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    let re = coerce_search_arg_to_regex(arg);
    let s = s_handle.get_raw_const_ptr::<StringHeader>();
    js_string_match(s, re)
}

/// Find matches in a string
/// string.match(regex) -> string[] | null (returns array pointer, null if no match)
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_match(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut ArrayHeader {
    if !is_valid_ptr(s) || !is_valid_regex_ptr(re) {
        return ptr::null_mut();
    }

    let str_data = string_as_str(s);

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;

        // If this regex couldn't be compiled by the `regex` crate (e.g.
        // backreferences like `(\w)\1*`, used by date-fns' format token
        // regex), `get_or_compile_regex` substituted a never-match
        // `[^\s\S]` placeholder and stashed the real pattern in
        // `FANCY_CACHE`. Route through fancy-regex so `.match()` returns
        // real results instead of always-null.
        if let Some(fre) = lookup_fancy_regex(re) {
            if global {
                // Collect all non-overlapping matches via fancy-regex's
                // find_iter. Mirrors the `regex` crate global path below.
                let mut matches: Vec<String> = Vec::new();
                let mut iter = fre.find_iter(str_data);
                while let Some(Ok(m)) = iter.next() {
                    matches.push(m.as_str().to_string());
                }
                if matches.is_empty() {
                    return ptr::null_mut();
                }
                let arr = crate::array::js_array_alloc(matches.len() as u32);
                let scope = crate::gc::RuntimeHandleScope::new();
                let arr_handle = scope.root_raw_mut_ptr(arr);
                (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = matches.len() as u32;
                for (i, m) in matches.iter().enumerate() {
                    let str_ptr = js_string_from_str(m);
                    let nanboxed = js_nanbox_string(str_ptr as i64);
                    let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                    // GC_STORE_AUDIT(BARRIERED): regex match array slot uses the shared array slot-store helper.
                    crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                }
                return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
            } else {
                // Non-global: first match + capture groups (parallels the
                // standard-regex non-global branch below).
                match fre.captures(str_data) {
                    Ok(Some(caps)) => {
                        let arr = crate::array::js_array_alloc(caps.len() as u32);
                        let scope = crate::gc::RuntimeHandleScope::new();
                        let arr_handle = scope.root_raw_mut_ptr(arr);
                        (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;
                        for i in 0..caps.len() {
                            if let Some(m) = caps.get(i) {
                                let str_ptr = js_string_from_str(m.as_str());
                                let nanboxed = js_nanbox_string(str_ptr as i64);
                                let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                                // GC_STORE_AUDIT(BARRIERED): regex capture array slot uses the shared array slot-store helper.
                                crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                            } else {
                                let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
                                let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                                // GC_STORE_AUDIT(BARRIERED): regex unmatched capture slot uses the shared array slot-store helper.
                                crate::array::store_array_slot(arr, i, undefined.to_bits());
                            }
                        }
                        // Attach .index / .input as real own properties.
                        let match_char_offset = caps
                            .get(0)
                            .map(|m| super::utf16::byte_index_to_utf16_index(str_data, m.start()))
                            .unwrap_or(0);
                        set_exec_array_metadata(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            str_data,
                            match_char_offset as f64,
                        );
                        // Extract named-capture groups through the fancy path
                        // (fancy-regex exposes `capture_names()` just like the
                        // `regex` crate), so `s.match(/(?<=x)(?<y>\d+)/).groups`
                        // works for lookbehind+named patterns.
                        let groups_obj = build_fancy_groups(&fre, &caps, &scope);
                        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = groups_obj);
                        set_exec_array_groups(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            groups_obj,
                        );
                        // Build `indices` if the `d` flag (hasIndices) is set —
                        // non-global `String.prototype.match` delegates to
                        // RegExpExec, so it carries the same `indices` as exec().
                        if (*re).has_indices {
                            set_exec_array_indices_fancy(
                                arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                                str_data,
                                0,
                                &fre,
                                &caps,
                            );
                        }
                        return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                    }
                    _ => {
                        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                        return ptr::null_mut();
                    }
                }
            }
        }

        if global {
            // Global flag: return all matches
            let matches: Vec<&str> = regex.find_iter(str_data).map(|m| m.as_str()).collect();

            if matches.is_empty() {
                return ptr::null_mut();
            }

            // Create array of string pointers
            let arr = crate::array::js_array_alloc(matches.len() as u32);
            let scope = crate::gc::RuntimeHandleScope::new();
            let arr_handle = scope.root_raw_mut_ptr(arr);
            (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = matches.len() as u32;

            for (i, m) in matches.iter().enumerate() {
                let str_ptr = js_string_from_str(m);
                let nanboxed = js_nanbox_string(str_ptr as i64);
                let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                // GC_STORE_AUDIT(BARRIERED): regex global match array slot uses the shared array slot-store helper.
                crate::array::store_array_slot(arr, i, nanboxed.to_bits());
            }

            arr_handle.get_raw_mut_ptr::<ArrayHeader>()
        } else {
            // Non-global: return first match only (or with capture groups)
            match regex.captures(str_data) {
                Some(caps) => {
                    // Return array with full match and capture groups
                    let arr = crate::array::js_array_alloc(caps.len() as u32);
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let arr_handle = scope.root_raw_mut_ptr(arr);
                    (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;

                    for (i, cap) in caps.iter().enumerate() {
                        if let Some(m) = cap {
                            let str_ptr = js_string_from_str(m.as_str());
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                            // GC_STORE_AUDIT(INIT): fresh match-array slot; layout is
                            // noted per store and the exact layout/barrier rebuild
                            // below the loop covers a mid-loop tenuring (#6386).
                            crate::array::note_array_slot_layout_only(arr, i, nanboxed.to_bits());
                        } else {
                            // Undefined capture group - store as undefined (TAG_UNDEFINED = 0x7FFC_0000_0000_0001)
                            let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
                            let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                            // GC_STORE_AUDIT(INIT): fresh match-array slot; see above.
                            crate::array::note_array_slot_layout_only(arr, i, undefined.to_bits());
                        }
                    }
                    // GC_STORE_AUDIT(BARRIERED): one exact rebuild replays any
                    // old-gen barriers for the whole capture prefix.
                    crate::array::rebuild_array_layout_exact(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                    );

                    // Attach .index / .input as real own properties (mirrors
                    // js_regexp_exec) so they survive aliasing and a later match
                    // on another regex, instead of a most-recent-match thread-local.
                    // Deferred into the combined fresh-array decoration below
                    // (#6386) so index/input/groups cost one side-table probe
                    // and the subject string is re-boxed, not copied.
                    let match_char_offset = caps
                        .get(0)
                        .map(|m| super::utf16::byte_index_to_utf16_index(str_data, m.start()))
                        .unwrap_or(0);

                    // Build groups object for named captures (same shape as
                    // `regex.exec(str)` does in `js_regexp_exec`). Stored in
                    // `LAST_EXEC_GROUPS` thread-local so the HIR fold for
                    // `result.groups` (extended in lower.rs::is_regex_exec_init
                    // to also recognize `str.match(regex)` results) reads it
                    // via the existing `Expr::RegExpExecGroups` codegen path.
                    // Same caveats as exec()'s thread-local: only the most
                    // recent match's groups are stashed, so `m1.groups` after
                    // an intervening `m2 = ...match(...)` reads m2's groups —
                    // acceptable for the common inline `m.groups.x` pattern.
                    let group_names: Vec<(&str, Option<regex::Match>)> = regex
                        .capture_names()
                        .enumerate()
                        .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
                        .collect();
                    if !group_names.is_empty() {
                        // Use the by-name setter (and a plain `js_object_alloc`)
                        // so each match's groups object grows its own shape from
                        // its own keys. Pre-fix this took the
                        // `js_object_alloc_with_shape(shape_id=const, ...)` path
                        // — every match's groups object collapsed to the same
                        // interned shape, so a later match with different named
                        // captures inherited the prior call's key names (e.g.
                        // `.match(/(?<year>...)/)` followed by
                        // `.match(/(?<id>...)/)` made the second result expose
                        // `.year` instead of `.id`).
                        let groups_obj = crate::object::js_object_alloc(0, 0);
                        let groups_handle = scope.root_raw_mut_ptr(groups_obj);
                        for (name, m) in &group_names {
                            let val = if let Some(m) = m {
                                let str_ptr = js_string_from_str(m.as_str());
                                js_nanbox_string(str_ptr as i64)
                            } else {
                                f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
                            };
                            let key_ptr = crate::string::js_string_from_bytes(
                                name.as_ptr(),
                                name.len() as u32,
                            );
                            let groups_obj =
                                groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                            crate::object::js_object_set_field_by_name(groups_obj, key_ptr, val);
                        }
                        LAST_EXEC_GROUPS.with(|g| {
                            *g.borrow_mut() =
                                groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>()
                        });
                        super::exec_array::set_exec_array_metadata_groups_fresh(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            s,
                            match_char_offset as f64,
                            groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
                        );
                    } else {
                        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                        super::exec_array::set_exec_array_metadata_groups_fresh(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            s,
                            match_char_offset as f64,
                            ptr::null_mut(),
                        );
                    }

                    // Build `indices` if the `d` flag (hasIndices) is set —
                    // non-global `String.prototype.match` delegates to
                    // RegExpExec, so it carries the same `indices` as exec().
                    if (*re).has_indices {
                        set_exec_array_indices(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            str_data,
                            0,
                            &caps,
                            regex,
                        );
                    }

                    arr_handle.get_raw_mut_ptr::<ArrayHeader>()
                }
                None => {
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                    ptr::null_mut()
                }
            }
        }
    }
}
