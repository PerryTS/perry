//! `RegExp.prototype.compile(pattern, flags)` (Annex B §B.2.4.1).
//!
//! Split out of `regex.rs` to keep that file under the 2000-line size gate.

use std::sync::Arc;

use regex::Regex;

use super::grammar::{
    has_invalid_repeated_quantifier, has_unicode_forbidden_legacy_escape,
    has_unicode_forbidden_pattern, js_regex_to_rust,
};
use super::{
    build_fancy_regex, build_std_regex, get_or_compile_regex, is_regex_pointer, is_valid_ptr,
    is_valid_regex_ptr, js_regexp_get_flags, js_regexp_get_source, js_string_from_str,
    string_as_str, throw_regexp_syntax_error, validate_and_canonicalize_flags, RegExpHeader,
};

/// `RegExp.prototype.compile(pattern, flags)`. Re-initializes the receiver
/// RegExp *in place*: re-validates and recompiles the pattern, updates
/// `.source`/`.flags`, and resets `lastIndex` to 0. Returns the receiver
/// (NaN-boxed). When `pattern` is itself a RegExp its source+flags are adopted
/// and a non-`undefined` `flags` argument is a TypeError.
#[no_mangle]
pub extern "C" fn js_regexp_compile_value(
    re: *mut RegExpHeader,
    pattern_val: f64,
    flags_val: f64,
) -> f64 {
    if !is_valid_regex_ptr(re) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let pj = crate::value::JSValue::from_bits(pattern_val.to_bits());
    let fj = crate::value::JSValue::from_bits(flags_val.to_bits());

    let (pattern_owned, flags_owned) = if pj.is_pointer() && is_regex_pointer(pj.as_pointer::<u8>())
    {
        // RegExp source: adopt source+flags; supplying flags is a TypeError.
        if !fj.is_undefined() {
            crate::collection_iter::throw_type_error(
                "Cannot supply flags when constructing one RegExp from another",
            );
        }
        let src_re = pj.as_pointer::<RegExpHeader>();
        let src = js_regexp_get_source(src_re);
        let flg = js_regexp_get_flags(src_re);
        let src_s = if is_valid_ptr(src) {
            string_as_str(src).to_string()
        } else {
            String::new()
        };
        let flg_s = if is_valid_ptr(flg) {
            string_as_str(flg).to_string()
        } else {
            String::new()
        };
        (src_s, flg_s)
    } else {
        // ToString(pattern), with `undefined` -> "" (spec); same for flags.
        let pat = if pj.is_undefined() {
            String::new()
        } else {
            let p = crate::builtins::js_string_coerce(pattern_val);
            if is_valid_ptr(p) {
                string_as_str(p).to_string()
            } else {
                String::new()
            }
        };
        let flg = if fj.is_undefined() {
            String::new()
        } else {
            let f = crate::builtins::js_string_coerce(flags_val);
            if is_valid_ptr(f) {
                string_as_str(f).to_string()
            } else {
                String::new()
            }
        };
        (pat, flg)
    };

    let canonical_flags = validate_and_canonicalize_flags(&flags_owned);
    let flags_str = canonical_flags.as_str();
    let pattern_str = pattern_owned.as_str();

    // Same SyntaxError validation as `js_regexp_new`: only reject patterns that
    // neither the `regex` crate nor `fancy-regex` accept.
    if has_invalid_repeated_quantifier(pattern_str) {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }
    // Annex B.1.4 leniencies are hard `SyntaxError`s under `/u` (mirror of
    // `js_regexp_new`): legacy escapes for `u`/`v`, plus the structural
    // restrictions for `u` specifically.
    let unicode = flags_str.contains('u') || flags_str.contains('v');
    if unicode && has_unicode_forbidden_legacy_escape(pattern_str) {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }
    if flags_str.contains('u') && has_unicode_forbidden_pattern(pattern_str) {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }
    let translated = js_regex_to_rust(pattern_str);
    if build_std_regex(&translated).is_err() && build_fancy_regex(&translated).is_err() {
        throw_regexp_syntax_error(&format!(
            "Invalid regular expression: /{}/: invalid pattern",
            pattern_str
        ));
    }

    let arc = get_or_compile_regex(pattern_str, flags_str);
    let regex_ptr = Arc::as_ptr(&arc) as *mut Regex;
    let canonical_flags_ptr = js_string_from_str(flags_str);
    let pattern_ptr = js_string_from_str(pattern_str);
    unsafe {
        (*re).regex_ptr = regex_ptr;
        (*re).pattern_ptr = pattern_ptr;
        (*re).flags_ptr = canonical_flags_ptr;
        (*re).case_insensitive = flags_str.contains('i');
        (*re).global = flags_str.contains('g');
        (*re).multiline = flags_str.contains('m');
        (*re).sticky = flags_str.contains('y');
        (*re).dot_all = flags_str.contains('s');
        (*re).unicode = flags_str.contains('u') || flags_str.contains('v');
        (*re).has_indices = flags_str.contains('d');
        (*re).last_index = 0;
        super::REGEX_SOURCE_TABLE.with(|t| {
            t.borrow_mut().insert(
                re as usize,
                (pattern_str.to_string(), flags_str.to_string()),
            );
        });
    }
    f64::from_bits(crate::value::JSValue::pointer(re as *const u8).bits())
}
