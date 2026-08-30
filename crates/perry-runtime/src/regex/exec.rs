use super::*;

use std::ptr;

use crate::string::StringHeader;

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
    if !is_valid_regex_ptr(re) || !is_valid_ptr(s) {
        LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
        return ptr::null_mut();
    }

    // Spec RegExpBuiltinExec step 4 is `ToLength(Get(R, "lastIndex"))`, and it
    // runs before anything else. The ToNumber half may execute user JS, so root
    // both arguments and take the subject payload borrow only after it returns
    // (#8428 / #8446).
    let scope = crate::gc::RuntimeHandleScope::new();
    let re_handle = scope.root_raw_mut_ptr(re);
    let s_handle = scope.root_string_ptr(s);
    let ((last_index_read, re), s) = s_handle.across_const::<StringHeader, _>(|| {
        re_handle
            .across_mut::<RegExpHeader, _>(|| re_handle.with_const_ptr(regex_last_index_offset))
    });

    // Phase 1 (borrowing, no JS allocation): run the engine and snapshot every
    // subject-derived value into byte ranges/scalars. A rooted pointer slot can
    // be rewritten by a moving GC; `&str`, `Match`, and `Captures` cannot. None
    // of them may reach Phase 2 (#8449).
    let (owned, has_indices) = unsafe {
        let str_data = string_as_str(s);
        let regex = super::lazy::header_std_regex(re);
        let global = (*re).global;
        let sticky = (*re).sticky;
        let has_indices = (*re).has_indices;
        let use_last_index = global || sticky;
        let last_index = if use_last_index { last_index_read } else { 0 };
        let search_start_byte = if use_last_index && last_index > 0 {
            super::exec_array::utf16_index_to_byte(str_data, last_index)
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

        let owned = if let Some(repeat_matcher) = lookup_repeat_matcher(re) {
            repeat_matcher
                .regex
                .find(search_str)
                .filter(|matched| !sticky || matched.start() == 0)
                .map(|matched| {
                    if use_last_index {
                        set_last_index_throwing(
                            re,
                            super::exec_array::byte_index_to_utf16_index(
                                str_data,
                                search_start_byte + matched.end(),
                            ),
                        );
                    }
                    OwnedExecMatch::from_repeat_matcher(
                        str_data,
                        search_start_byte,
                        &repeat_matcher,
                        &matched,
                        has_indices,
                    )
                })
        } else if let Some(fre) = lookup_fancy_regex(re) {
            match fre.captures(search_str) {
                Ok(Some(caps)) if !sticky || caps.get(0).is_some_and(|full| full.start() == 0) => {
                    let full = caps.get(0).expect("capture zero is the full match");
                    if use_last_index {
                        set_last_index_throwing(
                            re,
                            super::exec_array::byte_index_to_utf16_index(
                                str_data,
                                search_start_byte + full.end(),
                            ),
                        );
                    }
                    Some(OwnedExecMatch::from_fancy(
                        str_data,
                        search_start_byte,
                        &fre,
                        &caps,
                        has_indices,
                    ))
                }
                Ok(Some(_)) | Ok(None) | Err(_) => None,
            }
        } else {
            regex
                .captures(search_str)
                .filter(|caps| !sticky || caps.get(0).is_some_and(|full| full.start() == 0))
                .map(|caps| {
                    let full = caps.get(0).expect("capture zero is the full match");
                    if use_last_index {
                        set_last_index_throwing(
                            re,
                            super::exec_array::byte_index_to_utf16_index(
                                str_data,
                                search_start_byte + full.end(),
                            ),
                        );
                    }
                    OwnedExecMatch::from_standard(
                        str_data,
                        search_start_byte,
                        regex,
                        &caps,
                        has_indices,
                    )
                })
        };

        let Some(owned) = owned else {
            if use_last_index {
                set_last_index_throwing(re, 0);
            }
            LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
            LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
            return ptr::null_mut();
        };
        (owned, has_indices)
    };

    // Phase 2 (allocating, no subject borrow): copy each snapshotted range from
    // the current rooted subject address. `string_copy_range` roots and re-reads
    // the source after its destination allocation.
    let (result, groups) = s_handle.with_const_ptr::<StringHeader, _>(|source_now| unsafe {
        materialize_exec_match(source_now, &owned, has_indices)
    });
    LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = owned.match_index);
    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = groups);
    result
}
