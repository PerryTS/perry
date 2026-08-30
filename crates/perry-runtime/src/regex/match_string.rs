use super::*;

use std::ptr;

use crate::array::ArrayHeader;
use crate::string::StringHeader;

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
    // dereferenced by `lookup_fancy_regex`.
    let src: *const StringHeader = if jv.is_undefined() {
        crate::string::js_string_from_str("") as *const StringHeader
    } else {
        crate::builtins::js_string_coerce(arg) as *const StringHeader
    };
    let flags = crate::string::js_string_from_str("") as *const StringHeader;
    js_regexp_new(src, flags)
}

/// `String.prototype.search(regexp)` (ECMA-262 §22.1.3.12) with full argument
/// coercion. `s` is the already-`ToString`-coerced `this`.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_search_value(s: *const StringHeader, arg: f64) -> i32 {
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

enum OwnedStringMatch {
    Global(Vec<OwnedCapture>),
    NonGlobal(OwnedExecMatch, bool),
}

/// Find matches in a string.
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

    // Phase 1 (borrowing, no JS allocation): capture byte ranges and all
    // UTF-16/WTF-8 metadata while the engine's `Captures` may borrow `s`.
    let owned = unsafe {
        let str_data = string_as_str(s);
        let regex = super::lazy::header_std_regex(re);
        let global = (*re).global;
        let has_indices = (*re).has_indices;

        if let Some(repeat_matcher) = lookup_repeat_matcher(re) {
            if global {
                let matches: Vec<OwnedCapture> = repeat_matcher
                    .regex
                    .find_iter(str_data)
                    .map(|matched| {
                        OwnedCapture::from_range(str_data, matched.start(), matched.end())
                    })
                    .collect();
                if matches.is_empty() {
                    return ptr::null_mut();
                }
                OwnedStringMatch::Global(matches)
            } else {
                let Some(matched) = repeat_matcher.regex.find(str_data) else {
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                    return ptr::null_mut();
                };
                OwnedStringMatch::NonGlobal(
                    OwnedExecMatch::from_repeat_matcher(
                        str_data,
                        0,
                        &repeat_matcher,
                        &matched,
                        has_indices,
                    ),
                    has_indices,
                )
            }
        } else if let Some(fre) = lookup_fancy_regex(re) {
            if global {
                let matches: Vec<OwnedCapture> = fre
                    .find_iter(str_data)
                    .filter_map(Result::ok)
                    .map(|m| OwnedCapture::from_range(str_data, m.start(), m.end()))
                    .collect();
                if matches.is_empty() {
                    return ptr::null_mut();
                }
                OwnedStringMatch::Global(matches)
            } else {
                let Ok(Some(caps)) = fre.captures(str_data) else {
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                    return ptr::null_mut();
                };
                OwnedStringMatch::NonGlobal(
                    OwnedExecMatch::from_fancy(str_data, 0, &fre, &caps, has_indices),
                    has_indices,
                )
            }
        } else if global {
            let matches: Vec<OwnedCapture> = regex
                .find_iter(str_data)
                .map(|m| OwnedCapture::from_range(str_data, m.start(), m.end()))
                .collect();
            if matches.is_empty() {
                return ptr::null_mut();
            }
            OwnedStringMatch::Global(matches)
        } else {
            let Some(caps) = regex.captures(str_data) else {
                LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                return ptr::null_mut();
            };
            OwnedStringMatch::NonGlobal(
                OwnedExecMatch::from_standard(str_data, 0, regex, &caps, has_indices),
                has_indices,
            )
        }
    };

    // Phase 2 (allocating, no subject borrow): materializers root the subject
    // before their first runtime allocation and copy only from saved ranges.
    unsafe {
        match owned {
            OwnedStringMatch::Global(matches) => materialize_match_list(s, &matches),
            OwnedStringMatch::NonGlobal(data, has_indices) => {
                let (result, groups) = materialize_exec_match(s, &data, has_indices);
                LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = groups);
                result
            }
        }
    }
}
