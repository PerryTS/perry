use super::*;
// `js_nanbox_string` is re-exported from the parent only when the regex engine
// is on (it's part of the gated engine `use` cluster). The string-replacement
// helpers below are always compiled, so import it directly here.
use crate::value::js_nanbox_string;

pub(super) unsafe fn call_replace_callback(callback: f64, args: &[f64]) -> String {
    let prev = crate::object::js_implicit_this_set(f64::from_bits(crate::value::TAG_UNDEFINED));
    let ret = crate::closure::js_native_call_value(callback, args.as_ptr(), args.len());
    crate::object::js_implicit_this_set(prev);
    let ptr = crate::value::js_get_string_pointer_unified(ret) as *const StringHeader;
    if is_valid_ptr(ptr) {
        string_as_str(ptr).to_string()
    } else {
        String::new()
    }
}

/// Invoke a string-pattern replacer callback with `(matched, offset, whole)`.
///
/// `matched` must be an OWNED (or static) Rust string — never a slice of a GC
/// heap string, because boxing it below allocates and an alloc-point minor
/// can be MOVING under the evacuation policy. The subject travels as a rooted
/// handle and is NaN-boxed directly (same string value, CURRENT address) —
/// no per-call copy, and no borrow of the heap string crosses user code.
unsafe fn call_string_replace_callback(
    callback: f64,
    matched: &str,
    offset: usize,
    whole_handle: &crate::gc::RuntimeHandle<'_>,
) -> String {
    let scope = crate::gc::RuntimeHandleScope::new();
    let matched_value = js_nanbox_string(js_string_from_str(matched) as i64);
    let matched_handle = scope.root_nanbox_f64(matched_value);
    let args = [
        matched_handle.get_nanbox_f64(),
        offset as f64,
        js_nanbox_string(whole_handle.get_raw_const_ptr::<StringHeader>() as i64),
    ];
    call_replace_callback(callback, &args)
}

/// string.replace(pattern, replacerFn) for a non-regex string pattern.
///
/// GC discipline (2026-07-09 audit, wave 1): the replacer callback is user
/// code — it can allocate and trigger a minor GC that sweeps the subject
/// (bare Rust locals are invisible without a conservative stack scan) or
/// moves it. The subject is rooted in a `RuntimeHandleScope` and the `&str`
/// view is re-derived from the rooted handle after every callback; the
/// pattern is copied to an owned Rust string up front.
#[no_mangle]
pub extern "C" fn js_string_replace_string_fn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    callback: f64,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    // Root the callback too: a GC it triggers can relocate its own closure
    // header, and the raw `f64` parameter would keep the pre-move address.
    let callback_handle = scope.root_nanbox_f64(callback);
    let cur_str = || string_as_str(s_handle.get_raw_const_ptr::<StringHeader>());
    let pattern_str: String = if is_valid_ptr(pattern) {
        string_as_str(pattern).to_string()
    } else {
        String::new()
    };

    unsafe {
        if pattern_str.is_empty() {
            let replacement =
                call_string_replace_callback(callback_handle.get_nanbox_f64(), "", 0, &s_handle);
            let str_data = cur_str();
            let mut result = String::with_capacity(replacement.len() + str_data.len());
            result.push_str(&replacement);
            result.push_str(str_data);
            return js_string_from_str(&result);
        }

        let str_data = cur_str();
        let Some(byte_idx) = str_data.find(pattern_str.as_str()) else {
            return js_string_from_str(str_data);
        };
        let char_offset = str_data[..byte_idx].chars().count();
        let replacement = call_string_replace_callback(
            callback_handle.get_nanbox_f64(),
            &pattern_str,
            char_offset,
            &s_handle,
        );
        // Re-derive the subject: the callback may have moved it.
        let str_data = cur_str();
        let mut result = String::with_capacity(str_data.len() + replacement.len());
        result.push_str(&str_data[..byte_idx]);
        result.push_str(&replacement);
        result.push_str(&str_data[byte_idx + pattern_str.len()..]);
        js_string_from_str(&result)
    }
}

/// string.replaceAll(pattern, replacerFn) for a non-regex string pattern.
///
/// Same GC discipline as [`js_string_replace_string_fn`], plus: match
/// positions are computed into an owned Vec BEFORE the first callback — the
/// old code kept a lazy `match_indices` iterator (a live borrow of the heap
/// string) running WHILE callbacks executed between its steps.
#[no_mangle]
pub extern "C" fn js_string_replace_all_string_fn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    callback: f64,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    // Root the callback too: a GC it triggers can relocate its own closure
    // header, and the raw `f64` parameter would keep the pre-move address
    // for every call after the first.
    let callback_handle = scope.root_nanbox_f64(callback);
    let cur_str = || string_as_str(s_handle.get_raw_const_ptr::<StringHeader>());
    let pattern_str: String = if is_valid_ptr(pattern) {
        string_as_str(pattern).to_string()
    } else {
        String::new()
    };

    unsafe {
        if pattern_str.is_empty() {
            // Owned char snapshot: the old code iterated `str_data.chars()`
            // while the callback ran between steps — a stale borrow across
            // user code.
            let chars: Vec<char> = cur_str().chars().collect();
            let mut result = String::new();
            result.push_str(&call_string_replace_callback(
                callback_handle.get_nanbox_f64(),
                "",
                0,
                &s_handle,
            ));
            let mut offset = 0usize;
            for ch in chars {
                result.push(ch);
                offset += 1;
                result.push_str(&call_string_replace_callback(
                    callback_handle.get_nanbox_f64(),
                    "",
                    offset,
                    &s_handle,
                ));
            }
            return js_string_from_str(&result);
        }

        // Precompute every match position (byte index + char offset) before
        // the first callback runs.
        let matches: Vec<(usize, usize)> = {
            let str_data = cur_str();
            let mut char_pos = 0usize;
            let mut last_byte = 0usize;
            str_data
                .match_indices(pattern_str.as_str())
                .map(|(byte_idx, _)| {
                    char_pos += str_data[last_byte..byte_idx].chars().count();
                    last_byte = byte_idx;
                    (byte_idx, char_pos)
                })
                .collect()
        };
        if matches.is_empty() {
            return js_string_from_str(cur_str());
        }
        let mut result = String::new();
        let mut last_end = 0usize;
        for (byte_idx, char_offset) in matches {
            // Between-match text is re-sliced from the CURRENT subject
            // address (the previous callback may have moved it).
            result.push_str(&cur_str()[last_end..byte_idx]);
            result.push_str(&call_string_replace_callback(
                callback_handle.get_nanbox_f64(),
                &pattern_str,
                char_offset,
                &s_handle,
            ));
            last_end = byte_idx + pattern_str.len();
        }
        result.push_str(&cur_str()[last_end..]);
        js_string_from_str(&result)
    }
}

/// Expand a replacement template against a single string-pattern match, per
/// ECMAScript `GetSubstitution` (22.1.3.19.1) for a *string* `searchValue`:
/// `$$` → `$`, `$&` → matched, `` $` `` → text before the match, `$'` → text
/// after it. There are no capture groups for a string pattern, so `$n` /
/// `$<name>` are left verbatim. A `$` not starting a recognised escape is also
/// left verbatim.
fn expand_string_pattern_replacement(
    repl: &str,
    full: &str,
    match_start: usize,
    matched: &str,
) -> String {
    let mut out = String::with_capacity(repl.len());
    let mut chars = repl.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                out.push('$');
                chars.next();
            }
            Some('&') => {
                out.push_str(matched);
                chars.next();
            }
            Some('`') => {
                out.push_str(&full[..match_start]);
                chars.next();
            }
            Some('\'') => {
                out.push_str(&full[match_start + matched.len()..]);
                chars.next();
            }
            // `$` followed by anything else (incl. a digit, since a string
            // pattern has no captures) stays literal.
            _ => out.push('$'),
        }
    }
    out
}

/// Replace with a simple string pattern (not regex)
/// string.replace(pattern, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_string(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let pattern_str = if is_valid_ptr(pattern) {
        string_as_str(pattern)
    } else {
        ""
    };
    let repl_str = if is_valid_ptr(replacement) {
        string_as_str(replacement)
    } else {
        "undefined"
    };

    // String.replace with a string pattern only replaces the first occurrence.
    // Fast path: a replacement with no `$` needs no substitution.
    if !repl_str.contains('$') || pattern_str.is_empty() {
        let result = str_data.replacen(pattern_str, repl_str, 1);
        return js_string_from_str(&result);
    }
    let result = match str_data.find(pattern_str) {
        Some(pos) => {
            let expanded = expand_string_pattern_replacement(repl_str, str_data, pos, pattern_str);
            let mut out = String::with_capacity(str_data.len() + expanded.len());
            out.push_str(&str_data[..pos]);
            out.push_str(&expanded);
            out.push_str(&str_data[pos + pattern_str.len()..]);
            out
        }
        None => str_data.to_string(),
    };
    js_string_from_str(&result)
}

/// Replace ALL occurrences with a simple string pattern (not regex)
/// string.replaceAll(pattern, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_all_string(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let pattern_str = if is_valid_ptr(pattern) {
        string_as_str(pattern)
    } else {
        ""
    };
    let repl_str = if is_valid_ptr(replacement) {
        string_as_str(replacement)
    } else {
        "undefined"
    };

    // Fast path: a replacement with no `$` (or an empty pattern, whose
    // between-every-char match positions are left to Rust's `replace`) needs
    // no `$$`/`$&`/`` $` ``/`$'` substitution.
    if !repl_str.contains('$') || pattern_str.is_empty() {
        let result = str_data.replace(pattern_str, repl_str);
        return js_string_from_str(&result);
    }
    let mut result = String::with_capacity(str_data.len());
    let mut last = 0;
    for (pos, m) in str_data.match_indices(pattern_str) {
        result.push_str(&str_data[last..pos]);
        result.push_str(&expand_string_pattern_replacement(
            repl_str, str_data, pos, m,
        ));
        last = pos + m.len();
    }
    result.push_str(&str_data[last..]);
    js_string_from_str(&result)
}

/// `replaceValue` whose function-ness is only knowable at RUNTIME (a closure
/// returned from an IIFE / call / property read — codegen's static
/// `repl_is_function` detection can't see it). Route to the callback variant
/// when the value is callable, else ToString-coerce and take the plain
/// string-replacement path — pre-fix the coercion stringified the closure
/// source into the result (test262 10.4.3-1-102-s, react-family replacer
/// callbacks).
fn replacement_is_callable(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return false;
    }
    crate::closure::is_closure_ptr((bits & crate::value::POINTER_MASK) as usize)
}

#[no_mangle]
pub extern "C" fn js_string_replace_string_dyn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: f64,
) -> *mut StringHeader {
    if replacement_is_callable(replacement) {
        return js_string_replace_string_fn(s, pattern, replacement);
    }
    js_string_replace_string(s, pattern, crate::builtins::js_string_coerce(replacement))
}

#[no_mangle]
pub extern "C" fn js_string_replace_all_string_dyn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: f64,
) -> *mut StringHeader {
    if replacement_is_callable(replacement) {
        return js_string_replace_all_string_fn(s, pattern, replacement);
    }
    js_string_replace_all_string(s, pattern, crate::builtins::js_string_coerce(replacement))
}

/// Resolve a runtime-dynamic `searchValue` (an object-property read, call
/// result, destructured loop binding, …) to a registered RegExp pointer, or
/// `None` when the value isn't a RegExp.
#[cfg(feature = "regex-engine")]
fn needle_regex_ptr(needle: f64) -> Option<*const crate::regex::RegExpHeader> {
    let bits = needle.to_bits();
    let top16 = bits >> 48;
    let addr = if top16 == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 {
        // Module-level slots store heap pointers as raw I64 bits.
        bits as usize
    } else {
        return None;
    };
    if crate::regex::is_regex_pointer(addr as *const u8) {
        Some(addr as *const crate::regex::RegExpHeader)
    } else {
        None
    }
}

/// `searchValue` whose RegExp-ness is only knowable at RUNTIME (#4871):
/// codegen's static detection covers RegExp literals and RegExp-typed locals,
/// but a RegExp read back from an object property (or destructured in a
/// `for...of`) arrives as an opaque NaN-boxed value. Pre-fix it was
/// ToString-coerced to "/foo/g" and searched literally — replace silently
/// became a no-op. Dispatch on the registered-RegExp check, then defer to the
/// replacement-shape dispatchers.
#[no_mangle]
pub extern "C" fn js_string_replace_search_dyn(
    s: *const StringHeader,
    needle: f64,
    replacement: f64,
) -> *mut StringHeader {
    #[cfg(feature = "regex-engine")]
    if let Some(re) = needle_regex_ptr(needle) {
        return js_string_replace_regex_dyn(s, re, replacement);
    }
    js_string_replace_string_dyn(s, crate::builtins::js_string_coerce(needle), replacement)
}

/// `replaceAll` twin of [`js_string_replace_search_dyn`].
#[no_mangle]
pub extern "C" fn js_string_replace_all_search_dyn(
    s: *const StringHeader,
    needle: f64,
    replacement: f64,
) -> *mut StringHeader {
    #[cfg(feature = "regex-engine")]
    if let Some(re) = needle_regex_ptr(needle) {
        return js_string_replace_all_regex_dyn(s, re, replacement);
    }
    js_string_replace_all_string_dyn(s, crate::builtins::js_string_coerce(needle), replacement)
}

#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_replace_regex_dyn(
    s: *const StringHeader,
    re: *const crate::regex::RegExpHeader,
    replacement: f64,
) -> *mut StringHeader {
    if replacement_is_callable(replacement) {
        return crate::regex::js_string_replace_regex_fn(s, re, replacement);
    }
    // The `_named` variant handles both `$1` and `$<name>` expansion.
    crate::regex::js_string_replace_regex_named(
        s,
        re,
        crate::builtins::js_string_coerce(replacement),
    )
}

#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_replace_all_regex_dyn(
    s: *const StringHeader,
    re: *const crate::regex::RegExpHeader,
    replacement: f64,
) -> *mut StringHeader {
    if replacement_is_callable(replacement) {
        return crate::regex::js_string_replace_all_regex_fn(s, re, replacement);
    }
    crate::regex::js_string_replace_all_regex_named(
        s,
        re,
        crate::builtins::js_string_coerce(replacement),
    )
}

#[cfg(all(test, feature = "regex-engine"))]
mod tests {
    use super::*;

    /// #4871: a RegExp arriving as an opaque NaN-boxed value (object-property
    /// read) must dispatch to the regex path, not be ToString-coerced into a
    /// literal "/foo/g" search.
    #[test]
    fn search_dyn_dispatches_runtime_regex_and_coerces_non_regex() {
        let s = js_string_from_str("foofoo");
        let pat = js_string_from_str("foo");
        let flags = js_string_from_str("g");
        let re = crate::regex::js_regexp_new(pat, flags);
        let re_boxed = f64::from_bits(0x7FFD_0000_0000_0000u64 | (re as u64 & 0xFFFF_FFFF_FFFF));
        let repl = js_nanbox_string(js_string_from_str("X") as i64);

        // /foo/g: the g flag makes .replace substitute every match.
        let out = js_string_replace_search_dyn(s, re_boxed, repl);
        assert_eq!(string_as_str(out), "XX");

        let out_all = js_string_replace_all_search_dyn(s, re_boxed, repl);
        assert_eq!(string_as_str(out_all), "XX");

        // Non-regex needle: ToString-coerce and search literally.
        let needle_num = 12.0_f64;
        let s2 = js_string_from_str("a12b");
        let out2 = js_string_replace_search_dyn(s2, needle_num, repl);
        assert_eq!(string_as_str(out2), "aXb");
    }
}
