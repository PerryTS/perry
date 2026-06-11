use super::*;

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

unsafe fn call_string_replace_callback(
    callback: f64,
    matched: &str,
    offset: usize,
    whole: &str,
) -> String {
    let scope = crate::gc::RuntimeHandleScope::new();
    let matched_value = js_nanbox_string(js_string_from_str(matched) as i64);
    let matched_handle = scope.root_nanbox_f64(matched_value);
    let offset_handle = scope.root_nanbox_f64(offset as f64);
    let whole_value = js_nanbox_string(js_string_from_str(whole) as i64);
    let whole_handle = scope.root_nanbox_f64(whole_value);
    let args = [
        matched_handle.get_nanbox_f64(),
        offset_handle.get_nanbox_f64(),
        whole_handle.get_nanbox_f64(),
    ];
    call_replace_callback(callback, &args)
}

/// string.replace(pattern, replacerFn) for a non-regex string pattern.
#[no_mangle]
pub extern "C" fn js_string_replace_string_fn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    callback: f64,
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

    unsafe {
        if pattern_str.is_empty() {
            let replacement = call_string_replace_callback(callback, "", 0, str_data);
            let mut result = String::with_capacity(replacement.len() + str_data.len());
            result.push_str(&replacement);
            result.push_str(str_data);
            return js_string_from_str(&result);
        }

        let Some(byte_idx) = str_data.find(pattern_str) else {
            return js_string_from_str(str_data);
        };
        let char_offset = str_data[..byte_idx].chars().count();
        let replacement =
            call_string_replace_callback(callback, pattern_str, char_offset, str_data);
        let mut result = String::with_capacity(str_data.len() + replacement.len());
        result.push_str(&str_data[..byte_idx]);
        result.push_str(&replacement);
        result.push_str(&str_data[byte_idx + pattern_str.len()..]);
        js_string_from_str(&result)
    }
}

/// string.replaceAll(pattern, replacerFn) for a non-regex string pattern.
#[no_mangle]
pub extern "C" fn js_string_replace_all_string_fn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    callback: f64,
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

    unsafe {
        if pattern_str.is_empty() {
            let mut result = String::new();
            result.push_str(&call_string_replace_callback(callback, "", 0, str_data));
            let mut offset = 0usize;
            for ch in str_data.chars() {
                result.push(ch);
                offset += 1;
                result.push_str(&call_string_replace_callback(
                    callback, "", offset, str_data,
                ));
            }
            return js_string_from_str(&result);
        }

        let mut result = String::new();
        let mut last_end = 0usize;
        for (byte_idx, matched) in str_data.match_indices(pattern_str) {
            result.push_str(&str_data[last_end..byte_idx]);
            let char_offset = str_data[..byte_idx].chars().count();
            result.push_str(&call_string_replace_callback(
                callback,
                matched,
                char_offset,
                str_data,
            ));
            last_end = byte_idx + matched.len();
        }
        if last_end == 0 {
            return js_string_from_str(str_data);
        }
        result.push_str(&str_data[last_end..]);
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
