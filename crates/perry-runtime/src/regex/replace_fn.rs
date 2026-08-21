use super::*;
// `js_nanbox_string` is re-exported from the parent only when the regex engine
// is on (it's part of the gated engine `use` cluster). The string-replacement
// helpers below are always compiled, so import it directly here.
use crate::value::js_nanbox_string;

pub(super) unsafe fn call_replace_callback(callback: f64, args: &[f64]) -> String {
    let prev = crate::object::js_implicit_this_set(f64::from_bits(crate::value::TAG_UNDEFINED));
    let ret = crate::closure::js_native_call_value(callback, args.as_ptr(), args.len());
    crate::object::js_implicit_this_set(prev);
    // §22.1.3.19 step "Let replacement be ? ToString(? Call(replaceValue, …))":
    // the callback result is ToString-coerced — `undefined` renders as
    // "undefined", a number stringifies, an object runs its `toString`, and a
    // Symbol throws a TypeError. The old raw pointer-extract silently dropped
    // every non-string result to "" (test262 replace S15.5.4.11_A1_T5/T10).
    crate::builtins::reject_symbol_to_string(ret);
    let ptr = crate::builtins::js_string_coerce(ret) as *const StringHeader;
    if is_valid_ptr(ptr) {
        string_as_str(ptr).to_string()
    } else {
        String::new()
    }
}

#[derive(Clone, Copy)]
enum Utf16StringUnit {
    Source { start: usize, len: usize },
    InlineSurrogate([u8; 3]),
}

#[inline]
fn encode_wtf8_surrogate(unit: u16) -> [u8; 3] {
    [
        0xE0 | ((unit >> 12) as u8),
        0x80 | (((unit >> 6) & 0x3F) as u8),
        0x80 | ((unit & 0x3F) as u8),
    ]
}

/// Snapshot a Perry UTF-8/WTF-8 payload as JavaScript UTF-16 units. Astral
/// UTF-8 sequences become two WTF-8 surrogate halves so an empty search string
/// can match between them, as ECMAScript requires.
fn snapshot_utf16_units(bytes: &[u8]) -> Vec<Utf16StringUnit> {
    let mut units = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let (advance, utf16_units, code_point) = crate::string::wtf8_step(bytes, i);
        let end = (i + advance).min(bytes.len());
        if utf16_units == 2 && code_point >= 0x10000 {
            let astral = code_point - 0x10000;
            units.push(Utf16StringUnit::InlineSurrogate(encode_wtf8_surrogate(
                0xD800 + (astral >> 10) as u16,
            )));
            units.push(Utf16StringUnit::InlineSurrogate(encode_wtf8_surrogate(
                0xDC00 + (astral & 0x3FF) as u16,
            )));
        } else if utf16_units == 1 {
            units.push(Utf16StringUnit::Source {
                start: i,
                len: end - i,
            });
        }
        i = end;
    }
    units
}

fn append_utf16_units(out: &mut Vec<u8>, source: &[u8], units: &[Utf16StringUnit]) {
    for unit in units {
        match unit {
            Utf16StringUnit::Source { start, len } => {
                out.extend_from_slice(&source[*start..*start + *len]);
            }
            Utf16StringUnit::InlineSurrogate(bytes) => out.extend_from_slice(bytes),
        }
    }
}

/// Expand one replacement template for an empty string-pattern match at a
/// UTF-16 boundary. In particular, `$\`` and `$'` select unit ranges rather
/// than UTF-8 byte/Unicode-scalar ranges.
fn append_empty_pattern_replacement(
    out: &mut Vec<u8>,
    replacement: &[u8],
    source: &[u8],
    units: &[Utf16StringUnit],
    position: usize,
) {
    let mut i = 0usize;
    while i < replacement.len() {
        if replacement[i] != b'$' || i + 1 == replacement.len() {
            out.push(replacement[i]);
            i += 1;
            continue;
        }
        match replacement[i + 1] {
            b'$' => out.push(b'$'),
            b'&' => {}
            b'`' => append_utf16_units(out, source, &units[..position]),
            b'\'' => append_utf16_units(out, source, &units[position..]),
            _ => {
                out.push(b'$');
                i += 1;
                continue;
            }
        }
        i += 2;
    }
}

pub(super) fn finish_replace_bytes(bytes: &[u8]) -> *mut StringHeader {
    let result = crate::string::js_string_from_builder_bytes(bytes);
    std::hint::black_box(bytes);
    result
}

pub(super) fn copy_replace_source(s: *const StringHeader) -> *mut StringHeader {
    let (byte_len, utf16_len, flags) = unsafe { ((*s).byte_len, (*s).utf16_len, (*s).flags) };
    crate::string::string_copy_range(s, 0, byte_len, utf16_len, flags)
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
            let source = cur_str().as_bytes();
            let mut result = Vec::with_capacity(replacement.len() + source.len());
            result.extend_from_slice(replacement.as_bytes());
            result.extend_from_slice(source);
            return finish_replace_bytes(&result);
        }

        let str_data = cur_str();
        let Some(byte_idx) = str_data.find(pattern_str.as_str()) else {
            return s_handle
                .with_const_ptr(|s_now: *const StringHeader| copy_replace_source(s_now));
        };
        let char_offset = super::utf16::byte_index_to_utf16_index(str_data, byte_idx);
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
        finish_replace_bytes(result.as_bytes())
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
            // Owned byte/unit snapshot: callbacks may move the subject, and JS
            // empty-pattern matches occur at UTF-16 (not Unicode scalar)
            // boundaries, including between an astral surrogate pair.
            let source = cur_str().as_bytes().to_vec();
            let units = snapshot_utf16_units(&source);
            let mut result = Vec::with_capacity(source.len());
            result.extend_from_slice(
                call_string_replace_callback(callback_handle.get_nanbox_f64(), "", 0, &s_handle)
                    .as_bytes(),
            );
            for (index, unit) in units.iter().enumerate() {
                append_utf16_units(&mut result, &source, std::slice::from_ref(unit));
                result.extend_from_slice(
                    call_string_replace_callback(
                        callback_handle.get_nanbox_f64(),
                        "",
                        index + 1,
                        &s_handle,
                    )
                    .as_bytes(),
                );
            }
            return finish_replace_bytes(&result);
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
                    // Advance in UTF-16 code units — the offset handed to a
                    // `String.prototype.replace` callback is a JS string index.
                    char_pos += str_data[last_byte..byte_idx]
                        .chars()
                        .map(char::len_utf16)
                        .sum::<usize>();
                    last_byte = byte_idx;
                    (byte_idx, char_pos)
                })
                .collect()
        };
        if matches.is_empty() {
            return s_handle
                .with_const_ptr(|s_now: *const StringHeader| copy_replace_source(s_now));
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
        finish_replace_bytes(result.as_bytes())
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

    if pattern_str.is_empty() {
        let source = str_data.as_bytes();
        let units = snapshot_utf16_units(source);
        let mut result = Vec::with_capacity(source.len().saturating_add(repl_str.len()));
        append_empty_pattern_replacement(&mut result, repl_str.as_bytes(), source, &units, 0);
        append_utf16_units(&mut result, source, &units);
        return finish_replace_bytes(&result);
    }

    // String.replace with a string pattern only replaces the first occurrence.
    // Fast path: a replacement with no `$` needs no substitution.
    if !repl_str.contains('$') {
        let result = str_data.replacen(pattern_str, repl_str, 1);
        return finish_replace_bytes(result.as_bytes());
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
    finish_replace_bytes(result.as_bytes())
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

    if pattern_str.is_empty() {
        let source = str_data.as_bytes();
        let units = snapshot_utf16_units(source);
        let insertion_count = units.len().saturating_add(1);
        let mut result = Vec::with_capacity(
            source
                .len()
                .saturating_add(repl_str.len().saturating_mul(insertion_count)),
        );
        for position in 0..=units.len() {
            append_empty_pattern_replacement(
                &mut result,
                repl_str.as_bytes(),
                source,
                &units,
                position,
            );
            if position < units.len() {
                append_utf16_units(&mut result, source, std::slice::from_ref(&units[position]));
            }
        }
        return finish_replace_bytes(&result);
    }

    // Fast path: a replacement with no `$` needs no substitution.
    if !repl_str.contains('$') {
        let result = str_data.replace(pattern_str, repl_str);
        return finish_replace_bytes(result.as_bytes());
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
    finish_replace_bytes(result.as_bytes())
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

/// Root TWO raw receivers across one allocating coercion and hand the callee
/// the POST-collection addresses.
///
/// `RuntimeHandle::across_const` pairs exactly one allocating call with one
/// re-read, so two receivers compose by NESTING: the inner call runs `coerce`
/// and re-reads `b`, the outer then re-reads `a`. Neither pre-call address is
/// ever bound, which is the property `across_*` exists to provide (#7341) and
/// the one `scripts/raw_handle_debt.py` counts. `path::value_args`'
/// `with_two_headers` uses the same nesting for the same reason.
///
/// Both receivers are rooted BEFORE `coerce` runs, so a collection inside it
/// marks and rewrites both slots; `f` then sees two addresses that are current
/// as of the same collection.
fn with_two_receivers_across<A, B, C, R>(
    a: *const A,
    b: *const B,
    coerce: impl FnOnce() -> C,
    f: impl FnOnce(*const A, *const B, C) -> R,
) -> R {
    let scope = crate::gc::RuntimeHandleScope::new();
    let a_handle = scope.root_raw_const_ptr(a);
    let b_handle = scope.root_raw_const_ptr(b);
    let ((coerced, b), a) = a_handle.across_const::<A, _>(|| b_handle.across_const::<B, _>(coerce));
    f(a, b, coerced)
}

/// One-receiver twin of [`with_two_receivers_across`].
fn with_receiver_across<A, C, R>(
    a: *const A,
    coerce: impl FnOnce() -> C,
    f: impl FnOnce(*const A, C) -> R,
) -> R {
    let scope = crate::gc::RuntimeHandleScope::new();
    let a_handle = scope.root_raw_const_ptr(a);
    let (coerced, a) = a_handle.across_const::<A, _>(coerce);
    f(a, coerced)
}

#[no_mangle]
pub extern "C" fn js_string_replace_string_dyn(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: f64,
) -> *mut StringHeader {
    // #6949(a): `js_string_coerce` is a plain ToString ARGUMENT coercion here,
    // and it allocates for every shape except an already-heap STRING_TAG value
    // (`builtins::string_coerce_is_inert`) — an SSO short string materialises,
    // a number/bool/null/BigInt builds its stringification, and a POINTER_TAG
    // object runs a user `toString`/`valueOf`. Any of those can collect and
    // EVACUATE. Rust evaluates arguments left to right, so the raw receiver
    // params below are copied BEFORE the coercion runs; the copies survive, the
    // pointees move, and the callee dereferences the stale ones.
    if replacement_is_callable(replacement) {
        return js_string_replace_string_fn(s, pattern, replacement);
    }
    with_two_receivers_across(
        s,
        pattern,
        || crate::builtins::js_string_coerce(replacement),
        |s, pattern, coerced| js_string_replace_string(s, pattern, coerced),
    )
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
    with_two_receivers_across(
        s,
        pattern,
        || crate::builtins::js_string_coerce(replacement),
        |s, pattern, coerced| js_string_replace_all_string(s, pattern, coerced),
    )
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
    with_receiver_across(
        s,
        || crate::builtins::js_string_coerce(needle),
        |s, needle| js_string_replace_string_dyn(s, needle, replacement),
    )
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
    with_receiver_across(
        s,
        || crate::builtins::js_string_coerce(needle),
        |s, needle| js_string_replace_all_string_dyn(s, needle, replacement),
    )
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
    // #6949(a): both raw params span the coercion — and `re` is a
    // `RegExpHeader`, not a string, so a stale one is read for its compiled
    // pattern rather than merely for bytes.
    with_two_receivers_across(
        s,
        re,
        || crate::builtins::js_string_coerce(replacement),
        |s, re, coerced| crate::regex::js_string_replace_regex_named(s, re, coerced),
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
    // #6949(a): both raw params span the coercion — and `re` is a
    // `RegExpHeader`, not a string, so a stale one is read for its compiled
    // pattern rather than merely for bytes.
    with_two_receivers_across(
        s,
        re,
        || crate::builtins::js_string_coerce(replacement),
        |s, re, coerced| crate::regex::js_string_replace_all_regex_named(s, re, coerced),
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
