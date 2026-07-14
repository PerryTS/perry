//! `String.prototype.replace` substitution machinery split out of
//! `regex.rs` to keep that file under the file-size gate:
//! `expand_js_replacement` (ECMAScript `$`-pattern expansion) and
//! `replace_regex_fn_fancy` (the fancy-regex callback-replace fallback).

use super::*;

/// of `String.prototype.replace` special patterns that the Rust `regex`
/// crate's own `$`-expansion does NOT cover: `$&` (matched substring),
/// `` $` `` (text before the match), `$'` (text after the match), plus the
/// shared `$$`, `$n`/`$nn` (numbered groups, largest-valid-group rule), and
/// `$<name>` (named groups). An unmatched group expands to the empty string;
/// an invalid `$`-sequence is emitted literally — both matching Node.
pub(super) fn expand_js_replacement(
    repl: &str,
    caps: &regex::Captures,
    subject: &str,
    has_named_groups: bool,
) -> String {
    let m0 = match caps.get(0) {
        Some(m) => m,
        None => return String::new(),
    };
    let (mstart, mend) = (m0.start(), m0.end());
    let ngroups = caps.len(); // valid group indices are 1..ngroups
    let b = repl.as_bytes();
    let mut out = String::with_capacity(repl.len() + 16);
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' {
            // Copy the run of non-`$` bytes in one go ('$' is ASCII, so the
            // slice boundaries are always on UTF-8 char boundaries).
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
                // JS tries the two-digit group first when it's valid, else
                // the single digit, else emits the `$` literally.
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
            b'<'
                // `$<name>` is a named-group reference ONLY when the regex
                // actually defines named capture groups. With no named groups,
                // JS emits `$<...>` literally (e.g. /n/ has none, so
                // "$<bad>" stays "$<bad>"). When the regex has named groups but
                // this particular name is absent, JS substitutes the empty
                // string.
                if has_named_groups => {
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

/// Owned, GC-inert snapshot of one match: everything the callback-argument
/// builder needs, copied OUT of the subject string BEFORE any JS allocation
/// or callback runs. The old code kept `Captures` (borrows of the heap
/// subject) alive across the callback loop: from the 2nd match on, every
/// `.as_str()` read through the ORIGINAL base address, which a moving GC
/// during an earlier callback had already retired (2026-07-09 audit, wave 1).
struct OwnedMatchData {
    /// Full-match byte range in the subject.
    start: usize,
    end: usize,
    /// Char offset of the match start (the callback's `offset` argument).
    char_offset: usize,
    /// Group 0 (full match) + capture groups (None = non-participating).
    groups: Vec<Option<String>>,
    /// Named groups in declaration order: (name, text).
    named: Vec<(String, Option<String>)>,
}

/// Shared callback-replace loop over pre-snapshotted matches. The subject is
/// rooted (`s_handle`) and re-derived after every callback; the callback
/// closure is rooted too, so a GC it triggers relocates — not strands — it.
unsafe fn replace_fn_run_matches(
    s_handle: &crate::gc::RuntimeHandle<'_>,
    matches: &[OwnedMatchData],
    closure_ptr: *const crate::closure::ClosureHeader,
    has_named_groups: bool,
) -> *mut StringHeader {
    let cur_str = || string_as_str(s_handle.get_raw_const_ptr::<StringHeader>());
    if matches.is_empty() {
        return js_string_from_str(cur_str());
    }
    let outer = crate::gc::RuntimeHandleScope::new();
    let closure_handle = outer.root_raw_const_ptr(closure_ptr);

    let mut result = String::new();
    let mut last_end = 0usize;

    for m in matches {
        // Between-match text is re-sliced from the CURRENT subject address
        // (the previous callback may have moved the string).
        result.push_str(&cur_str()[last_end..m.start]);

        // Build the full ECMAScript callback argument list:
        //   (match, p1, ..., pN, offset, string, groups?)
        // Every NaN-boxed heap value is rooted as it is created, so the next
        // allocation can't reclaim (or silently move) an earlier argument.
        let scope = crate::gc::RuntimeHandleScope::new();
        let mut arg_handles: Vec<crate::gc::RuntimeHandle<'_>> = Vec::new();

        for group in &m.groups {
            let group_val = match group {
                Some(text) => js_nanbox_string(js_string_from_str(text) as i64),
                None => f64::from_bits(crate::value::TAG_UNDEFINED),
            };
            arg_handles.push(scope.root_nanbox_f64(group_val));
        }

        arg_handles.push(scope.root_nanbox_f64(m.char_offset as f64));
        // The `string` argument is the subject itself — pass the rooted
        // original (same string value, current address) instead of
        // allocating a fresh copy per match.
        arg_handles.push(scope.root_nanbox_f64(js_nanbox_string(
            s_handle.get_raw_const_ptr::<StringHeader>() as i64,
        )));

        if has_named_groups {
            let groups_obj = crate::object::js_object_alloc(0, 0);
            let groups_handle = scope.root_raw_mut_ptr(groups_obj);
            for (name, text) in &m.named {
                let val = match text {
                    Some(t) => js_nanbox_string(js_string_from_str(t) as i64),
                    None => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
                // Root the value across the key allocation below.
                let val_handle = scope.root_nanbox_f64(val);
                let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let groups_obj = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                crate::object::js_object_set_field_by_name(
                    groups_obj,
                    key_ptr,
                    val_handle.get_nanbox_f64(),
                );
            }
            let groups_ptr = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
            let groups_value = crate::value::js_nanbox_pointer(groups_ptr as i64);
            arg_handles.push(scope.root_nanbox_f64(groups_value));
        }

        let call_args: Vec<f64> = arg_handles.iter().map(|h| h.get_nanbox_f64()).collect();
        let callback_value = f64::from_bits(
            crate::value::JSValue::pointer(closure_handle.get_raw_const_ptr::<u8>()).bits(),
        );
        result.push_str(&call_replace_callback(callback_value, &call_args));

        last_end = m.end;
    }

    result.push_str(&cur_str()[last_end..]);
    js_string_from_str(&result)
}

/// Fancy-regex fallback for `js_string_replace_regex_fn`: used when the pattern
/// needs lookahead/backreferences that the `regex` crate can't compile. Mirrors
/// the standard-engine loop below (full ECMAScript callback argument list,
/// char-based offset, named-group `groups` object) but drives the match loop
/// with `fancy_regex`.
pub(super) unsafe fn replace_regex_fn_fancy(
    s_handle: &crate::gc::RuntimeHandle<'_>,
    fre: &fancy_regex::Regex,
    global: bool,
    closure_ptr: *const crate::closure::ClosureHeader,
) -> *mut StringHeader {
    let has_named_groups = fre.capture_names().any(|n| n.is_some());
    let named_names: Vec<(usize, String)> = fre
        .capture_names()
        .enumerate()
        .filter_map(|(i, name)| name.map(|n| (i, n.to_string())))
        .collect();

    // Snapshot every match into owned data BEFORE the first callback (see
    // `OwnedMatchData`).
    let str_data = string_as_str(s_handle.get_raw_const_ptr::<StringHeader>());
    let mut matches: Vec<OwnedMatchData> = Vec::new();
    let mut iter = fre.captures_iter(str_data);
    while let Some(Ok(caps)) = iter.next() {
        let full_match = caps.get(0).unwrap();
        matches.push(OwnedMatchData {
            start: full_match.start(),
            end: full_match.end(),
            char_offset: super::exec_array::byte_index_to_utf16_index(str_data, full_match.start()),
            groups: (0..caps.len())
                .map(|gi| caps.get(gi).map(|m| m.as_str().to_string()))
                .collect(),
            named: named_names
                .iter()
                .map(|(gi, n)| (n.clone(), caps.get(*gi).map(|m| m.as_str().to_string())))
                .collect(),
        });
        if !global {
            break;
        }
    }

    replace_fn_run_matches(s_handle, &matches, closure_ptr, has_named_groups)
}

/// string.replace(regex, replacerFn) — replace with a callback function.
///
/// The callback receives the full ECMAScript argument list (#2867):
///   `(match, p1, p2, ..., offset, string, groups?)`
/// i.e. the whole match, then every capture group (undefined for
/// non-participating groups), then the 0-based offset of the match in the
/// input, then the whole input string, and finally — only when the pattern
/// has named capture groups — a `groups` object mapping each name to its
/// captured substring.
#[no_mangle]
pub extern "C" fn js_string_replace_regex_fn(
    s: *const StringHeader,
    re: *const RegExpHeader,
    callback: f64, // NaN-boxed closure pointer
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    // Root the subject for the whole replace: the replacer callback is user
    // code — it can allocate → trigger a minor GC → the subject is swept
    // (bare Rust locals are invisible without a conservative stack scan) or
    // moved. Match data is snapshotted into owned Rust strings BEFORE the
    // first callback, and the `&str` view is re-derived from this handle
    // after every callback.
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);

    if !is_valid_regex_ptr(re) {
        return js_string_from_str(string_as_str(s));
    }

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;

        // Extract closure pointer from NaN-boxed value
        let closure_ptr =
            crate::value::js_nanbox_get_pointer(callback) as *const crate::closure::ClosureHeader;
        if closure_ptr.is_null() {
            return js_string_from_str(string_as_str(s));
        }

        // If the `regex` crate couldn't compile this pattern (lookahead,
        // backreferences, …), `get_or_compile_regex` stashed a never-match
        // placeholder in `(*re).regex_ptr` and the real pattern in
        // `FANCY_CACHE`. Route the callback-replace through fancy-regex so the
        // callback actually fires — otherwise `captures_iter` below would
        // silently match nothing and return the input unchanged. (get-intrinsic's
        // `stringToPath` relies on `String.prototype.replace(/…(?=…)…/g, fn)`.)
        if let Some(fre) = lookup_fancy_regex(re) {
            return replace_regex_fn_fancy(&s_handle, &fre, global, closure_ptr);
        }

        // Does the pattern declare any named capture groups? If so we pass a
        // trailing `groups` object to the callback (matching Node). Computed
        // once outside the match loop.
        let has_named_groups = regex.capture_names().any(|n| n.is_some());
        let named_names: Vec<(usize, String)> = regex
            .capture_names()
            .enumerate()
            .filter_map(|(i, name)| name.map(|n| (i, n.to_string())))
            .collect();

        // Snapshot every match into owned data BEFORE the first callback (see
        // `OwnedMatchData`).
        let str_data = string_as_str(s_handle.get_raw_const_ptr::<StringHeader>());
        let mut matches: Vec<OwnedMatchData> = Vec::new();
        let mut push_caps = |caps: regex::Captures<'_>| {
            let full_match = caps.get(0).unwrap();
            matches.push(OwnedMatchData {
                start: full_match.start(),
                end: full_match.end(),
                char_offset: super::exec_array::byte_index_to_utf16_index(
                    str_data,
                    full_match.start(),
                ),
                groups: (0..caps.len())
                    .map(|gi| caps.get(gi).map(|m| m.as_str().to_string()))
                    .collect(),
                named: named_names
                    .iter()
                    .map(|(gi, n)| (n.clone(), caps.get(*gi).map(|m| m.as_str().to_string())))
                    .collect(),
            });
        };
        if global {
            for caps in regex.captures_iter(str_data) {
                push_caps(caps);
            }
        } else if let Some(caps) = regex.captures(str_data) {
            push_caps(caps);
        }

        replace_fn_run_matches(&s_handle, &matches, closure_ptr, has_named_groups)
    }
}

/// string.replaceAll(regex, replacerFn) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_all_regex_fn(
    s: *const StringHeader,
    re: *const RegExpHeader,
    callback: f64,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    if !is_valid_regex_ptr(re) {
        return js_string_from_str(str_data);
    }

    ensure_replace_all_regex_global(re);
    js_string_replace_regex_fn(s, re, callback)
}

/// string.replace(regex, replacement) with named group references ($<name>)
/// Handles $<name> replacement patterns for named capture groups
#[no_mangle]
pub extern "C" fn js_string_replace_regex_named(
    s: *const StringHeader,
    re: *const RegExpHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }
    let str_data = string_as_str(s);
    let repl_str = if is_valid_ptr(replacement) {
        string_as_str(replacement)
    } else {
        "undefined"
    };

    if !is_valid_regex_ptr(re) {
        return js_string_from_str(str_data);
    }

    // Check if replacement contains $<name> patterns
    let has_named_refs = repl_str.contains("$<");

    if !has_named_refs {
        // Fall back to regular replace
        return js_string_replace_regex(s, re, replacement);
    }

    unsafe {
        // Fancy-regex fallback (lookbehind/backreferences): expand `$<name>`
        // and friends against the fancy captures instead of the never-match
        // placeholder stored in `regex_ptr`.
        if let Some(fre) = lookup_fancy_regex(re) {
            return replace_regex_str_fancy(str_data, &fre, (*re).global, repl_str);
        }

        let regex = &*(*re).regex_ptr;
        let global = (*re).global;
        let has_named_groups = regex.capture_names().any(|n| n.is_some());

        let mut result = String::new();
        let mut last_end = 0usize;

        let captures_list: Vec<regex::Captures> = if global {
            regex.captures_iter(str_data).collect()
        } else {
            match regex.captures(str_data) {
                Some(caps) => vec![caps],
                None => vec![],
            }
        };

        if captures_list.is_empty() {
            return js_string_from_str(str_data);
        }

        for caps in &captures_list {
            let full_match = caps.get(0).unwrap();
            result.push_str(&str_data[last_end..full_match.start()]);

            // Delegate to the unified JS-aware expander so `$<name>` follows the
            // spec: literal when the regex has no named groups, empty when the
            // named group is absent (and `$&`/`` $` ``/`$'`/`$n`/`$$` all work).
            result.push_str(&expand_js_replacement(
                repl_str,
                caps,
                str_data,
                has_named_groups,
            ));
            last_end = full_match.end();
        }

        result.push_str(&str_data[last_end..]);
        js_string_from_str(&result)
    }
}

/// string.replaceAll(regex, replacement) with named group references ($<name>)
#[no_mangle]
pub extern "C" fn js_string_replace_all_regex_named(
    s: *const StringHeader,
    re: *const RegExpHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    if !is_valid_regex_ptr(re) {
        return js_string_from_str(str_data);
    }

    ensure_replace_all_regex_global(re);
    js_string_replace_regex_named(s, re, replacement)
}
