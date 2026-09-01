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

        // Spec RegExpBuiltinExec step 12.a: `lastIndex > length` is "no match"
        // outright — NOT a search clamped to the end of the subject. The bound
        // is in UTF-16 code units, the same unit `lastIndex` is stored in;
        // comparing byte offsets can't express it because
        // `utf16_index_to_byte` saturates at `str_data.len()`.
        if use_last_index && last_index > (*s).utf16_len as usize {
            set_last_index_throwing(re, 0);
            LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
            LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
            return ptr::null_mut();
        }

        let search_start_byte = if use_last_index && last_index > 0 {
            super::exec_array::utf16_index_to_byte(str_data, last_index)
        } else {
            0
        };

        // #9429: search FROM `search_start_byte` in the whole subject rather
        // than searching a `&str_data[search_start_byte..]` slice. Every
        // zero-width assertion — `^`, `$`, `\b`, and both lookaround
        // directions — is defined against the real subject, and a slice both
        // invents context at its left edge (`^`/`\b` hold where they must not)
        // and destroys it (`(?<=a)` fails where it must hold). All three
        // engines expose a positional entry point with exactly these
        // semantics, documented as such: `regex::Regex::captures_at`,
        // `fancy_regex::Regex::captures_from_pos` and
        // `regress::Regex::find_from`. Their reported offsets are absolute, so
        // nothing downstream re-bases them.
        let owned = if let Some(repeat_matcher) = lookup_repeat_matcher(re) {
            repeat_matcher
                .regex
                .find_from(str_data, search_start_byte)
                .next()
                .filter(|matched| !sticky || matched.start() == search_start_byte)
                .map(|matched| {
                    if use_last_index {
                        set_last_index_throwing(
                            re,
                            super::exec_array::byte_index_to_utf16_index(str_data, matched.end()),
                        );
                    }
                    OwnedExecMatch::from_repeat_matcher(
                        str_data,
                        &repeat_matcher,
                        &matched,
                        has_indices,
                    )
                })
        } else if let Some(fre) = lookup_fancy_regex(re) {
            match fre.captures_from_pos(str_data, search_start_byte) {
                Ok(Some(caps))
                    if !sticky
                        || caps
                            .get(0)
                            .is_some_and(|full| full.start() == search_start_byte) =>
                {
                    let full = caps.get(0).expect("capture zero is the full match");
                    if use_last_index {
                        set_last_index_throwing(
                            re,
                            super::exec_array::byte_index_to_utf16_index(str_data, full.end()),
                        );
                    }
                    Some(OwnedExecMatch::from_fancy(
                        str_data,
                        &fre,
                        &caps,
                        has_indices,
                    ))
                }
                Ok(Some(_)) | Ok(None) | Err(_) => None,
            }
        } else {
            regex
                .captures_at(str_data, search_start_byte)
                .filter(|caps| {
                    !sticky
                        || caps
                            .get(0)
                            .is_some_and(|full| full.start() == search_start_byte)
                })
                .map(|caps| {
                    let full = caps.get(0).expect("capture zero is the full match");
                    if use_last_index {
                        set_last_index_throwing(
                            re,
                            super::exec_array::byte_index_to_utf16_index(str_data, full.end()),
                        );
                    }
                    OwnedExecMatch::from_standard(str_data, regex, &caps, has_indices)
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
