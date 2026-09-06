//! `String.prototype.replace` substitution expansion for the fancy-regex path.
//!
//! Split out of `regex.rs` to keep that file under the 2000-line size gate.

use super::*;

pub(super) fn expand_js_replacement_fancy(
    repl: &str,
    caps: &fancy_regex::Captures,
    subject: &str,
    has_named_groups: bool,
) -> String {
    let m0 = match caps.get(0) {
        Some(m) => m,
        None => return String::new(),
    };
    let (mstart, mend) = (m0.start(), m0.end());
    let ngroups = caps.len();
    let b = repl.as_bytes();
    let mut out = String::with_capacity(repl.len() + 16);
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' {
            let start = i;
            while i < b.len() && b[i] != b'$' {
                i += 1;
            }
            out.push_str(&repl[start..i]);
            continue;
        }
        if i + 1 >= b.len() {
            out.push('$');
            i += 1;
            continue;
        }
        match b[i + 1] {
            b'$' => {
                out.push('$');
                i += 2;
            }
            b'&' => {
                out.push_str(&subject[mstart..mend]);
                i += 2;
            }
            b'`' => {
                out.push_str(&subject[..mstart]);
                i += 2;
            }
            b'\'' => {
                out.push_str(&subject[mend..]);
                i += 2;
            }
            b'0'..=b'9' => {
                let d1 = (b[i + 1] - b'0') as usize;
                let (group, consumed) = if i + 2 < b.len() && b[i + 2].is_ascii_digit() {
                    let two = d1 * 10 + (b[i + 2] - b'0') as usize;
                    if two >= 1 && two < ngroups {
                        (Some(two), 2)
                    } else if d1 >= 1 && d1 < ngroups {
                        (Some(d1), 1)
                    } else {
                        (None, 0)
                    }
                } else if d1 >= 1 && d1 < ngroups {
                    (Some(d1), 1)
                } else {
                    (None, 0)
                };
                match group {
                    Some(g) => {
                        if let Some(m) = caps.get(g) {
                            out.push_str(m.as_str());
                        }
                        i += 1 + consumed;
                    }
                    None => {
                        out.push('$');
                        i += 1;
                    }
                }
            }
            b'<' if has_named_groups => {
                if let Some(rel) = repl[i + 2..].find('>') {
                    let name = &repl[i + 2..i + 2 + rel];
                    if let Some(m) = caps.name(name) {
                        out.push_str(m.as_str());
                    }
                    i += 2 + rel + 1;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

/// Fancy-regex fallback for the string-replacement (non-callback) forms of
/// `String.prototype.replace`/`replaceAll`. Drives a manual non-overlapping
/// match loop with `fancy_regex` and expands the replacement string via
/// [`expand_js_replacement_fancy`]. Used when the pattern needs
/// lookbehind/backreferences the `regex` crate can't compile.
#[cfg(feature = "regex-engine")]
pub(super) unsafe fn replace_regex_str_fancy(
    str_data: &str,
    fre: &fancy_regex::Regex,
    global: bool,
    repl_str: &str,
) -> *mut StringHeader {
    let has_named_groups = fre.capture_names().any(|n| n.is_some());
    // #9430: the ECMAScript scan for the global form. fancy-regex's own
    // iterator drops a zero-width match that lands where the previous match
    // ended, so `"a".replace(/(?<=x)?a*/g, …)`-shaped patterns lost their
    // trailing (and every interior) empty replacement.
    let captures_list: Vec<fancy_regex::Captures> = if global {
        global_scan::fancy_captures(fre, str_data, 0)
    } else {
        match fre.captures(str_data) {
            Ok(Some(caps)) => vec![caps],
            Ok(None) | Err(_) => Vec::new(),
        }
    };
    let mut result = String::new();
    let mut last_end = 0usize;
    for caps in &captures_list {
        let full_match = caps.get(0).unwrap();
        result.push_str(&str_data[last_end..full_match.start()]);
        result.push_str(&expand_js_replacement_fancy(
            repl_str,
            caps,
            str_data,
            has_named_groups,
        ));
        last_end = full_match.end();
    }
    result.push_str(&str_data[last_end..]);
    finish_replace_bytes(result.as_bytes())
}

/// string.replace(regex, replacement) -> string
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_replace_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    if !is_valid_regex_ptr(re) {
        // If regex is null, return original string
        return copy_replace_source(s);
    }
    if crate::hot_diag::regex_on() {
        diag_note_op(re, crate::hot_diag::RegexOp::Replace);
    }

    unsafe {
        // The Rust string engines require scalar-value UTF-8, while Perry
        // stores lone JavaScript surrogates as WTF-8. Match those subjects as
        // UTF-16 code units with the ECMAScript engine and rebuild the result
        // through the WTF-8-aware string builder.
        if (*s).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES != 0 {
            let replacement_bytes = if is_valid_ptr(replacement) {
                string_as_bytes(replacement)
            } else {
                b"undefined"
            };
            if let Some(result) = repeat_matcher::replace_wtf8_subject(
                re,
                string_as_bytes(s),
                replacement_bytes,
                (*re).global,
            ) {
                return finish_replace_bytes(&result);
            }
        }

        let str_data = string_as_str(s);
        let repl_str = if is_valid_ptr(replacement) {
            string_as_str(replacement)
        } else {
            "undefined"
        };

        if let Some(repeat_matcher) = lookup_repeat_matcher_for(re, str_data, 0) {
            let result = repeat_matcher.replace(str_data, repl_str, (*re).global);
            return finish_replace_bytes(result.as_bytes());
        }

        // Pattern the `regex` crate couldn't compile (lookbehind/backreferences)
        // → drive the replacement through fancy-regex. Otherwise the never-match
        // placeholder standard program would leave the input unchanged.
        if let Some(fre) = lookup_fancy_regex(re) {
            return replace_regex_str_fancy(str_data, &fre, (*re).global, repl_str);
        }

        let regex = lazy::header_std_regex(re);
        let global = (*re).global;
        let has_named_groups = regex.capture_names().any(|n| n.is_some());

        // Route through a JS-aware expander (closure form) so `$&` / `` $` `` /
        // `$'` — which the regex crate's native `$` syntax doesn't support —
        // are substituted per match. `$$`, `$n`, and `$<name>` are handled too.
        // #9430: `Regex::replace_all` runs the crate's own match iterator,
        // whose empty-match rule is not ECMAScript's. Drive the ECMAScript
        // scan and splice the replacements here instead; the non-global form
        // is the same loop over a one-element list.
        let captures_list: Vec<regex::Captures> = if global {
            global_scan::std_captures(regex, str_data, 0)
        } else {
            regex.captures(str_data).into_iter().collect()
        };
        if crate::hot_diag::regex_on() {
            let n = captures_list.len() as u64;
            crate::hot_diag::regex_with(|d| d.replace_matches += n);
        }
        let mut result = String::with_capacity(str_data.len());
        let mut last_end = 0usize;
        for caps in &captures_list {
            let full = caps.get(0).expect("capture zero is the full match");
            result.push_str(&str_data[last_end..full.start()]);
            result.push_str(&expand_js_replacement(
                repl_str,
                caps,
                str_data,
                has_named_groups,
            ));
            last_end = full.end();
        }
        result.push_str(&str_data[last_end..]);

        finish_replace_bytes(result.as_bytes())
    }
}

/// string.replaceAll(regex, replacement) -> string
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_replace_all_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    if !is_valid_regex_ptr(re) {
        return copy_replace_source(s);
    }

    ensure_replace_all_regex_global(re);
    js_string_replace_regex(s, re, replacement)
}

/// Split a string by a regex delimiter
/// string.split(regex) -> string[] (array of NaN-boxed string pointers)
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_split_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut ArrayHeader {
    js_string_split_regex_n(s, re, -1)
}

/// string.split(regex, limit) — limit<0 means no limit, limit==0 means empty
/// (issue #567).
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_split_regex_n(
    s: *const StringHeader,
    re: *const RegExpHeader,
    limit: i32,
) -> *mut ArrayHeader {
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    if !is_valid_ptr(s) {
        return crate::array::js_array_alloc(0);
    }
    if limit == 0 {
        return crate::array::js_array_alloc(0);
    }
    let str_data = string_as_str(s).to_owned();

    if !is_valid_regex_ptr(re) {
        // No regex: return array with the whole string as a single element
        let arr = crate::array::js_array_alloc(1);
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        // Allocating string + array re-read as one combinator (#7341).
        let (str_ptr, arr) =
            arr_handle.across_mut::<ArrayHeader, _>(|| js_string_from_str(&str_data) as u64);
        unsafe {
            (*arr).length = 1;
            let nanboxed = STRING_TAG | (str_ptr & POINTER_MASK);
            // GC_STORE_AUDIT(BARRIERED): regex split fallback slot uses the shared array slot-store helper.
            crate::array::store_array_slot(arr, 0, nanboxed);
        }
        return arr_handle.with_mut_ptr::<ArrayHeader, _>(|a| a);
    }

    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    unsafe {
        // Each element is either a substring (`Some`) or `undefined` (`None`,
        // for an unmatched capture group spliced into the result).
        let parts: Vec<Option<String>> =
            if let Some(repeat_matcher) = lookup_repeat_matcher_for(re, &str_data, 0) {
                repeat_matcher.split(&str_data, limit)
            } else if let Some(fre) = lookup_fancy_regex(re) {
                crate::string::spec_fancy_regex_split(&fre, &str_data, limit)
            } else {
                // Standard engine: the JS `RegExp.prototype[Symbol.split]` algorithm
                // (21.2.5.11). The `regex` crate's own `split` diverges from JS for
                // zero-width matches (it emits leading/trailing/consecutive empty
                // strings the spec's `e == p` skip suppresses) and never splices
                // captured groups, so walk the string the spec's way instead.
                crate::string::spec_regex_split(lazy::header_std_regex(re), &str_data, limit)
            };

        let arr = crate::array::js_array_alloc(parts.len() as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        arr_handle.with_mut_ptr::<ArrayHeader, _>(|a| (*a).length = parts.len() as u32);

        for (i, part) in parts.iter().enumerate() {
            let nanboxed = match part {
                Some(text) => {
                    let str_ptr = js_string_from_str(text) as u64;
                    STRING_TAG | (str_ptr & POINTER_MASK)
                }
                None => TAG_UNDEFINED,
            };
            // Re-read per iteration: `js_string_from_str` above allocates.
            let arr = arr_handle.with_mut_ptr::<ArrayHeader, _>(|a| a);
            // GC_STORE_AUDIT(BARRIERED): regex split result slot uses the shared array slot-store helper.
            crate::array::store_array_slot(arr, i, nanboxed);
        }
        arr_handle.with_mut_ptr::<ArrayHeader, _>(|a| a)
    }
}

/// Search for a regex match in a string
/// string.search(regex) -> number (index of first match, -1 if none)
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_search_regex(s: *const StringHeader, re: *const RegExpHeader) -> i32 {
    if !is_valid_ptr(s) || !is_valid_regex_ptr(re) {
        return -1;
    }
    let str_data = string_as_str(s);

    unsafe {
        if let Some(repeat_matcher) = lookup_repeat_matcher_for(re, str_data, 0) {
            return repeat_matcher
                .regex
                .find(str_data)
                .map(|matched| byte_index_to_utf16_index(str_data, matched.start()) as i32)
                .unwrap_or(-1);
        }

        // Fancy-regex fallback (lookbehind/backreferences): the never-match
        // placeholder standard program would always report -1 otherwise.
        if let Some(fre) = lookup_fancy_regex(re) {
            return match fre.find(str_data) {
                Ok(Some(m)) => byte_index_to_utf16_index(str_data, m.start()) as i32,
                _ => -1,
            };
        }

        let regex = lazy::header_std_regex(re);
        match regex.find(str_data) {
            Some(m) => {
                // `String.prototype.search` returns a JS string index — UTF-16
                // code units, matching `.index` / `lastIndex` / `str.length`.
                byte_index_to_utf16_index(str_data, m.start()) as i32
            }
            None => -1,
        }
    }
}
